// Pass-through binary installed as `RUSTC_WORKSPACE_WRAPPER`. Its presence in
// the env var is what triggers cargo to use the workspace-shape rustc chain
// (`drop-point drop-point-skip rustc <args>`) for workspace members; drop-point
// detects that shape and exec's rustc directly, so this binary normally never
// runs. If detection ever fails, this transparently passes through to rustc so
// the build still succeeds (without caching).

use std::env;
use std::os::unix::process::CommandExt;
use std::process::{Command, exit};

fn main() {
    let mut args = env::args_os();
    args.next();
    let Some(rustc) = args.next() else {
        eprintln!("drop-point-skip: must be invoked as a rustc wrapper");
        exit(2);
    };
    let err = Command::new(&rustc).args(args).exec();
    eprintln!(
        "drop-point-skip: failed to exec {}: {err}",
        rustc.to_string_lossy()
    );
    exit(2);
}
