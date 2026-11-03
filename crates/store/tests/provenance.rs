// SPDX-License-Identifier: AGPL-3.0-or-later
//! Provenance (ADR-0026, migration 4): written through the op log, so a run's rows undo with the
//! run, replay in `verify`, go when their object is deleted for good, and are checked by the same
//! rules as every other row.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::op::{Entity, OpKind};
use dbme_core::{ContentHash, Id};
use dbme_store::{NewObject, NewProvenance, StoreError};

use common::{TestVault, new_vault};

fn an_object(t: &mut TestVault, title: &str) -> Id {
    t.tick();
    t.vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: title.into(),
                ..Default::default()
            })
        })
        .unwrap()
        .0
}

fn imported(object_id: Id, run: Id, blob: Option<ContentHash>) -> NewProvenance {
    NewProvenance {
        object_id,
        source_kind: "import:csv".into(),
        source_uri: Some("C:/people/team.csv#sheet=People".into()),
        import_run_id: Some(run),
        original_blob: blob,
        extractor: "dbme-ingest".into(),
        extractor_version: "0.0.0".into(),
        confidence: 0.9,
        accepted: true,
    }
}

#[test]
fn provenance_is_written_read_by_object_and_by_run_and_undone_with_its_step() {
    let mut t = new_vault();
    let a = an_object(&mut t, "Ada");
    let b = an_object(&mut t, "Bob");
    let run = Id::from_parts(7, 7);
    let blob = t.vault.blobs().put(b"name,note\nAda,x\n").unwrap();

    t.tick();
    let ((pa, pb), committed) = t
        .vault
        .transact(|txn| {
            let pa = txn.add_provenance(imported(a, run, Some(blob.hash.clone())))?;
            let pb = txn.add_provenance(imported(b, run, None))?;
            Ok((pa, pb))
        })
        .unwrap();
    let committed = committed.unwrap();
    assert_eq!(
        committed.changed,
        [a, b],
        "the ops target the objects, so the change event names them"
    );

    let of_a = t.vault.read().provenance_of(a).unwrap();
    assert_eq!(of_a.len(), 1);
    assert_eq!(of_a[0].id, pa);
    assert_eq!(of_a[0].object_id, a);
    assert_eq!(of_a[0].source_kind, "import:csv");
    assert_eq!(
        of_a[0].source_uri.as_deref(),
        Some("C:/people/team.csv#sheet=People")
    );
    assert_eq!(of_a[0].import_run_id, Some(run));
    assert_eq!(of_a[0].original_blob, Some(blob.hash.clone()));
    assert_eq!(of_a[0].extractor, "dbme-ingest");
    assert!((of_a[0].confidence - 0.9).abs() < f64::EPSILON);
    assert!(of_a[0].accepted);
    assert_eq!(of_a[0].created_at, common::START + 3);
    let of_run = t.vault.read().provenance_of_run(run).unwrap();
    assert_eq!(
        of_run.iter().map(|p| p.id).collect::<Vec<_>>(),
        [pa, pb],
        "the run's receipt, in order"
    );
    assert!(
        t.vault
            .read()
            .provenance_of_run(Id::from_parts(7, 8))
            .unwrap()
            .is_empty()
    );
    t.assert_verifies();

    // One transaction is one undo step: both rows go, and come back.
    t.tick();
    assert!(t.vault.undo().unwrap().is_some());
    assert!(t.vault.read().provenance_of(a).unwrap().is_empty());
    assert!(t.vault.read().provenance_of_run(run).unwrap().is_empty());
    t.assert_verifies();
    t.tick();
    assert!(t.vault.redo().unwrap().is_some());
    assert_eq!(t.vault.read().provenance_of_run(run).unwrap().len(), 2);
    t.assert_verifies();

    // The rows survive a reopen, and the log replays them row for row.
    let t = t.reopen();
    assert_eq!(t.vault.read().provenance_of(b).unwrap()[0].id, pb);
    t.assert_verifies();
}

#[test]
fn provenance_goes_with_its_object_when_it_is_deleted_for_good() {
    let mut t = new_vault();
    let a = an_object(&mut t, "Ada");
    let run = Id::from_parts(7, 7);
    t.tick();
    t.vault
        .transact(|txn| txn.add_provenance(imported(a, run, None)))
        .unwrap();
    t.tick();
    t.vault.transact(|txn| txn.trash_object(a)).unwrap();
    t.tick();
    t.vault.transact(|txn| txn.delete_object(a)).unwrap();
    assert!(t.vault.read().provenance_of(a).unwrap().is_empty());
    assert!(t.vault.read().provenance_of_run(run).unwrap().is_empty());
    t.assert_verifies();

    // Undoing the delete brings the object and its provenance back together.
    t.tick();
    assert!(t.vault.undo().unwrap().is_some());
    assert_eq!(t.vault.read().provenance_of(a).unwrap().len(), 1);
    t.assert_verifies();
}

#[test]
fn provenance_is_checked_like_every_other_row() {
    let mut t = new_vault();
    let a = an_object(&mut t, "Ada");
    let run = Id::from_parts(7, 7);

    // A confidence outside 0..=1 is refused before anything is written.
    for bad in [-0.5, 1.5, f64::NAN] {
        t.tick();
        let mut new = imported(a, run, None);
        new.confidence = bad;
        let result = t.vault.transact(|txn| txn.add_provenance(new.clone()));
        assert!(
            matches!(result, Err(StoreError::Core(_))),
            "{bad}: {result:?}"
        );
    }
    // A blob that is not in the blob store is refused, as an attachment's would be.
    t.tick();
    let missing = ContentHash::of(b"never stored");
    let result = t
        .vault
        .transact(|txn| txn.add_provenance(imported(a, run, Some(missing.clone()))));
    assert!(matches!(result, Err(StoreError::MissingBlob(h)) if h == missing));
    // An object in the Trash takes no provenance.
    t.tick();
    t.vault.transact(|txn| txn.trash_object(a)).unwrap();
    t.tick();
    assert!(
        t.vault
            .transact(|txn| txn.add_provenance(imported(a, run, None)))
            .is_err()
    );
    assert!(t.vault.read().provenance_of(a).unwrap().is_empty());
    t.assert_verifies();

    // The entity takes create and delete only.
    assert!(OpKind::parse("provenance.create").is_ok());
    assert!(OpKind::parse("provenance.delete").is_ok());
    assert!(OpKind::parse("provenance.update").is_err());
    assert_eq!(Entity::Provenance.as_str(), "provenance");
}
