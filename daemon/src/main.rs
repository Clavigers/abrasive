mod auth;
mod constants;
mod errors;
mod slots;

use abrasive_protocol::{BuildRequest, FileEntry, Manifest, Message, PlatformTriple, SpeculativeSync};
use rayon::prelude::*;
use rustls::ServerConnection;
use rustls::StreamOwned;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread;
use std::thread::JoinHandle;
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
    match serve(tcp_stream, tls_config, &peer, &slots, &fingerprints) {
        Ok(()) | Err(DaemonError::ClientClosed) => println!("[{peer}] disconnected"),
        Err(e) => println!("[{peer}] {e}"),
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
    loop {
        match serve_one_request(&mut stream, peer, &login, slots, fingerprints) {
            Ok(()) => continue,
            Err(DaemonError::ClientClosed) => break Ok(()),
            Err(DaemonError::WebSocket(tungstenite::Error::Io(ref e)))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break Ok(()),
            Err(e) => break Err(e),
        }
    }
}

fn serve_one_request(
    stream: &mut WsConn,
    peer: &str,
    login: &str,
    slots: &SlotTable,
    fingerprints: &FingerprintCache,
) -> Result<(), DaemonError> {
    match recv_msg(stream)? {
        Message::Probe { fingerprint, request, speculative } => serve_build(
            stream,
            peer,
            login,
            slots,
            fingerprints,
            ProbeInfo { fingerprint, request, speculative },
        ),
        Message::TipRequest => serve_tip(stream),
        other => Err(DaemonError::UnexpectedMessage {
            expected: "Probe or TipRequest",
            got: other.kind().to_string(),
        }),
    }
}

fn serve_build(
    stream: &mut WsConn,
    peer: &str,
    login: &str,
    slots: &SlotTable,
    fingerprints: &FingerprintCache,
    probe: ProbeInfo,
) -> Result<(), DaemonError> {
    let team = probe.request.team.clone();
    let scope = probe.request.scope.clone();
    let Some(slot) = slots.try_acquire(&team, &scope, login) else {
        return reject_busy(stream, peer, &team, &scope);
    };
    println!("[{peer}] acquired slot {}", slot.index);
    let workspace = setup_workspace(&slot, &team, &scope)?;
    if fingerprints.matches(&slot, &team, &scope, &probe.fingerprint) {
        fast_path(stream, peer, &workspace, probe.request)
    } else {
        slow_path(stream, peer, &workspace, &slot, probe, fingerprints)
    }
}

fn serve_tip(stream: &mut WsConn) -> Result<(), DaemonError> {
    send_msg(stream, &Message::Tip(pick_a_tip().to_string()))
}

fn pick_a_tip() -> &'static str {
    "logging stuff"
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
    match probe.speculative {
        Some(spec) => speculative_sync(stream, peer, workspace, &spec)?,
        None => legacy_sync(stream, peer, workspace)?,
    }
    fingerprints.insert(slot, &probe.request.team, &probe.request.scope, probe.fingerprint);
    run_build(stream, peer, workspace, probe.request);
    Ok(())
}

/// New speculative-sync path: the client already sent the manifest and a
/// guess at the files we'll need. If the guess is complete we skip to
/// SyncAck in one round-trip; otherwise we send NeedFiles for the
/// stragglers and finish via the legacy FileData → SyncAck flow.
fn speculative_sync(
    stream: &mut WsConn,
    peer: &str,
    workspace: &Path,
    spec: &SpeculativeSync,
) -> Result<(), DaemonError> {
    let files = spec.manifest.decode_files()?;
    write_bundled_files(workspace, &spec.files)?;
    let local = local_manifest(workspace);
    let needed = needed_files(&files, &local);
    delete_stale(workspace, &local, &files);
    if needed.is_empty() {
        println!("[{peer}] speculative sync complete ({} files bundled)", spec.files.len());
        send_msg(stream, &Message::SyncAck)
    } else {
        println!(
            "[{peer}] speculative partial: {} bundled, {} still needed",
            spec.files.len(),
            needed.len()
        );
        send_msg(stream, &Message::NeedFiles(needed))?;
        receive_files(stream, workspace)?;
        send_msg(stream, &Message::SyncAck)
    }
}

fn write_bundled_files(
    workspace: &Path,
    bundled: &[(String, Vec<u8>)],
) -> Result<(), DaemonError> {
    for (path, contents) in bundled {
        let full = workspace.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full, contents)?;
    }
    Ok(())
}

