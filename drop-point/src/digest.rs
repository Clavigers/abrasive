use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;

use crate::rustc_args::{Argument, ArgumentValue, ParsedArguments};

/// Mix everything that affects the rlib's bytes into the hasher. Cargo's
/// metadata hash on extern paths and the target/profile flags in argv carry
/// upstream identity for third-party crates; we hash basenames (not full
/// paths, not file contents) so two workspaces compiling the same crate from
/// the same registry produce the same key.
pub fn hash_rustc_args(parsed_args: &ParsedArguments, m: &mut blake3::Hasher) -> io::Result<()> {
    hash_argv(parsed_args, m);
    hash_basenames(parsed_args.externs.iter().filter_map(|p| p.file_name()), m);
    hash_basenames(
        parsed_args.staticlibs.iter().filter_map(|p| p.file_name()),
        m,
    );
    if let Some(p) = &parsed_args.target_json {
        io::copy(&mut File::open(p)?, m)?;
    }
    Ok(())
}

fn hash_argv(parsed_args: &ParsedArguments, m: &mut blake3::Hasher) {
    let target_json_present = parsed_args.target_json.is_some();
    let (mut sortables, rest): (Vec<_>, Vec<_>) = parsed_args
        .arguments
        .iter()
        .filter(|a| should_hash(a, target_json_present))
        .partition(|a| a.flag_str() == Some("--cfg"));
    // Older cargo versions emit --cfg in non-deterministic order, so sort
    // them by their on-the-wire byte representation.
    sortables.sort_by_cached_key(|a| join(a.iter_os_strings()));
    let bytes = join(
        rest.into_iter()
            .chain(sortables)
            .flat_map(|a| a.iter_os_strings()),
    );
    m.update(bytes.as_bytes());
}

/// True when `arg` should contribute to the cache key.
///
/// Excludes:
/// * `--extern`, `-L`, `--out-dir`, `--diagnostic-width`: values are
///   absolute paths or transient build-environment noise. Upstream identity
///   for externs/staticlibs is hashed separately (basenames).
/// * `--check-cfg`: lint hint, doesn't affect output bytes.
/// * `--target`: only when it points to a JSON file (whose content is
///   hashed separately). Built-in target name strings stay.
fn should_hash<T: ArgumentValue>(arg: &Argument<T>, target_json_present: bool) -> bool {
    let Some(flag) = arg.flag_str() else {
        return true;
    };
    if matches!(
        flag,
        "--extern" | "-L" | "--check-cfg" | "--out-dir" | "--diagnostic-width"
    ) {
        return false;
    }
    if target_json_present && flag == "--target" {
        return false;
    }
    true
}

fn join(parts: impl Iterator<Item = OsString>) -> OsString {
    parts.fold(OsString::new(), |mut acc, s| {
        acc.push(s);
        acc
    })
}

fn hash_basenames<'a>(names: impl Iterator<Item = &'a OsStr>, m: &mut blake3::Hasher) {
    let mut sorted: Vec<&OsStr> = names.collect();
    sorted.sort();
    for n in sorted {
        m.update(n.as_bytes());
        // NUL-terminate so adjacent names can't be confused for one longer
        // name (no rust filename can contain NUL).
        m.update(b"\0");
    }
}
