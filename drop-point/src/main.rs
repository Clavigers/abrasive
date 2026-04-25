use blake3::Hasher;
use env_logger::Env;
use log::{error, info, warn};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

mod digest;
mod disk_cache;
mod rustc_args;

use digest::hash_rustc_args;
use disk_cache::DiskCache;
use rustc_args::{ParseOutcome, ParsedArguments, parse_arguments};

fn main() {
    init_logger();
    let (rustc, rest) = parse_args();
    let plan = plan_third_party_cache(&rest);
    if let Some((parsed, key)) = &plan
        && try_serve_from_cache(parsed, key)
    {
        exit(0);
    }
    let exit_code = run_rustc(&rustc, &rest);
    if exit_code == 0
        && let Some((parsed, key)) = plan
        && let Err(e) = save_outputs(&parsed, &key)
    {
        warn!("drop-point: cache store failed: {e}");
    }
    exit(exit_code);
}

fn init_logger() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
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

fn parse_args() -> (OsString, Vec<OsString>) {
    let mut args = env::args_os();
    args.next();
    let Some(rustc) = args.next() else {
        error!("drop-point: must be used as a rustc wrapper!");
        exit(2);
    };
    let rest: Vec<_> = args.collect();
    (rustc, rest)
}

fn plan_third_party_cache(rest: &[OsString]) -> Option<(ParsedArguments, String)> {
    let cwd = env::current_dir().ok()?;
    let parsed = match parse_arguments(rest, &cwd) {
        ParseOutcome::Ok(p) => p,
        _ => return None,
    };
    if !is_third_party(&parsed.input) {
        return None;
    }
    let mut hasher = Hasher::new();
    hash_rustc_args(&parsed, &mut hasher);
    let key = hasher.finalize().to_hex().to_string();
    Some((parsed, key))
}

/// True when the input source is outside every workspace member dir.
/// `DROP_POINT_WORKSPACE_MEMBERS` is colon-separated absolute paths set by
/// the daemon from `cargo metadata`. Unset means we don't have authoritative
/// info, so we fail closed and skip caching.
fn is_third_party(input: &Path) -> bool {
    let Some(env) = env::var_os("DROP_POINT_WORKSPACE_MEMBERS") else {
        return false;
    };
    let env = env.to_string_lossy();
    !env.split(':')
        .filter(|d| !d.is_empty())
        .any(|d| input.starts_with(d))
}

fn try_serve_from_cache(parsed: &ParsedArguments, key: &str) -> bool {
    let Ok(cache) = DiskCache::new(cache_root()) else {
        return false;
    };
    let Some(src) = cache.get(key) else {
        return false;
    };
    if let Err(e) = hardlink_into(&src, &parsed.output_dir) {
        warn!("drop-point: cache hit but materialize failed: {e}");
        return false;
    }
    info!("hit {} {}", parsed.crate_name, &key[..16]);
    true
}

fn hardlink_into(src_dir: &Path, out_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(out_dir)?;
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let dest = out_dir.join(entry.file_name());
        let _ = fs::remove_file(&dest);
        if fs::hard_link(entry.path(), &dest).is_err() {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

fn save_outputs(parsed: &ParsedArguments, key: &str) -> io::Result<()> {
    let cache = DiskCache::new(cache_root())?;
    if cache.put(key, |dst| copy_outputs_into(parsed, dst))? {
        info!("cached {} {}", parsed.crate_name, &key[..16]);
    }
    Ok(())
}

fn copy_outputs_into(parsed: &ParsedArguments, dst: &Path) -> io::Result<()> {
    let dep_info = parsed
        .dep_info
        .as_ref()
        .ok_or_else(|| io::Error::other("no dep-info"))?;
    let stem = dep_info
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::other("dep-info missing usable stem"))?;
    let out = &parsed.output_dir;
    for src in [
        out.join(format!("lib{stem}.rlib")),
        out.join(format!("lib{stem}.rmeta")),
        out.join(dep_info),
    ] {
        if src.exists() {
            let name = src.file_name().expect("has file name");
            fs::copy(&src, dst.join(name))?;
        }
    }
    Ok(())
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
