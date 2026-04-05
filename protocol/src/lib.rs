mod errors;

pub use errors::DecodeError;

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

/// Architecture
#[derive(Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum Arch {
    X86_64 = 0,
    Aarch64 = 1,
}

/// Operating System
#[derive(Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum Os {
    Windows = 0,
    Linux = 1,
    Mac = 2,
}

/// Application Binary Interface
#[derive(Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum Abi {
    Gnu = 0,
    Musl = 1,
    Msvc = 2,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformTriple {
    pub arch: Arch,
    pub os: Os,
    pub abi: Abi,
}

impl PlatformTriple {
    pub fn as_cargo_target_string(&self) -> String {
        match (&self.arch, &self.os, &self.abi) {
            (Arch::X86_64, Os::Linux, Abi::Gnu) => "x86_64-unknown-linux-gnu",
            (Arch::X86_64, Os::Linux, Abi::Musl) => "x86_64-unknown-linux-musl",
            (Arch::Aarch64, Os::Linux, Abi::Gnu) => "aarch64-unknown-linux-gnu",
            (Arch::Aarch64, Os::Linux, Abi::Musl) => "aarch64-unknown-linux-musl",
            (Arch::X86_64, Os::Windows, Abi::Msvc) => "x86_64-pc-windows-msvc",
            (Arch::X86_64, Os::Windows, Abi::Gnu) => "x86_64-pc-windows-gnu",
            (Arch::Aarch64, Os::Windows, Abi::Msvc) => "aarch64-pc-windows-msvc",
            (Arch::X86_64, Os::Mac, _) => "x86_64-apple-darwin",
            (Arch::Aarch64, Os::Mac, _) => "aarch64-apple-darwin",
            _ => unimplemented!(),
        }
        .to_string()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildRequest {
    pub cargo_args: Vec<String>,
    pub subdir: Option<String>,
    pub host_platform: PlatformTriple, 
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

pub fn decode(raw: &[u8]) -> Result<Frame, DecodeError> {
    let header = Header::from_bytes(raw[..Header::SIZE].try_into().unwrap());
    let message = bincode::deserialize(&raw[Header::SIZE..]).map_err(DecodeError)?;
    Ok(Frame { header, message })
}
