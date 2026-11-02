// SPDX-License-Identifier: AGPL-3.0-or-later
//! The file-system seam. Every path is relative to the folder's root, with forward slashes, and
//! never escapes it: the engine builds paths from titles the store checked and from listings.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// What a listing says about one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The path inside the folder.
    pub rel_path: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its modification time in milliseconds since the Unix epoch, or `0` when unknown.
    pub mtime_ms: i64,
}

/// What `stat` says about a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    /// Whether it is a folder.
    pub is_dir: bool,
    /// Its size in bytes (`0` for a folder).
    pub size: u64,
    /// Its modification time in milliseconds since the Unix epoch, or `0` when unknown.
    pub mtime_ms: i64,
}

/// A file system rooted at the files folder.
pub trait Fs {
    /// The whole file.
    fn read(&self, rel_path: &str) -> io::Result<Vec<u8>>;
    /// Writes the file whole or not at all: a dot-prefixed temporary file next to it, flushed,
    /// then renamed over the destination. Folders on the way are created.
    fn write_atomic(&mut self, rel_path: &str, bytes: &[u8]) -> io::Result<()>;
    /// Removes a file. Removing a file that is not there is not an error.
    fn remove(&mut self, rel_path: &str) -> io::Result<()>;
    /// Renames a file, creating the destination's folders. Fails if the destination exists.
    fn rename(&mut self, from: &str, to: &str) -> io::Result<()>;
    /// What is at a path, or `None`.
    fn stat(&self, rel_path: &str) -> io::Result<Option<Stat>>;
    /// Every file under the root, recursively, by path. Symbolic links and junctions are not
    /// followed and not listed.
    fn list(&self) -> io::Result<Vec<Entry>>;
    /// Removes every folder that holds nothing, deepest first. The root stays.
    fn remove_empty_dirs(&mut self) -> io::Result<()>;
}

fn parent_of(rel_path: &str) -> &str {
    rel_path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn name_of(rel_path: &str) -> &str {
    rel_path.rsplit_once('/').map_or(rel_path, |(_, name)| name)
}

/// An in-memory file system for tests: deterministic, with a modification counter for `mtime`
/// and the ability to make the next renames fail, as a locked file on Windows does.
#[derive(Debug, Default)]
pub struct MemFs {
    files: BTreeMap<String, (Vec<u8>, i64)>,
    dirs: std::collections::BTreeSet<String>,
    clock: i64,
    fail_renames: u32,
    writes: u64,
}

impl MemFs {
    /// An empty folder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next `count` renames fail with `PermissionDenied`, the way a file another program
    /// holds open fails on Windows (ADR-0024).
    pub fn fail_next_renames(&mut self, count: u32) {
        self.fail_renames = count;
    }

    /// Writes a file as another program would: directly, with a new modification time.
    pub fn put(&mut self, rel_path: &str, bytes: &[u8]) {
        self.clock += 1;
        self.add_dirs(rel_path);
        self.files
            .insert(rel_path.to_owned(), (bytes.to_vec(), self.clock));
    }

    /// Makes an empty folder, as another program would.
    pub fn mkdir(&mut self, rel_path: &str) {
        self.dirs.insert(rel_path.to_owned());
        self.add_dirs(&format!("{rel_path}/x"));
    }

    /// The file's text, for assertions.
    pub fn text(&self, rel_path: &str) -> Option<String> {
        self.files
            .get(rel_path)
            .map(|(bytes, _)| String::from_utf8_lossy(bytes).into_owned())
    }

    /// Every path, for assertions.
    pub fn paths(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    /// Every folder, for assertions.
    pub fn dirs(&self) -> Vec<String> {
        self.dirs.iter().cloned().collect()
    }

    /// How many files were written through the seam.
    pub fn writes(&self) -> u64 {
        self.writes
    }

    fn add_dirs(&mut self, rel_path: &str) {
        let mut dir = parent_of(rel_path);
        while !dir.is_empty() {
            self.dirs.insert(dir.to_owned());
            dir = parent_of(dir);
        }
    }
}

impl Fs for MemFs {
    fn read(&self, rel_path: &str) -> io::Result<Vec<u8>> {
        self.files
            .get(rel_path)
            .map(|(bytes, _)| bytes.clone())
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, rel_path.to_owned()))
    }

    fn write_atomic(&mut self, rel_path: &str, bytes: &[u8]) -> io::Result<()> {
        self.writes += 1;
        self.put(rel_path, bytes);
        Ok(())
    }

    fn remove(&mut self, rel_path: &str) -> io::Result<()> {
        self.files.remove(rel_path);
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        if self.fail_renames > 0 {
            self.fail_renames -= 1;
            return Err(io::Error::new(
                ErrorKind::PermissionDenied,
                "another program holds the file open",
            ));
        }
        if self.files.contains_key(to) {
            return Err(io::Error::new(ErrorKind::AlreadyExists, to.to_owned()));
        }
        let (bytes, mtime) = self
            .files
            .remove(from)
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, from.to_owned()))?;
        self.add_dirs(to);
        self.files.insert(to.to_owned(), (bytes, mtime));
        Ok(())
    }

    fn stat(&self, rel_path: &str) -> io::Result<Option<Stat>> {
        if let Some((bytes, mtime)) = self.files.get(rel_path) {
            return Ok(Some(Stat {
                is_dir: false,
                size: bytes.len() as u64,
                mtime_ms: *mtime,
            }));
        }
        if self.dirs.contains(rel_path) {
            return Ok(Some(Stat {
                is_dir: true,
                size: 0,
                mtime_ms: 0,
            }));
        }
        Ok(None)
    }

    fn list(&self) -> io::Result<Vec<Entry>> {
        Ok(self
            .files
            .iter()
            .map(|(path, (bytes, mtime))| Entry {
                rel_path: path.clone(),
                size: bytes.len() as u64,
                mtime_ms: *mtime,
            })
            .collect())
    }

    fn remove_empty_dirs(&mut self) -> io::Result<()> {
        let mut used = std::collections::BTreeSet::new();
        for path in self.files.keys() {
            let mut dir = parent_of(path);
            while !dir.is_empty() {
                used.insert(dir.to_owned());
                dir = parent_of(dir);
            }
        }
        self.dirs = used;
        Ok(())
    }
}

