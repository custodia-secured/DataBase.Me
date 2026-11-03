// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rows become objects (M6 acceptance): a CSV and a workbook become `Dataset`s with one typed
//! object per row, kinds inferred per column, an existing type reused with its kinds ruling, a
//! misfit cell kept as text and reported, the title column, delimiters and ragged rows, and
//! 10,000 rows in one run and one undo.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dbme_core::model::{AttachmentRole, FieldKind, TypeRow};
use dbme_core::{Clock, Id, ManualClock};
use dbme_ingest::tables::KEPT_AS_TEXT;
use dbme_ingest::{ImportOptions, Importers, apply, plan};
use dbme_store::{NewField, NewType, Vault, VaultOptions};
use serde_json::json;

use common::{fixture, workbook};

fn vault(dir: &Path) -> Vault {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_128_000_000));
    let options = VaultOptions {
        identity: dbme_store::Identity::generate(clock.as_ref()).unwrap(),
        clock,
    };
    Vault::create(&dir.join("Test.dbme"), options).unwrap()
}

fn type_named(vault: &Vault, name: &str) -> TypeRow {
    vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no type {name}"))
}

fn kinds_of(vault: &Vault, type_id: Id) -> Vec<(String, FieldKind)> {
    vault
        .read()
        .fields(type_id)
        .unwrap()
        .into_iter()
        .map(|f| (f.key, f.kind))
        .collect()
}

/// The workbook on disk: written once from the builder, then kept as the fixture.
fn people_xlsx() -> PathBuf {
    let path = fixture("xlsx/people.xlsx");
    if !path.exists() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, workbook()).unwrap();
    }
    assert_eq!(
        fs::read(&path).unwrap(),
        workbook(),
        "the fixture is the builder's output"
    );
    path
}

#[test]
fn a_csv_becomes_a_dataset_with_one_typed_object_per_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let plan = plan(
        &vault,
        &[fixture("csv/people.csv")],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.entries[0].importer, "csv");
    assert_eq!(
        (plan.objects, plan.transactions),
        (4, 1),
        "the dataset and three rows"
    );
    let draft = &plan.entries[0].extract.types[0];
    assert_eq!(draft.name, "people");
    let kinds: Vec<(&str, FieldKind)> = draft
        .fields
        .iter()
        .map(|f| (f.key.as_str(), f.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("email", FieldKind::Email),
            ("website", FieldKind::Url),
            ("joined", FieldKind::Date),
            ("active", FieldKind::Boolean),
            ("score", FieldKind::Number),
            ("notes", FieldKind::Text),
        ],
        "the title column is not a field; one misfit in ten is allowed"
    );
    assert_eq!(draft.fields[0].label, "Email");
    assert_eq!(plan.warnings.len(), 3, "{:?}", plan.warnings);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("\"Joined\"") && w.contains("did not fit date"))
    );

    let run = apply(&mut vault, &plan).unwrap();
    let read = vault.read();
    let people = type_named(&vault, "people");
    assert_eq!(kinds_of(&vault, people.id).len(), 6);
    let dataset = read.object(run.created[0]).unwrap();
    assert_eq!(dataset.title, "people");
    assert_eq!(
        read.type_row(read.object_type_ids(dataset.id).unwrap()[0])
            .unwrap()
            .name,
        "Dataset"
    );
    assert_eq!(
        read.attachments(dataset.id).unwrap()[0].role,
        AttachmentRole::Original
    );
    let rows = read.children(Some(dataset.id)).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Ada Lovelace", "Bob", "Cy"]
    );
    let ada = &rows[0];
    assert_eq!(ada.props_json["email"], "ada@example.org");
    assert_eq!(ada.props_json["website"], "https://example.org/ada");
    assert_eq!(ada.props_json["joined"], "2025-01-15");
    assert_eq!(ada.props_json["active"], true);
    assert_eq!(ada.props_json["score"], 9.5);
    assert_eq!(ada.props_json["notes"], "Likes \"engines\"\nand poetry");
    assert!(!ada.props_json.contains_key(KEPT_AS_TEXT));
    let bob = &rows[1];
    assert_eq!(bob.props_json["score"], 7);
    assert_eq!(bob.props_json["active"], false);
    assert!(
        !bob.props_json.contains_key("website"),
        "an empty cell is no value"
    );
    let cy = &rows[2];
    assert_eq!(cy.props_json["active"], true, "yes is a boolean");
    assert_eq!(
        cy.props_json[KEPT_AS_TEXT],
        json!({"email": "not-an-email", "joined": "2025-02-30", "score": "n/a"})
    );
    for (n, row) in rows.iter().enumerate() {
        let types = read.object_type_ids(row.id).unwrap();
        assert_eq!(types, [people.id]);
        let provenance = read.provenance_of(row.id).unwrap();
        assert_eq!(provenance[0].source_kind, "import:csv");
        assert!(
            provenance[0]
                .source_uri
                .as_deref()
                .unwrap()
                .ends_with(&format!("people.csv#row={}", n + 1))
        );
        assert_eq!(
            provenance[0].original_blob,
            Some(read.attachments(dataset.id).unwrap()[0].blob_hash.clone())
        );
    }
    assert!((read.provenance_of(cy.id).unwrap()[0].confidence - 0.8).abs() < f64::EPSILON);
    assert!((read.provenance_of(ada.id).unwrap()[0].confidence - 1.0).abs() < f64::EPSILON);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);

    // One undo step takes the rows, the dataset and the type with them.
    assert!(vault.undo().unwrap().is_some());
    assert!(
        vault
            .read()
            .types()
            .unwrap()
            .iter()
            .all(|t| t.name != "people")
    );
    assert!(vault.read().object(run.created[0]).is_err());
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn an_existing_type_is_reused_and_the_kinds_of_its_fields_rule() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    vault
        .transact(|txn| {
            let people = txn.create_type(NewType {
                name: "people".into(),
                ..NewType::default()
            })?;
            txn.create_field(NewField {
                type_id: people,
                key: "score".into(),
                label: "Score".into(),
                kind: FieldKind::Text,
                options: json!({}),
                required: false,
            })?;
            txn.create_field(NewField {
                type_id: people,
                key: "active".into(),
                label: "Active".into(),
                kind: FieldKind::Number,
                options: json!({}),
                required: false,
            })?;
            Ok(people)
        })
        .unwrap();
    let plan = plan(
        &vault,
        &[fixture("csv/people.csv")],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    let run = apply(&mut vault, &plan).unwrap();
    let people = type_named(&vault, "people");
    let kinds = kinds_of(&vault, people.id);
    assert_eq!(
        kinds[0],
        ("score".to_owned(), FieldKind::Text),
        "the kind it had"
    );
    assert_eq!(kinds[1], ("active".to_owned(), FieldKind::Number));
    assert_eq!(kinds.len(), 6, "the four it lacked were added");
    let rows = vault.read().children(Some(run.created[0])).unwrap();
    assert_eq!(
        rows[0].props_json["score"], "9.5",
        "text, as the field says"
    );
    assert_eq!(rows[2].props_json["score"], "n/a", "and so n/a fits now");
    assert_eq!(
        rows[0].props_json[KEPT_AS_TEXT]["active"], "true",
        "a boolean does not fit a number"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("3 cells of \"Active\" did not fit number")),
        "{:?}",
        plan.warnings
    );
    assert_eq!(
        vault
            .read()
            .types()
            .unwrap()
            .iter()
            .filter(|t| t.name == "people")
            .count(),
        1
    );
}

