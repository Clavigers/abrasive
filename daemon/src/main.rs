mod auth;
mod constants;
mod errors;
mod slots;

use abrasive_protocol::{BuildRequest, FileEntry, Manifest, Message, PlatformTriple};
use rayon::prelude::*;
use rustls::ServerConnection;
use rustls::StreamOwned;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Instant;
use tungstenite::Message as WsMessage;
use tungstenite::WebSocket;
use tungstenite::handshake::HandshakeError;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http;

use crate::errors::{AuthError, DaemonError};
use crate::slots::{FingerprintCache, SlotGuard, SlotTable};

/// Commands that accept --target
const TARGET_COMMANDS: &[&str] = &["build", "check", "test", "bench", "clippy", "doc"];

type TlsStream = StreamOwned<ServerConnection, TcpStream>;
type WsConn = WebSocket<TlsStream>;

fn main() {
    let tls_config = load_tls_config();
    let slots = SlotTable::new();
    let fingerprints = FingerprintCache::new();
    let listener = TcpListener::bind("0.0.0.0:8400").unwrap();
    println!("abrasived TEST listening on :8400 (TLS+WS)");
    for stream in listener.incoming().flatten() {
        let config = tls_config.clone();
        let slots = slots.clone();
        let fingerprints = fingerprints.clone();
        thread::spawn(move || handle(stream, config, slots, fingerprints));
    }
}

fn load_tls_config() -> Arc<rustls::ServerConfig> {
    let cert_file = fs::File::open("server.crt").expect("cannot open server.crt");
    let key_file = fs::File::open("server.key").expect("cannot open server.key");
    let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
        .collect::<Result<_, _>>()
        .expect("invalid certs");
    let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
        .expect("cannot read key")
        .expect("no private key found");
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("bad cert/key");
    Arc::new(config)
}

fn handle(
    tcp_stream: TcpStream,
    tls_config: Arc<rustls::ServerConfig>,
    slots: SlotTable,
    fingerprints: FingerprintCache,
) {
    let peer = peer_addr(&tcp_stream);
    println!("[{peer}] connected");
    if let Err(e) = serve(tcp_stream, tls_config, &peer, &slots, &fingerprints) {
        println!("[{peer}] {e}");
    }
}

fn peer_addr(stream: &TcpStream) -> String {
    stream.peer_addr().map(|a| a.to_string()).unwrap_or_default()
}

fn serve(
    tcp_stream: TcpStream,
    tls_config: Arc<rustls::ServerConfig>,
    peer: &str,
    slots: &SlotTable,
    fingerprints: &FingerprintCache,
) -> Result<(), DaemonError> {
    let tls_stream = tls_handshake(tcp_stream, tls_config)?;
    let (mut stream, login) = ws_handshake(tls_stream, peer)?;
    let probe = expect_probe(&mut stream)?;
    let team = probe.request.team.clone();
    let scope = probe.request.scope.clone();
    let Some(slot) = slots.try_acquire(&team, &scope, &login) else {
        return reject_busy(&mut stream, peer, &team, &scope);
    };
    println!("[{peer}] acquired slot {}", slot.index);
    let workspace = setup_workspace(&slot, &team, &scope, peer)?;
    if fingerprints.matches(&slot, &team, &scope, &probe.fingerprint) {
        fast_path(&mut stream, peer, &workspace, probe.request)
    } else {
        slow_path(&mut stream, peer, &workspace, &slot, probe, fingerprints)
    }
}

fn reject_busy(
    stream: &mut WsConn,
    peer: &str,
    team: &str,
    scope: &str,
) -> Result<(), DaemonError> {
    println!("[{peer}] all slots busy for {team}/{scope}, rejecting");
    send_msg(stream, &Message::SlotsBusy)
}

fn fast_path(
    stream: &mut WsConn,
    peer: &str,
    workspace: &Path,
    request: BuildRequest,
) -> Result<(), DaemonError> {
    println!("[{peer}] fingerprint matches, skipping sync");
    send_msg(stream, &Message::ProbeAccepted)?;
    run_build(stream, peer, workspace, request);
    Ok(())
}

