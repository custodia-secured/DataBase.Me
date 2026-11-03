// SPDX-License-Identifier: AGPL-3.0-or-later
//! Entry points for the fuzz targets and the property tests (M6 Task 9, brief §10): the CSV and
//! PDF importers on bytes in memory, with no vault and no disk. The app never calls these: an
//! importer plans from a [`Source`] on disk with the vault's types in view. What they share with
//! the real path is everything after the read, so a panic found here is a panic in the importer.

use std::collections::HashMap;
use std::time::Duration;

use crate::documents::Pdf;
use crate::importer::Extract;
use crate::source::Source;
use crate::{IngestError, tables};

/// The CSV importer on `bytes`, as if they were the file `name`: the dataset, its rows and the
/// type they need, or the error the reader gave. Never panics.
///
/// # Errors
/// What the CSV reader refuses: a record it cannot parse, or no header row.
pub fn csv(name: &str, bytes: &[u8]) -> Result<Extract, IngestError> {
    tables::csv_extract(&Source::in_memory(name), bytes, &HashMap::new(), None)
}

/// The PDF importer on `bytes`, as if they were the file `name`, with `budget` for the text:
/// always a `Document` with the original attached, its text when the parser takes the file and a
/// warning when it does not. Never panics; returns inside the budget plus one page.
pub fn pdf(name: &str, bytes: &[u8], budget: Duration) -> Extract {
    Pdf { budget }.extract(&Source::in_memory(name), bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;

    #[test]
    fn the_probes_run_without_a_vault_or_a_disk() {
        let extract = csv("t.csv", b"name,n\nAda,1\n=cmd,2\n").unwrap();
        assert_eq!(extract.objects[0].children.len(), 2);
        assert_eq!(extract.types[0].name, "t");
        assert!(
            extract
                .warnings
                .iter()
                .any(|w| w.contains("like a formula"))
        );
        assert!(csv("t.csv", b"").is_err());

        let extract = pdf("t.pdf", b"not a pdf", Duration::from_secs(1));
        assert_eq!(extract.objects.len(), 1);
        assert_eq!(extract.objects[0].attachments.len(), 1);
        assert!(extract.warnings[0].contains("not a PDF"));
    }
}
