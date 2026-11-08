// SPDX-License-Identifier: AGPL-3.0-or-later
//! Migration 3 and the files folder's bookkeeping (M5 Task 3).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{TestVault, new_vault};
use dbme_core::{ContentHash, Id};
use dbme_store::{
    ConflictKind, FULL_REWRITE_NEEDED, MirrorFileState, MirrorState, NewConflict, NewObject,
    SCHEMA_VERSION, StoreError, Vault,
};

fn user_version(t: &TestVault) -> u32 {
    t.raw()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn state(object_id: Id, rel_path: &str) -> MirrorState {
    MirrorState {
        object_id,
        rel_path: rel_path.to_owned(),
        path_key: rel_path.to_lowercase(),
        last_hash: Some(ContentHash::of(rel_path.as_bytes())),
        base_text: Some(format!("# {rel_path}\n")),
        synced_seq: Some(7),
        synced_at: Some(common::START),
        file_size: Some(42),
        file_mtime_ms: Some(common::START + 5),
        state: MirrorFileState::Synced,
    }
}

fn an_object(vault: &mut Vault, title: &str) -> Id {
    vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: title.to_owned(),
                ..NewObject::default()
            })
        })
        .unwrap()
        .0
}

#[test]
fn a_new_vault_is_at_schema_version_5() {
    let t = new_vault();
    assert_eq!(SCHEMA_VERSION, 5);
    assert_eq!(user_version(&t), 5);
    let report = t.vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
    assert_eq!(report.ops, 25, "migrations 3, 4 and 5 write no ops");
    assert_eq!(report.schema_version, 5);
}

#[test]
fn a_version_2_vault_migrates_and_keeps_its_objects() {
    let mut t = new_vault();
    let id = an_object(&mut t.vault, "Kept");
    let before = common::snapshot(&t.path);
    {
        // Make the vault look like one migration 2 left behind.
        let raw = t.raw();
        raw.execute_batch(
            "DROP TABLE mirror_state; DROP TABLE mirror_dirty; DROP TABLE mirror_meta; \
             DROP TABLE conflicts; DROP TABLE provenance; DROP TABLE canvas_items; \n             PRAGMA user_version = 2",
        )
        .unwrap();
    }
    assert_eq!(user_version(&t), 2);
    let t = t.reopen();
    assert_eq!(user_version(&t), 5);
    assert_eq!(common::snapshot(&t.path), before);
    assert_eq!(t.vault.read().object(id).unwrap().title, "Kept");
    assert!(t.vault.mirror_states(None).unwrap().is_empty());
    t.assert_verifies();
}

#[test]
fn verify_ignores_the_bookkeeping_tables() {
    let mut t = new_vault();
    let a = an_object(&mut t.vault, "A");
    let b = an_object(&mut t.vault, "B");
    t.vault.mirror_state_set(&state(a, "A.md")).unwrap();
    t.vault.mirror_dirty_mark(&[a, b]).unwrap();
    t.vault
        .mirror_meta_set(FULL_REWRITE_NEEDED, Some("1"))
        .unwrap();
    t.vault
        .conflict_add(NewConflict {
            object_id: Some(b),
            kind: Some(ConflictKind::ParseError),
            details: serde_json::json!({"line": 3}),
            ..NewConflict::default()
        })
        .unwrap();
    let report = t.vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
    // The tables survive a reopen untouched.
    let t = t.reopen();
    assert_eq!(t.vault.mirror_states(None).unwrap().len(), 1);
    assert_eq!(t.vault.mirror_dirty_count().unwrap(), 2);
    assert_eq!(t.vault.conflicts(true).unwrap().len(), 1);
}

