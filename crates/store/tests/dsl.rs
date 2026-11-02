// SPDX-License-Identifier: AGPL-3.0-or-later
//! The query DSL run against a vault (ADR-0020): where it looks, what must hold, search, relations,
//! order, paging, counts and facets.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::model::{FieldKind, Props};
use dbme_core::query::Query;
use dbme_store::{
    NewBlock, NewField, NewObject, NewRelation, NewType, ObjectSummary, Position, StoreError,
};
use serde_json::{Value, json};

use common::{TestVault, new_vault};

struct World {
    t: TestVault,
    task: Id,
    chore: Id,
    note: Id,
    home: Id,
    /// Home's children: Plan (task), Shopping (chore), Idle (task, no status), Memo (note).
    plan: Id,
    shopping: Id,
    idle: Id,
    memo: Id,
    /// Outside Home.
    loose: Id,
}

fn props(pairs: &[(&str, Value)]) -> Props {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn paragraph(text: &str) -> Value {
    json!({"inline": [{"text": text}]})
}

fn world() -> World {
    let mut t = new_vault();
    t.tick();
    let (ids, _) = t
        .vault
        .transact(|txn| {
            let task = txn.create_type(NewType {
                name: "Task".into(),
                ..Default::default()
            })?;
            let chore = txn.create_type(NewType {
                name: "Chore".into(),
                extends: Some(task),
                ..Default::default()
            })?;
            let note = txn.create_type(NewType {
                name: "Memo type".into(),
                ..Default::default()
            })?;
            txn.create_field(NewField {
                type_id: task,
                key: "status".into(),
                label: "Status".into(),
                kind: FieldKind::Select,
                options: json!({"options": [{"id": "todo"}, {"id": "doing"}, {"id": "done"}]}),
                required: false,
            })?;
            txn.create_field(NewField {
                type_id: task,
                key: "tags".into(),
                label: "Tags".into(),
                kind: FieldKind::MultiSelect,
                options: json!({"options": [{"id": "urgent"}, {"id": "home"}, {"id": "work"}]}),
                required: false,
            })?;
            txn.create_field(NewField {
                type_id: task,
                key: "estimate".into(),
                label: "Estimate".into(),
                kind: FieldKind::Number,
                options: json!({}),
                required: false,
            })?;
            let home = txn.create_object(NewObject {
                title: "Home".into(),
                ..Default::default()
            })?;
            let plan = txn.create_object(NewObject {
                title: "Plan the garden".into(),
                parent_id: Some(home),
                type_ids: vec![task],
                props: props(&[
                    ("status", json!("doing")),
                    ("tags", json!(["home", "urgent"])),
                    ("estimate", json!(3)),
                ]),
                ..Default::default()
            })?;
            let shopping = txn.create_object(NewObject {
                title: "Shopping".into(),
                parent_id: Some(home),
                type_ids: vec![chore],
                props: props(&[
                    ("status", json!("todo")),
                    ("tags", json!(["home"])),
                    ("estimate", json!(1)),
                ]),
                ..Default::default()
            })?;
            let idle = txn.create_object(NewObject {
                title: "Idle thought".into(),
                parent_id: Some(home),
                type_ids: vec![task],
                ..Default::default()
            })?;
            let memo = txn.create_object(NewObject {
                title: "Memo".into(),
                parent_id: Some(home),
                type_ids: vec![note],
                ..Default::default()
            })?;
            txn.create_block(NewBlock {
                id: None,
                object_id: memo,
                parent_block_id: None,
                position: Position::Last,
                kind: "paragraph".into(),
                content: paragraph("Remember to plan the garden beds."),
            })?;
            let loose = txn.create_object(NewObject {
                title: "Loose task".into(),
                type_ids: vec![task],
                props: props(&[("status", json!("done")), ("estimate", json!(8))]),
                ..Default::default()
            })?;
            txn.create_relation(NewRelation {
                src_id: plan,
                dst_id: shopping,
                rel_type: "related".into(),
                field_id: None,
                src_block_id: None,
                label: None,
                props: Props::new(),
            })?;
            txn.create_relation(NewRelation {
                src_id: loose,
                dst_id: plan,
                rel_type: "depends_on".into(),
                field_id: None,
                src_block_id: None,
                label: None,
                props: Props::new(),
            })?;
            Ok((task, chore, note, home, plan, shopping, idle, memo, loose))
        })
        .unwrap();
    let (task, chore, note, home, plan, shopping, idle, memo, loose) = ids;
    World {
        t,
        task,
        chore,
        note,
        home,
        plan,
        shopping,
        idle,
        memo,
        loose,
    }
}

fn q(json: Value) -> Query {
    serde_json::from_value(json).unwrap()
}

fn titles(results: &[ObjectSummary]) -> Vec<&str> {
    results.iter().map(|s| s.object.title.as_str()).collect()
}

impl World {
    fn run(&self, query: Value) -> Vec<&'static str> {
        let results = self.t.vault.read().query(&q(query)).unwrap();
        titles(&results)
            .into_iter()
            .map(|title| {
                [
                    "Home",
                    "Plan the garden",
                    "Shopping",
                    "Idle thought",
                    "Memo",
                    "Loose task",
                ]
                .into_iter()
                .find(|known| *known == title)
                .unwrap()
            })
            .collect()
    }

    fn count(&self, query: Value) -> u64 {
        self.t.vault.read().query_count(&q(query)).unwrap()
    }
}

