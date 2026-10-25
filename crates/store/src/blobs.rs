// SPDX-License-Identifier: AGPL-3.0-or-later
//! The content-addressed blob store (FORMAT.md §4).
//!
//! A blob with hash `blake3:<hex>` lives at `blobs/<hex[0..2]>/<hex[2..4]>/<hex>` and never
//! changes. A new blob is written to a temporary file in `blobs/`, flushed to disk, and then moved
//! into place without replacing anything, so a reader never sees a partial blob and storing the
//! same content twice keeps one copy.

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use dbme_core::{ContentHash, ContentHasher};

use crate::{Result, StoreError};

/// The blob store of one vault.
#[derive(Clone, Debug)]
pub struct BlobStore {
    root: PathBuf,
}

/// The result of storing a blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredBlob {
    /// The content's hash, which is also its name.
    pub hash: ContentHash,
    /// Its size in bytes.
    pub size: u64,
    /// `false` if the store already had this content.
    pub new: bool,
}

impl BlobStore {
    /// The blob store in folder `root` (a vault's `blobs/`).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Where the blob with `hash` lives.
    pub fn path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.hex();
        self.root.join(&hex[0..2]).join(&hex[2..4]).join(hex)
    }

    /// Stores `bytes`.
    pub fn put(&self, bytes: &[u8]) -> Result<StoredBlob> {
        self.put_reader(bytes)
    }

    /// Stores the contents of the file at `path`.
    pub fn put_file(&self, path: &Path) -> Result<StoredBlob> {
        let file = File::open(path).map_err(|e| StoreError::io("opening", path, e))?;
        self.put_reader(file)
    }

    /// Stores everything `reader` yields, hashing it on the way.
    pub fn put_reader(&self, mut reader: impl Read) -> Result<StoredBlob> {
        let io = |action, e| StoreError::io(action, &self.root, e);
        let mut temp = tempfile::Builder::new()
            .prefix(".incoming-")
            .tempfile_in(&self.root)
            .map_err(|e| io("creating a temporary file in", e))?;
        let mut hasher = ContentHasher::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(io("reading data for", e)),
            };
            hasher.update(&buffer[..read]);
            temp.write_all(&buffer[..read])
                .map_err(|e| io("writing to", e))?;
            size += read as u64;
        }
        let hash = hasher.finalize();
        let path = self.path(&hash);
        if path.exists() {
            return Ok(StoredBlob {
                hash,
                size,
                new: false,
            });
        }
        temp.as_file()
            .sync_all()
            .map_err(|e| io("flushing to", e))?;
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(|e| StoreError::io("creating", folder, e))?;
        }
        let new = match temp.persist_noclobber(&path) {
            Ok(_) => true,
            Err(e) if e.error.kind() == ErrorKind::AlreadyExists => false,
            Err(e) => return Err(StoreError::io("storing", path, e.error)),
        };
        Ok(StoredBlob { hash, size, new })
    }

    /// The content of a blob.
    pub fn read(&self, hash: &ContentHash) -> Result<Vec<u8>> {
        let path = self.path(hash);
        fs::read(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => StoreError::MissingBlob(hash.clone()),
            _ => StoreError::io("reading", path, e),
        })
    }

    /// A blob's size, or `None` if the store does not have it.
    pub fn size(&self, hash: &ContentHash) -> Result<Option<u64>> {
        let path = self.path(hash);
        match fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::io("reading", path, e)),
        }
    }

    /// Reads a blob back and checks that its content still has its hash.
    pub fn check(&self, hash: &ContentHash) -> Result<bool> {
        let path = self.path(hash);
        let mut file = File::open(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => StoreError::MissingBlob(hash.clone()),
            _ => StoreError::io("opening", &path, e),
        })?;
        let mut hasher = ContentHasher::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    hasher.update(&buffer[..read]);
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(StoreError::io("reading", &path, e)),
            }
        }
        Ok(hasher.finalize() == *hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = BlobStore::new(dir.path());
        (dir, store)
    }

    #[test]
    fn stores_once_and_reads_back() {
        let (_dir, store) = store();
        let first = store.put(b"hello").unwrap();
        assert!(first.new);
        assert_eq!(first.size, 5);
        assert_eq!(first.hash, ContentHash::of(b"hello"));
        let second = store.put(b"hello").unwrap();
        assert_eq!(
            second,
            StoredBlob {
                new: false,
                ..first.clone()
            }
        );
        assert_eq!(store.read(&first.hash).unwrap(), b"hello");
        assert_eq!(store.size(&first.hash).unwrap(), Some(5));
        assert!(store.check(&first.hash).unwrap());

        let path = store.path(&first.hash);
        let hex = first.hash.hex();
        assert!(path.ends_with(Path::new(&hex[0..2]).join(&hex[2..4]).join(hex)));
        // Only the blob is left: no temporary files.
        let entries: Vec<_> = fs::read_dir(store.root.clone())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from(&hex[0..2])]);
    }

    #[test]
    fn missing_and_damaged_blobs() {
        let (dir, store) = store();
        let absent = ContentHash::of(b"absent");
        assert!(matches!(
            store.read(&absent),
            Err(StoreError::MissingBlob(_))
        ));
        assert!(matches!(
            store.check(&absent),
            Err(StoreError::MissingBlob(_))
        ));
        assert_eq!(store.size(&absent).unwrap(), None);

        let stored = store.put(b"original").unwrap();
        fs::write(store.path(&stored.hash), b"tampered").unwrap();
        assert!(!store.check(&stored.hash).unwrap());

        let source = dir.path().join("source.bin");
        fs::write(&source, vec![7_u8; 200_000]).unwrap();
        let from_file = store.put_file(&source).unwrap();
        assert_eq!(from_file.size, 200_000);
        assert!(store.check(&from_file.hash).unwrap());
        assert!(matches!(
            store.put_file(&dir.path().join("nope")),
            Err(StoreError::Io { .. })
        ));
    }

    #[test]
    fn a_folder_in_the_way_is_not_a_blob() {
        let (_dir, store) = store();
        let hash = ContentHash::of(b"x");
        fs::create_dir_all(store.path(&hash)).unwrap();
        assert_eq!(store.size(&hash).unwrap(), None);
    }
}
