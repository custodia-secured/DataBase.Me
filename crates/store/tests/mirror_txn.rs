// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transaction side of the files folder (M5 Task 3): which objects a change marks for the
//! folder to write again, transactions on the folder's behalf, objects created with a given ID,
//! and the readers the folder and the export need.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{TestVault, new_vault};
use dbme_core::Id;
use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_core::query::Query;
use dbme_store::{
    FULL_REWRITE_NEEDED, NewAttachment, NewBlock, NewField, NewObject, NewRelation, NewType,
    NewView, Position, StoreError, Vault,
};
use serde_json::json;

fn dirty(t: &mut TestVault) -> Vec<Id> {
    t.vault.mirror_dirty_take().unwrap()
}

fn full_rewrite(t: &mut TestVault) -> bool {
    let set = t
        .vault
        .mirror_meta_get(FULL_REWRITE_NEEDED)
        .unwrap()
        .is_some();
    t.vault.mirror_meta_set(FULL_REWRITE_NEEDED, None).unwrap();
    set
}

fn object(vault: &mut Vault, title: &str, parent: Option<Id>) -> Id {
    vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: title.to_owned(),
                parent_id: parent,
                ..NewObject::default()
            })
        })
        .unwrap()
        .0
}

fn paragraph(object_id: Id, content: serde_json::Value) -> NewBlock {
    NewBlock {
        id: None,
        object_id,
        parent_block_id: None,
        position: Position::Last,
        kind: "paragraph".to_owned(),
        content,
    }
}

fn related(src_id: Id, dst_id: Id) -> NewRelation {
    NewRelation {
        src_id,
        dst_id,
        rel_type: "related".to_owned(),
        field_id: None,
        src_block_id: None,
        label: None,
        props: Props::new(),
    }
}

fn sorted(mut ids: Vec<Id>) -> Vec<Id> {
    ids.sort();
    ids
}

