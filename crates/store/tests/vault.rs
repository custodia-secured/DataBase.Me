// SPDX-License-Identifier: AGPL-3.0-or-later
//! Behaviour of vaults: creating and opening, every service, undo and redo, and verification.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fs;
use std::sync::Arc;

use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_core::{Clock, CoreError, Id, ManualClock};
use dbme_store::{
    APPLICATION_ID, BLOBS_FOLDER, DATABASE_FILE, FORMAT_FILE, NewAttachment, NewBlock, NewField,
    NewObject, NewRelation, NewType, Position, SCHEMA_VERSION, StoreError, Vault,
};
use serde_json::json;

use common::{START, TestVault, new_vault, options, snapshot};

fn type_id(vault: &Vault, name: &str) -> Id {
    vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn object(title: &str) -> NewObject {
    NewObject {
        title: title.into(),
        ..Default::default()
    }
}

fn create(t: &mut TestVault, new: NewObject) -> Id {
    t.tick();
    t.vault.transact(|txn| txn.create_object(new)).unwrap().0
}

fn titles(t: &TestVault, parent: Option<Id>) -> Vec<String> {
    t.vault
        .read()
        .children(parent)
        .unwrap()
        .into_iter()
        .map(|o| o.title)
        .collect()
}

fn props(pairs: &[(&str, serde_json::Value)]) -> Props {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

#[test]
fn a_new_vault_has_the_built_in_types_and_verifies() {
    let t = new_vault();
    let types = t.vault.read().types().unwrap();
    assert_eq!(types.len(), 25);
    assert!(types.iter().all(|t| t.is_system && t.created_at == START));
    let media = type_id(&t.vault, "Media");
    assert_eq!(
        types.iter().find(|t| t.name == "Image").unwrap().extends,
        Some(media)
    );
    assert!(
        !t.vault.can_undo() && !t.vault.can_redo(),
        "the built-in types are not an undo step"
    );
    assert!(t.path.join(FORMAT_FILE).is_file() && t.path.join(BLOBS_FOLDER).is_dir());

    let report = t.vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:?}", report.problems);
    assert_eq!(
        (
            report.ops,
            report.schema_version,
            report.objects,
            report.blobs
        ),
        (25, SCHEMA_VERSION, 0, 0)
    );
    assert_eq!(
        report.head,
        Some(t.vault.read().history(1).unwrap()[0].hash.clone())
    );
    assert_eq!(t.vault.read().history(100).unwrap().len(), 25);

    let entries: Vec<_> = fs::read_dir(t.dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(entries, ["Test.dbme"], "no staging folder is left behind");
    let app: i32 = t
        .raw()
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app, APPLICATION_ID);
    let mode: String = t
        .raw()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");

    let (id, root) = (t.vault.id(), t.vault.root().to_owned());
    let t = t.reopen();
    assert_eq!((t.vault.id(), t.vault.root()), (id, root.as_path()));
    assert!(format!("{:?}", t.vault).contains("Test.dbme"));
}

#[test]
fn a_vault_is_open_in_one_place_at_a_time() {
    let t = new_vault();
    assert!(matches!(
        Vault::open(&t.path, options(&t.clock)),
        Err(StoreError::Locked(_))
    ));
    let t = t.reopen();
    t.assert_verifies();
}

#[test]
fn creating_and_opening_check_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(START));
    for bad in ["Notes", ".dbme", "Notes.txt", ".hidden.dbme"] {
        let path = dir.path().join(bad);
        assert!(
            matches!(
                Vault::create(&path, options(&clock)),
                Err(StoreError::BadVaultName(_))
            ),
            "{bad}"
        );
        assert!(
            matches!(
                Vault::open(&path, options(&clock)),
                Err(StoreError::BadVaultName(_))
            ),
            "{bad}"
        );
    }

    let full = dir.path().join("Full.dbme");
    fs::create_dir(&full).unwrap();
    fs::write(full.join("note.txt"), "x").unwrap();
    assert!(matches!(
        Vault::create(&full, options(&clock)),
        Err(StoreError::AlreadyExists(_))
    ));
    assert!(matches!(
        Vault::open(&full, options(&clock)),
        Err(StoreError::NotAVault(_))
    ));

    let empty = dir.path().join("Empty.dbme");
    fs::create_dir(&empty).unwrap();
    drop(Vault::create(&empty, options(&clock)).unwrap());
    assert!(Vault::open(&empty, options(&clock)).is_ok());

    // A database that belongs to something else is left alone.
    let other = dir.path().join("Other.dbme");
    fs::create_dir(&other).unwrap();
    rusqlite::Connection::open(other.join(DATABASE_FILE))
        .unwrap()
        .execute_batch("CREATE TABLE t (x)")
        .unwrap();
    assert!(matches!(
        Vault::open(&other, options(&clock)),
        Err(StoreError::NotAVault(_))
    ));
    let foreign = dir.path().join("Foreign.dbme");
    fs::create_dir(&foreign).unwrap();
    rusqlite::Connection::open(foreign.join(DATABASE_FILE))
        .unwrap()
        .execute_batch("PRAGMA application_id = 7")
        .unwrap();
    assert!(matches!(
        Vault::open(&foreign, options(&clock)),
        Err(StoreError::NotAVault(_))
    ));
    let journal: String = rusqlite::Connection::open(foreign.join(DATABASE_FILE))
        .unwrap()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        journal, "delete",
        "a database that is not a vault is not switched to WAL"
    );

    // A failed creation leaves nothing behind.
    let blocked = dir.path().join("Blocked.dbme");
    fs::write(&blocked, "a file, not a folder").unwrap();
    assert!(Vault::create(&blocked, options(&clock)).is_err());
    let names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.contains("creating")), "{names:?}");
}

