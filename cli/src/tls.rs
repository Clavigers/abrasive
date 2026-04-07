use rustls::ClientConnection;
use rustls::StreamOwned;
use std::io::BufReader;
use std::net::TcpStream;
use std::sync::Arc;

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// Loads the server's CA cert from a bundled PEM or a file path.
/// For now, uses the cert bundled at compile time.
fn tls_config() -> Arc<rustls::ClientConfig> {
    let cert_pem = include_bytes!("../../certs/server.crt");
    let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(&cert_pem[..]))
        .collect::<Result<_, _>>()
        .expect("invalid server cert");

    let mut root_store = rustls::RootCertStore::empty();
    for cert in certs {
        root_store.add(cert).expect("failed to add cert");
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Arc::new(config)
}

pub fn connect(tcp_stream: TcpStream) -> std::io::Result<TlsStream> {
    let config = tls_config();
    let server_name = "abrasive".try_into().unwrap();
    let tls_conn = ClientConnection::new(config, server_name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(StreamOwned::new(tls_conn, tcp_stream))
}
