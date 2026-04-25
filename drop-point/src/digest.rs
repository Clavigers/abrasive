use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::iter;
use std::os::unix::ffi::OsStrExt;

use crate::rustc_args::{IntoArg, ParsedArguments};

/// Mix everything that affects the rlib's bytes into the hasher. Cargo's
/// metadata hash on extern paths and the target/profile flags in argv carry
/// upstream identity for third-party crates; we hash basenames (not full
/// paths, not file contents) so two workspaces compiling the same crate from
/// the same registry produce the same key.
pub fn hash_rustc_args(parsed_args: &ParsedArguments, m: &mut blake3::Hasher) -> io::Result<()> {
    hash_argv(parsed_args, m);
    hash_basenames(parsed_args.externs.iter().filter_map(|p| p.file_name()), m);
    hash_basenames(parsed_args.staticlibs.iter().filter_map(|p| p.file_name()), m);
    if let Some(p) = &parsed_args.target_json {
        io::copy(&mut File::open(p)?, m)?;
    }
    Ok(())
}

fn hash_argv(parsed_args: &ParsedArguments, m: &mut blake3::Hasher) {
    // TODO: this doesn't produce correct bytes for Concatenated args, but
    // parse_arguments normalizes everything to Separated, so it's not
    // reachable today. Switch to iter_os_strings if that ever changes.
    let os_string_arguments: Vec<(OsString, Option<OsString>)> = parsed_args
        .arguments
        .iter()
        .map(|arg| {
            (
                arg.to_os_string(),
                arg.get_data().cloned().map(IntoArg::into_arg_os_string),
            )
        })
        .collect();

    // TODO: there will be full paths here, it would be nice to
    // normalize them so we can get cross-machine cache hits.
    // A few argument types are not passed in a deterministic order
    // by cargo: --extern, -L, --cfg. We'll filter those out, sort them,
    // and append them to the rest of the arguments.
    let args = {
        let (mut sortables, rest): (Vec<_>, Vec<_>) = os_string_arguments
            .iter()
            // We exclude a few arguments from the hash:
            //   -L, --extern, --out-dir, --diagnostic-width
            // These contain paths which aren't relevant to the output, and the compiler inputs
            // in those paths (rlibs and static libs used in the compilation) are used as hash
            // inputs below.
            .filter(|&(arg, _)| {
                !(arg == "--extern"
                    || arg == "-L"
                    || arg == "--check-cfg"
                    || arg == "--out-dir"
                    || arg == "--diagnostic-width")
            })
            // We also exclude `--target` if it specifies a path to a .json file. The file content
            // is used as hash input below.
            // If `--target` specifies a string, it continues to be hashed as part of the arguments.
            .filter(|&(arg, _)| parsed_args.target_json.is_none() || arg != "--target")
            // A few argument types were not passed in a deterministic order
            // by older versions of cargo: --extern, -L, --cfg. We'll filter the rest of those
            // out, sort them, and append them to the rest of the arguments.
            .partition(|&(arg, _)| arg == "--cfg");
        sortables.sort();
        rest.into_iter()
            .chain(sortables)
            .flat_map(|(arg, val)| iter::once(arg).chain(val.as_ref()))
            .fold(OsString::new(), |mut a, b| {
                a.push(b);
                a
            })
    };
    m.update(args.as_bytes());
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
