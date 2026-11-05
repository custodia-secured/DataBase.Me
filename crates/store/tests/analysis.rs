// SPDX-License-Identifier: AGPL-3.0-or-later
//! The reads behind the visual views (M8): `$geo_*` and event columns in the DSL, `related_to` with
//! depth, aggregates, profiles and the graph.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use dbme_core::Id;
use dbme_core::model::{FieldKind, Props};
use dbme_core::query::{
    AggregateQuery, Compare, Condition, Direction, From, Group, Measure, MeasureOp, Query,
    RelatedTo, Sort, SortBy, TimeBin,
};
use dbme_store::{NewField, NewObject, NewRelation, NewType, StoreError};
use serde_json::{Value, json};

use common::{TestVault, new_vault};

fn props(pairs: &[(&str, Value)]) -> Props {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

struct World {
    t: TestVault,
    task: Id,
    one: Id,
    two: Id,
    three: Id,
    four: Id,
    five: Id,
}

/// Five tasks under one parent, with statuses, amounts, tags, dates and places.
fn world() -> World {
    let mut t = new_vault();
    t.tick();
    let ((task, one, two, three, four, five), _) = t
        .vault
        .transact(|txn| {
            let task = txn.create_type(NewType {
                name: "Project task".into(),
                ..Default::default()
            })?;
            for (key, kind) in [
                ("status", FieldKind::Text),
                ("amount", FieldKind::Json),
                ("tags", FieldKind::Json),
                ("due", FieldKind::Date),
            ] {
                txn.create_field(NewField {
                    type_id: task,
                    key: key.into(),
                    label: key.into(),
                    kind,
                    options: json!({}),
                    required: false,
                })?;
            }
            let home = txn.create_object(NewObject {
                title: "Home".into(),
                ..Default::default()
            })?;
            let mut make = |title: &str, values: &[(&str, Value)]| {
                txn.create_object(NewObject {
                    title: title.into(),
                    parent_id: Some(home),
                    type_ids: vec![task],
                    props: props(values),
                    ..Default::default()
                })
            };
            let one = make(
                "One",
                &[
                    ("status", json!("todo")),
                    ("amount", json!(10)),
                    ("tags", json!(["red", "blue"])),
                    ("due", json!("2026-01-15")),
                ],
            )?;
            let two = make(
                "Two",
                &[
                    ("status", json!("todo")),
                    ("amount", json!(20.5)),
                    ("tags", json!(["red"])),
                    ("due", json!("2026-02-20")),
                ],
            )?;
            let three = make(
                "Three",
                &[
                    ("status", json!("done")),
                    ("amount", json!("n/a")),
                    ("due", json!("2026-04-01")),
                ],
            )?;
            let four = make("Four", &[("status", json!("done")), ("amount", json!(5))])?;
            let five = make("Five", &[])?;
            Ok((task, one, two, three, four, five))
        })
        .unwrap();
    World {
        t,
        task,
        one,
        two,
        three,
        four,
        five,
    }
}

fn of_tasks(world: &World) -> Query {
    Query {
        from: From {
            types: vec![world.task],
            under: None,
        },
        ..Query::default()
    }
}

fn group(field: &str) -> Group {
    Group {
        field: field.into(),
        bin: None,
    }
}

fn count() -> Measure {
    Measure {
        op: MeasureOp::Count,
        field: None,
    }
}

fn measure(op: MeasureOp, field: &str) -> Measure {
    Measure {
        op,
        field: Some(field.into()),
    }
}

fn buckets(
    world: &World,
    group_by: Vec<Group>,
    measures: Vec<Measure>,
) -> Vec<(Vec<Value>, Vec<Option<f64>>)> {
    let result = world
        .t
        .vault
        .read()
        .aggregate(&AggregateQuery {
            query: of_tasks(world),
            group_by,
            measures,
            limit: 0,
        })
        .unwrap();
    assert!(!result.truncated);
    result
        .rows
        .into_iter()
        .map(|row| (row.keys, row.values))
        .collect()
}

#[test]
fn an_aggregate_counts_each_value_and_keeps_no_value_as_a_bucket() {
    let world = world();
    assert_eq!(
        buckets(&world, vec![group("status")], vec![count()]),
        vec![
            (vec![Value::Null], vec![Some(1.0)]),
            (vec![json!("done")], vec![Some(2.0)]),
            (vec![json!("todo")], vec![Some(2.0)]),
        ]
    );
}

#[test]
fn an_aggregate_counts_a_list_once_per_item() {
    let world = world();
    assert_eq!(
        buckets(&world, vec![group("tags")], vec![count()]),
        vec![
            (vec![Value::Null], vec![Some(3.0)]),
            (vec![json!("blue")], vec![Some(1.0)]),
            (vec![json!("red")], vec![Some(2.0)]),
        ]
    );
}

#[test]
fn measures_read_numbers_and_nothing_else() {
    let world = world();
    let rows = buckets(
        &world,
        Vec::new(),
        vec![
            count(),
            measure(MeasureOp::Sum, "amount"),
            measure(MeasureOp::Min, "amount"),
            measure(MeasureOp::Max, "amount"),
        ],
    );
    // "n/a" would count as 0 if it were read, and would then be the minimum.
    assert_eq!(
        rows,
        vec![(
            Vec::new(),
            vec![Some(5.0), Some(35.5), Some(5.0), Some(20.5)]
        )]
    );
}

#[test]
fn a_mean_leaves_out_what_has_no_number() {
    let world = world();
    let rows = buckets(
        &world,
        vec![group("status")],
        vec![measure(MeasureOp::Avg, "amount")],
    );
    assert_eq!(rows[0].1, vec![None]);
    assert_eq!(rows[1].1, vec![Some(5.0)]);
    assert_eq!(rows[2].1, vec![Some(15.25)]);
}

#[test]
fn two_groups_make_one_bucket_per_pair() {
    let world = world();
    let rows = buckets(&world, vec![group("status"), group("tags")], vec![count()]);
    assert!(rows.contains(&(vec![json!("todo"), json!("red")], vec![Some(2.0)])));
    assert!(rows.contains(&(vec![json!("todo"), json!("blue")], vec![Some(1.0)])));
    assert!(rows.contains(&(vec![json!("done"), Value::Null], vec![Some(2.0)])));
}

#[test]
fn dates_in_text_fall_into_their_periods() {
    let world = world();
    let binned = |bin| {
        buckets(
            &world,
            vec![Group {
                field: "due".into(),
                bin: Some(bin),
            }],
            vec![count()],
        )
        .into_iter()
        .map(|(keys, _)| keys[0].clone())
        .collect::<Vec<_>>()
    };
    let none = Value::Null;
    assert_eq!(binned(TimeBin::Year), vec![none.clone(), json!("2026")]);
    assert_eq!(
        binned(TimeBin::Quarter),
        vec![none.clone(), json!("2026-Q1"), json!("2026-Q2")]
    );
    assert_eq!(
        binned(TimeBin::Month),
        vec![
            none.clone(),
            json!("2026-01"),
            json!("2026-02"),
            json!("2026-04")
        ]
    );
    // 2026-01-15 is a Thursday, 2026-02-20 a Friday, 2026-04-01 a Wednesday.
    assert_eq!(
        binned(TimeBin::Week),
        vec![
            none,
            json!("2026-01-12"),
            json!("2026-02-16"),
            json!("2026-03-30")
        ]
    );
}

#[test]
fn dates_in_columns_are_milliseconds() {
    let world = world();
    let result = world
        .t
        .vault
        .read()
        .aggregate(&AggregateQuery {
            query: Query::default(),
            group_by: vec![Group {
                field: "$created_at".into(),
                bin: Some(TimeBin::Year),
            }],
            measures: vec![count()],
            limit: 0,
        })
        .unwrap();
    let year = result.rows[0].keys[0].as_str().unwrap().to_owned();
    // The test clock starts near 2023; what matters is a real year, not 1970 or the far future.
    assert!(("2000".."2100").contains(&year.as_str()), "{year}");
}

#[test]
fn the_list_of_buckets_says_when_it_was_cut() {
    let world = world();
    let result = world
        .t
        .vault
        .read()
        .aggregate(&AggregateQuery {
            query: of_tasks(&world),
            group_by: vec![group("status")],
            measures: vec![count()],
            limit: 2,
        })
        .unwrap();
    assert_eq!(result.rows.len(), 2);
    assert!(result.truncated);
}

#[test]
fn an_aggregate_refuses_a_bad_shape() {
    let world = world();
    let read = world.t.vault.read();
    let bad = |group_by: Vec<Group>, measures: Vec<Measure>, limit| {
        read.aggregate(&AggregateQuery {
            query: Query::default(),
            group_by,
            measures,
            limit,
        })
        .unwrap_err()
    };
    assert!(matches!(bad(vec![], vec![], 0), StoreError::Core(_)));
    assert!(matches!(
        bad(vec![group("a"), group("b"), group("c")], vec![count()], 0),
        StoreError::Core(_)
    ));
    assert!(matches!(
        bad(vec![group("no such; key")], vec![count()], 0),
        StoreError::Core(_)
    ));
    assert!(matches!(
        bad(
            vec![],
            vec![Measure {
                op: MeasureOp::Sum,
                field: None
            }],
            0
        ),
        StoreError::Core(_)
    ));
    assert!(matches!(
        bad(vec![], vec![count()], 5001),
        StoreError::Core(_)
    ));
}

#[test]
fn the_dsl_reads_geo_and_sorts_by_event_columns() {
    let world = world();
    let mut t = world.t;
    t.tick();
    let (places, one, two) = (world.task, world.one, world.two);
    t.vault
        .transact(|txn| {
            txn.update_object(one, |row| {
                row.geo_lat = Some(47.5);
                row.geo_lon = Some(8.5);
                row.event_start = Some(2_000);
            })?;
            txn.update_object(two, |row| {
                row.geo_lat = Some(-33.9);
                row.geo_lon = Some(18.4);
                row.event_start = Some(1_000);
            })?;
            Ok(())
        })
        .unwrap();
    let located = Query {
        from: From {
            types: vec![places],
            under: None,
        },
        conditions: vec![Condition {
            field: "$geo_lat".into(),
            op: Compare::Gt,
            value: json!(0),
        }],
        ..Query::default()
    };
    let read = t.vault.read();
    let ids: Vec<Id> = read
        .query(&located)
        .unwrap()
        .into_iter()
        .map(|summary| summary.object.id)
        .collect();
    assert_eq!(ids, vec![one]);

    let by_event = Query {
        from: From {
            types: vec![places],
            under: None,
        },
        sort: Sort {
            by: SortBy::Field,
            field: Some("$event_start".into()),
            descending: false,
        },
        ..Query::default()
    };
    let ids: Vec<Id> = read
        .query(&by_event)
        .unwrap()
        .into_iter()
        .map(|summary| summary.object.id)
        .collect();
    // Earliest first, and no date last.
    assert_eq!(&ids[..2], &[two, one]);
}

#[test]
fn a_sort_cannot_name_a_column_with_its_own_order() {
    let world = world();
    let query = Query {
        sort: Sort {
            by: SortBy::Field,
            field: Some("$title".into()),
            descending: false,
        },
        ..Query::default()
    };
    assert!(world.t.vault.read().query(&query).is_err());
}

/// one -> two -> three -> four, and four -> one closes a cycle; five stands apart.
fn chain(world: World) -> World {
    let (one, two, three, four) = (world.one, world.two, world.three, world.four);
    let mut world = world;
    world.t.tick();
    world
        .t
        .vault
        .transact(|txn| {
            for (src, dst) in [(one, two), (two, three), (three, four), (four, one)] {
                txn.create_relation(NewRelation {
                    src_id: src,
                    dst_id: dst,
                    rel_type: "links".into(),
                    field_id: None,
                    src_block_id: None,
                    label: None,
                    props: Props::default(),
                })?;
            }
            Ok(())
        })
        .unwrap();
    world
}

fn reached(world: &World, id: Id, direction: Direction, depth: u8) -> Vec<Id> {
    let mut ids: Vec<Id> = world
        .t
        .vault
        .read()
        .query(&Query {
            related_to: Some(RelatedTo {
                id,
                rel_type: None,
                direction,
                depth,
            }),
            ..Query::default()
        })
        .unwrap()
        .into_iter()
        .map(|summary| summary.object.id)
        .collect();
    ids.sort();
    ids
}

fn sorted(mut ids: Vec<Id>) -> Vec<Id> {
    ids.sort();
    ids
}

#[test]
fn related_to_walks_as_far_as_its_depth_and_never_returns_the_start() {
    let world = chain(world());
    let (one, two, three, four) = (world.one, world.two, world.three, world.four);
    assert_eq!(reached(&world, one, Direction::Outgoing, 1), vec![two]);
    assert_eq!(
        reached(&world, one, Direction::Outgoing, 2),
        sorted(vec![two, three])
    );
    // The cycle comes back to `one` at step four: it is still not a result of itself.
    assert_eq!(
        reached(&world, one, Direction::Outgoing, 6),
        sorted(vec![two, three, four])
    );
    assert_eq!(
        reached(&world, one, Direction::Incoming, 2),
        sorted(vec![four, three])
    );
    assert_eq!(
        reached(&world, one, Direction::Either, 2),
        sorted(vec![two, three, four])
    );
    assert!(!reached(&world, one, Direction::Either, 6).contains(&world.five));
}

#[test]
fn related_to_refuses_a_depth_out_of_range() {
    let world = world();
    for depth in [0, 7] {
        let query = Query {
            related_to: Some(RelatedTo {
                id: world.one,
                rel_type: None,
                direction: Direction::Either,
                depth,
            }),
            ..Query::default()
        };
        assert!(world.t.vault.read().query(&query).is_err(), "depth {depth}");
    }
}

#[test]
fn a_walk_does_not_follow_a_deleted_relation_or_another_type() {
    let world = chain(world());
    let (one, two, three) = (world.one, world.two, world.three);
    let mut world = world;
    world.t.tick();
    let read = world.t.vault.read();
    let typed = |rel_type: &str| {
        read.query(&Query {
            related_to: Some(RelatedTo {
                id: one,
                rel_type: Some(rel_type.into()),
                direction: Direction::Outgoing,
                depth: 3,
            }),
            ..Query::default()
        })
        .unwrap()
        .len()
    };
    assert_eq!(typed("links"), 3);
    assert_eq!(typed("other"), 0);
    let relation = world
        .t
        .vault
        .read()
        .relations_from(two)
        .unwrap()
        .into_iter()
        .find(|relation| relation.dst_id == three)
        .unwrap();
    world
        .t
        .vault
        .transact(|txn| txn.delete_relation(relation.id))
        .unwrap();
    assert_eq!(reached(&world, one, Direction::Outgoing, 6), vec![two]);
}

#[test]
fn a_profile_reports_the_shape_of_the_results() {
    let world = world();
    let (one, two) = (world.one, world.two);
    let task = world.task;
    let mut t = world.t;
    t.tick();
    t.vault
        .transact(|txn| {
            txn.update_object(one, |row| {
                row.geo_lat = Some(47.5);
                row.geo_lon = Some(8.5);
            })?;
            txn.update_object(two, |row| row.event_start = Some(1_000))?;
            Ok(())
        })
        .unwrap();
    let query = Query {
        from: From {
            types: vec![task],
            under: None,
        },
        ..Query::default()
    };
    let keys = ["status", "amount", "tags", "due", "missing"].map(String::from);
    let profile = t.vault.read().data_profile(&query, &keys).unwrap();
    assert_eq!(profile.objects, 5);
    assert_eq!(profile.sampled, 5);
    assert_eq!(profile.with_geo, 1);
    assert_eq!(profile.with_event, 1);
    assert_eq!(profile.with_children, 0);
    assert_eq!(profile.with_relations, 0);

    let field = |key: &str| {
        profile
            .fields
            .iter()
            .find(|field| field.key == key)
            .unwrap()
    };
    assert_eq!(field("status").non_empty, 4);
    assert_eq!(field("status").distinct, 2);
    assert_eq!(field("status").numeric, 0);
    assert_eq!(field("amount").non_empty, 4);
    assert_eq!(field("amount").numeric, 3);
    assert_eq!(field("tags").lists, 2);
    assert_eq!(field("due").min, Some(json!("2026-01-15")));
    assert_eq!(field("due").max, Some(json!("2026-04-01")));
    assert_eq!(field("missing").non_empty, 0);
    assert_eq!(field("missing").min, None);
}

#[test]
fn a_profile_refuses_what_is_not_a_field_key() {
    let world = world();
    let keys = ["a; DROP TABLE objects".to_owned()];
    assert!(
        world
            .t
            .vault
            .read()
            .data_profile(&Query::default(), &keys)
            .is_err()
    );
}

#[test]
fn the_graph_has_relations_and_parents_among_its_nodes_only() {
    let world = chain(world());
    let (one, two, task) = (world.one, world.two, world.task);
    let read = world.t.vault.read();
    let everything = read.graph(&Query::default(), 1000).unwrap();
    assert!(!everything.truncated);
    // Five tasks and Home, four relations, five parent links.
    assert_eq!(everything.ids.len(), 6);
    assert_eq!(everything.titles.len(), 6);
    assert_eq!(everything.node_types.len(), 6);
    assert_eq!(everything.edges.len(), 8);
    assert_eq!(everything.edge_types.len(), 4);
    assert_eq!(everything.rel_types, vec!["links".to_owned()]);
    assert_eq!(everything.parent_edges.len(), 10);
    assert_eq!(everything.types, vec![task]);
    let at = |id: Id| {
        u32::try_from(everything.ids.iter().position(|node| *node == id).unwrap()).unwrap()
    };
    let pairs: Vec<(u32, u32)> = everything
        .edges
        .chunks(2)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    assert!(pairs.contains(&(at(one), at(two))));
    let home = everything
        .titles
        .iter()
        .position(|title| title == "Home")
        .unwrap();
    assert_eq!(everything.node_types[home], -1);

    let cut = read.graph(&Query::default(), 2).unwrap();
    assert!(cut.truncated);
    assert_eq!(cut.ids.len(), 2);
    for &index in cut.edges.iter().chain(&cut.parent_edges) {
        assert!((index as usize) < cut.ids.len());
    }
}

#[test]
fn the_graph_leaves_out_deleted_objects_and_their_relations() {
    let world = chain(world());
    let (one, two) = (world.one, world.two);
    let mut world = world;
    world.t.tick();
    world.t.vault.transact(|txn| txn.trash_object(two)).unwrap();
    let graph = world.t.vault.read().graph(&Query::default(), 1000).unwrap();
    assert!(!graph.ids.contains(&two));
    assert!(graph.ids.contains(&one));
    assert_eq!(
        graph.edges.len(),
        4,
        "two relations touched the deleted object"
    );
}
