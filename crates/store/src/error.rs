// SPDX-License-Identifier: AGPL-3.0-or-later
//! The store's error type.

use std::path::PathBuf;

use dbme_core::{ContentHash, CoreError};

/// Everything that can go wrong in `dbme-store`.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A domain rule was broken, or an op's precondition failed.
    #[error(transparent)]
    Core(#[from] CoreError),

    /// SQLite reported an error.
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),

    /// A file operation failed.
    #[error("{action} {path}: {source}")]
    Io {
        /// What was being done, for example `writing`.
        action: &'static str,
        /// The file or folder.
        path: PathBuf,
        /// The operating system's error.
        source: std::io::Error,
    },

    /// The path is not a DataBase.Me vault.
    #[error("{0} is not a DataBase.Me vault")]
    NotAVault(PathBuf),

    /// The vault was written by a newer version, which this one must not modify.
    #[error(
        "this vault has schema version {found}, but this version of DataBase.Me only understands up to {supported}; update the app to open it"
    )]
    TooNew {
        /// The vault's schema version.
        found: u32,
        /// The newest schema version this build knows.
        supported: u32,
    },

    /// Another process has the vault open.
    #[error("{0} is open in another window or process")]
    Locked(PathBuf),

    /// Something is already there.
    #[error("{0} already exists and is not an empty folder")]
    AlreadyExists(PathBuf),

    /// Vault folders are named `<name>.dbme`.
    #[error("vault folders must be named <name>.dbme, not {0}")]
    BadVaultName(PathBuf),

    /// A row the request refers to does not exist.
    #[error("{entity} {id} not found")]
    NotFound {
        /// The entity kind, for example `object`.
        entity: &'static str,
        /// Its ID.
        id: String,
    },

    /// The request breaks a rule of the vault, for example deleting a type still in use.
    #[error("{0}")]
    Rule(String),

    /// An undo or redo would overwrite a later change, so nothing was changed.
    #[error("cannot undo this step: {0}")]
    UndoBlocked(String),

    /// A blob the database refers to is not in the blob store.
    #[error("blob {0} is missing from the blob store")]
    MissingBlob(ContentHash),
}

impl StoreError {
    pub(crate) fn io(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }

    pub(crate) fn not_found(entity: &'static str, id: impl ToString) -> Self {
        Self::NotFound {
            entity,
            id: id.to_string(),
        }
    }

    /// Turns precondition and constraint failures met while reversing a step into
    /// [`StoreError::UndoBlocked`].
    pub(crate) fn blocking_undo(self) -> Self {
        match self {
            Self::Core(error @ CoreError::Conflict { .. }) => Self::UndoBlocked(error.to_string()),
            Self::Sql(rusqlite::Error::SqliteFailure(failure, message))
                if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::UndoBlocked(
                    message.unwrap_or_else(|| "other data now depends on it".to_owned()),
                )
            }
            other => other,
        }
    }
}

/// The store's result type.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;
