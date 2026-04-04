use abrasive_protocol::{decode, encode, BuildRequest, Header, Message};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::thread;

fn recv_msg(stream: &mut TcpStream) -> std::io::Result<Message> {
    let mut header_buf = [0u8; Header::SIZE];
    stream.read_exact(&mut header_buf)?;
    let header = Header::from_bytes(&header_buf);
    let mut raw = vec![0u8; Header::SIZE + header.length as usize];
    raw[..Header::SIZE].copy_from_slice(&header_buf);
    stream.read_exact(&mut raw[Header::SIZE..])?;
    decode(&raw)
        .map(|f| f.message)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn send_msg(stream: &mut TcpStream, msg: &Message) -> std::io::Result<()> {
    let frame = encode(msg);
    stream.write_all(&frame)?;
    stream.flush()
}

fn handle(mut stream: TcpStream) {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    println!("[{peer}] connected");

    let msg = match recv_msg(&mut stream) {
        Ok(m) => m,
        Err(e) => {
            println!("[{peer}] read error: {e}");
            return;
        }
    };

    let BuildRequest { cargo_args, subdir } = match msg {
        Message::BuildRequest(req) => req,
        other => {
            println!("[{peer}] unexpected message: {other:?}");
            return;
        }
    };

    println!("[{peer}] cargo {}", cargo_args.join(" "));

    // TODO: use subdir for cd target
    let mut child = match Command::new("cargo")
        .args(&cargo_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = send_msg(
                &mut stream,
                &Message::BuildOutput(format!("failed to spawn cargo: {e}\n").into_bytes()),
            );
            let _ = send_msg(&mut stream, &Message::BuildFinished { exit_code: 1 });
            return;
        }
    };

    let stderr = child.stderr.take().unwrap();
    let mut stream2 = stream.try_clone().unwrap();
    let stderr_handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut reader = stderr;
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = send_msg(&mut stream2, &Message::BuildOutput(buf[..n].to_vec()));
                }
            }
        }
    });

    let mut stdout = child.stdout.take().unwrap();
    let mut buf = [0u8; 4096];
    loop {
        match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let _ = send_msg(&mut stream, &Message::BuildOutput(buf[..n].to_vec()));
            }
        }
    }

    stderr_handle.join().unwrap();
    let status = child.wait().unwrap();
    let _ = send_msg(
        &mut stream,
        &Message::BuildFinished {
            exit_code: status.code().unwrap_or(1) as u8,
        },
    );
    println!("[{peer}] done");
}

fn main() {
    let listener = TcpListener::bind("0.0.0.0:8400").unwrap();
    println!("abrasived listening on :8400");
    for stream in listener.incoming().flatten() {
        thread::spawn(|| handle(stream));
    }
}
