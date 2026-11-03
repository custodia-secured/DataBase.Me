// SPDX-License-Identifier: AGPL-3.0-or-later
//! Import runs (ADR-0026): a plan writes nothing, an apply is one undo step that lands under the
//! parent named or a receipt note, every object gets its provenance, duplicates are reported and
//! skipped or attached, refused sources are listed, and a large run is split into steps.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbme_core::model::AttachmentRole;
use dbme_core::{Clock, Id, ManualClock};
use dbme_ingest::apply::date_of;
use dbme_ingest::importer::{Ctx, Extract, ExtractedObject, Importer, ProvenanceDraft};
use dbme_ingest::{
    DuplicatePolicy, EXTRACTOR, ImportOptions, Importers, IngestError, Sniffed, Source, apply, plan,
};
use dbme_store::{Identity, NewObject, Vault, VaultOptions};

const NOW: i64 = 1_762_128_000_000;

fn vault(dir: &Path) -> (Vault, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(NOW));
    let as_clock: Arc<dyn Clock> = clock.clone();
    let options = VaultOptions {
        identity: Identity::generate(as_clock.as_ref()).unwrap(),
        clock: as_clock,
    };
    (
        Vault::create(&dir.join("Test.dbme"), options).unwrap(),
        clock,
    )
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

/// A WAV header: `infer` says audio/x-wav, so the generic importer makes a Media.
const WAV: &[u8] = b"RIFF\x24\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0";

fn ops(vault: &Vault) -> u64 {
    vault.verify(false).unwrap().ops
}