fn slow_path(
    stream: &mut WsConn,
    peer: &str,
    workspace: &Path,
    slot: &SlotGuard,
    probe: ProbeInfo,
    fingerprints: &FingerprintCache,
) -> Result<(), DaemonError> {
    send_msg(stream, &Message::ProbeMiss)?;
    let manifest = expect_manifest(stream)?;
    let files = manifest.decode_files()?;
    handle_sync(stream, workspace, peer, &files)?;
    fingerprints.insert(slot, &probe.request.team, &probe.request.scope, probe.fingerprint);
    run_build(stream, peer, workspace, probe.request);
    Ok(())
}

struct ProbeInfo {
    fingerprint: [u8; 32],
    request: BuildRequest,
}

fn expect_probe(stream: &mut WsConn) -> Result<ProbeInfo, DaemonError> {
    match recv_msg(stream)? {
        Message::Probe { fingerprint, request } => Ok(ProbeInfo { fingerprint, request }),
        other => Err(DaemonError::UnexpectedMessage {
            expected: "Probe",
            got: other.kind().to_string(),
        }),
    }
}

fn tls_handshake(
    tcp: TcpStream,
    config: Arc<rustls::ServerConfig>,
) -> Result<TlsStream, DaemonError> {
    let conn = ServerConnection::new(config)?;
    Ok(StreamOwned::new(conn, tcp))
}

fn ws_handshake(tls_stream: TlsStream, peer: &str) -> Result<(WsConn, String), DaemonError> {
    let auth_result: RefCell<Option<Result<String, AuthError>>> = RefCell::new(None);
    let ws_result = tungstenite::accept_hdr(tls_stream, |req: &Request, resp: Response| {
        authenticate(req, resp, &auth_result)
    });
    let stream_or_err: Result<WsConn, DaemonError> = match ws_result {
        Ok(s) => Ok(s),
        Err(HandshakeError::Failure(e)) => Err(DaemonError::WebSocket(e)),
        Err(HandshakeError::Interrupted(_)) => Err(DaemonError::WsHandshakeInterrupted),
    };
    let login = auth_result
        .into_inner()
        .transpose()?
        .ok_or(AuthError::NoBearerToken)?;
    println!("[{peer}] authenticated as github user '{login}'");
    Ok((stream_or_err?, login))
}

fn authenticate(
    req: &Request,
    resp: Response,
    auth_result: &RefCell<Option<Result<String, AuthError>>>,
) -> Result<Response, ErrorResponse> {
    let result = match bearer_token(req) {
        None => Err(AuthError::NoBearerToken),
        Some(t) => auth::validate(t),
    };
    let response = match &result {
        Ok(_) => Ok(resp),
        Err(_) => Err(unauthorized()),
    };
    *auth_result.borrow_mut() = Some(result);
    response
}

