use std::fmt;

#[derive(Debug)]
pub struct DecodeError(pub(crate) Box<bincode::ErrorKind>);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DecodeError {}
