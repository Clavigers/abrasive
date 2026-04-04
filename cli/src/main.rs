use clap::{CommandFactory, Parser, Subcommand};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command as Cmd, ExitCode, Stdio, exit},
};

const HOST: &'static str = "http://157.180.55.180:8400";

use clap::builder::styling::{AnsiColor, Styles};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Yellow.on_default().bold())
    .literal(AnsiColor::Yellow.on_default().bold())
    .placeholder(AnsiColor::Yellow.on_default());

/// Message sent from the CLI to the build server
#[derive(Debug, Serialize, Deserialize)]
struct BuildRequest {
    cargo_args: Vec<String>,
}

#[derive(Parser)]
#[command(name = "abrasive", disable_version_flag = true, disable_help_flag = true, trailing_var_arg = true, styles = STYLES)]
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
    /// Print the workspace info
    #[command(name = "workspace", aliases = ["-w"])]
    Workspace,
}

/// Print the Abrasive help first, followed by the cargo help
fn print_help() {
    println!("ABRASIVE {}\n", env!("CARGO_PKG_VERSION"));
    let _ = Cli::command().color(clap::ColorChoice::Always).print_help();
    println!("\n");
    let _ = Cmd::new("cargo").arg("--help").status();
}

/// Print the Abrasive workspace info
fn print_workspace() {
    match get_workspace() {
        Some(ctx) => println!("{:?}, {:?}", ctx.root_dir, ctx.called_from_subdir),
        None => println!(
            "This is not an abrasive workspace. abrasive commands run from here will pass through to cargo"
        ),
    }
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

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(format!("{HOST}/build"))
        .header("Content-Type", "application/octet-stream")
        .body(body)
        .send();

    match response {
        Ok(mut resp) => {
            let mut buf = [0u8; 4096];
            loop {
                match resp.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = io::stdout().write_all(&buf[..n]);
                    }
                    Err(e) => {
                        eprintln!("Read error: {e}");
                        break;
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to connect to build server: {e}");
        }
    }
}

struct WorkspaceContext {
    root_dir: PathBuf,
    // This will be None if abrasive is called from the workspace root
    called_from_subdir: Option<PathBuf>,
}

impl WorkspaceContext {
    /// The point of this is other functions will want what dir abrasive was called
    /// from relative to the workspace root (abrasive.toml)
    fn from_paths(config: &Path, called_from: &Path) -> Self {
        let parent = config.parent().unwrap();
        let subdir = relative_subdir(parent, called_from);
        Self {
            root_dir: parent.to_path_buf(),
            called_from_subdir: subdir,
        }
    }
}

/// Helper function to get, for example, "c/d" from ("a/b", "a/b/c/d")
fn relative_subdir(project_root: &Path, called_from: &Path) -> Option<PathBuf> {
    called_from.strip_prefix(project_root).ok().and_then(|rel| {
        if rel.as_os_str().is_empty() {
            None
        } else {
            Some(rel.to_path_buf())
        }
    })
}

fn get_workspace() -> Option<WorkspaceContext> {
    let cwd = get_cwd();
    let config = find_abrasive_toml(&cwd);
    config.map(|p| WorkspaceContext::from_paths(&p, &cwd))
}

/// Just Error and Exit right away if we cannot find cwd.
fn get_cwd() -> PathBuf {
    env::current_dir().unwrap_or_else(|e| {
        error!("Cannot determine current directory: {e}");
        exit(1);
    })
}

/// Walk up from start looking for abrasive.toml. Returns the full
/// path to abrasive.toml (including the "abrasive.toml" part)
fn find_abrasive_toml(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("abrasive.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        // mutate dir into parent dir. If there is no parent dir
        // just return None.
        if !dir.pop() {
            return None;
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
        Some(Command::Workspace) => print_workspace(),
        None if cli.cargo_args.is_empty() => print_help(),
        None => forward_args_to_remote(cli.cargo_args),
    }
}