#[test]
fn mirror_state_round_trips_and_refuses_a_taken_path() {
    let mut t = new_vault();
    let a = an_object(&mut t.vault, "A");
    let b = an_object(&mut t.vault, "B");
    assert_eq!(t.vault.mirror_state(a).unwrap(), None);

    let row = state(a, "Projects/A.md");
    t.vault.mirror_state_set(&row).unwrap();
    assert_eq!(t.vault.mirror_state(a).unwrap(), Some(row.clone()));
    assert_eq!(
        t.vault.mirror_state_by_path_key("projects/a.md").unwrap(),
        Some(row.clone())
    );
    assert_eq!(t.vault.mirror_state_by_path_key("nope").unwrap(), None);

    // The same object may change its path; another object may not take it.
    let mut moved = row.clone();
    moved.rel_path = "Projects/A (2).md".to_owned();
    moved.path_key = moved.rel_path.to_lowercase();
    moved.state = MirrorFileState::Pending;
    moved.last_hash = None;
    t.vault.mirror_state_set(&moved).unwrap();
    assert_eq!(t.vault.mirror_state(a).unwrap(), Some(moved.clone()));
    let clash = state(b, "Projects/A (2).md");
    match t.vault.mirror_state_set(&clash) {
        Err(StoreError::Rule(message)) => assert!(message.contains(&a.to_string()), "{message}"),
        other => panic!("{other:?}"),
    }
    let mut key_clash = state(b, "projects/a (2).MD");
    key_clash.path_key = moved.path_key.clone();
    assert!(matches!(
        t.vault.mirror_state_set(&key_clash),
        Err(StoreError::Rule(_))
    ));

    t.vault.mirror_state_set(&state(b, "B.md")).unwrap();
    let all = t.vault.mirror_states(None).unwrap();
    assert_eq!(
        all.iter().map(|s| s.rel_path.as_str()).collect::<Vec<_>>(),
        ["B.md", "Projects/A (2).md"]
    );
    let pending = t
        .vault
        .mirror_states(Some(MirrorFileState::Pending))
        .unwrap();
    assert_eq!(pending, vec![moved]);

    assert!(t.vault.mirror_state_remove(a).unwrap());
    assert!(!t.vault.mirror_state_remove(a).unwrap());
    assert_eq!(t.vault.mirror_states(None).unwrap().len(), 1);
}

#[test]
fn the_dirty_list_takes_and_clears() {
    let mut t = new_vault();
    let a = an_object(&mut t.vault, "A");
    let b = an_object(&mut t.vault, "B");
    t.vault.mirror_dirty_take().unwrap();
    t.vault.mirror_dirty_mark(&[b, a, b]).unwrap();
    assert_eq!(t.vault.mirror_dirty_count().unwrap(), 2);
    assert_eq!(t.vault.mirror_dirty_take().unwrap(), vec![a, b]);
    assert_eq!(t.vault.mirror_dirty_count().unwrap(), 0);
    assert!(t.vault.mirror_dirty_take().unwrap().is_empty());
}

#[test]
fn meta_sets_reads_and_removes() {
    let mut t = new_vault();
    assert_eq!(t.vault.mirror_meta_get("folder_enabled").unwrap(), None);
    t.vault
        .mirror_meta_set("folder_enabled", Some("1"))
        .unwrap();
    assert_eq!(
        t.vault
            .mirror_meta_get("folder_enabled")
            .unwrap()
            .as_deref(),
        Some("1")
    );
    t.vault
        .mirror_meta_set("folder_enabled", Some("0"))
        .unwrap();
    assert_eq!(
        t.vault
            .mirror_meta_get("folder_enabled")
            .unwrap()
            .as_deref(),
        Some("0")
    );
    t.vault.mirror_meta_set("folder_enabled", None).unwrap();
    assert_eq!(t.vault.mirror_meta_get("folder_enabled").unwrap(), None);
    t.vault.mirror_meta_set("folder_enabled", None).unwrap();
}

#[test]
fn conflicts_are_added_listed_and_resolved() {
    let mut t = new_vault();
    let a = an_object(&mut t.vault, "A");
    let first = t
        .vault
        .conflict_add(NewConflict {
            object_id: Some(a),
            rel_path: Some("A.md".to_owned()),
            kind: Some(ConflictKind::EditEdit),
            details: serde_json::json!({"block": "x"}),
            conflict_copy_id: Some(Id::from_parts(5, 5)),
        })
        .unwrap();
    t.tick();
    let second = t
        .vault
        .conflict_add(NewConflict {
            rel_path: Some("stray.md".to_owned()),
            kind: Some(ConflictKind::DroppedFile),
            ..NewConflict::default()
        })
        .unwrap();
    assert!(matches!(
        t.vault.conflict_add(NewConflict::default()),
        Err(StoreError::Rule(_))
    ));

    let open = t.vault.conflicts(true).unwrap();
    assert_eq!(
        open.iter().map(|c| c.id).collect::<Vec<_>>(),
        [first, second]
    );
    let c = t.vault.conflict(first).unwrap();
    assert_eq!(c.object_id, Some(a));
    assert_eq!(c.rel_path.as_deref(), Some("A.md"));
    assert_eq!(c.kind, ConflictKind::EditEdit);
    assert_eq!(c.details, serde_json::json!({"block": "x"}));
    assert_eq!(c.conflict_copy_id, Some(Id::from_parts(5, 5)));
    assert_eq!(c.created_at, common::START);
    assert_eq!((c.resolution, c.resolved_at), (None, None));
    let d = t.vault.conflict(second).unwrap();
    assert_eq!((d.object_id, d.conflict_copy_id), (None, None));
    assert_eq!(d.kind, ConflictKind::DroppedFile);
    assert_eq!(d.details, serde_json::Value::Null);

    t.tick();
    assert!(t.vault.conflict_resolve(first, "keep_app").unwrap());
    assert!(!t.vault.conflict_resolve(first, "again").unwrap());
    let c = t.vault.conflict(first).unwrap();
    assert_eq!(c.resolution.as_deref(), Some("keep_app"));
    assert_eq!(c.resolved_at, Some(common::START + 2));
    assert_eq!(t.vault.conflicts(true).unwrap().len(), 1);
    assert_eq!(t.vault.conflicts(false).unwrap().len(), 2);
    assert!(matches!(
        t.vault.conflict(Id::from_parts(9, 9)),
        Err(StoreError::NotFound {
            entity: "conflict",
            ..
        })
    ));
    assert!(matches!(
        t.vault.conflict_resolve(Id::from_parts(9, 9), "x"),
        Err(StoreError::NotFound { .. })
    ));
}

