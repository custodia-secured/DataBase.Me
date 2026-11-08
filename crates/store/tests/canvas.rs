// SPDX-License-Identifier: AGPL-3.0-or-later
//! The canvas's rows (M8 Task 12, ADR-0035): one row an element, through the op log, so that a change is
//! one undo, the log replays and a page goes with what shows it. Also the saved views that belong to a
//! page, which used to stop it being deleted for good.
#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{TestVault, new_vault};
use dbme_core::Id;
use dbme_store::{CanvasItem, NewObject, NewView, StoreError};
use serde_json::json;

fn item(name: &str, x: f64) -> CanvasItem {
    CanvasItem {
        item_id: name.to_owned(),
        object_id: None,
        x,
        y: 2.0,
        w: 30.0,
        h: 40.0,
        z: format!("a{name}"),
        style_json: json!({ "type": "rectangle", "version": 1 }),
    }
}

fn page(t: &mut TestVault, title: &str) -> Id {
    t.vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: title.to_owned(),
                ..Default::default()
            })
        })
        .unwrap()
        .0
}

fn apply(t: &mut TestVault, owner: Id, upsert: Vec<CanvasItem>, remove: Vec<&str>) -> Option<Id> {
    let remove = remove.into_iter().map(str::to_owned).collect();
    t.vault
        .transact(|txn| txn.apply_to_canvas(owner, upsert, remove))
        .unwrap()
        .0
}

fn items(t: &TestVault, owner: Id) -> Vec<(String, f64)> {
    let read = t.vault.read();
    let Some(view) = read.canvas_view(owner).unwrap() else {
        return Vec::new();
    };
    read.canvas_items(view.id)
        .unwrap()
        .into_iter()
        .map(|row| (row.item_id, row.x))
        .collect()
}

#[test]
fn the_first_thing_put_on_a_canvas_makes_its_view_and_nothing_else_does() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    assert_eq!(apply(&mut t, owner, vec![], vec!["nothing"]), None);
    assert!(t.vault.read().canvas_view(owner).unwrap().is_none());

    let view = apply(&mut t, owner, vec![item("a", 1.0)], vec![]);
    assert!(view.is_some());
    assert_eq!(apply(&mut t, owner, vec![item("b", 2.0)], vec![]), view);
    let row = t.vault.read().canvas_view(owner).unwrap().unwrap();
    assert_eq!(row.renderer, "canvas");
    assert_eq!(row.owner_object_id, Some(owner));
    assert_eq!(items(&t, owner), [("a".into(), 1.0), ("b".into(), 2.0)]);
}

#[test]
fn elements_come_back_in_stack_order_with_everything_they_were_given() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    let mut top = item("first", 1.0);
    top.z = "a9".into();
    let mut bottom = item("second", 2.0);
    bottom.z = "a1".into();
    bottom.style_json = json!({ "type": "text", "text": "hi", "points": [[0, 0], [1.5, 2.5]] });
    let view = apply(&mut t, owner, vec![top, bottom], vec![]).unwrap();
    let rows = t.vault.read().canvas_items(view).unwrap();
    assert_eq!(rows[0].item_id, "second");
    assert_eq!(rows[1].item_id, "first");
    assert_eq!(
        rows[0].style_json,
        json!({ "type": "text", "text": "hi", "points": [[0, 0], [1.5, 2.5]] })
    );
}

#[test]
fn a_change_is_one_undo_however_many_elements_it_touches() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    apply(&mut t, owner, vec![item("a", 1.0), item("b", 2.0)], vec![]);
    apply(
        &mut t,
        owner,
        vec![item("a", 5.0), item("c", 3.0)],
        vec!["b"],
    );
    assert_eq!(items(&t, owner), [("a".into(), 5.0), ("c".into(), 3.0)]);

    t.vault.undo().unwrap();
    assert_eq!(items(&t, owner), [("a".into(), 1.0), ("b".into(), 2.0)]);
    t.vault.redo().unwrap();
    assert_eq!(items(&t, owner), [("a".into(), 5.0), ("c".into(), 3.0)]);
    t.vault.undo().unwrap();
    t.vault.undo().unwrap();
    assert!(items(&t, owner).is_empty());
}

#[test]
fn writing_what_is_already_there_writes_nothing() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    apply(&mut t, owner, vec![item("a", 1.0)], vec![]);
    let before = t.vault.verify(false).unwrap().ops;
    apply(&mut t, owner, vec![item("a", 1.0)], vec![]);
    assert_eq!(t.vault.verify(false).unwrap().ops, before);
}

