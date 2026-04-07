use abrasive_protocol::{decode, encode, BuildRequest, Header, Manifest, Message, PlatformTriple};
use std::env;
use rustls::ServerConnection;
use rustls::StreamOwned;
use std::collections::HashMap;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;

/// Commands that accept --target
const TARGET_COMMANDS: &[&str] = &["build", "check", "test", "bench", "clippy", "doc"];

type TlsStream = StreamOwned<ServerConnection, TcpStream>;

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

fn recv_msg(stream: &mut TlsStream) -> std::io::Result<Message> {
    let mut header_buf = [0u8; Header::SIZE];
    stream.read_exact(&mut header_buf)?;
    let header = Header::from_bytes(&header_buf);
    let mut raw = vec![0u8; Header::SIZE + header.length as usize];
    raw[..Header::SIZE].copy_from_slice(&header_buf);
    stream.read_exact(&mut raw[Header::SIZE..])?;
    decode(&raw)
        .map(|f| f.message)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

fn send_msg(stream: &mut TlsStream, msg: &Message) -> std::io::Result<()> {
    let frame = encode(msg);
    stream.write_all(&frame)?;
    stream.flush()
}

fn hash_file(path: &Path) -> Option<[u8; 32]> {
    let data = fs::read(path).ok()?;
    Some(*blake3::hash(&data).as_bytes())
}

fn local_manifest(workspace: &Path) -> HashMap<String, [u8; 32]> {
    let mut map = HashMap::new();
    if !workspace.exists() {
        return map;
    }
    for entry in walkdir::WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        // Skip the target directory
        if entry.path().components().any(|c| c.as_os_str() == "target") {
            continue;
        }
        if let Ok(rel) = entry.path().strip_prefix(workspace) {
            if let Some(hash) = hash_file(entry.path()) {
                map.insert(rel.to_string_lossy().to_string(), hash);
            }
        }
    }
    map
}

fn handle_sync(
    stream: &mut TlsStream,
    workspace: &Path,
    peer: &str,
    client_files: &[abrasive_protocol::FileEntry],
) -> std::io::Result<()> {
    // Diff against local state
    let local = local_manifest(workspace);
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
    let mut stream = StreamOwned::new(tls_conn, tcp_stream);

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

    let Manifest { team, scope, files } = manifest;
    let workspace = workspace_path(&team, &scope);
    if let Err(e) = fs::create_dir_all(&workspace) {
        println!("[{peer}] failed to create workspace {}: {e}", workspace.display());
        return;
    }

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

fn workspace_path(team: &str, scope: &str) -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(format!("{}/{}_{}", home, team, scope))
}

fn run_build(stream: &mut TlsStream, peer: &str, workspace: &Path, req: BuildRequest) {
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

    println!("[{peer}] mold -run cargo {} (in {})", cargo_args.join(" "), cd_target.display());

    let mut child = match Command::new("mold")
        .arg("-run")
        .arg("cargo")
        .args(&cargo_args)
        .current_dir(&cd_target)
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
    let _ = stream.conn.send_close_notify();
    let _ = stream.flush();
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
    println!("abrasived listening on :8400 (TLS)");
    for stream in listener.incoming().flatten() {
        let config = tls_config.clone();
        thread::spawn(move || handle(stream, config));
    }
}