#[test]
fn every_kind_and_state_name_round_trips() {
    for kind in [
        ConflictKind::EditEdit,
        ConflictKind::EditDelete,
        ConflictKind::ParseError,
        ConflictKind::InvalidValue,
        ConflictKind::DuplicateId,
        ConflictKind::DroppedFile,
        ConflictKind::BulkChangeHold,
        ConflictKind::MissingId,
        ConflictKind::CloudConflictFile,
    ] {
        assert_eq!(ConflictKind::parse(kind.as_str()), Some(kind));
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{}\"", kind.as_str()));
    }
    assert_eq!(ConflictKind::parse("nope"), None);
    for state in [
        MirrorFileState::Synced,
        MirrorFileState::Pending,
        MirrorFileState::Held,
        MirrorFileState::Excluded,
        MirrorFileState::Trashed,
    ] {
        assert_eq!(MirrorFileState::parse(state.as_str()), Some(state));
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, format!("\"{}\"", state.as_str()));
    }
    assert_eq!(MirrorFileState::parse(""), None);
}

#[test]
fn the_same_functions_work_inside_a_transaction() {
    let mut t = new_vault();
    let a = an_object(&mut t.vault, "A");
    let (conflict_id, _) = t
        .vault
        .transact(|txn| {
            txn.mirror_state_set(&state(a, "A.md"))?;
            assert_eq!(
                txn.mirror_state(a)?.map(|s| s.rel_path),
                Some("A.md".into())
            );
            assert!(txn.mirror_state_by_path_key("a.md")?.is_some());
            assert_eq!(txn.mirror_states(None)?.len(), 1);
            txn.mirror_dirty_take()?;
            txn.mirror_dirty_mark(&[a])?;
            assert_eq!(txn.mirror_dirty_count()?, 1);
            assert_eq!(txn.mirror_dirty_take()?, vec![a]);
            txn.mirror_meta_set("k", Some("v"))?;
            assert_eq!(txn.mirror_meta_get("k")?.as_deref(), Some("v"));
            let id = txn.conflict_add(NewConflict {
                kind: Some(ConflictKind::MissingId),
                object_id: Some(a),
                ..NewConflict::default()
            })?;
            assert_eq!(txn.conflicts(true)?.len(), 1);
            assert_eq!(txn.conflict(id)?.kind, ConflictKind::MissingId);
            assert!(txn.conflict_resolve(id, "written_back")?);
            assert!(txn.mirror_state_remove(a)?);
            txn.mirror_state_set(&state(a, "A.md"))?;
            Ok(id)
        })
        .unwrap();
    // The bookkeeping writes added nothing to the op log: the seed plus the one create.
    assert_eq!(t.vault.verify(false).unwrap().ops, 25 + 1);
    assert_eq!(
        t.vault.conflict(conflict_id).unwrap().resolution.as_deref(),
        Some("written_back")
    );
    assert_eq!(t.vault.mirror_states(None).unwrap().len(), 1);

    // Bookkeeping inside a failed transaction is rolled back with it.
    let result = t.vault.transact(|txn| {
        txn.mirror_meta_set("rolled", Some("back"))?;
        txn.create_object(NewObject {
            parent_id: Some(Id::from_parts(1, 1)),
            ..NewObject::default()
        })
    });
    assert!(result.is_err());
    assert_eq!(t.vault.mirror_meta_get("rolled").unwrap(), None);

    t.vault.mirror_reset().unwrap();
    assert!(t.vault.mirror_states(None).unwrap().is_empty());
    assert_eq!(t.vault.mirror_dirty_count().unwrap(), 0);
    assert_eq!(t.vault.mirror_meta_get("k").unwrap(), None);
    assert!(t.vault.conflicts(false).unwrap().is_empty());
}
