use clap::{CommandFactory, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::process::Command as Cmd;

/// Message sent from the CLI to the build server
#[derive(Debug, Serialize, Deserialize)]
struct BuildRequest {
    cargo_args: Vec<String>,
}

#[derive(Parser)]
#[command(name = "abrasive", disable_version_flag = true, disable_help_flag = true, trailing_var_arg = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Args to forward to cargo
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    cargo_args: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize abrasive for this project
    RemoteInit, 
    /// Authenticate with the build server
    #[command(visible_alias = "login")]
    Auth,
    /// Print abrasive and cargo versions
    #[command(name = "--version", aliases = ["-V"])]
    Version,
    /// Get help for abrasive and cargo 
    #[command(name = "--help", aliases = ["-h"])]
    Help,
}

/// Print the Abrasive help first, followed by the cargo help
fn print_help() {
    print!("{}", Cli::command().render_help());
    println!();
    let _ = Cmd::new("cargo").arg("--help").status();
}

/// Print the Abrasive help first, followed by the cargo help
fn print_version() {
    println!("abrasive {}", env!("CARGO_PKG_VERSION"));
    let _ = Cmd::new("cargo").arg("--version").status();
}

fn remote_init() {
    todo!("remote_intit")
}

fn login() {
    todo!("login")
}

fn forward_args_to_remote(cargo_args: Vec<String>) {
    let request = BuildRequest { cargo_args };
    let body = bincode::serialize(&request).expect("failed to serialize request");

    // TODO: read host from config, api key from ~/.abrasive/credentials
    let host = "http://157.180.55.180:8400";

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(format!("{host}/build"))
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send();

    match response {
        Ok(mut resp) => {
            let mut buf = [0u8; 4096];
            loop {
                match resp.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => { let _ = io::stdout().write_all(&buf[..n]); }
                    Err(e) => { eprintln!("Read error: {e}"); break; }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to build server: {e}");
        }
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::RemoteInit) => remote_init(),
        Some(Command::Auth) => login(),
        Some(Command::Version) => print_version(),
        Some(Command::Help) => print_help(),
        None if cli.cargo_args.is_empty() => print_help(),
        None => forward_args_to_remote(cli.cargo_args),
    }
}