#[test]
fn newer_or_damaged_vaults_are_refused() {
    let TestVault {
        dir: _dir,
        path,
        clock,
        vault,
    } = new_vault();
    drop(vault);
    let raw = rusqlite::Connection::open(path.join(DATABASE_FILE)).unwrap();
    raw.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    let error = Vault::open(&path, options(&clock)).unwrap_err();
    assert!(
        matches!(error, StoreError::TooNew { found, supported } if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION)
    );
    assert!(error.to_string().contains("update the app"));

    raw.pragma_update(None, "user_version", SCHEMA_VERSION)
        .unwrap();
    raw.execute("DELETE FROM vault_meta", []).unwrap();
    assert!(matches!(
        Vault::open(&path, options(&clock)),
        Err(StoreError::NotAVault(_))
    ));
}

#[test]
fn objects_are_ordered_among_their_siblings_and_move() {
    let mut t = new_vault();
    let a = create(&mut t, object("a"));
    t.tick();
    let ((first, last), committed) = t
        .vault
        .transact(|txn| {
            let first = txn.create_object(NewObject {
                position: Position::First,
                ..object("first")
            })?;
            txn.create_object(NewObject {
                position: Position::Before(a),
                ..object("before a")
            })?;
            txn.create_object(NewObject {
                position: Position::After(a),
                ..object("after a")
            })?;
            let last = txn.create_object(object("last"))?;
            Ok((first, last))
        })
        .unwrap();
    assert_eq!(
        titles(&t, None),
        ["first", "before a", "a", "after a", "last"]
    );
    let committed = committed.unwrap();
    assert_eq!(committed.last_seq - committed.first_seq, 3);
    assert_eq!(committed.changed.len(), 4);

    t.tick();
    t.vault
        .transact(|txn| {
            assert!(txn.move_object(last, None, Position::First)?);
            assert!(txn.move_object(a, Some(first), Position::Last)?);
            Ok(())
        })
        .unwrap();
    assert_eq!(titles(&t, None), ["last", "first", "before a", "after a"]);
    assert_eq!(titles(&t, Some(first)), ["a"]);
    for (parent, position) in [
        (Some(a), Position::Last),
        (Some(first), Position::Last),
        (None, Position::Before(last)),
    ] {
        let moving = if parent == Some(a) || parent == Some(first) {
            first
        } else {
            last
        };
        let result = t
            .vault
            .transact(|txn| txn.move_object(moving, parent, position));
        assert!(
            matches!(result, Err(StoreError::Rule(_))),
            "{parent:?} {position:?}"
        );
    }
    let not_a_sibling = t.vault.transact(|txn| {
        txn.create_object(NewObject {
            position: Position::After(a),
            ..object("x")
        })
    });
    assert!(matches!(not_a_sibling, Err(StoreError::Rule(_))));
    t.assert_verifies();
}

#[test]
fn rebalancing_shortens_long_sort_keys_and_keeps_the_order() {
    let mut t = new_vault();
    let first = create(&mut t, object("first"));
    let x = create(&mut t, object("x"));
    t.tick();
    // Inserting again and again just after `first` makes the keys grow: a digit every few inserts.
    t.vault
        .transact(|txn| {
            (0..60).try_for_each(|n| {
                txn.create_object(NewObject {
                    position: Position::After(first),
                    ..object(&format!("{n:02}"))
                })
                .map(drop)
            })
        })
        .unwrap();
    let longest = |t: &TestVault| {
        let children = t.vault.read().children(None).unwrap();
        children.iter().map(|o| o.sort_key.as_str().len()).max()
    };
    let before = titles(&t, None);
    assert!(longest(&t) >= Some(10), "{:?}", longest(&t));

    t.tick();
    let (moved, _) = t.vault.transact(|txn| txn.rebalance_objects(None)).unwrap();
    assert!(moved > 0);
    assert_eq!(titles(&t, None), before);
    assert!(longest(&t) <= Some(3), "{:?}", longest(&t));
    let (again, committed) = t.vault.transact(|txn| txn.rebalance_objects(None)).unwrap();
    assert_eq!((again, committed), (0, None), "already balanced");

    t.tick();
    assert!(t.vault.undo().unwrap().is_some());
    assert_eq!(titles(&t, None), before);
    assert!(longest(&t) >= Some(10));

    let block = |position| NewBlock {
        object_id: x,
        parent_block_id: None,
        position,
        kind: "paragraph".into(),
        content: json!({}),
    };
    t.tick();
    let (order, _) = t
        .vault
        .transact(|txn| {
            let top = txn.create_block(block(Position::Last))?;
            let mut order = vec![top];
            for _ in 0..40 {
                order.insert(1, txn.create_block(block(Position::After(top)))?);
            }
            Ok(order)
        })
        .unwrap();
    t.tick();
    let (moved, _) = t
        .vault
        .transact(|txn| txn.rebalance_blocks(x, None))
        .unwrap();
    assert!(moved > 0);
    let blocks = t.vault.read().blocks(x).unwrap();
    assert_eq!(blocks.iter().map(|b| b.id).collect::<Vec<_>>(), order);
    assert!(blocks.iter().all(|b| b.sort_key.as_str().len() <= 3));
    let missing = t
        .vault
        .transact(|txn| txn.rebalance_blocks(Id::from_parts(9, 9), None));
    assert!(matches!(missing, Err(StoreError::NotFound { .. })));
    t.assert_verifies();
}

