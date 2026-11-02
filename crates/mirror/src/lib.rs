// SPDX-License-Identifier: AGPL-3.0-or-later
//! The two-way files folder (ADR-0006): a replica of the vault as Markdown files that commits
//! outside edits back through the store, like a second device would. M5 builds it here; this
//! crate holds the watcher, so it is desktop only and never a dependency of `dbme-core`.