#[test]
fn from_narrows_by_type_with_subtypes_and_by_place_in_the_hierarchy() {
    let w = world();
    let by_title = json!({"by": "title"});
    assert_eq!(
        w.run(json!({"sort": by_title})),
        [
            "Home",
            "Idle thought",
            "Loose task",
            "Memo",
            "Plan the garden",
            "Shopping"
        ]
    );
    assert_eq!(
        w.run(json!({"from": {"types": [w.task]}, "sort": by_title})),
        ["Idle thought", "Loose task", "Plan the garden", "Shopping"],
        "a Chore is a Task"
    );
    assert_eq!(
        w.run(json!({"from": {"types": [w.chore]}, "sort": by_title})),
        ["Shopping"]
    );
    assert_eq!(
        w.run(json!({"from": {"types": [w.chore, w.note]}, "sort": by_title})),
        ["Memo", "Shopping"]
    );
    assert_eq!(
        w.run(json!({"from": {"under": w.home}, "sort": by_title})),
        ["Idle thought", "Memo", "Plan the garden", "Shopping"],
        "not Home itself"
    );
    assert_eq!(
        w.run(json!({"from": {"types": [w.task], "under": w.home}, "sort": by_title})),
        ["Idle thought", "Plan the garden", "Shopping"]
    );
    assert_eq!(
        w.run(json!({"from": {"under": w.plan}})),
        Vec::<&str>::new()
    );
    let missing = Id::from_parts(9, 9);
    assert!(matches!(
        w.t.vault
            .read()
            .query(&q(json!({"from": {"types": [missing]}}))),
        Err(StoreError::NotFound { .. })
    ));
}

#[test]
fn conditions_compare_fields_and_columns() {
    let w = world();
    let by_title = json!({"by": "title"});
    let run = |conditions: Value| {
        w.run(json!({"from": {"types": [w.task]}, "where": conditions, "sort": by_title}))
    };
    assert_eq!(
        run(json!([{"field": "status", "op": "eq", "value": "doing"}])),
        ["Plan the garden"]
    );
    assert_eq!(
        run(json!([{"field": "status", "op": "ne", "value": "doing"}])),
        ["Idle thought", "Loose task", "Shopping"],
        "absent counts as not equal"
    );
    assert_eq!(
        run(json!([{"field": "estimate", "op": "lt", "value": 3}])),
        ["Shopping"],
        "absent never compares"
    );
    assert_eq!(
        run(json!([{"field": "estimate", "op": "ge", "value": 3}])),
        ["Loose task", "Plan the garden"]
    );
    assert_eq!(
        run(json!([{"field": "status", "op": "in", "value": ["todo", "done"]}])),
        ["Loose task", "Shopping"]
    );
    assert_eq!(
        run(json!([{"field": "tags", "op": "has", "value": "home"}])),
        ["Plan the garden", "Shopping"]
    );
    assert_eq!(
        run(json!([{"field": "tags", "op": "has", "value": "urgent"}])),
        ["Plan the garden"]
    );
    assert_eq!(
        run(json!([{"field": "status", "op": "is_empty"}])),
        ["Idle thought"]
    );
    assert_eq!(
        run(json!([{"field": "status", "op": "is_not_empty"}])),
        ["Loose task", "Plan the garden", "Shopping"]
    );
    assert_eq!(
        run(json!([{"field": "$title", "op": "contains", "value": "THE"}])),
        ["Plan the garden"]
    );
    assert_eq!(
        run(json!([{"field": "$title", "op": "starts_with", "value": "sho"}])),
        ["Shopping"]
    );
    assert_eq!(
        run(json!([
            {"field": "tags", "op": "has", "value": "home"},
            {"field": "status", "op": "eq", "value": "todo"}
        ])),
        ["Shopping"],
        "every condition must hold"
    );
    let created = w.t.vault.read().object(w.plan).unwrap().created_at;
    assert_eq!(
        run(json!([{"field": "$created_at", "op": "le", "value": created}])),
        ["Idle thought", "Loose task", "Plan the garden", "Shopping"]
    );
    assert!(matches!(
        w.t.vault.read().query(&q(
            json!({"where": [{"field": "$nope", "op": "eq", "value": 1}]})
        )),
        Err(StoreError::Core(_))
    ));
}