#[test]
fn property_values_are_checked_against_the_fields_of_the_objects_types() {
    let mut t = new_vault();
    let task = type_id(&t.vault, "Task");
    t.tick();
    let ((due, chore), _) = t
        .vault
        .transact(|txn| {
            let due = txn.create_field(NewField {
                type_id: task,
                key: "due".into(),
                label: "Due".into(),
                kind: FieldKind::Date,
                options: json!({}),
                required: false,
            })?;
            let chore = txn.create_type(NewType {
                name: "Chore".into(),
                extends: Some(task),
                ..Default::default()
            })?;
            Ok((due, chore))
        })
        .unwrap();

    let bad = t.vault.transact(|txn| {
        txn.create_object(NewObject {
            type_ids: vec![chore],
            props: props(&[("due", json!("tomorrow"))]),
            ..object("x")
        })
    });
    assert!(
        matches!(bad, Err(StoreError::Core(CoreError::Invalid { .. }))),
        "inherited fields are checked"
    );

    let id = create(
        &mut t,
        NewObject {
            type_ids: vec![chore, chore],
            props: props(&[
                ("due", json!("2026-11-30")),
                ("anything", json!({"free": "form"})),
            ]),
            ..object("Wash up")
        },
    );
    assert_eq!(t.vault.read().object_type_ids(id).unwrap(), [chore]);
    let bad = t.vault.transact(|txn| {
        txn.update_object(id, |o| {
            o.props_json.insert("due".into(), json!(5));
        })
    });
    assert!(matches!(
        bad,
        Err(StoreError::Core(CoreError::Invalid { .. }))
    ));
    t.tick();
    let (changed, _) = t
        .vault
        .transact(|txn| txn.update_object(id, |o| o.title = "Wash up!".into()))
        .unwrap();
    assert!(changed);
    let (changed, committed) = t
        .vault
        .transact(|txn| txn.update_object(id, |_| {}))
        .unwrap();
    assert!(!changed && committed.is_none());
    assert!(matches!(
        t.vault
            .transact(|txn| txn.update_object(id, |o| o.deleted_at = Some(1))),
        Err(StoreError::Core(CoreError::Invalid { .. }))
    ));

    // Fields: unique keys, editable labels, inherited by subtypes.
    let duplicate = t.vault.transact(|txn| {
        txn.create_field(NewField {
            type_id: task,
            key: "due".into(),
            label: "Again".into(),
            kind: FieldKind::Text,
            options: json!({}),
            required: false,
        })
    });
    assert!(matches!(duplicate, Err(StoreError::Rule(_))));
    t.tick();
    t.vault
        .transact(|txn| txn.update_field(due, |f| f.label = "Due date".into()))
        .unwrap();
    assert_eq!(t.vault.read().field(due).unwrap().label, "Due date");
    assert_eq!(t.vault.read().fields_of_types(&[chore]).unwrap().len(), 1);
    assert!(t.vault.read().fields_of_types(&[]).unwrap().is_empty());
    t.assert_verifies();
}

