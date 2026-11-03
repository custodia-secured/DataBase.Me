// SPDX-License-Identifier: AGPL-3.0-or-later
//! A full export followed by an import reproduces the vault (FORMAT.md §7, M5 Decision 18): the
//! same `vault_id` and facts, the same ops and chain head, the same rows read back through the
//! public API, the same blobs. The reader files hold what §7 says, and a broken export leaves
//! nothing behind.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_core::query::Query;
use dbme_core::{Clock, ContentHash, Id, ManualClock};
use dbme_export::{ExportError, OPS_FILE, SCHEMA_FILE, export, import, is_export};
use dbme_store::{
    Identity, NewAttachment, NewBlock, NewField, NewObject, NewProvenance, NewRelation, NewType,
    NewView, Position, Vault, VaultOptions,
};
use serde_json::json;

fn options() -> VaultOptions {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_000_000_000));
    VaultOptions {
        identity: Identity::generate(clock.as_ref()).unwrap(),
        clock,
    }
}

/// A vault with every kind of row, a trashed object, an undone step and a blob.
fn populated(path: &Path) -> (Vault, Id, Id, ContentHash) {
    let mut vault = Vault::create(path, options()).unwrap();
    let blob = vault.blobs().put(b"a picture").unwrap();
    let hash = blob.hash.clone();
    let ((a, b), _) = vault
        .transact(|txn| {
            let kind = txn.create_type(NewType {
                name: "Kind".into(),
                ..NewType::default()
            })?;
            txn.create_field(NewField {
                type_id: kind,
                key: "due".into(),
                label: "Due".into(),
                kind: FieldKind::Date,
                options: json!({}),
                required: false,
            })?;
            txn.create_field(NewField {
                type_id: kind,
                key: "note".into(),
                label: "Note".into(),
                kind: FieldKind::Text,
                options: json!({}),
                required: false,
            })?;
            let a = txn.create_object(NewObject {
                title: "A, with a comma".into(),
                type_ids: vec![kind],
                props: Props::from_iter([
                    ("due".to_owned(), json!("2026-11-30")),
                    ("note".to_owned(), json!("say \"hi\"")),
                ]),
                ..NewObject::default()
            })?;
            let b = txn.create_object(NewObject {
                title: "B".into(),
                parent_id: Some(a),
                type_ids: vec![kind],
                ..NewObject::default()
            })?;
            txn.create_block(NewBlock {
                id: None,
                object_id: a,
                parent_block_id: None,
                position: Position::Last,
                kind: "paragraph".into(),
                content: json!({"inline": [{"text": "see "}, {"mention": b.to_string()}]}),
            })?;
            txn.create_relation(NewRelation {
                src_id: b,
                dst_id: a,
                rel_type: "related".into(),
                field_id: None,
                src_block_id: None,
                label: Some("parent".into()),
                props: Props::new(),
            })?;
            txn.attach(NewAttachment {
                object_id: a,
                blob_hash: hash.clone(),
                role: AttachmentRole::Original,
                filename: Some("picture.png".into()),
                mime: Some("image/png".into()),
            })?;
            txn.add_provenance(NewProvenance {
                object_id: a,
                source_kind: "import:image".into(),
                source_uri: Some("C:/pictures/picture.png".into()),
                import_run_id: Some(Id::from_parts(9, 9)),
                original_blob: Some(hash.clone()),
                extractor: "dbme-ingest".into(),
                extractor_version: "0.0.0".into(),
                confidence: 1.0,
                accepted: true,
            })?;
            txn.create_view(NewView {
                name: "Everything".into(),
                renderer: "list".into(),
                query: Query::default(),
                config: json!({}),
                owner_object_id: None,
            })?;
            Ok((a, b))
        })
        .unwrap();
    let gone = vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Gone".into(),
                ..NewObject::default()
            })
        })
        .unwrap()
        .0;
    vault.transact(|txn| txn.trash_object(gone)).unwrap();
    vault
        .transact(|txn| txn.update_object(a, |o| o.title.push_str(" (edited)")))
        .unwrap();
    vault.undo().unwrap().unwrap();
    (vault, a, b, hash)
}