#[test]
fn every_change_marks_the_objects_whose_files_change() {
    let mut t = new_vault();
    assert_eq!(dirty(&mut t), []);

    let a = object(&mut t.vault, "A", None);
    assert_eq!(dirty(&mut t), [a]);
    let b = object(&mut t.vault, "B", Some(a));
    // C links to A twice: a relation, and a mention in a block (a relation too).
    let c = object(&mut t.vault, "C", None);
    t.vault
        .transact(|txn| {
            txn.create_relation(related(c, a))?;
            txn.create_block(paragraph(
                c,
                json!({"inline": [{"mention": a.to_string()}]}),
            ))
        })
        .unwrap();
    let d = object(&mut t.vault, "D", None);
    assert_eq!(dirty(&mut t), [b, c, d]);

    // A change to the object alone marks it alone.
    t.vault
        .transact(|txn| txn.update_object(a, |o| o.summary = Some("s".into())))
        .unwrap();
    assert_eq!(dirty(&mut t), [a]);
    // No change, no mark.
    t.vault
        .transact(|txn| txn.update_object(a, |o| o.summary = Some("s".into())))
        .unwrap();
    assert_eq!(dirty(&mut t), []);

    // A title change moves A's path and B's, and C's wikilinks name A's path.
    t.vault
        .transact(|txn| txn.update_object(a, |o| o.title = "A2".into()))
        .unwrap();
    assert_eq!(dirty(&mut t), sorted(vec![a, b, c]));

    // So does a move, a trash and a restore.
    t.vault
        .transact(|txn| txn.move_object(a, Some(d), Position::Last))
        .unwrap();
    assert_eq!(dirty(&mut t), sorted(vec![a, b, c]));
    t.vault.transact(|txn| txn.trash_object(a)).unwrap();
    assert_eq!(dirty(&mut t), sorted(vec![a, b, c]));
    t.vault.transact(|txn| txn.restore_object(a)).unwrap();
    assert_eq!(dirty(&mut t), sorted(vec![a, b, c]));
    // Rebalancing is a move too, for every object whose key actually changed.
    let e = object(&mut t.vault, "E", Some(d));
    dirty(&mut t);
    let (moved, _) = t
        .vault
        .transact(|txn| txn.rebalance_objects(Some(d)))
        .unwrap();
    let marked = dirty(&mut t);
    assert!(
        marked.iter().all(|id| [a, b, c, e].contains(id)),
        "{marked:?}"
    );
    assert_eq!(moved == 0, marked.is_empty());

    // Types on an object.
    let kind = t
        .vault
        .transact(|txn| {
            txn.create_type(NewType {
                name: "Kind".into(),
                ..NewType::default()
            })
        })
        .unwrap()
        .0;
    assert!(full_rewrite(&mut t));
    assert_eq!(dirty(&mut t), []);
    t.vault.transact(|txn| txn.add_type(b, kind)).unwrap();
    assert_eq!(dirty(&mut t), [b]);
    t.vault.transact(|txn| txn.remove_type(b, kind)).unwrap();
    assert_eq!(dirty(&mut t), [b]);
    assert!(!full_rewrite(&mut t));

    // Blocks mark their owner.
    let block = t
        .vault
        .transact(|txn| txn.create_block(paragraph(b, json!({"inline": [{"text": "x"}]}))))
        .unwrap()
        .0;
    assert_eq!(dirty(&mut t), [b]);
    t.vault
        .transact(|txn| {
            txn.update_block(block, |bl| {
                bl.content_json = json!({"inline": [{"text": "y"}]});
            })
        })
        .unwrap();
    assert_eq!(dirty(&mut t), [b]);
    let second = t
        .vault
        .transact(|txn| txn.create_block(paragraph(b, json!({"inline": [{"text": "z"}]}))))
        .unwrap()
        .0;
    dirty(&mut t);
    t.vault
        .transact(|txn| txn.move_block(second, None, Position::First))
        .unwrap();
    assert_eq!(dirty(&mut t), [b]);
    t.vault
        .transact(|txn| txn.rebalance_blocks(b, None))
        .unwrap();
    assert_eq!(dirty(&mut t), [b]);
    t.vault.transact(|txn| txn.delete_block(second)).unwrap();
    assert_eq!(dirty(&mut t), [b]);

    // Relations mark their source, whose file shows the link.
    let relation = t
        .vault
        .transact(|txn| txn.create_relation(related(c, b)))
        .unwrap()
        .0;
    assert_eq!(dirty(&mut t), [c]);
    t.vault
        .transact(|txn| txn.update_relation(relation, |r| r.label = Some("l".into())))
        .unwrap();
    assert_eq!(dirty(&mut t), [c]);
    t.vault
        .transact(|txn| txn.trash_relation(relation))
        .unwrap();
    assert_eq!(dirty(&mut t), [c]);
    t.vault
        .transact(|txn| txn.restore_relation(relation))
        .unwrap();
    assert_eq!(dirty(&mut t), [c]);
    t.vault
        .transact(|txn| txn.delete_relation(relation))
        .unwrap();
    assert_eq!(dirty(&mut t), [c]);

    // Attachments mark their object.
    let blob = t.vault.blobs().put(b"bytes").unwrap();
    t.vault
        .transact(|txn| {
            txn.attach(NewAttachment {
                object_id: d,
                blob_hash: blob.hash.clone(),
                role: AttachmentRole::Original,
                filename: Some("f.png".into()),
                mime: Some("image/png".into()),
            })
        })
        .unwrap();
    assert_eq!(dirty(&mut t), [d]);
    t.vault
        .transact(|txn| txn.detach(d, blob.hash.clone(), AttachmentRole::Original))
        .unwrap();
    assert_eq!(dirty(&mut t), [d]);

    // Fields and types mark every file instead of any object.
    let field = t
        .vault
        .transact(|txn| {
            txn.create_field(NewField {
                type_id: kind,
                key: "k".into(),
                label: "K".into(),
                kind: FieldKind::Text,
                options: json!({}),
                required: false,
            })
        })
        .unwrap()
        .0;
    assert!(full_rewrite(&mut t));
    t.vault
        .transact(|txn| txn.update_field(field, |f| f.label = "K2".into()))
        .unwrap();
    assert!(full_rewrite(&mut t));
    t.vault.transact(|txn| txn.delete_field(field)).unwrap();
    assert!(full_rewrite(&mut t));
    t.vault
        .transact(|txn| txn.update_type(kind, |ty| ty.plural = Some("Kinds".into())))
        .unwrap();
    assert!(full_rewrite(&mut t));
    t.vault.transact(|txn| txn.delete_type(kind)).unwrap();
    assert!(full_rewrite(&mut t));
    assert_eq!(dirty(&mut t), []);

    // Views live in the database only.
    t.vault
        .transact(|txn| {
            txn.create_view(NewView {
                name: "v".into(),
                renderer: "list".into(),
                query: Query::default(),
                config: json!({}),
                owner_object_id: None,
            })
        })
        .unwrap();
    assert_eq!(dirty(&mut t), []);
    assert!(!full_rewrite(&mut t));

    // A failed transaction marks nothing.
    let failed = t.vault.transact(|txn| {
        txn.create_type(NewType {
            name: "Lost".into(),
            ..NewType::default()
        })?;
        txn.update_object(d, |o| o.title = "D2".into())?;
        txn.create_object(NewObject {
            parent_id: Some(Id::from_parts(3, 3)),
            ..NewObject::default()
        })
    });
    assert!(failed.is_err());
    assert_eq!(dirty(&mut t), []);
    assert!(!full_rewrite(&mut t));

    // Permanent deletion marks the object whose file goes.
    t.vault.transact(|txn| txn.trash_object(d)).unwrap();
    dirty(&mut t);
    t.vault.transact(|txn| txn.delete_object(d)).unwrap();
    assert!(dirty(&mut t).contains(&d));
    t.assert_verifies();
}

