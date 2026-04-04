use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
struct BuildRequest {
    cargo_args: Vec<String>,
}

fn parse_body(buf: &[u8], n: usize) -> Option<BuildRequest> {
    let request = std::str::from_utf8(&buf[..n]).ok()?;
    let body_start = request.find("\r\n\r\n")? + 4;
    bincode::deserialize(&buf[body_start..n]).ok()
}

fn handle(mut stream: std::net::TcpStream) {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);

    let args = parse_body(&buf, n)
        .map(|r| r.cargo_args.join(" "))
        .unwrap_or_else(|| "???".to_string());

    let header = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    let _ = stream.write_all(header.as_bytes());

    for i in ["1", "2", "3"] {
        let chunk = format!("{i}: cargo {args}\n");
        let _ = write!(stream, "{:x}\r\n{}\r\n", chunk.len(), chunk);
        let _ = stream.flush();
        thread::sleep(Duration::from_secs(1));
    }

    let _ = stream.write_all(b"0\r\n\r\n");
}

fn main() {
    let listener = TcpListener::bind("0.0.0.0:8400").unwrap();
    println!("abrasived listening on :8400");
    for stream in listener.incoming().flatten() {
        thread::spawn(|| handle(stream));
    }
}
