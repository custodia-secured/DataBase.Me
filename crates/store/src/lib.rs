// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme-store`: the SQLite vault.
//!
//! A [`Vault`] is a folder holding `vault.sqlite` and a content-addressed blob store (FORMAT.md
//! §1). Every change goes through a [`Txn`] and is recorded as ops in a hash-chained log in the
//! same SQLite transaction, so the tables can always be rebuilt from the log ([`Vault::verify`]
//! checks that they are). Each transaction is one undo step; [`Vault::undo`] reverses it by
//! applying the inverse ops, and refuses if anything has changed since.
//!
//! All SQL in DataBase.Me lives in this crate.

mod blobs;
mod error;
mod identity;
mod log;
mod migrate;
mod query;
mod rows;
mod sql;
mod txn;
mod vault;
mod verify;

pub use blobs::{BlobStore, StoredBlob};
pub use error::{Result, StoreError};
pub use identity::{IDENTITY_FILE, Identity};
pub use migrate::{APPLICATION_ID, SCHEMA_VERSION};
pub use query::Reader;
pub use txn::{NewAttachment, NewBlock, NewField, NewObject, NewRelation, NewType, Position, Txn};
pub use vault::{
    BLOBS_FOLDER, Committed, DATABASE_FILE, FORMAT_FILE, LOCK_FILE, VAULT_EXTENSION, Vault,
    VaultOptions,
};
pub use verify::VerifyReport;
