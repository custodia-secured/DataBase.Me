// SPDX-License-Identifier: AGPL-3.0-or-later
//! The M1 performance budget: creating, editing, deleting and undoing 10,000 objects takes under
//! 2 seconds in a release build.
//!
//! Each phase is one bulk action, one transaction of 10,000 changes, the way the app applies an
//! import or a multi-selection (M1 plan, Risks). A second, informational measurement makes every
//! change its own transaction; its cost is dominated by SQLite committing to disk, and it is
//! printed rather than gated (ADR-0015).
//!
//! Run the gate with `cargo test --release -p dbme-store --test budget -- --test-threads=1
//! --nocapture`, and the measurement with `-- --ignored --nocapture`. Run them separately, and the
//! gates one at a time: together they slow each other down (the search gate seeds 100,000
//! objects while the bulk gate runs). Debug builds skip the gate.
//!
//! M3 adds the mind map's tree read, and M4 search on 100,000 objects (ADR-0020).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::time::{Duration, Instant};

use dbme_core::Id;
use dbme_core::model::{FieldKind, Props};
use dbme_core::query::Query;
use dbme_store::{NewBlock, NewField, NewObject, NewRelation, NewType, Position};
use serde_json::json;

use common::{TestVault, new_vault};

const OBJECTS: usize = 10_000;
const BUDGET: Duration = Duration::from_secs(2);

fn new_object(n: usize) -> NewObject {
    NewObject {
        title: format!("Object {n}"),
        ..Default::default()
    }
}

fn timed(phases: &mut Vec<(&'static str, Duration)>, name: &'static str, work: impl FnOnce()) {
    let started = Instant::now();
    work();
    phases.push((name, started.elapsed()));
}

fn total(phases: &[(&str, Duration)]) -> Duration {
    phases.iter().map(|(_, d)| *d).sum()
}

/// How many times the bulk gate runs; the best total is the measurement. A shared CI runner
/// varies by a third between identical builds (2026-11-02: the same code took 1.68 s and 2.20 s
/// on two runs of `main`), so one run cannot gate a 2 s budget.
const ROUNDS: usize = 3;

#[test]
#[cfg_attr(debug_assertions, ignore = "the budget applies to release builds")]
fn ten_thousand_objects_are_created_edited_deleted_and_undone_within_budget() {
    let rounds: Vec<Vec<(&str, Duration)>> = (0..ROUNDS).map(|_| bulk_round()).collect();
    let best = rounds.iter().min_by_key(|phases| total(phases)).unwrap();
    let total = total(best);
    eprintln!(
        "bulk, {OBJECTS} objects, best of {ROUNDS}: {best:?}, total {total:?} (budget {BUDGET:?})"
    );
    for phases in &rounds {
        eprintln!("  round: {:?}", self::total(phases));
    }
    assert!(
        total < BUDGET,
        "took {total:?}, over the {BUDGET:?} budget: {best:?}"
    );
}

/// One run of the bulk flow on a fresh vault: the four timed phases.
fn bulk_round() -> Vec<(&'static str, Duration)> {
    let mut t = new_vault();
    let mut phases = Vec::new();
    let mut ids: Vec<Id> = Vec::new();
    timed(&mut phases, "create", || {
        t.tick();
        ids = t
            .vault
            .transact(|txn| {
                (0..OBJECTS)
                    .map(|n| txn.create_object(new_object(n)))
                    .collect()
            })
            .unwrap()
            .0;
    });
    timed(&mut phases, "edit", || {
        t.tick();
        t.vault
            .transact(|txn| {
                ids.iter().try_for_each(|&id| {
                    txn.update_object(id, |o| o.title.push_str(" (edited)"))
                        .map(drop)
                })
            })
            .unwrap();
    });
    timed(&mut phases, "delete", || {
        t.tick();
        t.vault
            .transact(|txn| {
                ids.iter().try_for_each(|&id| {
                    txn.trash_object(id)?;
                    txn.delete_object(id)
                })
            })
            .unwrap();
    });
    assert_eq!(t.vault.read().object_count().unwrap(), 0);
    timed(&mut phases, "undo all three", || {
        for _ in 0..3 {
            t.tick();
            t.vault.undo().unwrap().unwrap();
        }
    });
    assert_eq!(
        t.vault.read().object_count().unwrap(),
        0,
        "undoing the create removes them again"
    );
    t.assert_verifies();
    phases
}

/// M3's read for the mind map: a tree of 2,011 objects, within what an edit may spend (100 ms) and
/// leaving most of it for the layout and the drawing. The best of five, so that this measures the
/// query rather than whatever else the shared runner was doing at that moment.
#[test]
#[cfg_attr(debug_assertions, ignore = "the budget applies to release builds")]
fn a_two_thousand_node_tree_is_read_within_budget() {
    const TREE_BUDGET: Duration = Duration::from_millis(50);
    let mut t = new_vault();
    t.tick();
    let (root, _) = t
        .vault
        .transact(|txn| {
            let child = |parent, title: String| NewObject {
                title,
                parent_id: parent,
                ..Default::default()
            };
            let root = txn.create_object(child(None, "Root".into()))?;
            for i in 0..10 {
                let a = txn.create_object(child(Some(root), format!("{i}")))?;
                for j in 0..10 {
                    let b = txn.create_object(child(Some(a), format!("{i}.{j}")))?;
                    // Two relations in each of these 100 families, since nodes with none cost little.
                    let mut previous = None;
                    for k in 0..19 {
                        let leaf = txn.create_object(child(Some(b), format!("{i}.{j}.{k}")))?;
                        if let (Some(src_id), 1 | 2) = (previous, k) {
                            txn.create_relation(NewRelation {
                                src_id,
                                dst_id: leaf,
                                rel_type: "related".into(),
                                field_id: None,
                                src_block_id: None,
                                label: None,
                                props: Props::new(),
                            })?;
                        }
                        previous = Some(leaf);
                    }
                }
            }
            Ok(root)
        })
        .unwrap();
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let started = Instant::now();
        let tree = t.vault.read().tree(root, 5_000).unwrap();
        best = best.min(started.elapsed());
        assert_eq!(tree.nodes.len(), 2_011);
        assert_eq!(tree.relations.len(), 200);
    }
    eprintln!("tree, 2,011 objects, 200 relations: best of five {best:?} (budget {TREE_BUDGET:?})");
    assert!(
        best < TREE_BUDGET,
        "took {best:?}, over the {TREE_BUDGET:?} budget"
    );
}

