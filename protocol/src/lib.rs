mod errors;

pub use errors::DecodeError;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    Manifest(Manifest),
    NeedFiles(Vec<String>),
    FileData { path: String, contents: Vec<u8> },
    SyncDone,
    SyncAck,
    BuildRequest(BuildRequest),
    BuildStdout(Vec<u8>),
    BuildStderr(Vec<u8>),
    BuildFinished { exit_code: u8 },
    /// Server-side rejection: all slots for this (team, scope) are
    /// currently busy. The client should sleep and retry the whole
    /// connection. Sent in place of NeedFiles.
    SlotsBusy,
    /// Cheap "is anything stale?" check sent before the full manifest.
    /// The fingerprint is a hash of (path, mtime, size) for every
    /// file in the workspace — no file contents read. The daemon
    /// caches the last accepted fingerprint per (slot, team, scope)
    /// in memory and answers ProbeAccepted (skip sync entirely) or
    /// ProbeMiss (fall through to the Manifest flow).
    Probe {
        team: String,
        scope: String,
        fingerprint: [u8; 32],
    },
    ProbeAccepted,
    ProbeMiss,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub team: String,
    pub scope: String,
    /// gzip(bincode(Vec<FileEntry>))
    pub files_gz: Vec<u8>,
}

impl Manifest {
    pub fn encode_files(files: &[FileEntry]) -> Vec<u8> {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let raw = bincode::serialize(files).unwrap();
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        enc.finish().unwrap()
    }

    pub fn decode_files(&self) -> Result<Vec<FileEntry>, DecodeError> {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut dec = GzDecoder::new(&self.files_gz[..]);
        let mut raw = Vec::new();
        dec.read_to_end(&mut raw).map_err(|e| DecodeError(Box::new(bincode::ErrorKind::Custom(e.to_string()))))?;
        bincode::deserialize(&raw).map_err(DecodeError)
    }
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
    pub team: String,
    pub scope: String,
}

/// Serialize a Message into a bincode payload. WebSocket framing handles
/// length-prefixing for us, so this is just the raw bincode bytes.
pub fn serialize(msg: &Message) -> Vec<u8> {
    bincode::serialize(msg).unwrap()
}

/// Deserialize a Message from a bincode payload received over WebSockets.
pub fn deserialize(raw: &[u8]) -> Result<Message, DecodeError> {
    bincode::deserialize(raw).map_err(DecodeError)
}