#[test]
fn types_can_be_edited_and_deleted_only_when_unused() {
    let mut t = new_vault();
    let note = type_id(&t.vault, "Note");
    let built_in = t.vault.transact(|txn| txn.delete_type(note));
    assert!(matches!(built_in, Err(StoreError::Rule(_))));
    t.tick();
    t.vault
        .transact(|txn| txn.update_type(note, |r| r.icon = Some("notebook".into())))
        .unwrap();
    assert_eq!(
        t.vault.read().type_row(note).unwrap().icon.as_deref(),
        Some("notebook")
    );

    t.tick();
    let ((a, b, link), _) = t
        .vault
        .transact(|txn| {
            let a = txn.create_type(NewType {
                name: "A".into(),
                ..Default::default()
            })?;
            let b = txn.create_type(NewType {
                name: "B".into(),
                extends: Some(a),
                ..Default::default()
            })?;
            let link = txn.create_field(NewField {
                type_id: b,
                key: "link".into(),
                label: "Link".into(),
                kind: FieldKind::Relation,
                options: json!({}),
                required: false,
            })?;
            txn.create_field(NewField {
                type_id: b,
                key: "size".into(),
                label: "Size".into(),
                kind: FieldKind::Number,
                options: json!({}),
                required: false,
            })?;
            Ok((a, b, link))
        })
        .unwrap();
    for cycle in [Some(b), Some(a)] {
        let result = t
            .vault
            .transact(|txn| txn.update_type(a, |r| r.extends = cycle));
        assert!(matches!(result, Err(StoreError::Rule(_))), "{cycle:?}");
    }
    assert!(
        matches!(
            t.vault.transact(|txn| txn.delete_type(a)),
            Err(StoreError::Rule(_))
        ),
        "B extends A"
    );

    let x = create(
        &mut t,
        NewObject {
            type_ids: vec![b],
            ..object("x")
        },
    );
    let y = create(&mut t, object("y"));
    assert!(
        matches!(
            t.vault.transact(|txn| txn.delete_type(b)),
            Err(StoreError::Rule(_))
        ),
        "x has type B"
    );
    t.tick();
    t.vault
        .transact(|txn| {
            assert!(!txn.add_type(x, b)?);
            assert!(txn.add_type(y, b)?);
            assert!(txn.remove_type(x, b)?);
            assert!(!txn.remove_type(x, b)?);
            txn.remove_type(y, b)?;
            txn.create_relation(NewRelation {
                src_id: x,
                dst_id: y,
                rel_type: "link".into(),
                field_id: Some(link),
                src_block_id: None,
                label: None,
                props: Props::new(),
            })
        })
        .unwrap();
    assert!(
        matches!(
            t.vault.transact(|txn| txn.delete_type(b)),
            Err(StoreError::Rule(_))
        ),
        "a relation uses its field"
    );
    assert!(matches!(
        t.vault.transact(|txn| txn.delete_field(link)),
        Err(StoreError::Rule(_))
    ));

    let relation = t.vault.read().relations_from(x).unwrap()[0].id;
    t.tick();
    t.vault
        .transact(|txn| txn.delete_relation(relation))
        .unwrap();
    let before_delete = snapshot(&t.path);
    t.tick();
    t.vault.transact(|txn| txn.delete_type(b)).unwrap();
    assert!(matches!(
        t.vault.read().type_row(b),
        Err(StoreError::NotFound { .. })
    ));
    assert!(t.vault.read().fields(b).unwrap().is_empty());
    t.vault.undo().unwrap().unwrap();
    assert_eq!(snapshot(&t.path), before_delete);
    t.tick();
    t.vault.transact(|txn| txn.delete_field(link)).unwrap();
    t.assert_verifies();
}

struct Tree {
    x: Id,
    parent: Id,
    child: Id,
    grandchild: Id,
}

/// x, plus parent > child > grandchild, with blocks, relations both ways, an attachment and a type.
fn tree(t: &mut TestVault) -> Tree {
    let blob = t.vault.blobs().put(b"scan").unwrap();
    let document = type_id(&t.vault, "Document");
    t.tick();
    t.vault
        .transact(|txn| {
            let x = txn.create_object(object("x"))?;
            let parent = txn.create_object(object("parent"))?;
            let child = txn.create_object(NewObject {
                parent_id: Some(parent),
                type_ids: vec![document],
                ..object("child")
            })?;
            let grandchild = txn.create_object(NewObject {
                parent_id: Some(child),
                ..object("grandchild")
            })?;
            let heading = txn.create_block(NewBlock {
                object_id: parent,
                parent_block_id: None,
                position: Position::Last,
                kind: "heading".into(),
                content: json!({"text": "Hi"}),
            })?;
            let mention = txn.create_block(NewBlock {
                object_id: parent,
                parent_block_id: Some(heading),
                position: Position::Last,
                kind: "paragraph".into(),
                content: json!({"text": "see x"}),
            })?;
            txn.create_relation(NewRelation {
                src_id: parent,
                dst_id: x,
                rel_type: "mentions".into(),
                field_id: None,
                src_block_id: Some(mention),
                label: None,
                props: Props::new(),
            })?;
            txn.create_relation(NewRelation {
                src_id: x,
                dst_id: child,
                rel_type: "links_to".into(),
                field_id: None,
                src_block_id: None,
                label: None,
                props: Props::new(),
            })?;
            txn.attach(NewAttachment {
                object_id: child,
                blob_hash: blob.hash.clone(),
                role: AttachmentRole::Original,
                filename: Some("scan.pdf".into()),
                mime: Some("application/pdf".into()),
            })?;
            Ok(Tree {
                x,
                parent,
                child,
                grandchild,
            })
        })
        .unwrap()
        .0
}

