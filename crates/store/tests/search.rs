// SPDX-License-Identifier: AGPL-3.0-or-later
//! Full-text search (ADR-0020): the index follows every change, undo included, and search ranks
//! titles above bodies.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::model::Props;
use dbme_store::{NewBlock, NewObject, Position, SearchHit};
use serde_json::{Value, json};

use common::{TestVault, new_vault};

fn paragraph(text: &str) -> Value {
    json!({"inline": [{"text": text}]})
}

fn titles(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter()
        .map(|h| h.summary.object.title.as_str())
        .collect()
}

fn find(t: &TestVault, text: &str) -> Vec<SearchHit> {
    t.vault.read().search(text, 20).unwrap()
}

fn create(t: &mut TestVault, title: &str, body: Option<&str>) -> Id {
    t.tick();
    t.vault
        .transact(|txn| {
            let id = txn.create_object(NewObject {
                title: title.into(),
                ..Default::default()
            })?;
            if let Some(body) = body {
                txn.create_block(NewBlock {
                    id: None,
                    object_id: id,
                    parent_block_id: None,
                    position: Position::Last,
                    kind: "paragraph".into(),
                    content: paragraph(body),
                })?;
            }
            Ok(id)
        })
        .unwrap()
        .0
}

#[test]
fn titles_rank_above_bodies_and_the_snippet_marks_the_match() {
    let mut t = new_vault();
    create(&mut t, "Budget review", Some("Numbers for the quarter."));
    create(
        &mut t,
        "Quarterly notes",
        Some("The budget was reviewed on Monday."),
    );
    create(&mut t, "Unrelated", Some("Nothing here."));

    let hits = find(&t, "budget");
    assert_eq!(titles(&hits), ["Budget review", "Quarterly notes"]);
    assert!(hits[0].rank < hits[1].rank, "lower is better");
    assert_eq!(hits[0].snippet, "[Budget] review");
    assert_eq!(hits[1].snippet, "The [budget] was reviewed on Monday.");

    assert_eq!(
        titles(&find(&t, "quart")),
        ["Quarterly notes", "Budget review"]
    );
    assert_eq!(titles(&find(&t, "\"budget was\"")), ["Quarterly notes"]);
    assert_eq!(titles(&find(&t, "monday numbers")), Vec::<&str>::new());
    assert_eq!(titles(&find(&t, "AND OR NOT (")), Vec::<&str>::new());
    t.assert_verifies();
}

#[test]
fn with_no_words_the_most_recently_changed_come_first() {
    let mut t = new_vault();
    create(&mut t, "First", None);
    create(&mut t, "Second", None);
    let hits = find(&t, "  ");
    assert_eq!(titles(&hits), ["Second", "First"]);
    assert!(hits.iter().all(|h| h.snippet.is_empty() && h.rank == 0.0));
}

#[test]
fn accents_and_case_do_not_matter() {
    let mut t = new_vault();
    create(&mut t, "Café plans", None);
    create(&mut t, "Plain", Some("We met at the CAFE."));
    assert_eq!(titles(&find(&t, "cafe")), ["Café plans", "Plain"]);
    assert_eq!(titles(&find(&t, "CAFÉ")), ["Café plans", "Plain"]);
}

#[test]
fn the_index_follows_every_change_and_its_undo() {
    let mut t = new_vault();
    let id = create(&mut t, "Draft", None);
    let other = create(&mut t, "Other", None);
    let found = |t: &TestVault, text: &str| titles(&find(t, text)).contains(&"Draft");
    let found_as = |t: &TestVault, text: &str, title: &str| titles(&find(t, text)).contains(&title);

    // A block gives the object a body.
    t.tick();
    let (block, _) = t
        .vault
        .transact(|txn| {
            txn.create_block(NewBlock {
                id: None,
                object_id: id,
                parent_block_id: None,
                position: Position::Last,
                kind: "paragraph".into(),
                content: paragraph("pelican"),
            })
        })
        .unwrap();
    assert!(found(&t, "pelican"));
    t.vault.undo().unwrap().unwrap();
    assert!(!found(&t, "pelican"));
    t.vault.redo().unwrap().unwrap();
    assert!(found(&t, "pelican"));

    // Editing the block changes the body; deleting it removes the body.
    t.tick();
    t.vault
        .transact(|txn| txn.update_block(block, |b| b.content_json = paragraph("heron")))
        .unwrap();
    assert!(found(&t, "heron") && !found(&t, "pelican"));
    t.tick();
    t.vault.transact(|txn| txn.delete_block(block)).unwrap();
    assert!(!found(&t, "heron"));
    t.vault.undo().unwrap().unwrap();
    assert!(found(&t, "heron"));

    // A field value is searchable text; a rename changes the title.
    t.tick();
    t.vault
        .transact(|txn| {
            txn.update_object(id, |o| {
                o.props_json.insert("code".into(), json!("ZX81"));
            })
        })
        .unwrap();
    assert!(found(&t, "zx81"));
    t.tick();
    t.vault
        .transact(|txn| txn.update_object(id, |o| o.title = "Final".into()))
        .unwrap();
    assert!(found_as(&t, "final", "Final") && !found(&t, "draft"));
    t.vault.undo().unwrap().unwrap();
    assert!(found(&t, "draft"));

    // The Trash hides an object from search; restoring brings it back; deleting removes it.
    t.tick();
    t.vault.transact(|txn| txn.trash_object(id)).unwrap();
    assert!(!found(&t, "draft"));
    t.vault.undo().unwrap().unwrap();
    assert!(found(&t, "draft"));
    t.tick();
    t.vault
        .transact(|txn| {
            txn.trash_object(id)?;
            txn.delete_object(id)
        })
        .unwrap();
    assert!(!found(&t, "draft"));
    t.vault.undo().unwrap().unwrap();
    assert!(found(&t, "draft"));

    // Moving changes nothing in the index, and the children of a trashed object leave with it.
    t.tick();
    let (child, _) = t
        .vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Nested egret".into(),
                parent_id: Some(other),
                ..Default::default()
            })
        })
        .unwrap();
    t.tick();
    t.vault
        .transact(|txn| txn.move_object(child, Some(id), Position::Last))
        .unwrap();
    assert!(found_as(&t, "egret", "Nested egret"));
    t.tick();
    t.vault.transact(|txn| txn.trash_object(id)).unwrap();
    assert!(!found_as(&t, "egret", "Nested egret"));
    t.tick();
    t.vault.transact(|txn| txn.restore_object(id)).unwrap();
    assert!(found_as(&t, "egret", "Nested egret"));
    t.assert_verifies();
}

