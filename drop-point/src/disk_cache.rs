// Mirrors sccache/src/cache/disk.rs. Each cache entry is a single file at
// `<root>/<key[0]>/<key[1]>/<key>` containing a zip archive of the cached
// outputs (see cache_io.rs). Lookup is `File::open`, write is
// "tempfile + atomic rename" so concurrent puts can't corrupt a reader.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::cache_io::{CacheIoError, CacheRead, CacheWrite};

pub struct DiskCache {
    root: PathBuf,
}

impl DiskCache {
    pub fn new(root: PathBuf) -> io::Result<DiskCache> {
        fs::create_dir_all(&root)?;
        Ok(DiskCache { root })
    }

    /// On hit: returns a [`CacheRead`] that the caller can extract objects
    /// from. On miss: returns Ok(None).
    pub fn get(&self, key: &str) -> Result<Option<CacheRead>, CacheIoError> {
        let path = self.path_for(key);
        match fs::File::open(&path) {
            Ok(f) => Ok(Some(CacheRead::from(f)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write a finished [`CacheWrite`] entry to disk. Atomic via
    /// temp-file-then-rename. Returns true if this call wrote the entry,
    /// false if it already existed (whether before we started or because
    /// another process won the race).
    pub fn put(&self, key: &str, entry: CacheWrite) -> Result<bool, CacheIoError> {
        let final_path = self.path_for(key);
        if final_path.is_file() {
            return Ok(false);
        }
        let parent = final_path.parent().expect("path_for produces a parent");
        fs::create_dir_all(parent)?;
        let bytes = entry.finish()?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(&bytes)?;
        match tmp.persist_noclobber(&final_path) {
            Ok(_) => Ok(true),
            Err(e) if final_path.is_file() => {
                let _ = fs::remove_file(e.file.path());
                Ok(false)
            }
            Err(e) => Err(io::Error::other(format!("persist failed: {e}")).into()),
        }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        // Two-level prefix sharding so no single dir gets too big.
        self.root.join(&key[0..1]).join(&key[1..2]).join(key)
    }
}
