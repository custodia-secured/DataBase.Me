// SPDX-License-Identifier: AGPL-3.0-or-later
//! Saved views (ADR-0020): written through the op log, so Undo covers them, and checked by the
//! same rules as every other row.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::query::{Query, SortBy};
use dbme_store::{NewObject, NewView, StoreError};
use serde_json::json;

use common::new_vault;

fn new_view(name: &str, owner: Option<Id>) -> NewView {
    NewView {
        name: name.into(),
        renderer: "list".into(),
        query: serde_json::from_value(json!({"search": "garden", "sort": {"by": "title"}}))
            .unwrap(),
        config: json!({"columns": ["title"]}),
        owner_object_id: owner,
    }
}

#[test]
fn views_are_saved_listed_edited_deleted_and_undone() {
    let mut t = new_vault();
    t.tick();
    let (home, _) = t
        .vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Home".into(),
                ..Default::default()
            })
        })
        .unwrap();

    t.tick();
    let (id, committed) = t
        .vault
        .transact(|txn| txn.create_view(new_view("Garden notes", Some(home))))
        .unwrap();
    assert_eq!(committed.unwrap().changed, [id]);
    let views = t.vault.read().views().unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "Garden notes");
    assert_eq!(views[0].owner_object_id, Some(home));
    assert_eq!(views[0].query_json.search.as_deref(), Some("garden"));
    assert_eq!(views[0].config_json, json!({"columns": ["title"]}));
    assert_eq!(t.vault.read().view(id).unwrap(), views[0]);

    t.tick();
    let (changed, _) = t
        .vault
        .transact(|txn| {
            txn.update_view(id, |v| {
                v.name = "All about gardens".into();
                v.query_json.sort.by = SortBy::Updated;
            })
        })
        .unwrap();
    assert!(changed);
    let view = t.vault.read().view(id).unwrap();
    assert_eq!(view.name, "All about gardens");
    assert_eq!(view.query_json.sort.by, SortBy::Updated);
    assert!(view.updated_at > view.created_at);
    assert!(
        !t.vault
            .transact(|txn| txn.update_view(id, |_| {}))
            .unwrap()
            .0
    );

    t.vault.undo().unwrap().unwrap();
    assert_eq!(t.vault.read().view(id).unwrap().name, "Garden notes");
    t.vault.redo().unwrap().unwrap();
    assert_eq!(t.vault.read().view(id).unwrap().name, "All about gardens");

    t.tick();
    t.vault.transact(|txn| txn.delete_view(id)).unwrap();
    assert!(t.vault.read().views().unwrap().is_empty());
    assert!(matches!(
        t.vault.read().view(id),
        Err(StoreError::NotFound { .. })
    ));
    t.vault.undo().unwrap().unwrap();
    assert_eq!(t.vault.read().views().unwrap().len(), 1);
    t.assert_verifies();

    let t = t.reopen();
    assert_eq!(t.vault.read().views().unwrap()[0].name, "All about gardens");
}

#[test]
fn a_view_keeps_the_rules_of_its_query_and_its_owner() {
    let mut t = new_vault();
    t.tick();
    let blank = t
        .vault
        .transact(|txn| txn.create_view(new_view("   ", None)));
    assert!(matches!(blank, Err(StoreError::Core(_))));

    let mut bad_query = new_view("Bad", None);
    bad_query.query = Query {
        limit: 0,
        ..Query::default()
    };
    assert!(matches!(
        t.vault.transact(|txn| txn.create_view(bad_query)),
        Err(StoreError::Core(_))
    ));

    let mut bad_renderer = new_view("Bad", None);
    bad_renderer.renderer = "Not An Id".into();
    assert!(
        t.vault
            .transact(|txn| txn.create_view(bad_renderer))
            .is_err()
    );

    let missing = Id::from_parts(7, 7);
    assert!(matches!(
        t.vault
            .transact(|txn| txn.create_view(new_view("Orphan", Some(missing)))),
        Err(StoreError::NotFound { .. })
    ));

    t.tick();
    let (id, _) = t
        .vault
        .transact(|txn| txn.create_view(new_view("Fine", None)))
        .unwrap();
    assert!(
        t.vault
            .transact(|txn| txn.update_view(id, |v| v.owner_object_id = Some(missing)))
            .is_err()
    );
    assert_eq!(t.vault.read().view(id).unwrap().owner_object_id, None);
    t.assert_verifies();
}

#[test]
fn views_are_listed_by_name_ignoring_case() {
    let mut t = new_vault();
    for name in ["beta", "Alpha", "gamma"] {
        t.tick();
        t.vault
            .transact(|txn| txn.create_view(new_view(name, None)))
            .unwrap();
    }
    let names: Vec<String> = t
        .vault
        .read()
        .views()
        .unwrap()
        .into_iter()
        .map(|v| v.name)
        .collect();
    assert_eq!(names, ["Alpha", "beta", "gamma"]);
}
