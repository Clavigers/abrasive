use serde::{Deserialize, Serialize};
use std::mem;

pub struct Frame {
    pub header: Header,
    pub message: Message,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Header {
    pub length: u32,
}

impl Header {
    pub const SIZE: usize = mem::size_of::<Self>();

    pub fn from_bytes(buf: &[u8; Self::SIZE]) -> Self {
        Header {
            length: u32::from_be_bytes(*buf),
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        self.length.to_be_bytes()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Manifest(Vec<FileEntry>),
    NeedFiles(Vec<String>),
    FileData { path: String, contents: Vec<u8> },
    SyncDone,
    SyncAck,
    BuildRequest(BuildRequest),
    BuildOutput(Vec<u8>),
    BuildFinished { exit_code: u8 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub hash: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildRequest {
    pub cargo_args: Vec<String>,
    pub subdir: Option<String>,
    // environment_variables: Vec<String>, TODO
}

pub fn encode(msg: &Message) -> Vec<u8> {
    let payload = bincode::serialize(msg).unwrap();
    let header = Header {
        length: payload.len() as u32,
    };
    let mut frame = header.to_bytes().to_vec();
    frame.extend(payload);
    frame
}

pub fn decode(raw: &[u8]) -> Result<Frame, bincode::Error> {
    let header = Header::from_bytes(raw[..Header::SIZE].try_into().unwrap());
    let message = bincode::deserialize(&raw[Header::SIZE..])?;
    Ok(Frame { header, message })
}
