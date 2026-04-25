use env_logger::Env;
use log::{debug, error, info};
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::process::{Command, exit};

mod rustc_args;
use rustc_args::{ParseOutcome, parse_arguments};

fn main() {
    init_logger();
    let (rustc, rest) = parse_args();
    log_command(&rustc, &rest);
    try_parse_rustc(&rest);
    run_rustc(&rustc, &rest);
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

fn log_command(rustc: &OsStr, rest: &[OsString]) {
    let mut cmdline = rustc.to_string_lossy().into_owned();
    for a in rest {
        cmdline.push(' ');
        cmdline.push_str(&a.to_string_lossy());
    }
    debug!("  {cmdline}");
}

fn try_parse_rustc(rest: &[OsString]) {
    let cwd = match env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            error!("drop-point: cwd unavailable: {e}");
            return;
        }
    };
    match parse_arguments(rest, &cwd) {
        ParseOutcome::Ok(_) => info!("parse: Ok"),
        ParseOutcome::CannotCache(why, None) => info!("parse: CannotCache({why})"),
        ParseOutcome::CannotCache(why, Some(extra)) => {
            info!("parse: CannotCache({why}, {extra})")
        }
        ParseOutcome::NotCompilation => info!("parse: NotCompilation"),
    }
}

fn run_rustc(rustc: &OsStr, rest: &[OsString]) -> ! {
    let status = match Command::new(rustc).args(rest).status() {
        Ok(s) => s,
        Err(e) => {
            error!(
                "drop-point: failed to spawn {}: {e}",
                rustc.to_string_lossy()
            );
            exit(2);
        }
    };
    exit(status.code().unwrap_or(1));
}
