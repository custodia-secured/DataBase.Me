// SPDX-License-Identifier: AGPL-3.0-or-later
//! A thing to import: a file or a folder the person named, canonicalised, with its size checked
//! against the cap before anything reads it.

use std::fs;
use std::path::{Path, PathBuf};

use dbme_core::{ContentHash, ContentHasher};
use serde::{Deserialize, Serialize};

use crate::{IngestError, MAX_SOURCE_BYTES, Sniffed};

/// A file or a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SourceKind {
    /// One file.
    File,
    /// A folder, imported with its structure.
    Folder,
}

/// A source as the plan sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
// The store has a `Source` too (a list's); the bindings need distinct names.
#[serde(rename = "ImportSource")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Source {
    /// The canonical path.
    pub path: PathBuf,
    /// File or folder.
    pub kind: SourceKind,
    /// The sniff of a file; a folder has none.
    pub sniffed: Option<Sniffed>,
}

impl Source {
    /// Looks at `path`: it must exist and be a file under the cap, or a folder.
    pub fn open(path: &Path) -> Result<Self, IngestError> {
        let path = path
            .canonicalize()
            .map_err(|_| IngestError::NotASource(path.to_owned()))?;
        let meta = fs::metadata(&path).map_err(|e| IngestError::io("reading", &path, e))?;
        if meta.is_dir() {
            return Ok(Self {
                path,
                kind: SourceKind::Folder,
                sniffed: None,
            });
        }
        if !meta.is_file() {
            return Err(IngestError::NotASource(path));
        }
        let size = meta.len();
        if size > MAX_SOURCE_BYTES {
            return Err(IngestError::TooLarge {
                path,
                size,
                cap: MAX_SOURCE_BYTES,
            });
        }
        let sniffed = Sniffed::of(&path, size)?;
        Ok(Self {
            path,
            kind: SourceKind::File,
            sniffed: Some(sniffed),
        })
    }

    /// A file that exists only as bytes a caller holds, for the fuzzers' probes: it has a name
    /// and no sniff, and [`Source::read`] refuses it.
    pub(crate) fn in_memory(name: &str) -> Self {
        Self {
            path: PathBuf::from(name),
            kind: SourceKind::File,
            sniffed: None,
        }
    }

    /// The path as text, forward slashes, for `source_uri` and for people.
    pub fn uri(&self) -> String {
        shown(&self.path)
    }

    /// The file's name without its extension, for a title; a folder's whole name.
    pub fn stem(&self) -> String {
        if let Some(sniffed) = &self.sniffed {
            return sniffed.stem().to_owned();
        }
        match self.kind {
            SourceKind::File => self.path.file_stem(),
            SourceKind::Folder => self.path.file_name(),
        }
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
    }

    /// Reads the whole file (a folder has no bytes), checking the cap again against what is read.
    pub fn read(&self) -> Result<Vec<u8>, IngestError> {
        if self.kind != SourceKind::File {
            return Err(IngestError::NotASource(self.path.clone()));
        }
        let bytes = fs::read(&self.path).map_err(|e| IngestError::io("reading", &self.path, e))?;
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err(IngestError::TooLarge {
                path: self.path.clone(),
                size: bytes.len() as u64,
                cap: MAX_SOURCE_BYTES,
            });
        }
        Ok(bytes)
    }

    /// The hash of the file's content.
    pub fn hash(&self) -> Result<ContentHash, IngestError> {
        let mut hasher = ContentHasher::new();
        hasher.update(&self.read()?);
        Ok(hasher.finalize())
    }
}

/// A path as text, without Windows' verbatim prefix and with forward slashes.
pub(crate) fn shown(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| text.strip_prefix(r"\\?\").map(str::to_owned))
        .unwrap_or_else(|| text.into_owned());
    text.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::shown;

    #[test]
    fn paths_are_shown_without_the_verbatim_prefix_and_with_forward_slashes() {
        assert_eq!(shown(Path::new(r"\\?\C:\in\a.txt")), "C:/in/a.txt");
        assert_eq!(
            shown(Path::new(r"\\?\UNC\server\share\a")),
            "//server/share/a"
        );
        assert_eq!(shown(Path::new("/home/x/a.txt")), "/home/x/a.txt");
    }
}

#[cfg(test)]
mod open_tests {
    use std::fs;

    use super::{Source, SourceKind};
    use crate::IngestError;

    #[test]
    fn files_folders_and_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, b"hello").unwrap();
        let source = Source::open(&file).unwrap();
        assert_eq!(source.kind, SourceKind::File);
        assert_eq!(source.stem(), "a");
        assert_eq!(source.read().unwrap(), b"hello");
        assert_eq!(source.hash().unwrap(), dbme_core::ContentHash::of(b"hello"));
        assert!(source.uri().ends_with("/a.txt"), "{}", source.uri());
        assert!(!source.uri().contains('\\'));

        let folder = dir.path().join("sub");
        fs::create_dir(&folder).unwrap();
        let source = Source::open(&folder).unwrap();
        assert_eq!(source.kind, SourceKind::Folder);
        assert_eq!(source.sniffed, None);
        assert_eq!(source.stem(), "sub");
        assert!(matches!(source.read(), Err(IngestError::NotASource(_))));

        let missing = dir.path().join("missing");
        assert!(matches!(
            Source::open(&missing),
            Err(IngestError::NotASource(p)) if p == missing
        ));
    }
}