#[test]
fn search_ranks_titles_first_and_combines_with_everything_else() {
    let w = world();
    assert_eq!(
        w.run(json!({"search": "garden"})),
        ["Plan the garden", "Memo"]
    );
    assert_eq!(
        w.run(json!({"search": "garden", "from": {"types": [w.note]}})),
        ["Memo"]
    );
    assert_eq!(
        w.run(json!({"search": "garden", "where": [{"field": "status", "op": "eq", "value": "doing"}]})),
        ["Plan the garden"]
    );
    assert_eq!(
        w.run(json!({"search": "garden", "sort": {"by": "title", "descending": true}})),
        ["Plan the garden", "Memo"],
        "an explicit order wins over relevance"
    );
    assert_eq!(w.count(json!({"search": "garden"})), 2);
    assert_eq!(
        w.run(json!({"search": "   "})).len(),
        6,
        "blank search text is no search"
    );
}

#[test]
fn related_to_follows_live_relations_in_each_direction() {
    let mut w = world();
    let related = |w: &World, id: Id, extra: Value| {
        let mut related = json!({"id": id});
        for (k, v) in extra.as_object().unwrap() {
            related[k] = v.clone();
        }
        w.run(json!({"related_to": related, "sort": {"by": "title"}}))
    };
    assert_eq!(related(&w, w.plan, json!({})), ["Loose task", "Shopping"]);
    assert_eq!(
        related(&w, w.plan, json!({"direction": "outgoing"})),
        ["Shopping"]
    );
    assert_eq!(
        related(&w, w.plan, json!({"direction": "incoming"})),
        ["Loose task"]
    );
    assert_eq!(
        related(&w, w.plan, json!({"rel_type": "depends_on"})),
        ["Loose task"]
    );
    assert_eq!(related(&w, w.memo, json!({})), Vec::<&str>::new());

    w.t.tick();
    let relation = w.t.vault.read().relations_from(w.plan).unwrap()[0].id;
    w.t.vault
        .transact(|txn| txn.trash_relation(relation))
        .unwrap();
    assert_eq!(related(&w, w.plan, json!({})), ["Loose task"]);
}

#[test]
fn order_and_paging_follow_the_sort() {
    let mut w = world();
    let tasks = |w: &World, sort: Value, limit: u32, offset: u32| {
        w.run(json!({"from": {"types": [w.task]}, "sort": sort, "limit": limit, "offset": offset}))
    };
    assert_eq!(
        tasks(&w, json!({"by": "field", "field": "estimate"}), 10, 0),
        ["Shopping", "Plan the garden", "Loose task", "Idle thought"],
        "no value comes last"
    );
    assert_eq!(
        tasks(
            &w,
            json!({"by": "field", "field": "estimate", "descending": true}),
            10,
            0
        ),
        ["Loose task", "Plan the garden", "Shopping", "Idle thought"]
    );
    assert_eq!(
        tasks(&w, json!({"by": "title"}), 2, 1),
        ["Loose task", "Plan the garden"]
    );
    assert_eq!(w.count(json!({"from": {"types": [w.task]}, "limit": 1})), 4);

    w.t.tick();
    w.t.vault
        .transact(|txn| {
            txn.update_object(w.idle, |o| {
                o.props_json.insert("estimate".into(), json!(2));
            })
        })
        .unwrap();
    assert_eq!(
        w.run(json!({"from": {"types": [w.task]}, "limit": 1}))[0],
        "Idle thought",
        "relevance without words is the most recently changed"
    );
}

#[test]
fn facets_count_types_and_field_values() {
    let w = world();
    let facets = |query: Value, field: &str| {
        w.t.vault
            .read()
            .facets(&q(query), field)
            .unwrap()
            .into_iter()
            .map(|b| (b.value, b.count))
            .collect::<Vec<_>>()
    };
    let everything = json!({});
    let mut types = facets(everything.clone(), "$type");
    types.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
    });
    assert_eq!(
        types,
        [
            (json!(w.task.to_string()), 3),
            (json!(w.chore.to_string()), 1),
            (json!(w.note.to_string()), 1),
        ]
    );
    assert_eq!(
        facets(everything.clone(), "status"),
        [(json!("doing"), 1), (json!("done"), 1), (json!("todo"), 1)]
    );
    assert_eq!(
        facets(everything.clone(), "tags"),
        [(json!("home"), 2), (json!("urgent"), 1)],
        "each item of a list counts"
    );
    assert_eq!(
        facets(json!({"from": {"under": w.home}}), "estimate"),
        [(json!(1), 1), (json!(3), 1)]
    );
    assert_eq!(
        facets(json!({"search": "garden"}), "status"),
        [(json!("doing"), 1)],
        "facets are over the whole result, not one page"
    );
    assert!(w.t.vault.read().facets(&q(everything), "$title").is_err());
}

#[test]
fn trashed_objects_are_never_results() {
    let mut w = world();
    w.t.tick();
    w.t.vault.transact(|txn| txn.trash_object(w.home)).unwrap();
    assert_eq!(w.run(json!({})), ["Loose task"]);
    assert_eq!(w.count(json!({})), 1);
    assert_eq!(
        w.run(json!({"related_to": {"id": w.loose}})),
        Vec::<&str>::new(),
        "a trashed object is not related to anything"
    );
    let _ = w.shopping;
    w.t.assert_verifies();
}
