mod auth;

use abrasive_protocol::{BuildRequest, Manifest, Message, PlatformTriple};
use std::env;
use rustls::ServerConnection;
use rustls::StreamOwned;
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use rayon::prelude::*;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use tungstenite::Message as WsMessage;
use tungstenite::WebSocket;
use tungstenite::handshake::HandshakeError;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::http;

/// Commands that accept --target
const TARGET_COMMANDS: &[&str] = &["build", "check", "test", "bench", "clippy", "doc"];

type TlsStream = StreamOwned<ServerConnection, TcpStream>;
type WsConn = WebSocket<TlsStream>;

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

fn ws_to_io(e: tungstenite::Error) -> std::io::Error {
    match e {
        tungstenite::Error::Io(io) => io,
        other => std::io::Error::new(std::io::ErrorKind::Other, other.to_string()),
    }
}

fn recv_msg(ws: &mut WsConn) -> std::io::Result<Message> {
    loop {
        match ws.read().map_err(ws_to_io)? {
            WsMessage::Binary(data) => {
                return abrasive_protocol::deserialize(&data)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()));
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            WsMessage::Close(_) => {
                return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "client closed"));
            }
            WsMessage::Text(_) | WsMessage::Frame(_) => continue,
        }
    }
}

fn send_msg(ws: &mut WsConn, msg: &Message) -> std::io::Result<()> {
    let payload = abrasive_protocol::serialize(msg);
    ws.send(WsMessage::Binary(payload)).map_err(ws_to_io)?;
    Ok(())
}

fn hash_file(path: &Path) -> Option<[u8; 32]> {
    let data = fs::read(path).ok()?;
    Some(*blake3::hash(&data).as_bytes())
}

fn local_manifest(workspace: &Path) -> HashMap<String, [u8; 32]> {
    if !workspace.exists() {
        return HashMap::new();
    }

    // 1. Walk (single-threaded)
    let paths: Vec<PathBuf> = walkdir::WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == "target"))
        .map(|e| e.into_path())
        .collect();

    // 2. Hash in parallel
    paths
        .par_iter()
        .filter_map(|p| {
            let rel = p.strip_prefix(workspace).ok()?.to_string_lossy().to_string();
            let hash = hash_file(p)?;
            Some((rel, hash))
        })
        .collect()
}

fn handle_sync(
    stream: &mut WsConn,
    workspace: &Path,
    peer: &str,
    client_files: &[abrasive_protocol::FileEntry],
) -> std::io::Result<()> {
    // Diff against local state
    let t0 = std::time::Instant::now();
    let local = local_manifest(workspace);
    println!("[{peer}] local_manifest: {} files in {:?}", local.len(), t0.elapsed());
    let needed: Vec<String> = client_files
        .iter()
        .filter(|f| local.get(&f.path) != Some(&f.hash))
        .map(|f| f.path.clone())
        .collect();

    // Delete stale files
    let client_paths: std::collections::HashSet<&str> =
        client_files.iter().map(|f| f.path.as_str()).collect();
    for local_path in local.keys() {
        if !client_paths.contains(local_path.as_str()) {
            let _ = fs::remove_file(workspace.join(local_path));
        }
    }

    println!("[{peer}] sync: need {}/{} files", needed.len(), client_files.len());
    send_msg(stream, &Message::NeedFiles(needed))?;

    // 3. Receive files
    loop {
        match recv_msg(stream)? {
            Message::FileData { path, contents } => {
                let dest = workspace.join(&path);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).ok();
                }
                fs::write(&dest, &contents)?;
            }
            Message::SyncDone => break,
            other => {
                println!("[{peer}] unexpected during sync: {other:?}");
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unexpected message"));
            }
        }
    }

    println!("[{peer}] sync complete");
    send_msg(stream, &Message::SyncAck)?;
    Ok(())
}