#[test]
fn a_plan_writes_nothing_and_an_apply_lands_under_a_receipt_as_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let (mut vault, clock) = vault(dir.path());
    let a = file(dir.path(), "notes.txt", b"plain text");
    let b = file(dir.path(), "beep.wav", WAV);
    let before = ops(&vault);

    let plan = plan(
        &vault,
        &[a.clone(), b.clone()],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.entries.len(), 2);
    assert_eq!((plan.objects, plan.transactions), (2, 1));
    assert!(plan.duplicates.is_empty() && plan.refused.is_empty() && plan.warnings.is_empty());
    assert_eq!(plan.entries[0].importer, "file");
    let first = &plan.entries[0].extract.objects[0];
    assert_eq!(
        (first.title.as_str(), first.type_name.as_str()),
        ("notes", "Document")
    );
    assert_eq!(plan.entries[1].extract.objects[0].type_name, "Media");
    assert_eq!(first.attachments[0].role, AttachmentRole::Original);
    assert_eq!(first.attachments[0].filename.as_deref(), Some("notes.txt"));
    assert_eq!(
        first.provenance.original_blob,
        Some(first.attachments[0].hash.clone())
    );
    assert!(
        first.provenance.source_uri.ends_with("/notes.txt"),
        "{}",
        first.provenance.source_uri
    );
    // Nothing written, no blob stored.
    assert_eq!(ops(&vault), before);
    assert_eq!(
        vault.blobs().size(&first.attachments[0].hash).unwrap(),
        None
    );
    // The plan is what crosses IPC: it round-trips as JSON.
    let json = serde_json::to_string(&plan).unwrap();
    assert_eq!(
        serde_json::from_str::<dbme_ingest::ImportPlan>(&json).unwrap(),
        plan
    );

    clock.advance(1);
    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!(run.run_id, plan.run_id);
    assert_eq!(
        (run.objects, run.transactions, run.created.len()),
        (2, 1, 2)
    );
    let receipt = run.receipt.unwrap();
    assert_eq!(run.parent, receipt);
    let read = vault.read();
    let note = read.object(receipt).unwrap();
    assert_eq!(note.title, format!("Import {}", date_of(NOW + 1)));
    assert_eq!(note.parent_id, None);
    let body = read.blocks(receipt).unwrap();
    assert!(
        body[0]
            .content_json
            .to_string()
            .contains("2 objects from 2 sources, in 1 step.")
    );
    for (id, (title, type_name)) in run
        .created
        .iter()
        .zip([("notes", "Document"), ("beep", "Media")])
    {
        let object = read.object(*id).unwrap();
        assert_eq!(
            (object.title.as_str(), object.parent_id),
            (title, Some(receipt))
        );
        let types = read.object_type_ids(*id).unwrap();
        assert_eq!(read.type_row(types[0]).unwrap().name, type_name);
        let attachments = read.attachments(*id).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].role, AttachmentRole::Original);
        assert!(vault.blobs().check(&attachments[0].blob_hash).unwrap());
        let provenance = read.provenance_of(*id).unwrap();
        assert_eq!(provenance.len(), 1);
        assert_eq!(provenance[0].source_kind, "import:file");
        assert_eq!(provenance[0].import_run_id, Some(run.run_id));
        assert_eq!(
            provenance[0].original_blob,
            Some(attachments[0].blob_hash.clone())
        );
        assert_eq!(provenance[0].extractor, EXTRACTOR);
        assert!(provenance[0].accepted);
    }
    assert_eq!(
        read.provenance_of(receipt).unwrap()[0].source_kind,
        "import:receipt"
    );
    assert_eq!(read.provenance_of_run(run.run_id).unwrap().len(), 3);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);

    // One undo step takes the whole run back; redo brings it back.
    clock.advance(1);
    assert!(vault.undo().unwrap().is_some());
    assert!(vault.read().object(receipt).is_err());
    assert!(vault.read().object(run.created[0]).is_err());
    assert!(
        vault
            .read()
            .provenance_of_run(run.run_id)
            .unwrap()
            .is_empty()
    );
    assert!(vault.read().attachments(run.created[0]).unwrap().is_empty());
    assert!(ops(&vault) > before, "the undo is appended, never erased");
    assert!(!vault.can_undo(), "the run was the only step");
    clock.advance(1);
    assert!(vault.redo().unwrap().is_some());
    assert_eq!(vault.read().provenance_of_run(run.run_id).unwrap().len(), 3);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn imports_land_under_a_named_parent_without_a_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let (mut vault, _) = vault(dir.path());
    let (home, _) = vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Home".into(),
                ..NewObject::default()
            })
        })
        .unwrap();
    let a = file(dir.path(), "a.txt", b"a");
    let options = ImportOptions {
        parent: Some(home),
        ..ImportOptions::default()
    };
    let plan = plan(&vault, &[a], options, &Importers::v0()).unwrap();
    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!((run.parent, run.receipt), (home, None));
    assert_eq!(
        vault.read().object(run.created[0]).unwrap().parent_id,
        Some(home)
    );
    assert_eq!(vault.read().provenance_of_run(run.run_id).unwrap().len(), 1);

    // A parent that is not live is refused by the plan and by the apply.
    let gone = Id::from_parts(5, 5);
    let bad = ImportOptions {
        parent: Some(gone),
        ..ImportOptions::default()
    };
    let b = file(dir.path(), "b.txt", b"b");
    assert!(matches!(
        dbme_ingest::plan(&vault, &[b], bad, &Importers::v0()),
        Err(IngestError::NoSuchParent(id)) if id == gone
    ));
    let mut stale = plan.clone();
    stale.options.parent = Some(gone);
    assert!(matches!(
        apply(&mut vault, &stale),
        Err(IngestError::NoSuchParent(_))
    ));
}