#[test]
fn trash_restore_and_delete_cover_the_whole_subtree() {
    let mut t = new_vault();
    let empty = snapshot(&t.path);
    let Tree {
        x,
        parent,
        child,
        grandchild,
    } = tree(&mut t);
    let built = snapshot(&t.path);
    assert_eq!(titles(&t, None), ["x", "parent"]);

    t.tick();
    t.vault.transact(|txn| txn.trash_object(parent)).unwrap();
    assert_eq!(titles(&t, None), ["x"]);
    for id in [parent, child, grandchild] {
        assert_eq!(
            t.vault.read().object(id).unwrap().deleted_at,
            Some(t.clock.now_ms())
        );
    }
    let trash: Vec<Id> = t
        .vault
        .read()
        .trash()
        .unwrap()
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(
        trash,
        [parent],
        "the Trash shows what was trashed, not everything below it"
    );
    assert!(matches!(
        t.vault.transact(|txn| txn.restore_object(child)),
        Err(StoreError::Rule(_))
    ));
    assert!(matches!(
        t.vault
            .transact(|txn| txn.update_object(child, |o| o.title = "no".into())),
        Err(StoreError::Rule(_))
    ));
    assert!(matches!(
        t.vault.transact(|txn| txn.trash_object(child)),
        Err(StoreError::Rule(_))
    ));
    assert!(matches!(
        t.vault.transact(|txn| txn.restore_object(x)),
        Err(StoreError::Rule(_))
    ));
    assert!(
        matches!(
            t.vault.transact(|txn| txn.delete_object(x)),
            Err(StoreError::Rule(_))
        ),
        "only from the Trash"
    );
    assert!(matches!(
        t.vault.transact(|txn| txn.create_object(NewObject {
            parent_id: Some(parent),
            ..object("into the bin")
        })),
        Err(StoreError::Rule(_))
    ));
    let trashed = snapshot(&t.path);

    t.tick();
    t.vault.transact(|txn| txn.delete_object(parent)).unwrap();
    assert_eq!(t.vault.read().object_count().unwrap(), 1);
    assert!(
        t.vault.read().relations_from(x).unwrap().is_empty()
            && t.vault.read().relations_to(x).unwrap().is_empty()
    );
    let report = t.vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:?}", report.problems);
    assert_eq!(report.blobs, 1, "the blob outlives its attachment");

    t.vault.undo().unwrap().unwrap();
    assert_eq!(snapshot(&t.path), trashed);
    t.tick();
    t.vault.undo().unwrap().unwrap();
    assert_eq!(snapshot(&t.path), built);
    assert_eq!(t.vault.read().attachments(child).unwrap()[0].blob_size, 4);
    t.vault.undo().unwrap().unwrap();
    assert_eq!(snapshot(&t.path), empty);
    assert!(!t.vault.can_undo());
    assert_eq!(t.vault.undo().unwrap(), None);

    for expected in [&built, &trashed] {
        t.tick();
        let committed = t.vault.redo().unwrap().unwrap();
        assert!(committed.undo_of.is_some());
        assert_eq!(&snapshot(&t.path), expected);
    }
    t.vault.redo().unwrap().unwrap();
    assert_eq!(t.vault.read().object_count().unwrap(), 1);
    assert_eq!(t.vault.redo().unwrap(), None);
    t.assert_verifies();

    // Restoring brings back exactly what was trashed together.
    t.vault.undo().unwrap().unwrap();
    t.tick();
    t.vault.transact(|txn| txn.restore_object(parent)).unwrap();
    assert_eq!(titles(&t, None), ["x", "parent"]);
    assert_eq!(titles(&t, Some(child)), ["grandchild"]);
    assert!(!t.vault.can_redo(), "a new change clears the redo history");
    t.assert_verifies();
}

#[test]
fn undo_refuses_to_overwrite_a_later_change() {
    let mut t = new_vault();
    let x = create(&mut t, object("x"));
    t.tick();
    t.vault
        .transact(|txn| txn.update_object(x, |o| o.title = "y".into()))
        .unwrap();

    // Another writer changes the title, outside this session's undo history.
    t.raw()
        .execute(
            "UPDATE objects SET title = 'z' WHERE id = ?1",
            [x.to_string()],
        )
        .unwrap();
    let error = t.vault.undo().unwrap_err();
    assert!(matches!(error, StoreError::UndoBlocked(_)), "{error}");
    assert_eq!(
        t.vault.read().object(x).unwrap().title,
        "z",
        "nothing was overwritten"
    );
    assert!(
        t.vault.can_undo(),
        "the blocked step is dropped; the one before it is next"
    );
    assert!(matches!(t.vault.undo(), Err(StoreError::UndoBlocked(_))));
    assert!(!t.vault.can_undo());

    // verify notices the change that bypassed the op log.
    let report = t.vault.verify(false).unwrap();
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("objects: row 0 does not match the op log (column title)")),
        "{report:?}"
    );
}

#[test]
fn undo_refuses_to_break_references_made_since() {
    let mut t = new_vault();
    let x = create(&mut t, object("x"));
    t.raw()
        .execute(
            "INSERT INTO blocks VALUES ('01JB3R7Q8Y5K3M2N4P6Q8R0S2T', ?1, NULL, 'a0', 'paragraph', '{}', 0, 0)",
            [x.to_string()],
        )
        .unwrap();
    let error = t.vault.undo().unwrap_err();
    assert!(matches!(error, StoreError::UndoBlocked(_)), "{error}");
    assert_eq!(t.vault.read().object(x).unwrap().title, "x");
}

#[test]
fn a_failed_service_stops_the_whole_transaction() {
    let mut t = new_vault();
    let before = snapshot(&t.path);
    let result = t.vault.transact(|txn| {
        txn.create_object(object("kept?"))?;
        assert!(matches!(
            txn.update_object(Id::from_parts(5, 5), |_| {}),
            Err(StoreError::NotFound { .. })
        ));
        assert_eq!(
            txn.read().children(None)?.len(),
            1,
            "the transaction sees its own changes"
        );
        Ok(())
    });
    assert!(matches!(result, Err(StoreError::Rule(_))));
    assert_eq!(snapshot(&t.path), before);

    let result = t.vault.transact(|txn| {
        txn.create_object(object("kept?"))?;
        Err::<(), _>(StoreError::Rule("stop".into()))
    });
    assert!(result.is_err());
    assert_eq!(snapshot(&t.path), before);

    let ((), committed) = t
        .vault
        .transact(|txn| {
            assert_eq!(
                i64::try_from(txn.group_id().timestamp_ms()).unwrap(),
                txn.now()
            );
            assert_ne!(txn.new_id()?, txn.new_id()?);
            Ok(())
        })
        .unwrap();
    assert!(
        committed.is_none() && !t.vault.can_undo(),
        "a transaction that changes nothing is not an undo step"
    );
}