#[test]
fn the_log_replays_to_the_same_canvas() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    apply(&mut t, owner, vec![item("a", 1.0), item("b", 2.0)], vec![]);
    apply(&mut t, owner, vec![item("a", 5.5)], vec!["b"]);
    t.vault.undo().unwrap();
    let report = t.vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn what_is_not_an_element_is_refused() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    for bad in [
        CanvasItem {
            item_id: String::new(),
            ..item("a", 1.0)
        },
        CanvasItem {
            item_id: "no/slashes".into(),
            ..item("a", 1.0)
        },
        CanvasItem {
            x: f64::NAN,
            ..item("a", 1.0)
        },
        CanvasItem {
            w: f64::INFINITY,
            ..item("a", 1.0)
        },
        CanvasItem {
            z: String::new(),
            ..item("a", 1.0)
        },
        CanvasItem {
            style_json: json!("a string"),
            ..item("a", 1.0)
        },
    ] {
        let result = t
            .vault
            .transact(|txn| txn.apply_to_canvas(owner, vec![bad.clone()], vec![]));
        assert!(result.is_err(), "{bad:?}");
    }
    assert!(items(&t, owner).is_empty());
}

#[test]
fn a_card_needs_a_live_object_and_stays_while_it_is_in_the_trash() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    let shown = page(&mut t, "Shown");
    let card = CanvasItem {
        object_id: Some(shown),
        ..item("card", 1.0)
    };
    apply(&mut t, owner, vec![card.clone()], vec![]);

    t.vault.transact(|txn| txn.trash_object(shown)).unwrap();
    assert_eq!(
        items(&t, owner).len(),
        1,
        "a restore brings it back to life"
    );
    // It can still be moved: only a new card has to show something live.
    apply(
        &mut t,
        owner,
        vec![CanvasItem {
            x: 9.0,
            ..card.clone()
        }],
        vec![],
    );
    let new_card = CanvasItem {
        item_id: "other".into(),
        ..card
    };
    assert!(matches!(
        t.vault
            .transact(|txn| txn.apply_to_canvas(owner, vec![new_card], vec![])),
        Err(StoreError::Rule(_))
    ));
}

#[test]
fn deleting_a_page_for_good_takes_its_cards_off_other_canvases() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    let shown = page(&mut t, "Shown");
    apply(
        &mut t,
        owner,
        vec![
            CanvasItem {
                object_id: Some(shown),
                ..item("card", 1.0)
            },
            item("shape", 2.0),
        ],
        vec![],
    );
    t.vault.transact(|txn| txn.trash_object(shown)).unwrap();
    t.vault.transact(|txn| txn.delete_object(shown)).unwrap();
    assert_eq!(items(&t, owner), [("shape".into(), 2.0)]);
    t.vault.undo().unwrap();
    assert_eq!(items(&t, owner).len(), 2, "and it all comes back together");
    assert!(t.vault.verify(true).unwrap().is_ok());
}

#[test]
fn deleting_a_page_for_good_takes_its_canvas_and_its_saved_views() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    apply(&mut t, owner, vec![item("a", 1.0)], vec![]);
    let saved = t
        .vault
        .transact(|txn| {
            txn.create_view(NewView {
                name: "Mine".into(),
                renderer: "table".into(),
                query: dbme_core::query::Query::default(),
                config: json!({}),
                owner_object_id: Some(owner),
            })
        })
        .unwrap()
        .0;
    t.vault.transact(|txn| txn.trash_object(owner)).unwrap();
    t.vault.transact(|txn| txn.delete_object(owner)).unwrap();
    assert!(t.vault.read().view(saved).is_err());
    assert!(t.vault.read().views().unwrap().is_empty());
    t.vault.undo().unwrap();
    assert_eq!(t.vault.read().views().unwrap().len(), 2);
    assert_eq!(items(&t, owner).len(), 1);
    assert!(t.vault.verify(true).unwrap().is_ok());
}

#[test]
fn deleting_a_canvas_view_takes_what_is_on_it() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    let view = apply(&mut t, owner, vec![item("a", 1.0)], vec![]).unwrap();
    t.vault.transact(|txn| txn.delete_view(view)).unwrap();
    assert!(items(&t, owner).is_empty());
    t.vault.undo().unwrap();
    assert_eq!(items(&t, owner).len(), 1);
}

#[test]
fn a_trashed_page_keeps_its_canvas_but_takes_no_changes() {
    let mut t = new_vault();
    let owner = page(&mut t, "Page");
    apply(&mut t, owner, vec![item("a", 1.0)], vec![]);
    t.vault.transact(|txn| txn.trash_object(owner)).unwrap();
    assert_eq!(items(&t, owner).len(), 1);
    assert!(
        t.vault
            .transact(|txn| txn.apply_to_canvas(owner, vec![item("b", 2.0)], vec![]))
            .is_err()
    );
}
