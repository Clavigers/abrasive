use blake3::Hasher;
use env_logger::Env;
use log::{debug, error, info, warn};
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, exit};

pub mod cache_io;
pub mod digest;
pub mod disk_cache;
pub mod rustc_args;

use cache_io::{CacheWrite, FileObjectSource};
use digest::hash_rustc_args;
use disk_cache::DiskCache;
use rustc_args::{ParseOutcome, ParsedArguments, parse_arguments};

/// Entry point for the `drop-point` binary. Initializes logging, classifies
/// the rustc-wrapper chain shape cargo handed us, and either exec's rustc
/// directly (workspace crates) or runs the cache logic (third-party crates).
/// `exit`s the process with the appropriate code rather than returning.
pub fn run() -> ! {
    init_logger();
    let argv: Vec<OsString> = env::args_os().collect();
    let ws_wrapper = env::var_os("RUSTC_WORKSPACE_WRAPPER");
    let Some(shape) = classify(&argv, ws_wrapper.as_deref()) else {
        error!("drop-point: must be used as a rustc wrapper");
        exit(2);
    };
    match shape {
        ChainShape::Workspace { rustc, rest } => exec_rustc(&rustc, &rest),
        ChainShape::Cache { rustc, rest } => cache_path(rustc, rest),
    }
}

/// Whether cargo invoked us in the workspace shape (`drop-point
/// drop-point-skip rustc <args>`) or the third-party shape (`drop-point rustc
/// <args>`). Cargo decides; we observe.
#[derive(Debug, PartialEq, Eq)]
pub enum ChainShape {
    /// argv[1] is the workspace wrapper. Skip caching, exec rustc directly.
    Workspace { rustc: OsString, rest: Vec<OsString> },
    /// argv[1] is rustc itself. Run the cache path.
    Cache { rustc: OsString, rest: Vec<OsString> },
}

/// Classify the wrapper chain from argv plus the value of
/// `RUSTC_WORKSPACE_WRAPPER`. Returns `None` only when argv has no
/// arguments past argv[0] (drop-point invoked with no rustc behind it).
pub fn classify(argv: &[OsString], workspace_wrapper: Option<&OsStr>) -> Option<ChainShape> {
    let arg1 = argv.get(1)?;
    if let Some(ws) = workspace_wrapper
        && arg1.as_os_str() == ws
    {
        let rustc = argv.get(2)?.clone();
        let rest = argv.get(3..).unwrap_or(&[]).to_vec();
        return Some(ChainShape::Workspace { rustc, rest });
    }
    let rustc = arg1.clone();
    let rest = argv.get(2..).unwrap_or(&[]).to_vec();
    Some(ChainShape::Cache { rustc, rest })
}

fn exec_rustc(rustc: &OsStr, rest: &[OsString]) -> ! {
    let err = Command::new(rustc).args(rest).exec();
    error!(
        "drop-point: failed to exec {}: {err}",
        rustc.to_string_lossy()
    );
    exit(2);
}

fn cache_path(rustc: OsString, rest: Vec<OsString>) -> ! {
    let plan = plan_cache(&rest);
    if let Some((parsed, key)) = &plan
        && try_serve_from_cache(&rustc, &rest, parsed, key)
    {
        exit(0);
    }
    let exit_code = run_rustc(&rustc, &rest);
    if exit_code == 0
        && let Some((parsed, key)) = plan
        && let Err(e) = save_outputs(&rustc, &rest, &parsed, &key)
    {
        warn!("drop-point: cache store failed: {e}");
    }
    exit(exit_code);
}

fn init_logger() {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug"))
        .write_style(env_logger::WriteStyle::Always)
        .format(|buf, record| {
            let style = buf.default_level_style(record.level());
            writeln!(
                buf,
                "[{style}{}{style:#} {}] {}",
                record.level(),
                record.target(),
                record.args()
            )
        })
        .init();
}

fn plan_cache(rest: &[OsString]) -> Option<(ParsedArguments, String)> {
    let cwd = env::current_dir().ok()?;
    let parsed = match parse_arguments(rest, &cwd) {
        ParseOutcome::Ok(p) => p,
        ParseOutcome::CannotCache(why, extra) => {
            let crate_name = crate_name_from_argv(rest);
            match extra {
                Some(e) => debug!("skip {crate_name}: {why} ({e})"),
                None => debug!("skip {crate_name}: {why}"),
            }
            return None;
        }
        ParseOutcome::NotCompilation => return None,
    };
    let mut hasher = Hasher::new();
    if let Err(e) = hash_rustc_args(&parsed, &mut hasher) {
        warn!("drop-point: hash failed for {}: {e}", parsed.crate_name);
        return None;
    }
    let key = hasher.finalize().to_hex().to_string();
    Some((parsed, key))
}

/// Best-effort `--crate-name` lookup so we can name what we skipped before
/// argv parsing finished. Returns "?" when not found.
fn crate_name_from_argv(rest: &[OsString]) -> String {
    let mut iter = rest.iter();
    while let Some(arg) = iter.next() {
        let s = arg.to_string_lossy();
        if let Some(name) = s.strip_prefix("--crate-name=") {
            return name.to_string();
        }
        if s == "--crate-name"
            && let Some(next) = iter.next()
        {
            return next.to_string_lossy().into_owned();
        }
    }
    "?".to_string()
}

