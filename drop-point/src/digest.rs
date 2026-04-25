use std::ffi::OsString;
use std::iter;
use std::os::unix::ffi::OsStrExt;

use crate::rustc_args::{IntoArg, ParsedArguments};

pub fn hash_rustc_args(parsed_args: &ParsedArguments, m: &mut blake3::Hasher) {
    // TODO: this doesn't produce correct arguments if they should be concatenated - should use iter_os_strings
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