fn handle(tcp_stream: TcpStream, tls_config: Arc<rustls::ServerConfig>) {
    let peer = tcp_stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    println!("[{peer}] connected");

    let tls_conn = match ServerConnection::new(tls_config) {
        Ok(c) => c,
        Err(e) => {
            println!("[{peer}] TLS handshake failed: {e}");
            return;
        }
    };
    let tls_stream = StreamOwned::new(tls_conn, tcp_stream);

    // WebSocket upgrade with GitHub token validation. We pull the bearer
    // token out of the Authorization header, then in the handshake
    // callback we call GitHub's API to confirm (a) the token is valid
    // and (b) the user is a member of the required org. Reject with 401
    // before doing any protocol work.
    let github_login: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    let auth_ok = std::cell::Cell::new(false);
    let ws_result = tungstenite::accept_hdr(tls_stream, |req: &Request, resp: Response| {
        let presented = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));

        let token = match presented {
            Some(t) if !t.is_empty() => t,
            _ => {
                let mut err: ErrorResponse =
                    http::Response::new(Some("missing bearer token".to_string()));
                *err.status_mut() = http::StatusCode::UNAUTHORIZED;
                return Err(err);
            }
        };

        match auth::validate(token) {
            Ok(login) => {
                *github_login.borrow_mut() = Some(login);
                auth_ok.set(true);
                Ok(resp)
            }
            Err(reason) => {
                println!("[{peer}] auth rejected: {reason}");
                let mut err: ErrorResponse = http::Response::new(Some(reason));
                *err.status_mut() = http::StatusCode::UNAUTHORIZED;
                Err(err)
            }
        }
    });
    let mut stream: WsConn = match ws_result {
        Ok(ws) => ws,
        Err(HandshakeError::Failure(e)) => {
            if !auth_ok.get() {
                println!("[{peer}] rejected: bad/missing/unauthorized github token");
            } else {
                println!("[{peer}] ws handshake failed: {e}");
            }
            return;
        }
        Err(HandshakeError::Interrupted(_)) => {
            println!("[{peer}] ws handshake interrupted");
            return;
        }
    };

    if let Some(login) = github_login.into_inner() {
        println!("[{peer}] authenticated as github user '{login}'");
    }

    let manifest = match recv_msg(&mut stream) {
        Ok(Message::Manifest(m)) => m,
        Ok(other) => {
            println!("[{peer}] expected Manifest, got: {other:?}");
            return;
        }
        Err(e) => {
            println!("[{peer}] read error: {e}");
            return;
        }
    };

    let files = match manifest.decode_files() {
        Ok(f) => f,
        Err(e) => {
            println!("[{peer}] failed to decode manifest: {e}");
            return;
        }
    };
    let Manifest { team, scope, files_gz: _ } = manifest;
    let workspace = workspace_path(&team, &scope);
    if let Err(e) = fs::create_dir_all(&workspace) {
        println!("[{peer}] failed to create workspace {}: {e}", workspace.display());
        return;
    }
    ensure_target_on_tmpfs(&workspace, &team, &scope, &peer);

    if let Err(e) = handle_sync(&mut stream, &workspace, &peer, &files) {
        println!("[{peer}] sync failed: {e}");
        return;
    }

    let req = match recv_msg(&mut stream) {
        Ok(Message::BuildRequest(req)) => req,
        Ok(other) => {
            println!("[{peer}] expected BuildRequest, got: {other:?}");
            return;
        }
        Err(e) => {
            println!("[{peer}] read error: {e}");
            return;
        }
    };

    // BuildRequest is self-addressing — resolve its own workspace rather
    // than assuming it matches the one we just synced.
    let build_workspace = workspace_path(&req.team, &req.scope);
    run_build(&mut stream, &peer, &build_workspace, req);
}

