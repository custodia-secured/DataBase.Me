// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one error type of the domain layer.

/// Everything that can go wrong in `dbme-core`. None of these involve I/O.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A string is not a valid ULID.
    #[error("invalid ID {0:?}")]
    InvalidId(String),

    /// A string is not a valid fractional sort key.
    #[error("invalid sort key {0:?}")]
    InvalidSortKey(String),

    /// Two sort keys were given in the wrong order, or equal.
    #[error("sort key {0:?} is not before {1:?}")]
    SortKeyOrder(String, String),

    /// No key exists beyond the largest (or before the smallest) possible key.
    #[error("sort key space is exhausted at {0:?}")]
    SortKeyExhausted(String),

    /// A string is not a valid `<algorithm>:<hex>` hash.
    #[error("invalid hash {0:?}")]
    InvalidHash(String),

    /// A hash names an algorithm this version does not know. Readers must not guess.
    #[error("unsupported hash algorithm {0:?}")]
    UnknownHashAlgorithm(String),

    /// A value breaks a rule of the data model.
    #[error("invalid {what}: {reason}")]
    Invalid {
        /// What was being checked, for example `field value "due"`.
        what: String,
        /// Why it was rejected.
        reason: String,
    },

    /// An op kind string is not one this version understands.
    #[error("unknown op kind {0:?}")]
    UnknownOpKind(String),

    /// The stored data does not match what an op expects to find, so it cannot apply. This is how
    /// an undo refuses to overwrite a later edit.
    #[error("conflict on {entity} {key}: {reason}")]
    Conflict {
        /// The entity kind, for example `object`.
        entity: &'static str,
        /// The row's key.
        key: String,
        /// What did not match.
        reason: String,
    },

    /// The operating system's random number source failed.
    #[error("the random number source failed")]
    Random,

    /// JSON could not be read or written.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl CoreError {
    /// Shorthand for [`CoreError::Invalid`].
    pub fn invalid(what: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Invalid {
            what: what.into(),
            reason: reason.into(),
        }
    }
}
