// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two-way files folder (ADR-0006, ADR-0021): a replica of the vault as Markdown files that
//! commits outside edits back through the store, like a second device would. This crate holds the
//! watcher, so it is desktop only and never a dependency of `dbme-core`.
//!
//! The engine, [`Mirror`], owns no thread and no timer. The caller drives it: [`Mirror::pump_out`]
//! writes the objects the store marked dirty, [`Mirror::rescan`] derives [`FileEvent`]s from the
//! folder's current state, and `pump_in` applies a batch of events (from a [`Watcher`] or from a
//! rescan) to the vault. File I/O goes through the [`Fs`] seam, so every rule is tested on the
//! in-memory [`MemFs`] and the real [`StdFs`] only has to be a faithful file system.

mod apply;
mod engine;
mod error;
mod fs;
mod inbound;
mod resolver;
mod watcher;

pub use engine::{FileEvent, MAX_RETRIES, Mirror, Outbound, Watcher, schema_of};
pub use error::{MirrorError, Result};
pub use fs::{Entry, Fs, MemFs, Stat, StdFs};
pub use inbound::{GUARD_COUNT, GUARD_PERCENT, HoldDecision, Inbound};
pub use resolver::VaultResolver;
pub use watcher::{DEFAULT_DEBOUNCE, NotifyWatcher};

/// The folder inside the files folder that the app owns: the schema, the trash.
pub const DBME_DIR: &str = ".dbme";
/// Where a trashed object's file waits for its restore: `.dbme/trash/<id>.md`.
pub const TRASH_DIR: &str = ".dbme/trash";
/// The schema the files can be read without the database with.
pub const SCHEMA_FILE: &str = ".dbme/schema.json";
/// Where attachment blobs are written for Obsidian to show.
pub const ATTACHMENTS_DIR: &str = "attachments";