/// M4's budget: a search over 100,000 objects answers within 50 ms (handoff brief §8), on the
/// titles, the bodies and the field values. Each measurement is the best of three, so that it
/// measures the query rather than the runner; the first run is printed too.
#[test]
#[cfg_attr(debug_assertions, ignore = "the budget applies to release builds")]
fn search_on_one_hundred_thousand_objects_is_within_budget() {
    const COUNT: usize = 100_000;
    const BATCH: usize = 10_000;
    const SEARCH_BUDGET: Duration = Duration::from_millis(50);
    const WORDS: [&str; 48] = [
        "garden", "budget", "meeting", "river", "lantern", "harvest", "copper", "window", "orbit",
        "pencil", "thunder", "harbour", "velvet", "ledger", "compass", "meadow", "quartz",
        "saddle", "beacon", "canvas", "ember", "fossil", "glacier", "hammock", "island", "jasmine",
        "kettle", "lagoon", "marble", "nectar", "oyster", "parcel", "quiver", "ribbon", "summit",
        "timber", "umbrella", "valley", "walnut", "yarrow", "zephyr", "anchor", "bramble",
        "cinder", "dune", "engine", "falcon", "granite",
    ];
    let mut t = new_vault();
    t.tick();
    let (task, _) = t
        .vault
        .transact(|txn| {
            let task = txn.create_type(NewType {
                name: "Task".into(),
                ..Default::default()
            })?;
            txn.create_field(NewField {
                type_id: task,
                key: "status".into(),
                label: "Status".into(),
                kind: FieldKind::Select,
                options: json!({"options": [{"id": "open"}, {"id": "done"}]}),
                required: false,
            })?;
            Ok(task)
        })
        .unwrap();

    // A fixed pseudo-random sequence, so every run seeds the same vault.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let seeded = Instant::now();
    for batch in 0..COUNT / BATCH {
        t.tick();
        t.vault
            .transact(|txn| {
                for i in 0..BATCH {
                    let n = batch * BATCH + i;
                    let word = |k: u64| WORDS[(k % WORDS.len() as u64) as usize];
                    let title = format!("{} {} {}", word(next()), word(next()), n);
                    let status = if next().is_multiple_of(3) {
                        "done"
                    } else {
                        "open"
                    };
                    let id = txn.create_object(NewObject {
                        title,
                        type_ids: vec![task],
                        props: [("status".to_owned(), json!(status))].into_iter().collect(),
                        ..Default::default()
                    })?;
                    if n.is_multiple_of(2) {
                        let text: Vec<&str> = (0..8).map(|_| word(next())).collect();
                        txn.create_block(NewBlock {
                            id: None,
                            object_id: id,
                            parent_block_id: None,
                            position: Position::Last,
                            kind: "paragraph".into(),
                            content: json!({"inline": [{"text": text.join(" ")}]}),
                        })?;
                    }
                }
                Ok(())
            })
            .unwrap();
    }
    t.tick();
    t.vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Zygomorphic flowers".into(),
                type_ids: vec![task],
                ..Default::default()
            })
        })
        .unwrap();
    eprintln!(
        "seeded {COUNT} objects, half with a paragraph, in {:?}",
        seeded.elapsed()
    );

    // The first search after a change brings the index up to date (ADR-0020): here, for every
    // object just seeded. Measured apart, so that the searches below measure searching.
    let reader = t.vault.read();
    let caught_up = Instant::now();
    reader.search("warm", 1).unwrap();
    eprintln!(
        "the index caught up with {COUNT} changed objects in {:?}",
        caught_up.elapsed()
    );
    let filtered: Query = serde_json::from_value(json!({
        "from": {"types": [task]},
        "where": [{"field": "status", "op": "eq", "value": "open"}],
        "search": "garden river",
        "limit": 20
    }))
    .unwrap();
    let everything = Query::default();
    let mut gated: Vec<(&str, Duration)> = Vec::new();
    let mut measure = |what: &'static str, gate: bool, run: &dyn Fn() -> usize| {
        let mut best = Duration::MAX;
        let mut first = Duration::ZERO;
        let mut results = 0;
        for attempt in 0..3 {
            let at = Instant::now();
            results = run();
            let took = at.elapsed();
            if attempt == 0 {
                first = took;
            }
            best = best.min(took);
        }
        eprintln!("{what}: best {best:?}, first {first:?}, {results} results");
        if gate {
            gated.push((what, best));
        }
    };
    measure("one common word", true, &|| {
        reader.search("garden", 20).unwrap().len()
    });
    measure("two words", true, &|| {
        reader.search("garden river", 20).unwrap().len()
    });
    measure("a prefix", true, &|| {
        reader.search("gar", 20).unwrap().len()
    });
    measure("a phrase", true, &|| {
        reader.search("\"garden river\"", 20).unwrap().len()
    });
    measure("a rare word", true, &|| {
        reader.search("zygomorphic", 20).unwrap().len()
    });
    measure("a field value", true, &|| {
        reader.search("done", 20).unwrap().len()
    });
    measure("search with a type and a field condition", true, &|| {
        reader.query(&filtered).unwrap().len()
    });
    measure("count of that query", true, &|| {
        usize::try_from(reader.query_count(&filtered).unwrap()).unwrap()
    });
    measure("facets of that query by status", true, &|| {
        reader.facets(&filtered, "status").unwrap().len()
    });
    measure("one page of everything, not gated", false, &|| {
        reader.query(&everything).unwrap().len()
    });
    measure("facets of everything by type, not gated", false, &|| {
        reader.facets(&everything, "$type").unwrap().len()
    });
    let (slowest, took) = gated.iter().max_by_key(|(_, took)| *took).unwrap();
    eprintln!("slowest gated search: {slowest} at {took:?} (budget {SEARCH_BUDGET:?})");
    assert!(
        *took < SEARCH_BUDGET,
        "{slowest} took {took:?}, over the {SEARCH_BUDGET:?} budget: {gated:?}"
    );
}

