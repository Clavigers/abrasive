#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("tls error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("websocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),

    #[error("websocket handshake interrupted")]
    WsHandshakeInterrupted,

    #[error("client closed connection")]
    ClientClosed,

    #[error("protocol decode error: {0}")]
    Decode(#[from] abrasive_protocol::DecodeError),

    #[error("expected {expected} message, got {got}")]
    UnexpectedMessage { expected: &'static str, got: String },

    #[error("auth rejected: {0}")]
    Auth(#[from] AuthError),
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing bearer token")]
    NoBearerToken,

    #[error("missing env var {0}")]
    MissingEnv(&'static str),

    #[error("token does not look like an abrasive API token")]
    InvalidTokenFormat,

    #[error("unknown or revoked token")]
    UnknownToken,

    #[error("token is expired")]
    TokenExpired,

    #[error("could not parse token expires_at: {0}")]
    TimestampParse(String),

    #[error("supabase call failed: {0}")]
    SupabaseCall(#[source] ureq::Error),

    #[error("could not parse supabase response: {0}")]
    SupabaseParseResponse(#[source] std::io::Error),
}