#[test]
fn changes_queue_up_and_the_next_search_drains_the_queue() {
    let mut t = new_vault();
    let dirty = |t: &TestVault| -> i64 {
        t.raw()
            .query_row("SELECT count(*) FROM search_dirty", [], |r| r.get(0))
            .unwrap()
    };
    let a = create(&mut t, "Queued one", None);
    create(&mut t, "Queued two", None);
    assert_eq!(
        dirty(&t),
        2,
        "a change notes its objects and does no index work"
    );
    assert_eq!(titles(&find(&t, "queued")).len(), 2);
    assert_eq!(dirty(&t), 0, "a search brings the index up to date first");

    t.tick();
    t.vault
        .transact(|txn| txn.move_object(a, None, Position::First))
        .unwrap();
    assert_eq!(dirty(&t), 0, "a move changes no text, so it notes nothing");
    t.tick();
    t.vault
        .transact(|txn| txn.update_object(a, |o| o.title = "Renamed".into()))
        .unwrap();
    assert_eq!(dirty(&t), 1);
    t.assert_verifies();
    assert_eq!(dirty(&t), 0, "a check of the vault drains it too");
}

#[test]
fn a_summary_and_code_and_image_text_are_in_the_body() {
    let mut t = new_vault();
    t.tick();
    t.vault
        .transact(|txn| {
            let id = txn.create_object(NewObject {
                title: "Snippets".into(),
                summary: Some("A kestrel of a summary".into()),
                props: Props::new(),
                ..Default::default()
            })?;
            txn.create_block(NewBlock {
                id: None,
                object_id: id,
                parent_block_id: None,
                position: Position::Last,
                kind: "code".into(),
                content: json!({"code": "let albatross = 1;", "language": "rust"}),
            })?;
            Ok(id)
        })
        .unwrap();
    assert_eq!(titles(&find(&t, "kestrel")), ["Snippets"]);
    assert_eq!(titles(&find(&t, "albatross")), ["Snippets"]);
}

#[test]
fn a_damaged_index_is_found_by_verify_and_rebuilt() {
    let mut t = new_vault();
    create(&mut t, "Kept", Some("osprey"));
    let lost = create(&mut t, "Lost", Some("cormorant"));
    t.assert_verifies();

    t.raw()
        .execute(
            "DELETE FROM search_text WHERE object_id = ?1",
            [lost.to_string()],
        )
        .unwrap();
    let report = t.vault.verify(false).unwrap();
    assert_eq!(
        report.problems,
        [format!("search: live object {lost} is not indexed")]
    );
    assert!(find(&t, "cormorant").is_empty());

    t.raw()
        .execute(
            "UPDATE search_text SET body = 'stale' WHERE object_id = (SELECT id FROM objects WHERE title = 'Kept')",
            [],
        )
        .unwrap();
    assert!(
        t.vault.verify(false).unwrap().problems.len() == 1,
        "a shallow check reads no text"
    );
    let deep = t.vault.verify(true).unwrap();
    assert_eq!(deep.problems.len(), 2, "{:?}", deep.problems);
    assert!(deep.problems[1].contains("old text"));

    t.vault.rebuild_search_index().unwrap();
    t.assert_verifies();
    assert_eq!(titles(&find(&t, "cormorant")), ["Lost"]);
    assert_eq!(titles(&find(&t, "osprey")), ["Kept"]);
}

#[test]
fn a_vault_from_before_the_index_gets_one_when_it_opens() {
    let mut t = new_vault();
    create(&mut t, "Old page", Some("A gannet from migration 1."));
    // Put the file back to migration 1: no views, no index.
    let raw = t.raw();
    raw.execute_batch(
        "DROP TRIGGER search_text_insert; DROP TRIGGER search_text_delete; DROP TRIGGER search_text_update; \
         DROP TABLE fts; DROP TABLE search_text; DROP TABLE search_dirty; DROP TABLE views; \
         DROP TABLE mirror_state; DROP TABLE mirror_dirty; DROP TABLE mirror_meta; DROP TABLE conflicts; \
         DROP TABLE provenance; PRAGMA user_version = 1",
    )
    .unwrap();
    drop(raw);

    let t = t.reopen();
    assert_eq!(
        t.vault.verify(false).unwrap().schema_version,
        dbme_store::SCHEMA_VERSION
    );
    assert_eq!(titles(&find(&t, "gannet")), ["Old page"]);
    t.assert_verifies();
}
