// SPDX-License-Identifier: AGPL-3.0-or-later
//! The mirror's error type.

use dbme_store::StoreError;

/// Everything that can stop the engine. A problem with one file is never an error: it becomes an
/// inbox entry and the engine goes on; an error is the vault or the folder itself failing.
#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    /// The store refused or failed.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// The folder could not be read or written.
    #[error("{action} {path}: {source}")]
    Io {
        /// What was being done, for example `writing`.
        action: &'static str,
        /// The path inside the folder, with forward slashes.
        path: String,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// The vault's rows could not be turned into a file.
    #[error(transparent)]
    Core(#[from] dbme_core::CoreError),
}

impl MirrorError {
    pub(crate) fn io(action: &'static str, path: &str, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

/// The mirror's result type.
pub type Result<T, E = MirrorError> = std::result::Result<T, E>;
