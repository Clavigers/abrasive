use crate::constants::REQUIRED_ORG;

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

    #[error("github /user call failed: {0}")]
    UserCall(#[source] ureq::Error),

    #[error("could not parse /user response: {0}")]
    UserResponseParse(#[source] std::io::Error),

    #[error("no 'login' field in /user response")]
    NoLoginField,

    #[error("user '{login}' is not a member of {}", REQUIRED_ORG)]
    NotMember { login: String },

    #[error("github membership check failed: {0}")]
    MembershipCheck(#[source] ureq::Error),
}