fn bearer_token(req: &Request) -> Option<&str> {
    req.headers()
        .get("Authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|t| !t.is_empty())
}

fn unauthorized() -> ErrorResponse {
    let mut err: ErrorResponse = http::Response::new(Some("unauthorized".to_string()));
    *err.status_mut() = http::StatusCode::UNAUTHORIZED;
    err
}

fn expect_manifest(stream: &mut WsConn) -> Result<Manifest, DaemonError> {
    match recv_msg(stream)? {
        Message::Manifest(m) => Ok(m),
        other => Err(DaemonError::UnexpectedMessage {
            expected: "Manifest",
            got: other.kind().to_string(),
        }),
    }
}

fn recv_msg(ws: &mut WsConn) -> Result<Message, DaemonError> {
    loop {
        match ws.read()? {
            WsMessage::Binary(data) => break Ok(abrasive_protocol::deserialize(&data)?),
            WsMessage::Close(_) => break Err(DaemonError::ClientClosed),
            _ => continue,
        }
    }
}

fn send_msg(ws: &mut WsConn, msg: &Message) -> Result<(), DaemonError> {
    let payload = abrasive_protocol::serialize(msg);
    ws.send(WsMessage::Binary(payload))?;
    Ok(())
}

fn setup_workspace(
    slot: &SlotGuard,
    team: &str,
    scope: &str,
    peer: &str,
) -> Result<PathBuf, DaemonError> {
    let workspace = slot.workspace(team, scope);
    fs::create_dir_all(&workspace)?;
    ensure_target_on_tmpfs(&workspace, slot, team, scope, peer);
    Ok(workspace)
}

fn handle_sync(
    stream: &mut WsConn,
    workspace: &Path,
    peer: &str,
    client_files: &[FileEntry],
) -> Result<(), DaemonError> {
    let local = local_manifest_timed(workspace, peer);
    let needed = needed_files(client_files, &local);
    delete_stale(workspace, &local, client_files);
    println!("[{peer}] sync: need {}/{} files", needed.len(), client_files.len());
    send_msg(stream, &Message::NeedFiles(needed))?;
    receive_files(stream, workspace)?;
    println!("[{peer}] sync complete");
    send_msg(stream, &Message::SyncAck)
}

fn local_manifest_timed(workspace: &Path, peer: &str) -> HashMap<String, [u8; 32]> {
    let t0 = Instant::now();
    let local = local_manifest(workspace);
    println!(
        "[{peer}] local_manifest: {} files in {:?}",
        local.len(),
        t0.elapsed()
    );
    local
}

fn needed_files(client: &[FileEntry], local: &HashMap<String, [u8; 32]>) -> Vec<String> {
    client
        .iter()
        .filter(|f| local.get(&f.path) != Some(&f.hash))
        .map(|f| f.path.clone())
        .collect()
}

fn delete_stale(workspace: &Path, local: &HashMap<String, [u8; 32]>, client: &[FileEntry]) {
    let client_paths: HashSet<&str> = client.iter().map(|f| f.path.as_str()).collect();
    for local_path in local.keys() {
        if !client_paths.contains(local_path.as_str()) {
            let _ = fs::remove_file(workspace.join(local_path));
        }
    }
}

fn receive_files(stream: &mut WsConn, workspace: &Path) -> Result<(), DaemonError> {
    loop {
        match recv_msg(stream)? {
            Message::FileData { path, contents } => write_file(workspace, &path, &contents)?,
            Message::SyncDone => break Ok(()),
            other => {
                break Err(DaemonError::UnexpectedMessage {
                    expected: "FileData or SyncDone",
                    got: other.kind().to_string(),
                });
            }
        }
    }
}

fn write_file(workspace: &Path, path: &str, contents: &[u8]) -> Result<(), DaemonError> {
    let dest = workspace.join(path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, contents)?;
    Ok(())
}

fn local_manifest(workspace: &Path) -> HashMap<String, [u8; 32]> {
    if !workspace.exists() {
        HashMap::new()
    } else {
        hash_paths(workspace, &walk_workspace(workspace))
    }
}

fn walk_workspace(workspace: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == "target"))
        .map(|e| e.into_path())
        .collect()
}

fn hash_paths(workspace: &Path, paths: &[PathBuf]) -> HashMap<String, [u8; 32]> {
    paths
        .par_iter()
        .filter_map(|p| {
            let rel = p.strip_prefix(workspace).ok()?.to_string_lossy().to_string();
            let hash = hash_file(p)?;
            Some((rel, hash))
        })
        .collect()
}

fn hash_file(path: &Path) -> Option<[u8; 32]> {
    let data = fs::read(path).ok()?;
    Some(*blake3::hash(&data).as_bytes())
}

/// Make `<workspace>/target` live on tmpfs (/dev/shm) so cargo's
/// write-heavy build artifacts skip the disk entirely. We do this with
/// a symlink rather than a mount so we don't need root or namespacing.
fn ensure_target_on_tmpfs(workspace: &Path, slot: &SlotGuard, team: &str, scope: &str, peer: &str) {
    let tmpfs_target = slot.tmpfs_target(team, scope);
    let target_link = workspace.join("target");
    if let Err(e) = fs::create_dir_all(&tmpfs_target) {
        println!("[{peer}] tmpfs target unavailable ({e}); falling back to disk");
        return;
    }
    wire_up_target_symlink(&target_link, &tmpfs_target, peer);
}

fn wire_up_target_symlink(target_link: &Path, tmpfs_target: &Path, peer: &str) {
    match fs::symlink_metadata(target_link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            warn_if_symlink_mismatch(target_link, tmpfs_target, peer)
        }
        Ok(_) => println!(
            "[{peer}] target/ is a real directory; leaving alone (delete it manually to enable tmpfs)"
        ),
        Err(_) => create_target_symlink(target_link, tmpfs_target, peer),
    }
}

fn warn_if_symlink_mismatch(target_link: &Path, tmpfs_target: &Path, peer: &str) {
    if fs::read_link(target_link).ok().as_deref() != Some(tmpfs_target) {
        println!("[{peer}] target/ is a symlink to something else; leaving alone");
    }
}