#[test]
fn duplicates_are_reported_and_skipped_or_attached() {
    let dir = tempfile::tempdir().unwrap();
    let (mut vault, _) = vault(dir.path());
    let a = file(dir.path(), "a.txt", b"same bytes");
    let first = plan(
        &vault,
        std::slice::from_ref(&a),
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    let run = apply(&mut vault, &first).unwrap();
    let existing = run.created[0];

    // The same file again, a copy of it, two new identical files, and one new file.
    let copy = file(dir.path(), "copy.txt", b"same bytes");
    let twin1 = file(dir.path(), "twin1.txt", b"twins");
    let twin2 = file(dir.path(), "twin2.txt", b"twins");
    let fresh = file(dir.path(), "fresh.txt", b"fresh");
    let sources = [
        a.clone(),
        copy.clone(),
        twin1.clone(),
        twin2.clone(),
        fresh.clone(),
    ];
    let second = plan(&vault, &sources, ImportOptions::default(), &Importers::v0()).unwrap();
    assert_eq!(second.entries.len(), 2, "twin1 and fresh");
    assert_eq!(second.duplicates.len(), 3);
    assert_eq!(second.duplicates[0].existing, Some(existing));
    assert_eq!(second.duplicates[0].of, "a");
    assert_eq!(second.duplicates[1].existing, Some(existing));
    assert_eq!(
        second.duplicates[2].existing, None,
        "a duplicate inside the run"
    );
    assert!(
        second.duplicates[2].of.ends_with("/twin1.txt"),
        "{}",
        second.duplicates[2].of
    );
    assert_eq!(second.duplicates[2].path, twin2);
    let run = apply(&mut vault, &second).unwrap();
    assert_eq!(run.created.len(), 2);
    assert!(run.attached.is_empty());
    let receipt = vault.read().blocks(run.receipt.unwrap()).unwrap();
    let text = receipt
        .iter()
        .map(|b| b.content_json.to_string())
        .collect::<String>();
    assert!(
        text.contains("Already in the vault") && text.contains("the same file as a"),
        "{text}"
    );

    // Attach: the existing object is related to the parent instead.
    let options = ImportOptions {
        parent: None,
        duplicates: DuplicatePolicy::Attach,
        ..ImportOptions::default()
    };
    let third = plan(&vault, &[a], options, &Importers::v0()).unwrap();
    assert!(third.is_empty(), "nothing new to make");
    assert_eq!(third.transactions, 1, "the receipt and the relation");
    let run = apply(&mut vault, &third).unwrap();
    assert_eq!(run.attached, [existing]);
    assert!(run.created.is_empty());
    let relations = vault.read().relations_from(run.parent).unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].dst_id, existing);
    assert_eq!(relations[0].label.as_deref(), Some("already imported"));
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn refused_sources_are_listed_and_a_changed_file_stops_the_apply_before_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut vault, _) = vault(dir.path());
    let good = file(dir.path(), "good.txt", b"good");
    let folder = dir.path().join("folder");
    fs::create_dir(&folder).unwrap();
    let missing = dir.path().join("missing.txt");
    let plan = plan(
        &vault,
        &[missing.clone(), folder.clone(), good.clone()],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.refused.len(), 2);
    assert_eq!(plan.refused[0].path, missing);
    assert!(
        plan.refused[0].reason.contains("not a file or folder"),
        "{}",
        plan.refused[0].reason
    );
    assert!(
        plan.refused[1].reason.contains("Task 6"),
        "{}",
        plan.refused[1].reason
    );

    let before = ops(&vault);
    fs::write(&good, b"changed after the plan").unwrap();
    let error = apply(&mut vault, &plan).unwrap_err();
    assert!(matches!(error, IngestError::Changed(_)), "{error}");
    assert_eq!(ops(&vault), before, "nothing was written");
    assert!(!vault.can_undo());

    // An empty plan with no parent still makes its receipt, which lists what was refused.
    let plan = plan_of(&vault, std::slice::from_ref(&missing));
    assert!(plan.is_empty());
    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!(run.transactions, 1);
    let text = vault
        .read()
        .blocks(run.receipt.unwrap())
        .unwrap()
        .iter()
        .map(|b| b.content_json.to_string())
        .collect::<String>();
    assert!(
        text.contains("Not imported") && text.contains("missing.txt"),
        "{text}"
    );
    // And under a named parent, an empty plan writes nothing at all.
    let plan = dbme_ingest::plan(
        &vault,
        &[missing],
        ImportOptions {
            parent: Some(run.receipt.unwrap()),
            ..ImportOptions::default()
        },
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.transactions, 0);
    let before = ops(&vault);
    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!((run.transactions, ops(&vault)), (0, before));
}

fn plan_of(vault: &Vault, sources: &[PathBuf]) -> dbme_ingest::ImportPlan {
    plan(vault, sources, ImportOptions::default(), &Importers::v0()).unwrap()
}

/// A test importer: a `.many` file holding a number makes that many `Note`s under one object.
struct Many;

