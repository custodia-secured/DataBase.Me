// SPDX-License-Identifier: AGPL-3.0-or-later
//! The PDF importer on arbitrary bytes (M6 Task 9, brief §10: fuzzing for the importers). The
//! property is the proptest layer's (`crates/ingest/tests/fuzz_pdf.rs`): it returns, always as
//! one `Document` with the original attached, and never panics. The text budget is two seconds,
//! under libFuzzer's own timeout of ten.
//!
//! This target is stricter than that layer: libfuzzer-sys's panic hook aborts before unwinding, so
//! a panic in `pdf-extract` is a crash here even where the importer's `catch_unwind` contains it
//! (M6 Decision 17).

#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let extract = dbme_ingest::probe::pdf("fuzz.pdf", data, Duration::from_secs(2));
    assert_eq!(extract.objects.len(), 1);
    assert_eq!(extract.objects[0].attachments.len(), 1);
});
