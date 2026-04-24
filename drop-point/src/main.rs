use std::env;
use std::process::{Command, exit};

fn main() {
    let mut args = env::args_os();
    args.next();
    let Some(compiler) = args.next() else {
        eprintln!("drop-point: missing compiler argument");
        exit(2);
    };
    let rest: Vec<_> = args.collect();

    eprintln!("hello from drop-point");

    let status = match Command::new(&compiler).args(&rest).status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "drop-point: failed to spawn {}: {e}",
                compiler.to_string_lossy()
            );
            exit(2);
        }
    };

    exit(status.code().unwrap_or(1));
}
