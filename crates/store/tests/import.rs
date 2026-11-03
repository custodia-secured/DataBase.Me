// SPDX-License-Identifier: AGPL-3.0-or-later
//! The lossless import (M5 Task 3, Decision 18): a vault created from another vault's ops has the
//! same tables and the same hash chain.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fs;
use std::path::Path;

use common::{TestVault, new_vault, options};
use dbme_core::Id;
use dbme_core::chain::OpRecord;
use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_core::query::Query;
use dbme_store::{
    NewAttachment, NewBlock, NewField, NewObject, NewRelation, NewType, NewView, Position,
    StoreError, Vault, VaultMeta,
};
use serde_json::json;

/// A vault with every kind of row, a trashed object, and an undone step.
fn populated() -> TestVault {
    let mut t = new_vault();
    let blob = t.vault.blobs().put(b"a picture").unwrap();
    t.vault
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
            let a = txn.create_object(NewObject {
                title: "A".into(),
                type_ids: vec![kind],
                props: Props::from_iter([("due".to_owned(), json!("2026-11-30"))]),
                ..NewObject::default()
            })?;
            let b = txn.create_object(NewObject {
                title: "B".into(),
                parent_id: Some(a),
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
                blob_hash: blob.hash.clone(),
                role: AttachmentRole::Original,
                filename: Some("p.png".into()),
                mime: Some("image/png".into()),
            })?;
            txn.create_view(NewView {
                name: "All".into(),
                renderer: "list".into(),
                query: Query::default(),
                config: json!({"columns": ["title"]}),
                owner_object_id: Some(a),
            })?;
            let gone = txn.create_object(NewObject {
                title: "Gone".into(),
                ..NewObject::default()
            })?;
            txn.trash_object(gone)?;
            Ok(())
        })
        .unwrap();
    t.tick();
    t.vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Undone".into(),
                ..NewObject::default()
            })
        })
        .unwrap();
    t.tick();
    t.vault.undo().unwrap();
    t
}

fn export(t: &TestVault) -> (VaultMeta, Vec<OpRecord>) {
    let meta: std::collections::BTreeMap<String, String> =
        t.vault.read().vault_meta().unwrap().into_iter().collect();
    let meta = VaultMeta {
        vault_id: Id::parse(&meta["vault_id"]).unwrap(),
        created_at: meta["created_at"].parse().unwrap(),
        created_by: meta["created_by"].clone(),
        format_version: meta["format_version"].parse().unwrap(),
    };
    (meta, t.vault.read().ops_after(0, u32::MAX).unwrap())
}

fn copy_blobs(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_blobs(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn views(path: &Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path.join(dbme_store::DATABASE_FILE)).unwrap();
    let mut statement = conn.prepare("SELECT * FROM views ORDER BY id").unwrap();
    let count = statement.column_count();
    let mut rows = statement.query([]).unwrap();
    let mut out = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        let mut line = String::new();
        for column in 0..count {
            let value: rusqlite::types::Value = row.get(column).unwrap();
            line.push_str(&format!("{value:?} "));
        }
        out.push(line);
    }
    out
}