/// Everything the public reader shows, as text, for comparing two vaults.
fn snapshot(vault: &Vault, a: Id, b: Id) -> String {
    let read = vault.read();
    let mut out = String::new();
    for t in read.types().unwrap() {
        out.push_str(&format!("{t:?}\n"));
        for f in read.fields(t.id).unwrap() {
            out.push_str(&format!("  {f:?}\n"));
        }
    }
    let mut objects = read.subtree(a).unwrap();
    objects.extend(read.trash().unwrap());
    for o in objects {
        out.push_str(&format!(
            "{o:?} types {:?}\n",
            read.object_type_ids(o.id).unwrap()
        ));
    }
    out.push_str(&format!("{:?}\n", read.blocks(a).unwrap()));
    out.push_str(&format!("{:?}\n", read.relations_from(b).unwrap()));
    out.push_str(&format!("{:?}\n", read.attachments(a).unwrap()));
    out.push_str(&format!("{:?}\n", read.provenance_of(a).unwrap()));
    out.push_str(&format!("{:?}\n", read.views().unwrap()));
    out
}

#[test]
fn a_full_export_imports_to_the_same_vault() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("Source.dbme");
    let (source, a, b, hash) = populated(&source_path);
    let export_dir = dir.path().join("export");

    let report = export(&source, &export_dir, true).unwrap();
    assert!(report.types >= 1 && report.views == 1, "{report:?}");
    assert_eq!(report.objects, 2, "A and B have the type; Gone has none");
    assert_eq!(report.blobs, Some(1));
    let ops = report.ops.unwrap();
    let verified = source.verify(true).unwrap();
    assert_eq!(ops, verified.ops);
    assert!(is_export(&export_dir));

    // The reader files.
    let types: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(export_dir.join(SCHEMA_FILE)).unwrap()).unwrap();
    let kind = types
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Kind")
        .unwrap();
    assert_eq!(kind["fields"][0]["key"], "due");
    assert_eq!(kind["fields"][1]["key"], "note");
    let csv = fs::read_to_string(export_dir.join("data").join("Kind.csv")).unwrap();
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        "id,title,parent_id,created_at,updated_at,due,note"
    );
    let row_a = lines.find(|l| l.starts_with(&a.to_string())).unwrap();
    assert!(
        row_a.contains(",\"A, with a comma\",,")
            && row_a.ends_with(",2026-11-30,\"say \"\"hi\"\"\""),
        "{row_a}"
    );
    assert_eq!(csv.lines().count(), 3, "{csv}");
    let views: Vec<_> = fs::read_dir(export_dir.join("views")).unwrap().collect();
    assert_eq!(views.len(), 1);
    assert_eq!(
        fs::read_to_string(export_dir.join(OPS_FILE))
            .unwrap()
            .lines()
            .count() as u64,
        ops
    );

    // The import.
    let copy_path = dir.path().join("Copy.dbme");
    let copy = import(&export_dir, &copy_path, options()).unwrap();
    assert_eq!(copy.id(), source.id());
    assert_eq!(
        copy.read().vault_meta_record().unwrap(),
        source.read().vault_meta_record().unwrap()
    );
    let copied = copy.verify(true).unwrap();
    assert!(copied.is_ok(), "{:#?}", copied.problems);
    assert_eq!((copied.ops, &copied.head), (verified.ops, &verified.head));
    assert_eq!(copied.blobs, 1);
    assert_eq!(copy.blobs().read(&hash).unwrap(), b"a picture");
    assert_eq!(snapshot(&copy, a, b), snapshot(&source, a, b));
    assert!(
        !copy.read().types().unwrap().is_empty(),
        "the built-in types came through the ops, not a seed"
    );
}