#[test]
fn blocks_form_a_tree_inside_their_object() {
    let mut t = new_vault();
    let x = create(&mut t, object("x"));
    let y = create(&mut t, object("y"));
    let block = |object_id, parent, position, text: &str| NewBlock {
        object_id,
        parent_block_id: parent,
        position,
        kind: "paragraph".into(),
        content: json!({ "text": text }),
    };
    t.tick();
    let ((one, two, inner), _) = t
        .vault
        .transact(|txn| {
            let two = txn.create_block(block(x, None, Position::Last, "two"))?;
            let one = txn.create_block(block(x, None, Position::First, "one"))?;
            let inner = txn.create_block(block(x, Some(one), Position::Last, "inner"))?;
            Ok((one, two, inner))
        })
        .unwrap();
    let texts = |t: &TestVault| -> Vec<String> {
        t.vault
            .read()
            .blocks(x)
            .unwrap()
            .into_iter()
            .map(|b| b.content_json["text"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(texts(&t), ["one", "inner", "two"]);

    let elsewhere = t
        .vault
        .transact(|txn| txn.create_block(block(y, Some(one), Position::Last, "elsewhere")));
    assert!(matches!(elsewhere, Err(StoreError::Rule(_))));
    assert!(matches!(
        t.vault
            .transact(|txn| txn.move_block(one, Some(inner), Position::Last)),
        Err(StoreError::Rule(_))
    ));
    let other = create(&mut t, object("other"));
    t.tick();
    let (foreign, _) = t
        .vault
        .transact(|txn| txn.create_block(block(other, None, Position::Last, "foreign")))
        .unwrap();
    assert!(matches!(
        t.vault
            .transact(|txn| txn.move_block(two, Some(foreign), Position::Last)),
        Err(StoreError::Rule(_))
    ));

    t.tick();
    t.vault
        .transact(|txn| {
            assert!(txn.update_block(inner, |b| b.content_json = json!({"text": "inner!"}))?);
            assert!(!txn.update_block(inner, |_| {})?);
            txn.move_block(inner, None, Position::After(two))
        })
        .unwrap();
    assert_eq!(texts(&t), ["one", "two", "inner!"]);

    t.tick();
    t.vault
        .transact(|txn| txn.move_block(two, Some(one), Position::First))
        .unwrap();
    let mention = |src_id, src_block_id| NewRelation {
        src_id,
        dst_id: if src_id == x { y } else { x },
        rel_type: "mentions".into(),
        field_id: None,
        src_block_id: Some(src_block_id),
        label: None,
        props: Props::new(),
    };
    t.tick();
    t.vault
        .transact(|txn| txn.create_relation(mention(x, two)))
        .unwrap();
    assert!(matches!(
        t.vault.transact(|txn| txn.create_relation(mention(y, two))),
        Err(StoreError::Rule(_))
    ));

    t.tick();
    t.vault.transact(|txn| txn.delete_block(one)).unwrap();
    assert_eq!(texts(&t), ["inner!"]);
    assert!(
        t.vault.read().relations_to(y).unwrap().is_empty(),
        "relations from deleted blocks go with them"
    );
    assert!(matches!(
        t.vault.read().block(two),
        Err(StoreError::NotFound { .. })
    ));
    t.assert_verifies();
}

#[test]
fn relations_link_objects_and_can_be_trashed() {
    let mut t = new_vault();
    let x = create(&mut t, object("x"));
    let y = create(&mut t, object("y"));
    let task = type_id(&t.vault, "Task");
    let link = |label: Option<&str>, field_id| NewRelation {
        src_id: x,
        dst_id: y,
        rel_type: "blocks".into(),
        field_id,
        src_block_id: None,
        label: label.map(str::to_owned),
        props: props(&[("weight", json!(1))]),
    };
    t.tick();
    let ((relation, text_field), _) = t
        .vault
        .transact(|txn| {
            let relation = txn.create_relation(link(Some("first"), None))?;
            let text_field = txn.create_field(NewField {
                type_id: task,
                key: "notes".into(),
                label: "Notes".into(),
                kind: FieldKind::Text,
                options: json!({}),
                required: false,
            })?;
            Ok((relation, text_field))
        })
        .unwrap();
    assert_eq!(t.vault.read().relations_from(x).unwrap()[0].id, relation);
    assert_eq!(
        t.vault.read().relations_to(y).unwrap()[0].label.as_deref(),
        Some("first")
    );
    assert!(matches!(
        t.vault
            .transact(|txn| txn.create_relation(link(None, Some(text_field)))),
        Err(StoreError::Rule(_))
    ));

    t.tick();
    t.vault
        .transact(|txn| {
            assert!(txn.update_relation(relation, |r| {
                r.props_json.insert("weight".into(), json!(2));
            })?);
            assert!(txn.trash_relation(relation)?);
            assert!(!txn.trash_relation(relation)?);
            Ok(())
        })
        .unwrap();
    assert!(t.vault.read().relations_from(x).unwrap().is_empty());
    assert_eq!(
        t.vault.read().relation(relation).unwrap().props_json["weight"],
        json!(2)
    );
    t.tick();
    let ((restored, again), _) = t
        .vault
        .transact(|txn| {
            Ok((
                txn.restore_relation(relation)?,
                txn.restore_relation(relation)?,
            ))
        })
        .unwrap();
    assert!(restored && !again);

    t.tick();
    t.vault.transact(|txn| txn.trash_object(y)).unwrap();
    assert!(matches!(
        t.vault
            .transact(|txn| txn.create_relation(link(None, None))),
        Err(StoreError::Rule(_))
    ));
    t.tick();
    t.vault
        .transact(|txn| txn.delete_relation(relation))
        .unwrap();
    assert!(matches!(
        t.vault.read().relation(relation),
        Err(StoreError::NotFound { .. })
    ));
    t.assert_verifies();
}

#[test]
fn attachments_use_blobs_from_the_blob_store() {
    let mut t = new_vault();
    let x = create(&mut t, object("x"));
    let y = create(&mut t, object("y"));
    let stored = t.vault.blobs().put(b"%PDF-1.7").unwrap();
    let again = t.vault.blobs().put(b"%PDF-1.7").unwrap();
    assert!(stored.new && !again.new, "the same content is stored once");
    let attach = |object_id, mime: &str| NewAttachment {
        object_id,
        blob_hash: stored.hash.clone(),
        role: AttachmentRole::Original,
        filename: Some("a.pdf".into()),
        mime: Some(mime.into()),
    };
    t.tick();
    t.vault
        .transact(|txn| {
            txn.attach(attach(x, "application/pdf"))?;
            txn.attach(attach(y, "application/octet-stream"))
        })
        .unwrap();
    let attachments = t.vault.read().attachments(y).unwrap();
    assert_eq!(
        (attachments[0].blob_size, attachments[0].mime.as_deref()),
        (8, Some("application/octet-stream"))
    );
    let twice = t
        .vault
        .transact(|txn| txn.attach(attach(x, "application/pdf")));
    assert!(matches!(
        twice,
        Err(StoreError::Core(CoreError::Conflict { .. }))
    ));
    let missing = NewAttachment {
        blob_hash: dbme_core::ContentHash::of(b"not stored"),
        ..attach(x, "text/plain")
    };
    assert!(matches!(
        t.vault.transact(|txn| txn.attach(missing)),
        Err(StoreError::MissingBlob(_))
    ));

    t.tick();
    t.vault
        .transact(|txn| {
            assert!(txn.detach(x, stored.hash.clone(), AttachmentRole::Original)?);
            assert!(!txn.detach(x, stored.hash.clone(), AttachmentRole::Original)?);
            Ok(())
        })
        .unwrap();
    t.assert_verifies();

    // Damaged and missing blobs are reported.
    let path = t.vault.blobs().path(&stored.hash);
    fs::write(&path, b"%PDF-1.8").unwrap();
    assert!(
        t.vault.verify(false).unwrap().is_ok(),
        "a shallow check only compares sizes"
    );
    let report = t.vault.verify(true).unwrap();
    assert!(
        report.problems.iter().any(|p| p.contains("damaged")),
        "{report:?}"
    );
    fs::write(&path, b"short").unwrap();
    assert!(
        t.vault
            .verify(false)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("bytes in the blob store"))
    );
    fs::remove_file(&path).unwrap();
    assert!(
        t.vault
            .verify(false)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("missing"))
    );
}

#[test]
fn the_op_log_is_append_only_and_tampering_is_found() {
    let t = new_vault();
    let raw = t.raw();
    let error = raw
        .execute("UPDATE ops SET kind = 'type.delete' WHERE seq = 1", [])
        .unwrap_err();
    assert!(error.to_string().contains("append-only"), "{error}");
    assert!(raw.execute("DELETE FROM ops WHERE seq = 1", []).is_err());

    raw.execute_batch("DROP TRIGGER ops_append_only_update")
        .unwrap();
    raw.execute(
        "UPDATE ops SET payload_json = replace(payload_json, 'Note', 'Gnote') WHERE seq = 1",
        [],
    )
    .unwrap();
    let report = t.vault.verify(false).unwrap();
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.starts_with("op log: op 1: hash should be")),
        "{report:?}"
    );
    assert_eq!(report.ops, 0);

    raw.execute("UPDATE ops SET op_id = 'not an id' WHERE seq = 1", [])
        .unwrap();
    assert!(
        t.vault
            .verify(false)
            .unwrap()
            .problems
            .iter()
            .any(|p| p.contains("cannot be read"))
    );
}