/// Legacy path for clients that don't bundle speculative data.
fn legacy_sync(
    stream: &mut WsConn,
    peer: &str,
    workspace: &Path,
) -> Result<(), DaemonError> {
    send_msg(stream, &Message::ProbeMiss)?;
    let manifest = expect_manifest(stream)?;
    let files = manifest.decode_files()?;
    handle_sync(stream, workspace, peer, &files)?;
    Ok(())
}

struct ProbeInfo {
    fingerprint: [u8; 32],
    request: BuildRequest,
    speculative: Option<SpeculativeSync>,
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
    println!("[{peer}] authenticated as user {login}");
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
) -> Result<PathBuf, DaemonError> {
    let workspace = slot.workspace(team, scope);
    fs::create_dir_all(&workspace)?;
    let target_link = workspace.join("target");
    if let Ok(meta) = fs::symlink_metadata(&target_link) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&target_link);
        }
    }
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

fn run_build(stream: &mut WsConn, peer: &str, workspace: &Path, req: BuildRequest) {
    if req.cargo_args.first().map_or(false, |c| c == "nop") {
        println!("[{peer}] nop (sync only)");
        let _ = send_msg(stream, &Message::BuildFinished { exit_code: 0 });
        return;
    }
    if req.cargo_args.first().map_or(false, |c| c == "clean") {
        let expunge = req.cargo_args.iter().any(|a| a == "--expunge");
        handle_clean(stream, peer, workspace, expunge);
        return;
    }
    let (cargo_args, run_it) = build_cargo_args(req.cargo_args, req.host_platform);
    let cd_target = build_dir(workspace, req.subdir.as_deref());
    println!(
        "[{peer}] mold -run cargo +nightly {} (in {})",
        cargo_args.join(" "),
        cd_target.display()
    );
    match spawn_cargo(&cargo_args, &cd_target) {
        Ok(child) if run_it => forward_run_output(stream, child, peer),
        Ok(child) => forward_build_output(stream, child, peer),
        Err(e) => send_spawn_failure(stream, &e),
    }
}

fn build_cargo_args(args: Vec<String>, platform: PlatformTriple) -> (Vec<String>, bool) {
    let (args, run_it) = rewrite_run_as_build(args);
    let mut args = amend_args_with_platform(args, platform);
    if run_it {
        // Ask cargo to emit structured build messages on stdout so we can
        // extract the produced executable's path. Diagnostics stay rendered
        // on stderr so the client still sees normal colored output.
        args.push("--message-format=json-render-diagnostics".to_string());
    }
    (args, run_it)
}

fn build_dir(workspace: &Path, subdir: Option<&str>) -> PathBuf {
    match subdir {
        Some(rel) => workspace.join(rel),
        None => workspace.to_path_buf(),
    }
}

fn spawn_cargo(args: &[String], cd: &Path) -> std::io::Result<Child> {
    let mut cmd = Command::new("mold");
    cmd.arg("-run")
        .arg("cargo")
        .arg("+nightly")
        .args(args)
        .current_dir(cd)
        // Override [profile.dev] debug = "line-tables-only" without
        // touching the user's Cargo.toml. Backtraces still work; rustc
        // skips most DWARF generation. Big win on cold builds.
        .env("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
        // Strip the remaining debug info at link time. Smaller binary
        // ships faster when `run` ships the artifact back to the client;
        // trade-off is panic backtraces lose line numbers.
        .env("CARGO_PROFILE_DEV_STRIP", "debuginfo")
        // Use the cranelift codegen backend for the dev profile.
        // Cranelift is much faster than LLVM at producing unoptimized
        // code, at the cost of slower runtime, perfect for dev builds.
        // Requires nightly toolchain + rustc-codegen-cranelift-preview
        // component installed on the remote.
        .env("CARGO_UNSTABLE_CODEGEN_BACKEND", "true")
        .env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "cranelift")
        // Keep cargo's ANSI color output even though stdout/stderr
        // are piped here, the client re-renders into its own TTY.
        .env("CARGO_TERM_COLOR", "always")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn()
}

fn send_spawn_failure(stream: &mut WsConn, e: &std::io::Error) {
    let msg = format!("failed to spawn cargo: {e}\n").into_bytes();
    let _ = send_msg(stream, &Message::BuildStderr(msg));
    let _ = send_msg(stream, &Message::BuildFinished { exit_code: 1 });
}