fn try_serve_from_cache(
    rustc: &OsStr,
    rest: &[OsString],
    parsed: &ParsedArguments,
    key: &str,
) -> bool {
    let Ok(cache) = DiskCache::new(cache_root()) else {
        return false;
    };
    let entry = match cache.get(key) {
        Ok(Some(e)) => e,
        Ok(None) => {
            debug!("miss {} {}", parsed.crate_name, &key[..16]);
            return false;
        }
        Err(e) => {
            warn!(
                "drop-point: cache read error for {}: {e}",
                parsed.crate_name
            );
            return false;
        }
    };
    let objects = match plan_outputs(rustc, rest, parsed) {
        Ok(v) => v,
        Err(e) => {
            warn!("drop-point: cache hit but couldn't plan outputs: {e}");
            return false;
        }
    };
    if let Err(e) = entry.extract_objects(objects) {
        warn!("drop-point: cache hit but materialize failed: {e}");
        return false;
    }
    info!("hit {} {}", parsed.crate_name, &key[..16]);
    true
}

fn save_outputs(
    rustc: &OsStr,
    rest: &[OsString],
    parsed: &ParsedArguments,
    key: &str,
) -> io::Result<()> {
    let cache = DiskCache::new(cache_root())?;
    let objects = plan_outputs(rustc, rest, parsed)?;
    let entry = CacheWrite::from_objects(objects)
        .map_err(|e| io::Error::other(format!("build cache entry: {e}")))?;
    let wrote = cache
        .put(key, entry)
        .map_err(|e| io::Error::other(format!("put: {e}")))?;
    if wrote {
        info!("cached {} {}", parsed.crate_name, &key[..16]);
    }
    Ok(())
}

/// Compute the [`FileObjectSource`] list — one entry per output file rustc
/// produces — for both the put (read these files into the zip) and get
/// (extract zip members into these paths) sides.
fn plan_outputs(
    rustc: &OsStr,
    rest: &[OsString],
    parsed: &ParsedArguments,
) -> io::Result<Vec<FileObjectSource>> {
    let mut names = refine_outputs(parsed, discover_outputs(rustc, rest)?);
    if let Some(d) = &parsed.dep_info {
        names.push(d.to_string_lossy().into_owned());
    }
    if let Some(p) = &parsed.profile {
        names.push(p.to_string_lossy().into_owned());
    }
    let out = &parsed.output_dir;
    Ok(names
        .into_iter()
        .map(|name| FileObjectSource {
            path: out.join(&name),
            key: name,
            optional: false,
        })
        .collect())
}

/// Ask rustc which files this invocation would produce. `--print file-names`
/// short-circuits compilation and writes one filename per line, all relative
/// to `--out-dir`.
fn discover_outputs(rustc: &OsStr, rest: &[OsString]) -> io::Result<Vec<String>> {
    let out = Command::new(rustc)
        .args(rest)
        .arg("--print")
        .arg("file-names")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "rustc --print file-names failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let stdout =
        String::from_utf8(out.stdout).map_err(|_| io::Error::other("non-utf8 rustc output"))?;
    Ok(stdout.lines().map(str::to_owned).collect())
}

/// Patch up rustc's --print file-names list to match what actually lands on
/// disk. Two quirks (also present in sccache):
/// 1. With `--emit=metadata` (no link), rustc still prints binaries that
///    won't exist; drop them.
/// 2. rustc doesn't print rmeta files even when --emit=metadata produces
///    them; synthesize an rmeta name per rlib.
fn refine_outputs(parsed: &ParsedArguments, mut outputs: Vec<String>) -> Vec<String> {
    let only_metadata = !parsed.emit.is_empty()
        && parsed
            .emit
            .iter()
            .all(|e| e == "metadata" || e == "dep-info");
    if only_metadata {
        outputs.retain(|o| o.ends_with(".rlib") || o.ends_with(".rmeta"));
    }
    if parsed.emit.contains("metadata") {
        let rlibs: Vec<String> = outputs
            .iter()
            .filter(|p| p.ends_with(".rlib"))
            .cloned()
            .collect();
        for lib in rlibs {
            let rmeta = lib.replacen(".rlib", ".rmeta", 1);
            if !outputs.contains(&rmeta) {
                outputs.push(rmeta);
            }
            if !parsed.emit.contains("link") {
                outputs.retain(|p| *p != lib);
            }
        }
    }
    outputs
}

fn cache_root() -> PathBuf {
    let home = env::var_os("HOME").unwrap_or_else(|| OsString::from("/tmp"));
    PathBuf::from(home).join(".cache").join("drop-point")
}

fn run_rustc(rustc: &OsStr, rest: &[OsString]) -> i32 {
    match Command::new(rustc).args(rest).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => spawn_failed(rustc, e),
    }
}

fn spawn_failed(rustc: &OsStr, e: io::Error) -> i32 {
    error!(
        "drop-point: failed to spawn {}: {e}",
        rustc.to_string_lossy()
    );
    2
}
