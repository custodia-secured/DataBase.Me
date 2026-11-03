// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CSV importer on arbitrary bytes (M6 Task 9, brief §10: fuzzing for the importers). The
//! property is the proptest layer's (`crates/ingest/tests/fuzz_csv.rs`): it returns, `Ok` or
//! `Err`, and never panics; an `Ok` holds one dataset whose rows all have titles.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(extract) = dbme_ingest::probe::csv("fuzz.csv", data) {
        assert_eq!(extract.objects.len(), 1);
        for row in &extract.objects[0].children {
            assert!(!row.title.is_empty());
        }
    }
});