fn handle_clean(stream: &mut WsConn, peer: &str, workspace: &Path, expunge: bool) {
    report_clean(stream, peer, "clean", clean_target(workspace));
    if expunge {
        report_clean(stream, peer, "expunge", expunge_drop_point_cache());
    }
    let _ = send_msg(stream, &Message::BuildFinished { exit_code: 0 });
}

fn report_clean(
    stream: &mut WsConn,
    peer: &str,
    label: &str,
    res: std::io::Result<(usize, u64)>,
) {
    match res {
        Ok((files, bytes)) => {
            let mib = bytes as f64 / (1024.0 * 1024.0);
            let line = format!("     {label}: removed {files} files, {mib:.1}MiB total\n");
            let _ = send_msg(stream, &Message::BuildStderr(line.into_bytes()));
            println!("[{peer}] {label}: removed {files} files, {bytes} bytes");
        }
        Err(e) => {
            let msg = format!("{label} failed: {e}\n").into_bytes();
            let _ = send_msg(stream, &Message::BuildStderr(msg));
            println!("[{peer}] {label} failed: {e}");
        }
    }
}

fn clean_target(workspace: &Path) -> std::io::Result<(usize, u64)> {
    let target = workspace.join("target");
    if !target.exists() {
        return Ok((0, 0));
    }
    let (files, bytes) = dir_size(&target);
    fs::remove_dir_all(&target)?;
    Ok((files, bytes))
}

/// Wipe drop-point's local disk cache. Path mirrors drop-point's
/// `cache_root()`: `<HOME>/.cache/drop-point`.
fn expunge_drop_point_cache() -> std::io::Result<(usize, u64)> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let cache = PathBuf::from(home).join(".cache").join("drop-point");
    if !cache.exists() {
        return Ok((0, 0));
    }
    let (files, bytes) = dir_size(&cache);
    fs::remove_dir_all(&cache)?;
    Ok((files, bytes))
}

fn dir_size(path: &Path) -> (usize, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
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
    println!("[{peer}] done");
}

fn forward_run_output(stream: &mut WsConn, mut child: Child, peer: &str) {
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Message>();
    spawn_pipe_reader(stderr, tx, Message::BuildStderr);
    let parser = spawn_artifact_parser(stdout);
    for msg in rx {
        let _ = send_msg(stream, &msg);
    }
    let exit_code = child.wait().map(|s| s.code().unwrap_or(1) as u8).unwrap_or(1);
    let artifact = parser.join().ok().flatten();
    if exit_code == 0 {
        if let Some(path) = artifact {
            send_artifact(stream, &path, peer);
        } else {
            eprintln!("[{peer}] no bin artifact found in cargo output");
        }
    }
    let _ = send_msg(stream, &Message::BuildFinished { exit_code });
    println!("[{peer}] done");
}

fn spawn_artifact_parser(stdout: ChildStdout) -> JoinHandle<Option<PathBuf>> {
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut last: Option<PathBuf> = None;
        for line in reader.lines().map_while(Result::ok) {
            if let Some(path) = parse_cargo_artifact_line(&line) {
                last = Some(path);
            }
        }
        last
    })
}

fn parse_cargo_artifact_line(line: &str) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("reason")?.as_str()? != "compiler-artifact" {
        return None;
    }
    let exe = v.get("executable")?.as_str()?;
    let kinds = v.get("target")?.get("kind")?.as_array()?;
    if !kinds.iter().any(|k| k.as_str() == Some("bin")) {
        return None;
    }
    Some(PathBuf::from(exe))
}

fn send_artifact(stream: &mut WsConn, path: &Path, peer: &str) {
    let Ok(contents) = std::fs::read(path) else {
        eprintln!("[{peer}] failed to read artifact {}", path.display());
        return;
    };
    let raw = contents.len();
    let compressed = match zstd::encode_all(&contents[..], 3) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[{peer}] zstd compression failed: {e}");
            return;
        }
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact".to_string());
    let sent = compressed.len();
    if let Err(e) = send_msg(stream, &Message::Executable { name, contents: compressed }) {
        eprintln!("[{peer}] failed to ship executable: {e}");
    } else {
        println!(
            "[{peer}] shipped {} → {} bytes ({})",
            raw,
            sent,
            path.display()
        );
    }
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
