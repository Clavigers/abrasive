// Cloned from sccache/src/cache/cache_io.rs (Apache-2.0, MPL-2.0).
// Stripped of async/tokio: drop-point is synchronous so we drop the
// `pool.spawn_blocking(...)` wrappers and run inline. Also stripped of
// `fs_err` dep, using std::fs directly.

use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use thiserror::Error;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Error)]
pub enum CacheIoError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("failed to decompress content")]
    DecompressionFailure,
    #[error("output file without a parent directory")]
    NoParent,
    #[error("tempfile persist error: {0}")]
    Persist(#[from] tempfile::PersistError),
}

pub type Result<T> = std::result::Result<T, CacheIoError>;

/// Cache object sourced by a file. Identifier `key` is the zip member name
/// (the output filename). `path` is the absolute on-disk location.
#[derive(Clone)]
pub struct FileObjectSource {
    pub key: String,
    pub path: PathBuf,
    pub optional: bool,
}

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// Data stored in the compiler cache.
pub struct CacheRead {
    zip: ZipArchive<Box<dyn ReadSeek>>,
}

impl CacheRead {
    pub fn from<R>(reader: R) -> Result<CacheRead>
    where
        R: ReadSeek + 'static,
    {
        let z = ZipArchive::new(Box::new(reader) as Box<dyn ReadSeek>)?;
        Ok(CacheRead { zip: z })
    }

    /// Get an object from this cache entry at `name` and write it to `to`.
    /// If the file has stored permissions, return them.
    pub fn get_object<T: Write>(&mut self, name: &str, to: &mut T) -> Result<Option<u32>> {
        let file = self
            .zip
            .by_name(name)
            .map_err(|_| CacheIoError::DecompressionFailure)?;
        if file.compression() != CompressionMethod::Stored {
            return Err(CacheIoError::DecompressionFailure);
        }
        let mode = file.unix_mode();
        zstd::stream::copy_decode(file, to).map_err(|_| CacheIoError::DecompressionFailure)?;
        Ok(mode)
    }

    pub fn extract_objects<T>(mut self, objects: T) -> Result<()>
    where
        T: IntoIterator<Item = FileObjectSource>,
    {
        for FileObjectSource {
            key,
            path,
            optional,
        } in objects
        {
            let dir = path.parent().ok_or(CacheIoError::NoParent)?;
            fs::create_dir_all(dir)?;
            // Write to a tempfile and atomically rename so concurrent
            // rustc invocations never see a partial file.
            let mut tmp = NamedTempFile::new_in(dir)?;
            match (self.get_object(&key, &mut tmp), optional) {
                (Ok(mode), _) => {
                    tmp.persist(&path)?;
                    if let Some(mode) = mode {
                        set_file_mode(&path, mode)?;
                    }
                }
                (Err(e), false) => return Err(e),
                (Err(_), true) => continue,
            }
        }
        Ok(())
    }
}

pub struct CacheWrite {
    zip: ZipWriter<Cursor<Vec<u8>>>,
}

impl CacheWrite {
    pub fn new() -> CacheWrite {
        CacheWrite {
            zip: ZipWriter::new(Cursor::new(vec![])),
        }
    }

    pub fn from_objects<T>(objects: T) -> Result<CacheWrite>
    where
        T: IntoIterator<Item = FileObjectSource>,
    {
        let mut entry = CacheWrite::new();
        for FileObjectSource {
            key,
            path,
            optional,
        } in objects
        {
            match (fs::File::open(&path), optional) {
                (Ok(mut f), _) => {
                    let mode = get_file_mode(&f)?;
                    entry.put_object(&key, &mut f, mode)?;
                }
                (Err(e), false) => return Err(e.into()),
                (Err(_), true) => continue,
            }
        }
        Ok(entry)
    }

    /// Add an object containing the contents of `from` to this cache entry at
    /// `name`. If `mode` is `Some`, store the file entry with that mode.
    pub fn put_object<T: Read>(
        &mut self,
        name: &str,
        from: &mut T,
        mode: Option<u32>,
    ) -> Result<()> {
        // Declare zip compression as "stored" but actually store
        // zstd-compressed blobs. Matches sccache's wire format.
        let opts = FileOptions::default().compression_method(CompressionMethod::Stored);
        let opts = if let Some(mode) = mode {
            opts.unix_permissions(mode)
        } else {
            opts
        };
        self.zip.start_file(name, opts)?;
        let level = std::env::var("DROP_POINT_ZSTD_LEVEL")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(3);
        zstd::stream::copy_encode(from, &mut self.zip, level)?;
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        let CacheWrite { mut zip } = self;
        let cur = zip.finish()?;
        Ok(cur.into_inner())
    }
}

impl Default for CacheWrite {
    fn default() -> Self {
        Self::new()
    }
}

fn get_file_mode(file: &fs::File) -> Result<Option<u32>> {
    Ok(Some(file.metadata()?.mode()))
}

fn set_file_mode(path: &std::path::Path, mode: u32) -> Result<()> {
    let p = std::fs::Permissions::from_mode(mode);
    fs::set_permissions(path, p)?;
    Ok(())
}
