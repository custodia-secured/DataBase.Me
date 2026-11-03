// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ingestion (ADR-0026, M6): importers that turn files into objects with provenance.
//!
//! M6 Task 1 lays down the crate and the parsers the gates probe (`tests/gate_*.rs`); the
//! importer contract, the plan and the apply follow in Task 3.

/// The `extractor` written into every provenance row this crate makes.
pub const EXTRACTOR: &str = "dbme-ingest";

/// The `extractor_version` beside it: this crate's version.
pub const EXTRACTOR_VERSION: &str = env!("CARGO_PKG_VERSION");
