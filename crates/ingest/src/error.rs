// SPDX-License-Identifier: AGPL-3.0-or-later
//! Everything that can stop a plan or an apply.

use std::path::PathBuf;

use dbme_core::CoreError;
use dbme_store::StoreError;

/// Why an import could not go on. An importer that cannot read a file returns one of these and
/// never panics; the plan then reports the source as refused and goes on with the others.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// The path names nothing, or something that is neither a file nor a folder.
    #[error("{0} is not a file or folder that can be imported")]
    NotASource(PathBuf),
    /// The file is larger than [`crate::MAX_SOURCE_BYTES`].
    #[error("{path} is {size} bytes, over the {cap}-byte limit")]
    TooLarge {
        /// The file.
        path: PathBuf,
        /// Its size.
        size: u64,
        /// The limit.
        cap: u64,
    },
    /// A file changed between the plan and the apply.
    #[error("{0} changed since the plan was made")]
    Changed(PathBuf),
    /// The vault has no type of this name, and the importer did not plan to create it.
    #[error("the vault has no type named {0:?}")]
    NoSuchType(String),
    /// The plan names a parent that is not a live object.
    #[error("the parent {0} is not a live object")]
    NoSuchParent(dbme_core::Id),
    /// The importer could not read the source for the reason given.
    #[error("{path}: {reason}")]
    Unreadable {
        /// The file.
        path: PathBuf,
        /// What the parser said.
        reason: String,
    },
    /// An I/O failure while reading a source.
    #[error("{what} {path}: {source}")]
    Io {
        /// What was being done.
        what: &'static str,
        /// The file.
        path: PathBuf,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
    /// A rule of the data model.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// The vault refused a change.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The files folder's engine, importing a folder, could not go on.
    #[error(transparent)]
    Mirror(#[from] dbme_mirror::MirrorError),
}

impl IngestError {
    pub(crate) fn io(what: &'static str, path: &std::path::Path, source: std::io::Error) -> Self {
        Self::Io {
            what,
            path: path.to_owned(),
            source,
        }
    }

    /// The error an importer returns for a file its parser refused.
    pub fn unreadable(path: &std::path::Path, reason: impl ToString) -> Self {
        Self::Unreadable {
            path: path.to_owned(),
            reason: reason.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::IngestError;

    #[test]
    fn errors_say_what_and_where() {
        let path = Path::new("a.pdf");
        assert_eq!(
            IngestError::io("reading", path, std::io::Error::other("boom")).to_string(),
            "reading a.pdf: boom"
        );
        assert_eq!(
            IngestError::unreadable(path, "bad xref").to_string(),
            "a.pdf: bad xref"
        );
        assert_eq!(
            IngestError::TooLarge {
                path: PathBuf::from("big.bin"),
                size: 300,
                cap: 256
            }
            .to_string(),
            "big.bin is 300 bytes, over the 256-byte limit"
        );
        assert_eq!(
            IngestError::NoSuchType("Starship".into()).to_string(),
            "the vault has no type named \"Starship\""
        );
    }
}
