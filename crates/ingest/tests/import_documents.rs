// SPDX-License-Identifier: AGPL-3.0-or-later
//! PDF text (M6 acceptance): a PDF becomes a `Document` with its text by page, its title and
//! author and page count; a scan or a broken file is attached with a warning, never dropped; the
//! budget stops extraction between pages.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use dbme_core::model::AttachmentRole;
use dbme_core::{Clock, ManualClock};
use dbme_ingest::documents::Pdf;
use dbme_ingest::{ImportOptions, Importers, apply, plan};
use dbme_store::{Vault, VaultOptions};
use serde_json::json;

use common::{fixture_file, scanned_pdf, two_pages_pdf};

fn vault(dir: &Path) -> Vault {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_128_000_000));
    let options = VaultOptions {
        identity: dbme_store::Identity::generate(clock.as_ref()).unwrap(),
        clock,
    };
    Vault::create(&dir.join("Test.dbme"), options).unwrap()
}

fn field_keys(vault: &Vault, type_name: &str) -> Vec<String> {
    let read = vault.read();
    let t = read
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == type_name)
        .unwrap();
    read.fields(t.id)
        .unwrap()
        .into_iter()
        .map(|f| f.key)
        .collect()
}

#[test]
fn a_pdf_becomes_a_document_with_its_text_by_page_and_its_facts() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let path = fixture_file("pdf/two-pages.pdf", &two_pages_pdf());
    let plan = plan(&vault, &[path], ImportOptions::default(), &Importers::v0()).unwrap();
    assert_eq!(plan.entries[0].importer, "pdf");
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let object = &plan.entries[0].extract.objects[0];
    assert_eq!(object.title, "Two pages", "the document's own title");
    assert_eq!(object.type_name, "Document");
    assert_eq!(object.props["page_count"], 2);
    assert_eq!(object.props["author"], "DataBase.Me tests");
    let blocks: Vec<(&str, String)> = object
        .blocks
        .iter()
        .map(|b| {
            (
                b.kind.as_str(),
                b.content["inline"][0]["text"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        blocks,
        [
            ("heading", "Page 1".to_owned()),
            ("paragraph", "Hello gate one".to_owned()),
            ("heading", "Page 2".to_owned()),
            ("paragraph", "Page two here".to_owned()),
        ]
    );
    assert_eq!(object.attachments.len(), 1);
    assert_eq!(object.attachments[0].role, AttachmentRole::Original);
    assert_eq!(
        object.attachments[0].mime.as_deref(),
        Some("application/pdf")
    );
    assert!((object.provenance.confidence - 0.9).abs() < f64::EPSILON);
    assert_eq!(plan.entries[0].extract.types[0].name, "Document");

    let run = apply(&mut vault, &plan).unwrap();
    let keys = field_keys(&vault, "Document");
    assert!(keys.contains(&"page_count".to_owned()) && keys.contains(&"author".to_owned()));
    let read = vault.read();
    let doc = read.object(run.created[0]).unwrap();
    assert_eq!(doc.title, "Two pages");
    assert_eq!(doc.props_json["page_count"], 2);
    assert_eq!(read.blocks(doc.id).unwrap().len(), 4);
    assert_eq!(
        read.provenance_of(doc.id).unwrap()[0].source_kind,
        "import:pdf"
    );
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);

    // The undo takes the fields it added to the system type with it.
    assert!(vault.undo().unwrap().is_some());
    let keys = field_keys(&vault, "Document");
    assert!(!keys.contains(&"page_count".to_owned()), "{keys:?}");
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn a_scan_and_a_broken_file_are_attached_with_a_warning_never_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let scanned = fixture_file("pdf/scanned.pdf", &scanned_pdf());
    let broken = dir.path().join("broken.pdf");
    let whole = two_pages_pdf();
    fs::write(&broken, &whole[..whole.len() / 2]).unwrap();
    let junk = dir.path().join("junk.pdf");
    fs::write(&junk, b"hello, not a PDF").unwrap();

    let plan = plan(
        &vault,
        &[scanned, broken, junk],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.entries.len(), 3, "{:?}", plan.refused);
    assert!(plan.entries.iter().all(|e| e.importer == "pdf"));
    let scan = &plan.entries[0].extract.objects[0];
    assert!(scan.blocks.is_empty());
    assert_eq!(scan.props["page_count"], 1);
    assert!((scan.provenance.confidence - 0.5).abs() < f64::EPSILON);
    assert!(
        plan.warnings[0].contains("no text in 1 page"),
        "{}",
        plan.warnings[0]
    );
    for entry in &plan.entries[1..] {
        let object = &entry.extract.objects[0];
        assert!(object.blocks.is_empty());
        assert!(!object.props.contains_key("page_count"));
        assert_eq!(object.attachments.len(), 1, "the file is kept");
        assert!((object.provenance.confidence - 0.5).abs() < f64::EPSILON);
    }
    assert!(
        plan.warnings
            .iter()
            .filter(|w| w.contains("not a PDF the parser takes"))
            .count()
            == 2,
        "{:?}",
        plan.warnings
    );

    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!(run.objects, 3);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn the_budget_stops_extraction_between_pages() {
    let dir = tempfile::tempdir().unwrap();
    let vault = vault(dir.path());
    let path = fixture_file("pdf/two-pages.pdf", &two_pages_pdf());
    let importers = Importers::v0().with_first(Box::new(Pdf {
        budget: Duration::ZERO,
    }));
    let plan = plan(&vault, &[path], ImportOptions::default(), &importers).unwrap();
    let object = &plan.entries[0].extract.objects[0];
    assert!(object.blocks.is_empty());
    assert_eq!(
        object.props["page_count"], 2,
        "the count comes before the budget"
    );
    assert_eq!(object.title, "Two pages");
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("stopped after 0 of 2 pages")),
        "{:?}",
        plan.warnings
    );
    assert_eq!(
        object.props.get("author"),
        Some(&json!("DataBase.Me tests"))
    );
}