#[cfg(unix)]
fn create_target_symlink(target_link: &Path, tmpfs_target: &Path, peer: &str) {
    if let Err(e) = std::os::unix::fs::symlink(tmpfs_target, target_link) {
        println!(
            "[{peer}] failed to symlink target -> {}: {e}",
            tmpfs_target.display()
        );
    } else {
        println!("[{peer}] target/ -> {}", tmpfs_target.display());
    }
}

#[cfg(not(unix))]
fn create_target_symlink(_target_link: &Path, _tmpfs_target: &Path, _peer: &str) {}

fn run_build(stream: &mut WsConn, peer: &str, workspace: &Path, req: BuildRequest) {
    let cargo_args = build_cargo_args(req.cargo_args, req.host_platform);
    let cd_target = build_dir(workspace, req.subdir.as_deref());
    println!(
        "[{peer}] mold -run cargo +nightly {} (in {})",
        cargo_args.join(" "),
        cd_target.display()
    );
    match spawn_cargo(&cargo_args, &cd_target) {
        Ok(child) => forward_build_output(stream, child, peer),
        Err(e) => send_spawn_failure(stream, &e),
    }
}

fn build_cargo_args(args: Vec<String>, platform: PlatformTriple) -> Vec<String> {
    let (args, _run_it) = rewrite_run_as_build(args);
    amend_args_with_platform(args, platform)
}

fn build_dir(workspace: &Path, subdir: Option<&str>) -> PathBuf {
    match subdir {
        Some(rel) => workspace.join(rel),
        None => workspace.to_path_buf(),
    }
}

fn spawn_cargo(args: &[String], cd: &Path) -> std::io::Result<Child> {
    Command::new("mold")
        .arg("-run")
        .arg("cargo")
        .arg("+nightly")
        .args(args)
        .current_dir(cd)
        // Override [profile.dev] debug = "line-tables-only" without
        // touching the user's Cargo.toml. Backtraces still work; rustc
        // skips most DWARF generation. Big win on cold builds.
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
        // Use the cranelift codegen backend for the dev profile.
        // Cranelift is much faster than LLVM at producing unoptimized
        // code, at the cost of slower runtime — perfect for dev builds.
        // Requires nightly toolchain + rustc-codegen-cranelift-preview
        // component installed on the remote.
        .env("CARGO_UNSTABLE_CODEGEN_BACKEND", "true")
        .env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn send_spawn_failure(stream: &mut WsConn, e: &std::io::Error) {
    let msg = format!("failed to spawn cargo: {e}\n").into_bytes();
    let _ = send_msg(stream, &Message::BuildStderr(msg));
    let _ = send_msg(stream, &Message::BuildFinished { exit_code: 1 });
}

fn forward_build_output(stream: &mut WsConn, mut child: Child, peer: &str) {
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Message>();
    spawn_pipe_reader(stdout, tx.clone(), Message::BuildStdout);
    spawn_pipe_reader(stderr, tx, Message::BuildStderr);
    for msg in rx {
        let _ = send_msg(stream, &msg);
    }
    let exit_code = child.wait().map(|s| s.code().unwrap_or(1) as u8).unwrap_or(1);
    let _ = send_msg(stream, &Message::BuildFinished { exit_code });
    let _ = stream.close(None);
    println!("[{peer}] done");
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    mut reader: R,
    tx: Sender<Message>,
    wrap: fn(Vec<u8>) -> Message,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = tx.send(wrap(buf[..n].to_vec()));
                }
            }
        }
    });
}

fn rewrite_run_as_build(args: Vec<String>) -> (Vec<String>, bool) {
    if args.first().map_or(true, |cmd| cmd != "run") {
        return (args, false);
    }
    let mut out = vec!["build".to_string()];
    for arg in args.into_iter().skip(1) {
        if arg == "--" {
            break;
        }
        out.push(arg);
    }
    (out, true)
}

fn amend_args_with_platform(mut args: Vec<String>, platform: PlatformTriple) -> Vec<String> {
    let accepts_target = args
        .first()
        .map_or(false, |cmd| TARGET_COMMANDS.contains(&cmd.as_str()));
    let already_has_target = args
        .iter()
        .any(|a| a == "--target" || a.starts_with("--target="));
    if accepts_target && !already_has_target {
        args.push("--target".to_string());
        args.push(platform.as_cargo_target_string());
    }
    args
}
