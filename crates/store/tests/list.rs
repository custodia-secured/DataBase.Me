// SPDX-License-Identifier: AGPL-3.0-or-later
//! The reads behind the views (ADR-0017): pages, counts, summaries, fields, title search,
//! backlinks and ancestors.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_store::{
    MAX_PAGE, NewAttachment, NewBlock, NewField, NewObject, NewRelation, NewType, ObjectSummary,
    Position, Sort, SortBy, Source, StoreError,
};
use serde_json::{Value, json};

use common::{TestVault, new_vault};

struct World {
    t: TestVault,
    task: Id,
    chore: Id,
    parent: Id,
    /// The children of `parent`, in the order they were created.
    kids: Vec<Id>,
}

fn props(pairs: &[(&str, Value)]) -> Props {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

/// A parent with five children: three Tasks (one of them through the subtype Chore) with `due`
/// and `estimate` fields, a Note, and an untyped object. Each is created a millisecond apart, in a
/// manual order different from creation order.
fn world() -> World {
    let mut t = new_vault();
    let note = t
        .vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|ty| ty.name == "Note")
        .unwrap()
        .id;
    t.tick();
    let ((task, chore), _) = t
        .vault
        .transact(|txn| {
            let task = txn.create_type(NewType {
                name: "Chore list".into(),
                ..Default::default()
            })?;
            let chore = txn.create_type(NewType {
                name: "Chore".into(),
                extends: Some(task),
                ..Default::default()
            })?;
            for (key, kind) in [("due", FieldKind::Date), ("estimate", FieldKind::Number)] {
                txn.create_field(NewField {
                    type_id: task,
                    key: key.into(),
                    label: key.into(),
                    kind,
                    options: json!({}),
                    required: false,
                })?;
            }
            // A second field with the key `due`, on the subtype: shown once.
            txn.create_field(NewField {
                type_id: chore,
                key: "due".into(),
                label: "Due (chore)".into(),
                kind: FieldKind::Date,
                options: json!({}),
                required: false,
            })?;
            Ok((task, chore))
        })
        .unwrap();
    t.tick();
    let (parent, _) = t
        .vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Parent".into(),
                ..Default::default()
            })
        })
        .unwrap();
    let specs: [(&str, Vec<Id>, Props, Position); 5] = [
        (
            "banana",
            vec![task],
            props(&[("due", json!("2026-03-01")), ("estimate", json!(3))]),
            Position::Last,
        ),
        (
            "Apple",
            vec![chore],
            props(&[("due", json!("2026-01-01"))]),
            Position::First,
        ),
        (
            "cherry",
            vec![task],
            props(&[("estimate", json!(1))]),
            Position::Last,
        ),
        ("100% done_now", vec![note], Props::new(), Position::First),
        ("Apple pie", vec![], Props::new(), Position::Last),
    ];
    let mut kids = Vec::new();
    for (title, type_ids, props, position) in specs {
        t.tick();
        let (id, _) = t
            .vault
            .transact(|txn| {
                txn.create_object(NewObject {
                    title: title.into(),
                    parent_id: Some(parent),
                    position,
                    type_ids,
                    props,
                    ..Default::default()
                })
            })
            .unwrap();
        kids.push(id);
    }
    World {
        t,
        task,
        chore,
        parent,
        kids,
    }
}

fn titles(summaries: &[ObjectSummary]) -> Vec<&str> {
    summaries.iter().map(|s| s.object.title.as_str()).collect()
}

fn sort(by: SortBy, descending: bool) -> Sort {
    Sort {
        by,
        field: None,
        descending,
    }
}

fn by_field(key: &str, descending: bool) -> Sort {
    Sort {
        by: SortBy::Field,
        field: Some(key.into()),
        descending,
    }
}