/// Make `<workspace>/target` live on tmpfs (/dev/shm) so cargo's
/// write-heavy build artifacts skip the disk entirely. We do this with
/// a symlink rather than a mount so we don't need root or namespacing.
///
/// Behavior:
/// - If `target` doesn't exist: create the tmpfs dir and symlink it in.
/// - If `target` is already the right symlink: nothing to do.
/// - If `target` is a real directory or some other symlink: leave it
///   alone and warn (we don't want to nuke prior build state by surprise).
fn ensure_target_on_tmpfs(workspace: &Path, team: &str, scope: &str, peer: &str) {
    let tmpfs_target = PathBuf::from(format!("/dev/shm/abrasive-targets/{}_{}", team, scope));
    let target_link = workspace.join("target");

    if let Err(e) = fs::create_dir_all(&tmpfs_target) {
        println!("[{peer}] tmpfs target unavailable ({e}); falling back to disk");
        return;
    }

    match fs::symlink_metadata(&target_link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(&target_link).ok().as_deref() == Some(tmpfs_target.as_path()) {
                return; // already wired up
            }
            println!("[{peer}] target/ is a symlink to something else; leaving alone");
        }
        Ok(_) => {
            println!("[{peer}] target/ is a real directory; leaving alone (delete it manually to enable tmpfs)");
        }
        Err(_) => {
            // Doesn't exist — create the symlink.
            #[cfg(unix)]
            if let Err(e) = std::os::unix::fs::symlink(&tmpfs_target, &target_link) {
                println!("[{peer}] failed to symlink target -> {}: {e}", tmpfs_target.display());
            } else {
                println!("[{peer}] target/ -> {}", tmpfs_target.display());
            }
        }
    }
}

fn workspace_path(team: &str, scope: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(format!("{}/{}_{}", home, team, scope))
}

fn run_build(stream: &mut WsConn, peer: &str, workspace: &Path, req: BuildRequest) {
    let BuildRequest {
        cargo_args,
        subdir,
        host_platform,
        team: _,
        scope: _,
    } = req;

    let (cargo_args, _run_it) = rewrite_run_as_build(cargo_args);
    let cargo_args = amend_args_with_platform(cargo_args, host_platform);

    let cd_target = match &subdir {
        Some(rel) => workspace.join(rel),
        None => workspace.to_path_buf(),
    };

    println!("[{peer}] mold -run cargo +nightly {} (in {})", cargo_args.join(" "), cd_target.display());

    let mut child = match Command::new("mold")
        .arg("-run")
        .arg("cargo")
        .arg("+nightly")
        .args(&cargo_args)
        .current_dir(&cd_target)
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
    {
        Ok(c) => c,
        Err(e) => {
            let _ = send_msg(
                stream,
                &Message::BuildStderr(format!("failed to spawn cargo: {e}\n").into_bytes()),
            );
            let _ = send_msg(stream, &Message::BuildFinished { exit_code: 1 });
            return;
        }
    };

    // Merge stdout and stderr via a channel so they interleave naturally
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<Message>();

    let tx_out = tx.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut reader = stdout;
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = tx_out.send(Message::BuildStdout(buf[..n].to_vec()));
                }
            }
        }
    });

    let tx_err = tx;
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut reader = stderr;
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = tx_err.send(Message::BuildStderr(buf[..n].to_vec()));
                }
            }
        }
    });

    for msg in rx {
        let _ = send_msg(stream, &msg);
    }

    let status = child.wait().unwrap();
    if let Err(e) = send_msg(
        stream,
        &Message::BuildFinished {
            exit_code: status.code().unwrap_or(1) as u8,
        },
    ) {
        println!("[{peer}] failed to send BuildFinished: {e}");
    }
    let _ = stream.close(None);
    println!("[{peer}] done");
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

fn main() {
    let tls_config = load_tls_config();
    let listener = TcpListener::bind("0.0.0.0:8400").unwrap();
    println!("abrasived TEST listening on :8400 (TLS+WS)");
    for stream in listener.incoming().flatten() {
        let config = tls_config.clone();
        thread::spawn(move || handle(stream, config));
    }
}