#[test]
fn a_vault_created_from_an_export_is_the_same_vault() {
    let source = populated();
    source.assert_verifies();
    let (meta, ops) = export(&source);
    assert_eq!(meta.vault_id, source.vault.id());
    assert!(ops.len() > 25);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Imported.dbme");
    let imported = Vault::create_from_ops(
        &path,
        options(&source.clock),
        meta.clone(),
        ops.iter().cloned().map(Ok),
    )
    .unwrap();
    copy_blobs(
        &source.path.join(dbme_store::BLOBS_FOLDER),
        &path.join(dbme_store::BLOBS_FOLDER),
    );

    assert_eq!(imported.id(), source.vault.id());
    assert_eq!(
        imported.read().vault_meta().unwrap(),
        source.vault.read().vault_meta().unwrap()
    );
    assert_eq!(common::snapshot(&path), common::snapshot(&source.path));
    assert_eq!(views(&path), views(&source.path));
    assert_eq!(imported.read().ops_after(0, u32::MAX).unwrap(), ops);
    assert_eq!(
        imported.read().types().unwrap().len(),
        source.vault.read().types().unwrap().len(),
        "no seed on top of the export's own types"
    );
    assert!(!imported.can_undo());
    assert!(!imported.can_redo());

    let ours = imported.verify(true).unwrap();
    let theirs = source.vault.verify(true).unwrap();
    assert!(ours.is_ok(), "{:#?}", ours.problems);
    assert_eq!((ours.ops, ours.head), (theirs.ops, theirs.head));
    assert_eq!(ours.schema_version, dbme_store::SCHEMA_VERSION);
    assert_eq!(ours.objects, theirs.objects);
    assert_eq!(ours.blobs, theirs.blobs);
    // The new vault is a working vault: it takes changes and verifies.
    drop(imported);
    let mut imported = Vault::open(&path, options(&source.clock)).unwrap();
    imported
        .transact(|txn| txn.create_object(NewObject::default()))
        .unwrap();
    assert!(imported.verify(false).unwrap().is_ok());
}

#[test]
fn a_tampered_or_broken_export_leaves_nothing_behind() {
    let source = populated();
    let (meta, ops) = export(&source);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Imported.dbme");

    // An edited payload no longer matches its hash.
    let mut tampered = ops.clone();
    tampered[30].entry.payload_json = tampered[30].entry.payload_json.replace('A', "Z");
    let result = Vault::create_from_ops(
        &path,
        options(&source.clock),
        meta.clone(),
        tampered.into_iter().map(Ok),
    );
    assert!(
        matches!(result, Err(StoreError::Rule(ref m)) if m.contains("chain")),
        "{result:?}"
    );
    assert!(!path.exists());
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        0,
        "no staging folder left"
    );

    // A record whose hash is wrong for its content.
    let mut wrong_hash = ops.clone();
    wrong_hash[3].hash = wrong_hash[4].hash.clone();
    let result = Vault::create_from_ops(
        &path,
        options(&source.clock),
        meta.clone(),
        wrong_hash.into_iter().map(Ok),
    );
    assert!(matches!(result, Err(StoreError::Rule(_))), "{result:?}");
    assert!(!path.exists());

    // An error from the reader stops the import.
    let failing = ops
        .iter()
        .cloned()
        .map(Ok)
        .take(5)
        .chain(std::iter::once(Err(StoreError::Rule(
            "unreadable line".into(),
        ))));
    let result = Vault::create_from_ops(&path, options(&source.clock), meta.clone(), failing);
    assert!(matches!(result, Err(StoreError::Rule(ref m)) if m == "unreadable line"));
    assert!(!path.exists());

    // An export from a newer format is refused before anything is made.
    let newer = VaultMeta {
        format_version: dbme_core::FORMAT_VERSION + 1,
        ..meta.clone()
    };
    let result = Vault::create_from_ops(
        &path,
        options(&source.clock),
        newer,
        ops.iter().cloned().map(Ok),
    );
    assert!(
        matches!(result, Err(StoreError::Rule(ref m)) if m.contains("format")),
        "{result:?}"
    );
    assert!(!path.exists());

    // The name rule and the taken-path rule hold as for `create`.
    assert!(matches!(
        Vault::create_from_ops(
            &dir.path().join("plain"),
            options(&source.clock),
            meta.clone(),
            std::iter::empty()
        ),
        Err(StoreError::BadVaultName(_))
    ));
    let empty =
        Vault::create_from_ops(&path, options(&source.clock), meta, std::iter::empty()).unwrap();
    assert_eq!(empty.verify(false).unwrap().ops, 0);
    assert!(empty.read().types().unwrap().is_empty());
}