#[test]
fn verify_reports_rows_missing_from_or_added_to_the_tables() {
    let mut t = new_vault();
    let x = create(&mut t, object("x"));
    let raw = t.raw();
    raw.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    raw.execute(
        "INSERT INTO object_types VALUES (?1, 'NOSUCHTYPE')",
        [x.to_string()],
    )
    .unwrap();
    let report = t.vault.verify(false).unwrap();
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("object_types has rows the op log does not create")),
        "{report:?}"
    );
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("refers to a row of types")),
        "{report:?}"
    );
    raw.execute("DELETE FROM object_types", []).unwrap();
    raw.execute(
        "DELETE FROM types WHERE id = (SELECT max(id) FROM types)",
        [],
    )
    .unwrap();
    let report = t.vault.verify(false).unwrap();
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("types is missing rows the op log creates")),
        "{report:?}"
    );
}

#[test]
fn tree_and_list_reads_keep_the_order_their_sql_defines() {
    // These reads walk trees and sort in Rust rather than in SQL (ADR-0015). The trees here have a
    // sort-key order that differs from their ID order at every level, and each read is compared
    // with its definition as one SQL query.
    let mut t = new_vault();
    let types = ["Note", "Task", "Person"].map(|name| type_id(&t.vault, name));
    let one = t.vault.blobs().put(b"one").unwrap().hash;
    let two = t.vault.blobs().put(b"two").unwrap().hash;
    t.tick();
    let ((root, top), _) = t
        .vault
        .transact(|txn| {
            let root = txn.create_object(NewObject {
                type_ids: types.into(),
                ..object("root")
            })?;
            let mut parents = vec![root];
            for _ in 0..3 {
                let mut next = Vec::new();
                for &parent in &parents {
                    let child = |position| NewObject {
                        parent_id: Some(parent),
                        position,
                        ..object("child")
                    };
                    let last = txn.create_object(child(Position::Last))?;
                    let first = txn.create_object(child(Position::First))?;
                    let between = txn.create_object(child(Position::After(first)))?;
                    next.extend([last, first, between]);
                }
                parents = next;
            }
            let block = |parent, position| NewBlock {
                object_id: root,
                parent_block_id: parent,
                position,
                kind: "paragraph".into(),
                content: json!({}),
            };
            let top = txn.create_block(block(None, Position::Last))?;
            let first = txn.create_block(block(None, Position::First))?;
            for parent in [top, first] {
                let last = txn.create_block(block(Some(parent), Position::Last))?;
                let first = txn.create_block(block(Some(parent), Position::First))?;
                txn.create_block(block(Some(last), Position::Last))?;
                txn.create_block(block(Some(first), Position::Last))?;
            }
            for (blob_hash, role) in [
                (&two, AttachmentRole::Thumbnail),
                (&one, AttachmentRole::Original),
                (&two, AttachmentRole::OcrText),
                (&one, AttachmentRole::OcrText),
            ] {
                txn.attach(NewAttachment {
                    object_id: root,
                    blob_hash: blob_hash.clone(),
                    role,
                    filename: None,
                    mime: None,
                })?;
            }
            Ok((root, top))
        })
        .unwrap();

    let db = rusqlite::Connection::open(t.path.join(DATABASE_FILE)).unwrap();
    let sql = |query: &str, id: Id| -> Vec<String> {
        db.prepare(query)
            .unwrap()
            .query_map([id.to_string()], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let read = t.vault.read();
    let ids = |ids: Vec<Id>| -> Vec<String> { ids.iter().map(Id::to_string).collect() };

    let subtree = ids(read.subtree(root).unwrap().iter().map(|o| o.id).collect());
    assert_eq!(subtree.len(), 40);
    assert_eq!(
        subtree,
        sql(
            "WITH RECURSIVE tree(node, depth) AS (SELECT ?1, 0 UNION ALL \
             SELECT o.id, tree.depth + 1 FROM objects o JOIN tree ON o.parent_id = tree.node) \
             SELECT objects.id FROM tree CROSS JOIN objects ON objects.id = tree.node \
             ORDER BY tree.depth, sort_key, objects.id",
            root
        )
    );
    let blocks = ids(read.blocks(root).unwrap().iter().map(|b| b.id).collect());
    assert_eq!(blocks.len(), 10);
    assert_eq!(
        blocks,
        sql(
            "WITH RECURSIVE tree(node, path) AS (SELECT id, sort_key || ' ' || id FROM blocks \
             WHERE object_id = ?1 AND parent_block_id IS NULL UNION ALL \
             SELECT b.id, tree.path || '/' || b.sort_key || ' ' || b.id \
             FROM blocks b JOIN tree ON b.parent_block_id = tree.node) \
             SELECT node FROM tree ORDER BY path",
            root
        )
    );
    assert_eq!(
        ids(read
            .block_subtree(top)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect()),
        sql(
            "WITH RECURSIVE tree(node, depth) AS (SELECT ?1, 0 UNION ALL \
             SELECT b.id, tree.depth + 1 FROM blocks b JOIN tree ON b.parent_block_id = tree.node) \
             SELECT blocks.id FROM tree CROSS JOIN blocks ON blocks.id = tree.node \
             ORDER BY tree.depth, sort_key, blocks.id",
            top
        )
    );
    assert_eq!(
        read.attachments(root)
            .unwrap()
            .iter()
            .map(|a| format!("{} {}", a.role.as_str(), a.blob_hash))
            .collect::<Vec<_>>(),
        sql(
            "SELECT role || ' ' || blob_hash FROM attachments WHERE object_id = ?1 \
             ORDER BY role, blob_hash",
            root
        )
    );
    assert_eq!(
        ids(read.object_type_ids(root).unwrap()),
        sql(
            "SELECT type_id FROM object_types WHERE object_id = ?1 ORDER BY type_id",
            root
        )
    );
    assert!(read.subtree(Id::from_parts(9, 9)).unwrap().is_empty());
    assert!(read.block_subtree(Id::from_parts(9, 9)).unwrap().is_empty());
    t.assert_verifies();
}