#[test]
fn pages_follow_the_chosen_order() {
    let w = world();
    let read = w.t.vault.read();
    let children = Source::Children {
        parent: Some(w.parent),
    };
    let all = |s: &Sort| titles(&read.page(children, s, 0, 100).unwrap()).join(" | ");
    assert_eq!(
        all(&sort(SortBy::Manual, false)),
        "100% done_now | Apple | banana | cherry | Apple pie"
    );
    assert_eq!(
        all(&sort(SortBy::Title, false)),
        "100% done_now | Apple | Apple pie | banana | cherry"
    );
    assert_eq!(
        all(&sort(SortBy::Created, true)),
        "Apple pie | 100% done_now | cherry | Apple | banana"
    );
    assert_eq!(
        all(&sort(SortBy::Updated, false)),
        "banana | Apple | cherry | 100% done_now | Apple pie"
    );
    // Missing values come last in both directions.
    assert_eq!(
        all(&by_field("due", false)),
        "Apple | banana | cherry | 100% done_now | Apple pie"
    );
    assert_eq!(
        all(&by_field("due", true)),
        "banana | Apple | Apple pie | 100% done_now | cherry"
    );
    assert_eq!(
        all(&by_field("estimate", false)),
        "cherry | banana | Apple | 100% done_now | Apple pie"
    );

    let manual = sort(SortBy::Manual, false);
    assert_eq!(
        titles(&read.page(children, &manual, 1, 2).unwrap()),
        ["Apple", "banana"]
    );
    assert!(read.page(children, &manual, 5, 2).unwrap().is_empty());
    assert!(matches!(
        read.page(children, &manual, 0, MAX_PAGE + 1),
        Err(StoreError::Rule(_))
    ));
    let unnamed = Sort {
        by: SortBy::Field,
        field: None,
        descending: false,
    };
    assert!(matches!(
        read.page(children, &unnamed, 0, 10),
        Err(StoreError::Core(_))
    ));
}

#[test]
fn a_type_lists_its_subtypes_objects_and_not_the_trash() {
    let mut w = world();
    let tasks = Source::Type { type_id: w.task };
    let chores = Source::Type { type_id: w.chore };
    let manual = sort(SortBy::Manual, false);
    let read = w.t.vault.read();
    assert_eq!(
        titles(&read.page(tasks, &manual, 0, 10).unwrap()),
        ["banana", "Apple", "cherry"],
        "creation order"
    );
    assert_eq!(read.count_in(tasks).unwrap(), 3);
    assert_eq!(
        titles(&read.page(chores, &manual, 0, 10).unwrap()),
        ["Apple"]
    );
    assert_eq!(read.count_in(chores).unwrap(), 1);
    assert_eq!(
        read.count_in(Source::Children {
            parent: Some(w.parent)
        })
        .unwrap(),
        5
    );
    assert_eq!(read.count_in(Source::Children { parent: None }).unwrap(), 1);
    let missing = Source::Type {
        type_id: Id::from_parts(1, 1),
    };
    assert!(matches!(
        read.count_in(missing),
        Err(StoreError::NotFound { .. })
    ));

    let banana = w.kids[0];
    w.t.tick();
    w.t.vault.transact(|txn| txn.trash_object(banana)).unwrap();
    let read = w.t.vault.read();
    assert_eq!(
        titles(&read.page(tasks, &manual, 0, 10).unwrap()),
        ["Apple", "cherry"]
    );
    assert_eq!(read.count_in(tasks).unwrap(), 2);
}

#[test]
fn summaries_carry_types_children_and_cover() {
    let mut w = world();
    let (banana, apple) = (w.kids[0], w.kids[1]);
    let png = w.t.vault.blobs().put(b"\x89PNG one").unwrap().hash;
    let jpeg = w.t.vault.blobs().put(b"\xff\xd8 two").unwrap().hash;
    let pdf = w.t.vault.blobs().put(b"%PDF-1.7").unwrap().hash;
    w.t.tick();
    w.t.vault
        .transact(|txn| {
            for (hash, role, mime) in [
                (&pdf, AttachmentRole::Original, "application/pdf"),
                (&png, AttachmentRole::Thumbnail, "image/png"),
                (&jpeg, AttachmentRole::Original, "image/jpeg"),
            ] {
                txn.attach(NewAttachment {
                    object_id: banana,
                    blob_hash: hash.clone(),
                    role,
                    filename: None,
                    mime: Some(mime.into()),
                })?;
            }
            txn.add_type(apple, w.task)?;
            for title in ["live", "trashed"] {
                txn.create_object(NewObject {
                    title: title.into(),
                    parent_id: Some(apple),
                    ..Default::default()
                })?;
            }
            Ok(())
        })
        .unwrap();
    let trashed = w.t.vault.read().children(Some(apple)).unwrap()[1].id;
    w.t.tick();
    w.t.vault.transact(|txn| txn.trash_object(trashed)).unwrap();

    let read = w.t.vault.read();
    let missing = Id::from_parts(1, 1);
    let summaries = read.summaries(&[apple, missing, banana]).unwrap();
    assert_eq!(titles(&summaries), ["Apple", "banana"]);
    let mut types = vec![w.task, w.chore];
    types.sort_unstable();
    assert_eq!(summaries[0].type_ids, types);
    assert_eq!(
        (summaries[0].child_count, summaries[0].cover.as_ref()),
        (1, None)
    );
    assert_eq!(
        (summaries[1].child_count, summaries[1].cover.as_ref()),
        (0, Some(&jpeg)),
        "only an original image is a cover"
    );
    let parent = &read.summaries(&[w.parent]).unwrap()[0];
    assert_eq!((parent.child_count, parent.type_ids.len()), (5, 0));
}

