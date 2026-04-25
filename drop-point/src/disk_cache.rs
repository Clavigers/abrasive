// TODO replace this with a lru disk cache almost identical to the one found in sccache.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct DiskCache {
    root: PathBuf,
}

impl DiskCache {
    pub fn new(root: PathBuf) -> io::Result<DiskCache> {
        fs::create_dir_all(&root)?;
        Ok(DiskCache { root })
    }

    /// On hit: directory holding this entry's outputs. Caller copies/links out.
    pub fn get(&self, key: &str) -> Option<PathBuf> {
        let path = self.path_for(key);
        path.is_dir().then_some(path)
    }

    /// `fill` writes outputs into a fresh tempdir; on success the tempdir
    /// is renamed into place. Returns true when this call wrote the entry,
    /// false when the entry already existed (either before we started or
    /// because another process won the race).
    pub fn put<F>(&self, key: &str, fill: F) -> io::Result<bool>
    where
        F: FnOnce(&Path) -> io::Result<()>,
    {
        let final_path = self.path_for(key);
        if final_path.is_dir() {
            return Ok(false);
        }
        let parent = final_path.parent().expect("path_for produces a parent");
        fs::create_dir_all(parent)?;
        let tmp = tempdir_in(parent)?;
        if let Err(e) = fill(&tmp) {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
        finalize(&tmp, &final_path)
    }

    fn path_for(&self, key: &str) -> PathBuf {
        // Two-level prefix sharding so no single dir gets too big.
        self.root.join(&key[0..1]).join(&key[1..2]).join(key)
    }
}

fn finalize(tmp: &Path, final_path: &Path) -> io::Result<bool> {
    match fs::rename(tmp, final_path) {
        Ok(()) => Ok(true),
        Err(e) => {
            let _ = fs::remove_dir_all(tmp);
            if final_path.is_dir() { Ok(false) } else { Err(e) }
        }
    }
}

fn tempdir_in(parent: &Path) -> io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".tmp-{pid}-{n}"));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}
