mod errors;

use abrasive_protocol::{BuildRequest, Header, Message, decode, encode};
use clap::builder::styling::{AnsiColor, Styles};
use clap::{CommandFactory, Parser, Subcommand};
use errors::{CliError, CliResult};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use std::{
    env,
    path::{Path, PathBuf},
    process::{Command as Cmd, ExitCode},
};

const IP: &str = "157.180.55.180";
const PORT: u16 = 8400;
const REMOTE_COMMANDS: &[&str] = &["build", "run", "test", "bench", "check", "clippy", "doc"];

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default().bold())
    .usage(AnsiColor::Yellow.on_default().bold())
    .literal(AnsiColor::Yellow.on_default().bold())
    .placeholder(AnsiColor::Yellow.on_default());

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

fn forward_args_to_remote(ctx: &WorkspaceContext, cargo_args: Vec<String>) -> ExitCode {
    eprintln!("REMOTE:");
    match try_remote(ctx, cargo_args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            e.exit_code()
        }
    }
}

fn try_remote(ctx: &WorkspaceContext, cargo_args: Vec<String>) -> CliResult<ExitCode> {
    let addr: SocketAddr = format!("{}:{}", IP, PORT).parse().unwrap();
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(CliError::Connect)?;
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    // TODO: sync files first

    let subdir = ctx
        .called_from_subdir
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let frame = encode(&Message::BuildRequest(BuildRequest { cargo_args, subdir }));
    stream.write_all(&frame)?;

    loop {
        let mut header_buf = [0u8; Header::SIZE];
        stream
            .read_exact(&mut header_buf)
            .map_err(|_| CliError::Disconnected)?;
        let header = Header::from_bytes(&header_buf);
        let mut raw = vec![0u8; Header::SIZE + header.length as usize];
        raw[..Header::SIZE].copy_from_slice(&header_buf);
        stream.read_exact(&mut raw[Header::SIZE..])?;
        let frame = decode(&raw)?;

        match frame.message {
            Message::BuildOutput(data) => {
                io::stdout().write_all(&data)?;
            }
            Message::BuildFinished { exit_code } => {
                return Ok(ExitCode::from(exit_code));
            }
            _ => {}
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

fn get_cwd() -> PathBuf {
    env::current_dir().unwrap_or_else(|e| {
        eprintln!("cannot determine current directory: {e}");
        std::process::exit(1);
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

/// Transparent on unix, probably close enough on windows
fn forward_args_to_local() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Cmd::new("cargo").args(&args).exec();
        // only reaches here if exec failed
        eprintln!("{err}");
        ExitCode::from(127)
    }

    #[cfg(not(unix))]
    {
        let status = Cmd::new("cargo")
            .args(&args)
            .status()
            .expect("cargo not found");
        ExitCode::from(status.code().unwrap_or(1) as u8)
    }
}

fn should_go_remote(args: &[String]) -> bool {
    args.first()
        .map_or(false, |cmd| REMOTE_COMMANDS.contains(&cmd.as_str()))
}

fn main() -> ExitCode {
    // First, Check if we are in an abrasive workspace
    // if not forward args to local cargo
    let ctx = match get_workspace() {
        None => return forward_args_to_local(),
        Some(ctx) => ctx,
    };

    // Check if the command is in the whitelist: REMOTE_COMMANDS
    // only whitelisted commands will be run remotely, the rest
    // uses local cargo 
    let raw_args: Vec<String> = env::args().skip(1).collect();
    if !should_go_remote(&raw_args) {
        return forward_args_to_local();
    }

    // Things Abrasive handles
    let cli = Cli::parse();
    match cli.command {
        Some(Command::RemoteInit) => remote_init(),
        Some(Command::Auth) => login(),
        Some(Command::Version) => print_version(),
        Some(Command::Help) => print_help(),
        Some(Command::Workspace) => print_workspace(),
        None if cli.cargo_args.is_empty() => print_help(),
        None => return forward_args_to_remote(&ctx, cli.cargo_args),
    }

    ExitCode::SUCCESS
}