#[test]
fn fields_come_once_per_key_from_the_listed_types() {
    let w = world();
    let read = w.t.vault.read();
    let keys = |source| -> Vec<(String, String)> {
        read.fields_in(source)
            .unwrap()
            .into_iter()
            .map(|f| (f.key, f.label))
            .collect()
    };
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(k, l)| ((*k).to_owned(), (*l).to_owned()))
            .collect()
    };
    assert_eq!(
        keys(Source::Type { type_id: w.task }),
        pairs(&[("due", "due"), ("estimate", "estimate")])
    );
    assert_eq!(
        keys(Source::Children {
            parent: Some(w.parent)
        })
        .len(),
        2
    );
    assert!(keys(Source::Children { parent: None }).is_empty());
}

#[test]
fn title_search_puts_prefixes_first_and_escapes_wildcards() {
    let mut w = world();
    let read = w.t.vault.read();
    let search = |text: &str| titles(&read.search_titles(text, 10).unwrap()).join(" | ");
    assert_eq!(search("apple"), "Apple | Apple pie");
    assert_eq!(search("PIE"), "Apple pie");
    assert_eq!(search("an"), "banana");
    assert_eq!(search("%"), "100% done_now");
    assert_eq!(search("_"), "100% done_now");
    assert_eq!(
        search("n_w"),
        "",
        "an escaped _ matches only itself, not the o of now"
    );
    assert_eq!(
        search("  "),
        "Apple pie | 100% done_now | cherry | Apple | banana | Parent"
    );
    assert_eq!(read.search_titles("", 2).unwrap().len(), 2);

    let apple = w.kids[1];
    w.t.tick();
    w.t.vault.transact(|txn| txn.trash_object(apple)).unwrap();
    assert_eq!(
        titles(&w.t.vault.read().search_titles("apple", 10).unwrap()),
        ["Apple pie"]
    );
}

#[test]
fn backlinks_name_their_source_and_block() {
    let mut w = world();
    let (banana, cherry) = (w.kids[0], w.kids[2]);
    w.t.tick();
    let (block, _) =
        w.t.vault
            .transact(|txn| {
                let block = txn.create_block(NewBlock {
                    id: None,
                    object_id: cherry,
                    parent_block_id: None,
                    position: Position::Last,
                    kind: "paragraph".into(),
                    content: json!({"inline": [{"text": "see "}, {"mention": banana.to_string()}]}),
                })?;
                txn.create_relation(NewRelation {
                    src_id: w.parent,
                    dst_id: banana,
                    rel_type: "links_to".into(),
                    field_id: None,
                    src_block_id: None,
                    label: None,
                    props: Props::new(),
                })?;
                Ok(block)
            })
            .unwrap();
    let links = w.t.vault.read().backlinks(banana).unwrap();
    let shape: Vec<(&str, &str, Option<_>)> = links
        .iter()
        .map(|l| {
            (
                l.relation.rel_type.as_str(),
                l.source.title.as_str(),
                l.block.as_ref().map(|b| b.id),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("links_to", "Parent", None),
            ("mentions", "cherry", Some(block))
        ]
    );
    assert!(w.t.vault.read().backlinks(cherry).unwrap().is_empty());
}

#[test]
fn ancestors_run_from_the_top_down() {
    let mut w = world();
    let apple = w.kids[1];
    w.t.tick();
    let (deep, _) =
        w.t.vault
            .transact(|txn| {
                txn.create_object(NewObject {
                    title: "deep".into(),
                    parent_id: Some(apple),
                    ..Default::default()
                })
            })
            .unwrap();
    let read = w.t.vault.read();
    let path: Vec<String> = read
        .ancestors(deep)
        .unwrap()
        .into_iter()
        .map(|o| o.title)
        .collect();
    assert_eq!(path, ["Parent", "Apple"]);
    assert!(read.ancestors(w.parent).unwrap().is_empty());
    assert!(matches!(
        read.ancestors(Id::from_parts(1, 1)),
        Err(StoreError::NotFound { .. })
    ));
}
