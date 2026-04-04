use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn handle(mut stream: std::net::TcpStream) {
    let mut buf = [0u8; 4096];
    let _ = stream.read(&mut buf);

    let header = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    let _ = stream.write_all(header.as_bytes());

    for chunk in ["1\n", "2\n", "3\n"] {
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