#[test]
fn a_broken_export_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let (source, ..) = populated(&dir.path().join("Source.dbme"));
    let export_dir = dir.path().join("export");
    export(&source, &export_dir, true).unwrap();

    // A line that is not an op names its line number.
    let ops_path = export_dir.join(OPS_FILE);
    let good = fs::read_to_string(&ops_path).unwrap();
    let mut lines: Vec<&str> = good.lines().collect();
    lines[2] = "{\"not\": \"an op\"}";
    fs::write(&ops_path, lines.join("\n")).unwrap();
    let target = dir.path().join("Bad.dbme");
    let error = import(&export_dir, &target, options()).unwrap_err();
    assert!(error.to_string().contains("line 3"), "{error}");
    assert!(!target.exists());

    // A record whose hash the replay does not reproduce fails the chain.
    let mut lines: Vec<String> = good.lines().map(str::to_owned).collect();
    lines[2] = lines[2].replace("\"ts\":", "\"ts\":1");
    fs::write(&ops_path, lines.join("\n")).unwrap();
    let error = import(&export_dir, &target, options()).unwrap_err();
    assert!(
        matches!(error, ExportError::Store(_)) && error.to_string().contains("op 3"),
        "{error}"
    );
    assert!(!target.exists());

    // Exporting into a folder that holds anything is refused, and a plain folder is no export.
    let error = export(&source, &export_dir, false).unwrap_err();
    assert!(matches!(error, ExportError::Rule(_)), "{error}");
    assert!(!is_export(dir.path()));
    assert!(matches!(
        import(dir.path(), &target, options()).unwrap_err(),
        ExportError::Io { .. }
    ));
}

#[test]
fn a_corrupt_meta_file_and_an_ops_file_that_is_not_text_are_refused_and_leave_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (source, ..) = populated(&dir.path().join("Source.dbme"));
    let export_dir = dir.path().join("export");
    export(&source, &export_dir, true).unwrap();
    let target = dir.path().join("Bad.dbme");

    // `vault_meta.json` that is not JSON names its file.
    let meta_path = export_dir.join(dbme_export::META_FILE);
    let good_meta = fs::read(&meta_path).unwrap();
    fs::write(&meta_path, "{").unwrap();
    let error = import(&export_dir, &target, options()).unwrap_err();
    assert!(
        matches!(&error, ExportError::Json { path, .. } if path == &meta_path),
        "{error}"
    );
    assert!(error.to_string().contains("vault_meta.json"), "{error}");
    assert!(!target.exists());
    fs::write(&meta_path, good_meta).unwrap();

    // `ops.jsonl` that is not UTF-8 is a read error on the way in, not a panic.
    let ops_path = export_dir.join(OPS_FILE);
    fs::write(&ops_path, [0xFF, 0xFE, b'{', b'}', b'\n']).unwrap();
    let error = import(&export_dir, &target, options()).unwrap_err();
    assert!(matches!(error, ExportError::Store(_)), "{error}");
    assert!(!target.exists());
}

#[test]
fn a_number_and_a_flag_export_as_their_json_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::create(&dir.path().join("Numbers.dbme"), options()).unwrap();
    vault
        .transact(|txn| {
            let kind = txn.create_type(NewType {
                name: "Gauge".into(),
                ..NewType::default()
            })?;
            for (key, field_kind) in [("count", FieldKind::Number), ("on", FieldKind::Boolean)] {
                txn.create_field(NewField {
                    type_id: kind,
                    key: key.into(),
                    label: key.into(),
                    kind: field_kind,
                    options: json!({}),
                    required: false,
                })?;
            }
            txn.create_object(NewObject {
                title: "Reading".into(),
                type_ids: vec![kind],
                props: Props::from_iter([
                    ("count".to_owned(), json!(42)),
                    ("on".to_owned(), json!(true)),
                ]),
                ..NewObject::default()
            })
        })
        .unwrap();
    let export_dir = dir.path().join("export");
    export(&vault, &export_dir, false).unwrap();
    let csv = fs::read_to_string(export_dir.join("data/Gauge.csv")).unwrap();
    let mut lines = csv.lines();
    assert!(lines.next().unwrap().ends_with(",count,on"), "{csv}");
    assert!(lines.next().unwrap().ends_with(",42,true"), "{csv}");
}
