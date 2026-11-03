// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ingestion (ADR-0026, M6): importers that turn files into objects with provenance.
//!
//! A run is a [`plan`] and then an [`apply`]. The plan reads every source, lets the importer that
//! accepts it say what it would make ([`Extract`]), finds duplicates by content hash, and writes
//! nothing: it is the dry run and the preview. The apply writes the plan to the vault in one
//! transaction (several, past [`TRANSACTION_OBJECTS`] objects), which is the undo step, and puts
//! a [`dbme_core::model::ProvenanceRow`] on every object it makes. Imports land under the parent
//! the caller names, or under a new "Import <date>" note that holds the run's receipt.
//!
//! The importers of v0 are the [`generic`] one (any file becomes a `Document` or `Media` with the
//! file attached) and, in later tasks, tables, PDFs, images and Markdown folders. Every importer
//! is pure Rust and reads nothing but its source.

pub mod apply;
pub mod documents;
pub mod generic;
pub mod images;
pub mod importer;
pub mod plan;
pub mod sniff;
pub mod source;
pub mod tables;

mod error;

pub use apply::{ImportRun, apply, date_of, datetime_of};
pub use error::IngestError;
pub use importer::{Ctx, Extract, ExtractedObject, Importer, Importers, TypeDraft};
pub use plan::{Duplicate, DuplicatePolicy, ImportOptions, ImportPlan, PlanEntry, Refused, plan};
pub use sniff::Sniffed;
pub use source::{Source, SourceKind};

/// The `extractor` written into every provenance row this crate makes.
pub const EXTRACTOR: &str = "dbme-ingest";

/// The `extractor_version` beside it: this crate's version.
pub const EXTRACTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A source larger than this is refused before it is read (M6.md Design, caps).
pub const MAX_SOURCE_BYTES: u64 = 256 << 20;

/// How many objects one transaction holds; a larger run is split into several undo steps, and
/// the plan says so (M6 Decision 7).
pub const TRANSACTION_OBJECTS: usize = 10_000;