/// The real file system under a root folder.
#[derive(Debug)]
pub struct StdFs {
    root: PathBuf,
    temp_counter: AtomicU64,
}

impl StdFs {
    /// A folder that exists, or will be created.
    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            temp_counter: AtomicU64::new(0),
        })
    }

    /// The root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The absolute path of a relative one. A path that would leave the root is refused.
    pub fn full(&self, rel_path: &str) -> io::Result<PathBuf> {
        let relative = Path::new(rel_path);
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)) && !matches!(c, Component::CurDir))
        {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("{rel_path} leaves the folder"),
            ));
        }
        Ok(self.root.join(relative))
    }

    fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        Some(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    fn walk(&self, dir: &Path, out: &mut Vec<Entry>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                self.walk(&path, out)?;
            } else if let Some(rel_path) = self.relative(&path) {
                let meta = entry.metadata()?;
                out.push(Entry {
                    rel_path,
                    size: meta.len(),
                    mtime_ms: mtime_ms(&meta),
                });
            }
        }
        Ok(())
    }

    fn prune(&self, dir: &Path) -> io::Result<bool> {
        let mut empty = true;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() && !file_type.is_symlink() && self.prune(&entry.path())? {
                fs::remove_dir(entry.path())?;
                continue;
            }
            empty = false;
        }
        Ok(empty)
    }
}

fn mtime_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

impl Fs for StdFs {
    fn read(&self, rel_path: &str) -> io::Result<Vec<u8>> {
        fs::read(self.full(rel_path)?)
    }

    fn write_atomic(&mut self, rel_path: &str, bytes: &[u8]) -> io::Result<()> {
        let target = self.full(rel_path)?;
        let dir = target.parent().unwrap_or(&self.root);
        fs::create_dir_all(dir)?;
        let n = self.temp_counter.fetch_add(1, Ordering::Relaxed);
        let temp = dir.join(format!(
            ".{}.tmp-{}-{n}",
            name_of(rel_path),
            std::process::id()
        ));
        let result = (|| {
            let mut file = fs::File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn remove(&mut self, rel_path: &str) -> io::Result<()> {
        match fs::remove_file(self.full(rel_path)?) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> io::Result<()> {
        let target = self.full(to)?;
        if target.exists() {
            return Err(io::Error::new(ErrorKind::AlreadyExists, to.to_owned()));
        }
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::rename(self.full(from)?, target)
    }

    fn stat(&self, rel_path: &str) -> io::Result<Option<Stat>> {
        match fs::symlink_metadata(self.full(rel_path)?) {
            Ok(meta) => Ok(Some(Stat {
                is_dir: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                mtime_ms: mtime_ms(&meta),
            })),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn list(&self) -> io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        self.walk(&self.root.clone(), &mut out)?;
        out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        Ok(out)
    }

    fn remove_empty_dirs(&mut self) -> io::Result<()> {
        self.prune(&self.root.clone())?;
        Ok(())
    }
}