#[test]
fn external_transactions_are_a_second_device_off_the_undo_stack() {
    let mut t = new_vault();
    let mirror = t.vault.mirror_identity();
    assert_eq!(mirror.actor_id, common::identity().actor_id);
    assert_ne!(mirror.device_id, common::identity().device_id);
    assert_eq!(mirror, common::identity().mirror());
    assert!(!t.vault.can_undo());

    let (a, committed) = t
        .vault
        .transact_external(|txn| {
            txn.create_object(NewObject {
                title: "From a file".into(),
                ..NewObject::default()
            })
        })
        .unwrap();
    let committed = committed.unwrap();
    assert_eq!(committed.changed, vec![a]);
    let last = &t.vault.read().history(1).unwrap()[0].entry;
    assert_eq!(last.device_id, mirror.device_id);
    assert_eq!(last.actor_id, mirror.actor_id);
    assert_eq!(last.group_id, committed.group_id);
    // Not an undo step, and nothing for the folder to write back.
    assert!(!t.vault.can_undo());
    assert!(!t.vault.can_redo());
    assert_eq!(t.vault.mirror_dirty_take().unwrap(), []);

    // An app step, then an external change to the same block.
    t.tick();
    let block = t
        .vault
        .transact(|txn| txn.create_block(paragraph(a, json!({"inline": [{"text": "app"}]}))))
        .unwrap()
        .0;
    assert!(t.vault.can_undo());
    t.tick();
    t.vault
        .transact_external(|txn| {
            txn.update_block(block, |bl| {
                bl.content_json = json!({"inline": [{"text": "file"}]});
            })
        })
        .unwrap();
    assert!(
        t.vault.can_undo(),
        "the external step did not touch the stack"
    );
    assert!(!t.vault.can_redo());
    assert!(matches!(t.vault.undo(), Err(StoreError::UndoBlocked(_))));
    assert!(!t.vault.can_undo(), "the blocked step is dropped");
    assert_eq!(
        t.vault.read().block(block).unwrap().content_json,
        json!({"inline": [{"text": "file"}]})
    );

    // An external transaction that fails saves nothing.
    let result = t.vault.transact_external(|txn| {
        txn.create_object(NewObject {
            parent_id: Some(Id::from_parts(3, 3)),
            ..NewObject::default()
        })
    });
    assert!(result.is_err());
    // One with no change commits nothing.
    let (_, none) = t.vault.transact_external(|_| Ok(())).unwrap();
    assert!(none.is_none());
    t.assert_verifies();
}

