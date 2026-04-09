mod auth;
mod errors;
mod platform;
mod tls;

use abrasive_protocol::{BuildRequest, FileEntry, Manifest, Message};
use clap::builder::styling::{AnsiColor, Styles};
use clap::{CommandFactory, Parser, Subcommand};
use errors::{CliError, CliResult};
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::Deserialize;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::sync_channel;
use std::time::Duration;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command as Cmd, ExitCode},
};
use tls::WsConn;
use tungstenite::Message as WsMessage;

use crate::platform::host_triple;

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
    Setup,
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
fn print_workspace() -> CliResult<()> {
    match get_workspace()? {
        Some(ctx) => println!("{:?}, {:?}", ctx.root_dir, ctx.subdir),
        None => println!(
            "This is not an abrasive workspace. abrasive commands run from here will pass through to cargo"
        ),
    }
    Ok(())
}

/// Print the Abrasive help first, followed by the cargo help
fn print_version() {
    println!("abrasive {}", env!("CARGO_PKG_VERSION"));
    let _ = Cmd::new("cargo").arg("--version").status();
}

fn remote_setup() {
    // create the toml with an interactive menu where the user selects stuff
    // concurrent with that sync the source to the remote. hopefully by the
    // time the user is done selecting stuff the sync is already complete
    // if not it just keeps syncing until its ready.
    todo!("remote_setup")
}

fn login() -> CliResult<()> {
    if auth::saved_token().is_some() {
        println!("already authenticated (token saved at ~/.config/abrasive/token)");
        return Ok(());
    }
    auth::login()?;
    Ok(())
}

fn build_manifest(root: &Path) -> Vec<FileEntry> {
    // 1. Walk (single-threaded; ignore's parallel walker is awkward to collect from)
    let paths: Vec<PathBuf> = WalkBuilder::new(root)
        .git_ignore(true)
        .git_exclude(true)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map_or(false, |ft| ft.is_file()))
        .map(|e| e.into_path())
        .collect();

    // 2. Hash in parallel
    paths
        .par_iter()
        .filter_map(|p| {
            let rel = p.strip_prefix(root).ok()?.to_string_lossy().to_string();
            let data = fs::read(p).ok()?;
            let hash = *blake3::hash(&data).as_bytes();
            Some(FileEntry { path: rel, hash })
        })
        .collect()
}

fn ws_err(e: tungstenite::Error) -> CliError {
    match e {
        tungstenite::Error::Io(io) => io.into(),
        _ => CliError::disconnected(),
    }
}

fn send_frame(ws: &mut WsConn, msg: &Message) -> CliResult<()> {
    let payload = abrasive_protocol::serialize(msg);
    ws.send(WsMessage::Binary(payload)).map_err(ws_err)?;
    Ok(())
}

fn recv_frame(ws: &mut WsConn) -> CliResult<Message> {
    loop {
        match ws.read().map_err(ws_err)? {
            WsMessage::Binary(data) => return Ok(abrasive_protocol::deserialize(&data)?),
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            WsMessage::Close(_) => return Err(CliError::disconnected()),
            // We never send text frames; ignore stray ones from libraries.
            WsMessage::Text(_) | WsMessage::Frame(_) => continue,
        }
    }
}

fn sync_files(stream: &mut WsConn, root: &Path, team: &str, scope: &str) -> CliResult<()> {
    eprintln!("[sync] scanning files...");
    let files = build_manifest(root);
    eprintln!("[sync] {} files in manifest", files.len());

    let files_gz = Manifest::encode_files(&files);
    eprintln!(
        "[sync] manifest: {} entries, {} bytes gzipped",
        files.len(),
        files_gz.len()
    );
    send_frame(
        stream,
        &Message::Manifest(Manifest {
            team: team.to_string(),
            scope: scope.to_string(),
            files_gz,
        }),
    )?;

    // Server tells us what it needs
    let needed = match recv_frame(stream)? {
        Message::NeedFiles(paths) => paths,
        other => {
            eprintln!("[sync] unexpected message: {other:?}");
            return Err(CliError::disconnected());
        }
    };

    eprintln!("[sync] sending {} files", needed.len());

    // Pipeline: rayon workers read files from disk in parallel and push them
    // into a bounded channel; this thread drains the channel and writes to
    // the (single-writer) TLS stream. Order doesn't matter to the server.
    let (tx, rx) = sync_channel::<(String, Vec<u8>)>(32);
    let root_buf = root.to_path_buf();
    let needed_owned = needed.clone();
    let producer = std::thread::spawn(move || {
        needed_owned.par_iter().for_each_with(tx, |tx, path| {
            if let Ok(contents) = fs::read(root_buf.join(path)) {
                let _ = tx.send((path.clone(), contents));
            }
        });
    });

    for (path, contents) in rx {
        send_frame(stream, &Message::FileData { path, contents })?;
    }
    let _ = producer.join();

    send_frame(stream, &Message::SyncDone)?;

    // Wait for ack
    match recv_frame(stream)? {
        Message::SyncAck => {}
        _ => return Err(CliError::disconnected()),
    }

    eprintln!("[sync] done");
    Ok(())
}

