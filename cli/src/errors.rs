use std::process::ExitCode;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("failed to connect to build server: {0}")]
    Connect(std::io::Error),

    #[error("build server IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid response from build server: {0}")]
    Protocol(#[from] abrasive_protocol::DecodeError),

    #[error("server closed connection before build finished")]
    Disconnected,
}

pub type CliResult<T> = Result<T, CliError>;

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::FAILURE
    }
}