#[test]
fn an_object_can_bring_its_id_and_creation_time() {
    let mut t = new_vault();
    let given = Id::from_parts(1_700_000_000_000, 77);
    let (id, _) = t
        .vault
        .transact(|txn| {
            txn.create_object(NewObject {
                id: Some(given),
                created_at: Some(1_700_000_000_000),
                title: "Adopted".into(),
                ..NewObject::default()
            })
        })
        .unwrap();
    assert_eq!(id, given);
    let row = t.vault.read().object(given).unwrap();
    assert_eq!(row.created_at, 1_700_000_000_000);
    assert_eq!(row.updated_at, common::START);

    // A duplicate is refused whether the holder is live or in the Trash.
    let duplicate = |vault: &mut Vault| {
        vault.transact(|txn| {
            txn.create_object(NewObject {
                id: Some(given),
                ..NewObject::default()
            })
        })
    };
    assert!(matches!(duplicate(&mut t.vault), Err(StoreError::Rule(_))));
    t.vault.transact(|txn| txn.trash_object(given)).unwrap();
    assert!(matches!(duplicate(&mut t.vault), Err(StoreError::Rule(_))));

    // The defaults are a fresh ID and the transaction's time.
    t.tick();
    let (fresh, _) = t
        .vault
        .transact(|txn| txn.create_object(NewObject::default()))
        .unwrap();
    assert_ne!(fresh, given);
    let row = t.vault.read().object(fresh).unwrap();
    assert_eq!(row.created_at, common::START + 1);
    assert_eq!(row.updated_at, common::START + 1);

    // The JSON shape of a client that knows neither key still parses.
    let parsed: NewObject = serde_json::from_str(r#"{"title": "plain"}"#).unwrap();
    assert_eq!((parsed.id, parsed.created_at), (None, None));
    t.assert_verifies();
}

#[test]
fn the_readers_the_folder_and_the_export_need() {
    let mut t = new_vault();
    let a = object(&mut t.vault, "Same", None);
    let a_child = object(&mut t.vault, "Same", Some(a));
    let b = object(&mut t.vault, "Same", None);
    object(&mut t.vault, "same", None);
    let gone = object(&mut t.vault, "Same", None);
    t.vault.transact(|txn| txn.trash_object(gone)).unwrap();

    let read = t.vault.read();
    assert_eq!(
        read.objects_titled(None, "Same")
            .unwrap()
            .iter()
            .map(|o| o.id)
            .collect::<Vec<_>>(),
        [a, b]
    );
    assert_eq!(
        read.objects_titled(Some(a), "Same")
            .unwrap()
            .iter()
            .map(|o| o.id)
            .collect::<Vec<_>>(),
        [a_child]
    );
    assert!(read.objects_titled(None, "Other").unwrap().is_empty());

    let all = read.ops_after(0, u32::MAX).unwrap();
    assert_eq!(all.len(), 25 + 6);
    assert_eq!(all[0].seq, 1);
    assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
    let page = read.ops_after(25, 3).unwrap();
    assert_eq!(page.iter().map(|r| r.seq).collect::<Vec<_>>(), [26, 27, 28]);
    assert_eq!(page[0].entry.target_id, a);
    assert!(read.ops_after(31, 10).unwrap().is_empty());

    let meta = read.vault_meta().unwrap();
    assert_eq!(
        meta.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["created_at", "created_by", "format_version", "vault_id"]
    );
    assert_eq!(meta[3].1, t.vault.id().to_string());
    assert_eq!(meta[0].1, common::START.to_string());
    assert_eq!(meta[2].1, dbme_core::FORMAT_VERSION.to_string());
}