impl Importer for Many {
    fn id(&self) -> &'static str {
        "many"
    }

    fn accepts(&self, sniffed: &Sniffed) -> bool {
        sniffed.has_extension(&["many"])
    }

    fn plan(&self, source: &Source, _ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        let text = String::from_utf8_lossy(&source.read()?).trim().to_owned();
        let n: usize = text
            .parse()
            .map_err(|e| IngestError::unreadable(&source.path, e))?;
        let provenance = ProvenanceDraft::from_source(source, None);
        let mut root = ExtractedObject::new(&source.stem(), "Note", provenance.clone());
        root.children = (0..n)
            .map(|i| {
                let mut child =
                    ExtractedObject::new(&format!("Item {i}"), "Note", provenance.clone());
                child.provenance.confidence = 0.5;
                child
            })
            .collect();
        Ok(Extract {
            types: Vec::new(),
            objects: vec![root],
            warnings: vec![format!("{n} items")],
        })
    }
}

#[test]
fn a_large_run_is_split_into_transactions_of_ten_thousand_objects() {
    let dir = tempfile::tempdir().unwrap();
    let (mut vault, _) = vault(dir.path());
    let one = file(dir.path(), "one.many", b"5999");
    let two = file(dir.path(), "two.many", b"5999");
    let bad = file(dir.path(), "bad.many", b"not a number");
    let importers = Importers::v0().with_first(Box::new(Many));
    assert_eq!(
        importers.ids(),
        ["many", "csv", "xlsx", "pdf", "image", "file"]
    );
    let plan = plan(
        &vault,
        &[one, two, bad.clone()],
        ImportOptions::default(),
        &importers,
    )
    .unwrap();
    assert_eq!((plan.objects, plan.transactions), (12_000, 2));
    assert_eq!(plan.refused.len(), 1, "{:?}", plan.refused);
    assert!(
        plan.refused[0].reason.contains("invalid digit"),
        "{}",
        plan.refused[0].reason
    );
    assert_eq!(plan.warnings.len(), 2);
    assert!(
        plan.warnings[0].ends_with("one.many: 5999 items"),
        "{}",
        plan.warnings[0]
    );

    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!((run.objects, run.transactions), (12_000, 2));
    let read = vault.read();
    assert_eq!(read.subtree(run.created[0]).unwrap().len(), 6_000);
    let child = read.subtree(run.created[1]).unwrap()[1].id;
    let provenance = read.provenance_of(child).unwrap();
    assert_eq!(provenance[0].source_kind, "import:many");
    assert!((provenance[0].confidence - 0.5).abs() < f64::EPSILON);
    assert_eq!(read.provenance_of_run(run.run_id).unwrap().len(), 12_001);
    // Two undo steps: the second entry alone, then the receipt with the first.
    assert!(vault.undo().unwrap().is_some());
    assert!(vault.read().object(run.created[0]).is_ok());
    assert!(vault.read().object(run.created[1]).is_err());
    assert!(vault.undo().unwrap().is_some());
    assert!(vault.read().object(run.receipt.unwrap()).is_err());
    assert!(!vault.can_undo());
}

#[test]
fn a_plan_naming_an_unknown_type_writes_nothing() {
    struct Alien;
    impl Importer for Alien {
        fn id(&self) -> &'static str {
            "alien"
        }
        fn accepts(&self, sniffed: &Sniffed) -> bool {
            sniffed.has_extension(&["alien"])
        }
        fn plan(&self, source: &Source, _ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
            Ok(Extract {
                types: Vec::new(),
                objects: vec![ExtractedObject::new(
                    "x",
                    "Starship",
                    ProvenanceDraft::from_source(source, None),
                )],
                warnings: Vec::new(),
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut vault, _) = vault(dir.path());
    let a = file(dir.path(), "a.alien", b"x");
    let importers = Importers::v0().with_first(Box::new(Alien));
    let plan = plan(&vault, &[a], ImportOptions::default(), &importers).unwrap();
    assert_eq!(plan.entries[0].importer, "alien");
    let before = ops(&vault);
    let error = apply(&mut vault, &plan).unwrap_err();
    assert!(
        matches!(error, IngestError::NoSuchType(ref t) if t == "Starship"),
        "{error}"
    );
    assert_eq!(ops(&vault), before);
}
