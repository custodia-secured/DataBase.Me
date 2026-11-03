// SPDX-License-Identifier: AGPL-3.0-or-later
//! What an importer is given to decide whether a file is its kind: the name, the extension, the
//! size and the media type `infer` reads from the first bytes (M6 gate 1d: PDF, pictures and zips
//! by magic; CSV by extension, since it has none).

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::IngestError;

/// How many leading bytes the sniff reads.
const HEAD: usize = 8 * 1024;

/// A file as seen before any importer reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Sniffed {
    /// The file's name, with its extension.
    pub file_name: String,
    /// The extension in lower case, without the dot, if there is one.
    pub extension: Option<String>,
    /// The size in bytes.
    pub size: u64,
    /// The media type `infer` names from the first bytes, if it knows the signature.
    pub mime: Option<String>,
}

impl Sniffed {
    /// Reads the first bytes of `path`, a file whose size is `size`.
    pub(crate) fn of(path: &Path, size: u64) -> Result<Self, IngestError> {
        let mut head = vec![0u8; HEAD.min(size as usize)];
        if !head.is_empty() {
            let mut file = File::open(path).map_err(|e| IngestError::io("opening", path, e))?;
            let mut filled = 0;
            while filled < head.len() {
                match file.read(&mut head[filled..]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(IngestError::io("reading", path, e)),
                }
            }
            head.truncate(filled);
        }
        Ok(Self::from_parts(path, size, &head))
    }

    /// The sniff of a file whose first bytes are `head`.
    pub fn from_parts(path: &Path, size: u64, head: &[u8]) -> Self {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
        let mime = infer::get(head).map(|t| t.mime_type().to_owned());
        Self {
            file_name,
            extension,
            size,
            mime,
        }
    }

    /// `true` if the extension is one of `extensions` (lower case, without dots).
    pub fn has_extension(&self, extensions: &[&str]) -> bool {
        self.extension
            .as_deref()
            .is_some_and(|e| extensions.contains(&e))
    }

    /// `true` if the media type starts with `prefix`, such as `image/`.
    pub fn mime_starts_with(&self, prefix: &str) -> bool {
        self.mime.as_deref().is_some_and(|m| m.starts_with(prefix))
    }

    /// The file's name without its extension, or the whole name when it has none.
    pub fn stem(&self) -> &str {
        match &self.extension {
            Some(ext) if self.file_name.len() > ext.len() + 1 => {
                &self.file_name[..self.file_name.len() - ext.len() - 1]
            }
            _ => &self.file_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Sniffed;

    #[test]
    fn names_extensions_stems_and_kinds() {
        let s = Sniffed::from_parts(Path::new("C:/in/Report.PDF"), 9, b"%PDF-1.4\n");
        assert_eq!(s.file_name, "Report.PDF");
        assert_eq!(s.extension.as_deref(), Some("pdf"));
        assert_eq!(s.stem(), "Report");
        assert_eq!(s.mime.as_deref(), Some("application/pdf"));
        assert!(s.has_extension(&["pdf", "csv"]) && !s.has_extension(&["png"]));
        assert!(s.mime_starts_with("application/") && !s.mime_starts_with("image/"));

        let s = Sniffed::from_parts(Path::new("/in/archive.tar.gz"), 0, b"");
        assert_eq!(
            (s.extension.as_deref(), s.stem()),
            (Some("gz"), "archive.tar")
        );
        assert_eq!(s.mime, None);
        let s = Sniffed::from_parts(Path::new("/in/.bashrc"), 0, b"");
        assert_eq!((s.extension.as_deref(), s.stem()), (None, ".bashrc"));
        let s = Sniffed::from_parts(Path::new("/in/notes.csv"), 5, b"a,b\n1,2\n");
        assert_eq!(s.mime, None, "CSV has no magic");
    }
}
