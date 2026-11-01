// SPDX-License-Identifier: AGPL-3.0-or-later
//! The M1 performance budget: creating, editing, deleting and undoing 10,000 objects takes under
//! 2 seconds in a release build.
//!
//! Each phase is one bulk action, one transaction of 10,000 changes, the way the app applies an
//! import or a multi-selection (M1 plan, Risks). A second, informational measurement makes every
//! change its own transaction; its cost is dominated by SQLite committing to disk, and it is
//! printed rather than gated (ADR-0015).
//!
//! Run the gate with `cargo test --release -p dbme-store --test budget -- --nocapture`, and the
//! measurement with `-- --ignored --nocapture`. Run them separately: together they slow each other
//! down. Debug builds skip the gate.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::time::{Duration, Instant};

use dbme_core::Id;
use dbme_store::NewObject;

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

#[test]
#[cfg_attr(debug_assertions, ignore = "the budget applies to release builds")]
fn ten_thousand_objects_are_created_edited_deleted_and_undone_within_budget() {
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

    let total = total(&phases);
    eprintln!("bulk, {OBJECTS} objects: {phases:?}, total {total:?} (budget {BUDGET:?})");
    assert!(
        total < BUDGET,
        "took {total:?}, over the {BUDGET:?} budget: {phases:?}"
    );
    t.assert_verifies();
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
                    for k in 0..19 {
                        txn.create_object(child(Some(b), format!("{i}.{j}.{k}")))?;
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
    }
    eprintln!("tree, 2,011 objects: best of five {best:?} (budget {TREE_BUDGET:?})");
    assert!(
        best < TREE_BUDGET,
        "took {best:?}, over the {TREE_BUDGET:?} budget"
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