fn try_remote(ctx: &WorkspaceContext, cargo_args: Vec<String>) -> CliResult<ExitCode> {
    // Only whitelisted cargo commands run remotely; everything else
    // (e.g. `clean`, `update`, `add`) falls through to local cargo.
    if !should_go_remote(&cargo_args) {
        return forward_args_to_local();
    }

    let token = auth::saved_token()
        .ok_or_else(|| CliError::auth("no saved token, run `abrasive-cli auth` first".into()))?;

    let addr: SocketAddr = format!("{}:{}", IP, PORT).parse().unwrap();
    let tcp =
        TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(CliError::connect)?;
    tcp.set_read_timeout(Some(Duration::from_secs(300)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut stream: WsConn = tls::connect(tcp, &token).map_err(CliError::connect)?;

    // Sync files first
    sync_files(
        &mut stream,
        &ctx.root_dir,
        &ctx.config.remote.team,
        &ctx.config.remote.scope,
    )?;

    // Send build request
    let host_platform = host_triple();
    send_frame(
        &mut stream,
        &Message::BuildRequest(BuildRequest {
            cargo_args,
            subdir: ctx.subdir.clone(),
            host_platform,
            team: ctx.config.remote.team.clone(),
            scope: ctx.config.remote.scope.clone(),
        }),
    )?;

    // Stream build output
    loop {
        let msg = recv_frame(&mut stream)?;
        match msg {
            Message::BuildStdout(data) => {
                io::stderr().write_all(b"[REMOTE] ")?;
                io::stdout().write_all(&data)?;
            }
            Message::BuildStderr(data) => {
                io::stderr().write_all(b"[REMOTE] ")?;
                io::stderr().write_all(&data)?;
            }
            Message::BuildFinished { exit_code } => {
                return Ok(ExitCode::from(exit_code));
            }
            _ => {}
        }
    }
}

#[derive(Deserialize)]
struct AbrasiveConfig {
    remote: RemoteConfig,
}

#[derive(Deserialize)]
struct RemoteConfig {
    #[allow(dead_code)]
    host: String,
    team: String,
    scope: String,
}

struct WorkspaceContext {
    root_dir: PathBuf,
    /// None if abrasive is called from the workspace root
    subdir: Option<String>,
    config: AbrasiveConfig,
}

impl WorkspaceContext {
    fn from_paths(config_path: &Path, called_from: &Path) -> CliResult<Self> {
        let root_dir = config_path
            .parent()
            .expect("abrasive.toml must have a parent directory")
            .to_path_buf();

        let subdir = relative_subdir(&root_dir, called_from)?;

        let config = fs::read_to_string(config_path).map_err(|_| CliError::no_toml())?;
        let config: AbrasiveConfig = toml::from_str(&config)?;

        Ok(Self {
            root_dir,
            subdir,
            config,
        })
    }
}

/// Helper function to get, for example, "c/d" from ("a/b", "a/b/c/d")
fn relative_subdir(project_root: &Path, called_from: &Path) -> CliResult<Option<String>> {
    let rel = match called_from.strip_prefix(project_root) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel,
        _ => return Ok(None),
    };
    let s = rel
        .to_str()
        .ok_or_else(|| CliError::invalid_path(rel.display().to_string()))?;
    Ok(Some(s.to_string()))
}

fn get_workspace() -> CliResult<Option<WorkspaceContext>> {
    let cwd = env::current_dir().map_err(CliError::no_cwd)?;
    match find_abrasive_toml(&cwd) {
        Some(config) => Ok(Some(WorkspaceContext::from_paths(&config, &cwd)?)),
        None => Ok(None),
    }
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
fn forward_args_to_local() -> CliResult<ExitCode> {
    let args: Vec<String> = env::args().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Cmd::new("cargo").args(&args).exec();
        // only reaches here if exec failed
        Err(CliError::cargo_not_found(err))
    }

    #[cfg(not(unix))]
    {
        let status = Cmd::new("cargo")
            .args(&args)
            .status()
            .map_err(CliError::cargo_not_found)?;
        Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
    }
}

fn should_go_remote(args: &[String]) -> bool {
    args.first()
        .map_or(false, |cmd| REMOTE_COMMANDS.contains(&cmd.as_str()))
}

fn run() -> CliResult<ExitCode> {
    // First, Check if we are in an abrasive workspace
    // if not forward args to local cargo
    let ctx = match get_workspace()? {
        None => return forward_args_to_local(),
        Some(ctx) => ctx,
    };

    // Things Abrasive handles
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Setup) => remote_setup(),
        Some(Command::Auth) => login()?,
        Some(Command::Version) => print_version(),
        Some(Command::Help) => print_help(),
        Some(Command::Workspace) => print_workspace()?,
        None if cli.cargo_args.is_empty() => print_help(),
        None => return try_remote(&ctx, cli.cargo_args),
    }

    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => e.exit(),
    }
}
