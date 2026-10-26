// SPDX-License-Identifier: AGPL-3.0-or-later
//! Block content rules (ADR-0016): checked content, client-made IDs, children only under list
//! items, and mentions kept as relations.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::block::MENTIONS;
use dbme_store::{NewBlock, NewObject, Position, StoreError};
use serde_json::{Value, json};

use common::{TestVault, new_vault};

fn create(t: &mut TestVault, title: &str) -> Id {
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

fn block(object_id: Id, kind: &str, content: Value) -> NewBlock {
    NewBlock {
        id: None,
        object_id,
        parent_block_id: None,
        position: Position::Last,
        kind: kind.into(),
        content,
    }
}

fn text(text: &str) -> Value {
    json!({ "inline": [{ "text": text }] })
}

fn mentioning(ids: &[Id]) -> Value {
    let runs: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "mention": id.to_string() }))
        .collect();
    json!({ "inline": runs })
}

/// The targets of `object`'s live mention relations, sorted.
fn mentioned(t: &TestVault, object: Id) -> Vec<Id> {
    let mut ids: Vec<Id> = t
        .vault
        .read()
        .relations_from(object)
        .unwrap()
        .into_iter()
        .filter(|r| r.rel_type == MENTIONS)
        .map(|r| r.dst_id)
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn content_is_checked_against_its_kind() {
    let mut t = new_vault();
    let x = create(&mut t, "x");
    t.tick();
    let bad = t
        .vault
        .transact(|txn| txn.create_block(block(x, "paragraph", json!({"text": "old shape"}))));
    assert!(matches!(bad, Err(StoreError::Core(_))), "{bad:?}");
    let (id, _) = t
        .vault
        .transact(|txn| txn.create_block(block(x, "paragraph", text("fine"))))
        .unwrap();
    t.tick();
    let bad = t.vault.transact(|txn| {
        txn.update_block(id, |b| b.content_json = json!({"inline": [{"text": ""}]}))
    });
    assert!(matches!(bad, Err(StoreError::Core(_))), "{bad:?}");
    let moved = t
        .vault
        .transact(|txn| txn.update_block(id, |b| b.parent_block_id = Some(id)));
    assert!(matches!(moved, Err(StoreError::Rule(_))), "{moved:?}");
    // A kind this version does not know keeps whatever object it holds.
    t.vault
        .transact(|txn| txn.create_block(block(x, "chart", json!({"spec": [1, 2]}))))
        .unwrap();
    t.assert_verifies();
}

#[test]
fn a_client_made_id_is_used_once() {
    let mut t = new_vault();
    let x = create(&mut t, "x");
    let id = Id::from_parts(1_761_380_471_500, 42);
    t.tick();
    let (created, _) = t
        .vault
        .transact(|txn| {
            txn.create_block(NewBlock {
                id: Some(id),
                ..block(x, "paragraph", text("mine"))
            })
        })
        .unwrap();
    assert_eq!(created, id);
    assert_eq!(t.vault.read().block(id).unwrap().content_json, text("mine"));
    t.tick();
    let again = t.vault.transact(|txn| {
        txn.create_block(NewBlock {
            id: Some(id),
            ..block(x, "paragraph", text("again"))
        })
    });
    assert!(matches!(again, Err(StoreError::Rule(_))), "{again:?}");

    // Undo removes it and redo brings it back under the same ID.
    t.tick();
    t.vault.undo().unwrap();
    assert!(t.vault.read().blocks(x).unwrap().is_empty());
    t.tick();
    t.vault.redo().unwrap();
    assert_eq!(t.vault.read().block(id).unwrap().id, id);
    t.assert_verifies();
}

#[test]
fn only_list_items_have_children() {
    let mut t = new_vault();
    let x = create(&mut t, "x");
    t.tick();
    let ((paragraph, item, child), _) = t
        .vault
        .transact(|txn| {
            let paragraph = txn.create_block(block(x, "paragraph", text("p")))?;
            let item = txn.create_block(block(x, "bullet", text("item")))?;
            let child = txn.create_block(NewBlock {
                parent_block_id: Some(item),
                ..block(x, "todo", json!({"checked": true, "inline": []}))
            })?;
            Ok((paragraph, item, child))
        })
        .unwrap();

    t.tick();
    let under_paragraph = t.vault.transact(|txn| {
        txn.create_block(NewBlock {
            parent_block_id: Some(paragraph),
            ..block(x, "paragraph", text("no"))
        })
    });
    assert!(matches!(under_paragraph, Err(StoreError::Rule(_))));
    let moved_under_paragraph = t
        .vault
        .transact(|txn| txn.move_block(child, Some(paragraph), Position::Last));
    assert!(matches!(moved_under_paragraph, Err(StoreError::Rule(_))));
    let item_to_paragraph = t
        .vault
        .transact(|txn| txn.update_block(item, |b| b.kind = "paragraph".into()));
    assert!(matches!(item_to_paragraph, Err(StoreError::Rule(_))));

    // Once its child has moved out, the item can become a paragraph, in the same step.
    t.tick();
    t.vault
        .transact(|txn| {
            txn.move_block(child, None, Position::After(item))?;
            txn.update_block(item, |b| b.kind = "paragraph".into())
        })
        .unwrap();
    let kinds: Vec<String> = t
        .vault
        .read()
        .blocks(x)
        .unwrap()
        .into_iter()
        .map(|b| b.kind)
        .collect();
    assert_eq!(kinds, ["paragraph", "paragraph", "todo"]);
    t.assert_verifies();
}

#[test]
fn mentions_are_kept_as_relations_in_the_same_step() {
    let mut t = new_vault();
    let page = create(&mut t, "page");
    let a = create(&mut t, "a");
    let b = create(&mut t, "b");
    let gone = Id::from_parts(1, 1);

    t.tick();
    let (id, created) = t
        .vault
        .transact(|txn| txn.create_block(block(page, "paragraph", mentioning(&[a, a, gone]))))
        .unwrap();
    assert_eq!(mentioned(&t, page), [a], "missing objects get no relation");
    let relation = &t.vault.read().relations_to(a).unwrap()[0];
    assert_eq!((relation.src_id, relation.src_block_id), (page, Some(id)));
    assert!(created.unwrap().changed.contains(&relation.id));

    // Changing the mentions changes the relations; other edits leave them alone.
    t.tick();
    t.vault
        .transact(|txn| txn.update_block(id, |blk| blk.content_json = mentioning(&[b])))
        .unwrap();
    assert_eq!(mentioned(&t, page), [b]);
    let before = t.vault.read().relations_from(page).unwrap();
    t.tick();
    t.vault
        .transact(|txn| {
            txn.update_block(id, |blk| {
                blk.content_json = json!({"inline": [{"mention": b.to_string()}, {"text": "!"}]});
            })
        })
        .unwrap();
    assert_eq!(t.vault.read().relations_from(page).unwrap(), before);

    // One undo reverts the block and its relations together.
    t.tick();
    t.vault.undo().unwrap();
    t.tick();
    t.vault.undo().unwrap();
    assert_eq!(mentioned(&t, page), [a]);
    t.tick();
    t.vault.redo().unwrap();
    assert_eq!(mentioned(&t, page), [b]);

    // A mention of an object in the Trash gets no relation; deleting the block removes its own.
    t.tick();
    t.vault.transact(|txn| txn.trash_object(a)).unwrap();
    t.tick();
    let (other, _) = t
        .vault
        .transact(|txn| txn.create_block(block(page, "paragraph", mentioning(&[a]))))
        .unwrap();
    assert_eq!(mentioned(&t, page), [b]);
    t.tick();
    t.vault
        .transact(|txn| {
            txn.delete_block(id)?;
            txn.delete_block(other)
        })
        .unwrap();
    assert!(mentioned(&t, page).is_empty());
    t.assert_verifies();
}