#[test]
#[ignore = "a measurement, not a gate: run with --include-ignored"]
fn one_transaction_per_change_measurement() {
    let mut t: TestVault = new_vault();
    let mut phases = Vec::new();
    let mut ids: Vec<Id> = Vec::with_capacity(OBJECTS);
    timed(&mut phases, "create", || {
        for n in 0..OBJECTS {
            t.tick();
            ids.push(
                t.vault
                    .transact(|txn| txn.create_object(new_object(n)))
                    .unwrap()
                    .0,
            );
        }
    });
    timed(&mut phases, "edit", || {
        for &id in &ids {
            t.tick();
            t.vault
                .transact(|txn| txn.update_object(id, |o| o.title.push_str(" (edited)")))
                .unwrap();
        }
    });
    timed(&mut phases, "delete", || {
        for &id in &ids {
            t.tick();
            t.vault
                .transact(|txn| {
                    txn.trash_object(id)?;
                    txn.delete_object(id)
                })
                .unwrap();
        }
    });
    timed(&mut phases, "undo", || {
        for _ in 0..OBJECTS {
            t.tick();
            t.vault.undo().unwrap().unwrap();
        }
    });
    let total = total(&phases);
    let commits = 4 * OBJECTS;
    eprintln!(
        "one transaction per change, {commits} commits: {phases:?}, total {total:?}, {:?} per commit",
        total / u32::try_from(commits).unwrap()
    );
    assert_eq!(t.vault.read().object_count().unwrap(), OBJECTS as u64);
}