#[test]
fn semicolons_a_bom_ragged_rows_and_a_chosen_title_column() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let options = ImportOptions {
        title_column: Some("note".into()),
        ..ImportOptions::default()
    };
    let plan = plan(
        &vault,
        &[fixture("csv/semicolon.csv"), fixture("csv/bom-ragged.csv")],
        options,
        &Importers::v0(),
    )
    .unwrap();
    let semicolon = &plan.entries[0].extract;
    let kinds: Vec<FieldKind> = semicolon.types[0].fields.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FieldKind::Number, FieldKind::Date], "split on ;");
    assert_eq!(
        semicolon.objects[0].children[0].title, "1",
        "no column named note: the first column"
    );
    let ragged = &plan.entries[1].extract;
    assert_eq!(
        ragged.types[0].fields.len(),
        1,
        "name is the only field once note is the title"
    );
    assert_eq!(
        ragged.types[0].fields[0].key, "name",
        "the BOM is not part of the key"
    );
    let titles: Vec<&str> = ragged.objects[0]
        .children
        .iter()
        .map(|o| o.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["line one\nline two", "x", "Row 3"],
        "an empty title cell is Row n"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.contains("2 row(s) had more or fewer cells")),
        "{:?}",
        plan.warnings
    );
    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!(run.objects, 2 + 2 + 3);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn a_workbook_becomes_a_dataset_per_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let path = people_xlsx();
    let plan = plan(&vault, &[path], ImportOptions::default(), &Importers::v0()).unwrap();
    assert_eq!(plan.entries[0].importer, "xlsx");
    let file = &plan.entries[0].extract.objects[0];
    assert_eq!(
        (file.title.as_str(), file.type_name.as_str()),
        ("people", "Dataset")
    );
    assert_eq!(file.attachments.len(), 1);
    let sheets: Vec<&str> = file.children.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(sheets, ["People", "Totals"]);
    let drafts = &plan.entries[0].extract.types;
    let kinds: Vec<(&str, FieldKind)> = drafts[0]
        .fields
        .iter()
        .map(|f| (f.key.as_str(), f.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("when", FieldKind::Datetime),
            ("amount", FieldKind::Number),
            ("total", FieldKind::Number),
            ("empty", FieldKind::Text),
        ],
        "a date beside a datetime makes the column datetime"
    );
    assert!(
        drafts[1].fields.is_empty(),
        "Totals has only its title column"
    );
    let people = &file.children[0];
    assert_eq!(people.children.len(), 3);
    assert_eq!(people.children[0].props["when"], "2025-12-01T00:00:00Z");
    assert_eq!(people.children[1].props["when"], "2025-12-02T12:00:00Z");
    assert_eq!(people.children[0].props["amount"], 12.5);
    assert_eq!(
        people.children[0].props["total"], 25,
        "the formula's cached value"
    );
    assert!(
        people.children[2].props.is_empty(),
        "Cy's row is only a title"
    );
    assert!(
        people.children[0]
            .provenance
            .source_uri
            .ends_with("people.xlsx#sheet=People&row=1")
    );
    assert_eq!(file.children[1].children[0].title, "42");
    assert_eq!(plan.objects, 1 + 2 + 3 + 1);

    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!(run.objects, 7);
    let read = vault.read();
    let sheets = read.children(Some(run.created[0])).unwrap();
    assert_eq!(sheets.len(), 2);
    let rows = read.children(Some(sheets[0].id)).unwrap();
    assert_eq!(rows[1].props_json["when"], "2025-12-02T12:00:00Z");
    assert_eq!(
        read.provenance_of(sheets[0].id).unwrap()[0]
            .source_uri
            .as_deref()
            .unwrap()
            .rsplit('#')
            .next(),
        Some("sheet=People")
    );
    assert_eq!(type_named(&vault, "People").name, "People");
    assert_eq!(type_named(&vault, "Totals").name, "Totals");
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn ten_thousand_rows_import_in_one_run_and_one_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let mut csv = String::from("Id,Title,Amount,Due,Done\n");
    for i in 0..10_000 {
        csv.push_str(&format!(
            "{i},Item {i},{}.5,2025-01-{:02},{}\n",
            i % 100,
            i % 28 + 1,
            i % 2 == 0
        ));
    }
    let path = dir.path().join("big.csv");
    fs::write(&path, csv).unwrap();
    let options = ImportOptions {
        title_column: Some("Title".into()),
        ..ImportOptions::default()
    };
    let started = Instant::now();
    let plan = plan(&vault, &[path], options, &Importers::v0()).unwrap();
    let planned = started.elapsed();
    assert_eq!(
        (plan.objects, plan.transactions),
        (10_001, 1),
        "one entry is one step"
    );
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let started = Instant::now();
    let run = apply(&mut vault, &plan).unwrap();
    let applied = started.elapsed();
    println!("10,000 rows: planned in {planned:?}, applied in {applied:?} (debug build)");
    assert_eq!((run.objects, run.transactions), (10_001, 1));
    let read = vault.read();
    let rows = read.children(Some(run.created[0])).unwrap();
    assert_eq!(rows.len(), 10_000);
    assert_eq!(rows[9_999].title, "Item 9999");
    assert_eq!(rows[9_999].props_json["id"], 9_999);
    assert_eq!(rows[9_999].props_json["done"], false);
    assert_eq!(rows[9_999].props_json["due"], "2025-01-04");
    assert!(vault.undo().unwrap().is_some());
    assert!(vault.read().object(run.created[0]).is_err());
    assert!(!vault.can_undo());
}

#[test]
fn what_cannot_be_read_is_refused_and_what_can_be_is_read_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let vault = vault(dir.path());
    let empty = dir.path().join("empty.csv");
    fs::write(&empty, b"").unwrap();
    let not_a_book = dir.path().join("bad.xlsx");
    fs::write(&not_a_book, b"PK\x03\x04junk").unwrap();
    let latin = dir.path().join("latin.csv");
    fs::write(&latin, b"name,city\nJos\xe9,Lyon\n").unwrap();
    let plan = plan(
        &vault,
        &[empty.clone(), not_a_book.clone(), latin],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.refused.len(), 2, "{:?}", plan.refused);
    assert!(plan.refused[0].reason.contains("no header row"));
    assert_eq!(plan.refused[1].path, not_a_book);
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(
        plan.entries[0].extract.objects[0].children[0].title,
        "Jos\u{FFFD}"
    );
    assert!(
        plan.warnings.iter().any(|w| w.contains("not UTF-8")),
        "{:?}",
        plan.warnings
    );
}
