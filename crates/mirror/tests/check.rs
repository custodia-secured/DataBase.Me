// SPDX-License-Identifier: AGPL-3.0-or-later
//! `check` (M5 Task 5, Decision 23): the bookkeeping counted and read for inconsistencies, and a
//! real folder's drift counted apart from them.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fs;
use std::sync::Arc;

use common::new_world;
use dbme_core::Id;
use dbme_mirror::{Drift, Mirror, StdFs, check};
use dbme_store::{ConflictKind, MirrorFileState, NewConflict};

#[test]
fn healthy_bookkeeping_counts_its_rows_and_the_folders_drift() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let beta = w.object("Beta", Some(alpha));
    let gone = w.object("Gone", None);
    let folder = w.dir.path().join("files");
    let clock: Arc<dbme_core::ManualClock> = w.clock.clone();
    let mut real = Mirror::new(StdFs::new(&folder).unwrap(), clock.clone());
    real.pump_out(&mut w.vault).unwrap();
    w.vault.transact(|txn| txn.trash_object(gone)).unwrap();
    real.pump_out(&mut w.vault).unwrap();

    let report = check(&w.vault, Some(&folder), clock.clone()).unwrap();
    assert!(report.is_ok(), "{report:#?}");
    assert_eq!((report.rows, report.synced, report.trashed), (3, 2, 1));
    assert_eq!((report.dirty, report.open_conflicts), (0, 0));
    assert!(!report.full_rewrite_needed);
    assert_eq!(report.drift, Some(Drift::default()));

    // Changes made while nothing watched are drift, not problems.
    fs::write(folder.join("New.md"), "new\n").unwrap();
    let alpha_path = folder.join("Alpha.md");
    let mut text = fs::read_to_string(&alpha_path).unwrap();
    text.push_str("\nmore\n");
    fs::write(&alpha_path, text).unwrap();
    fs::remove_file(folder.join("Alpha").join("Beta.md")).unwrap();
    let report = check(&w.vault, Some(&folder), clock.clone()).unwrap();
    assert!(report.is_ok(), "{report:#?}");
    let drift = report.drift.unwrap();
    assert_eq!((drift.created, drift.modified, drift.removed), (1, 1, 1));

    // Without a folder there is no drift to count.
    let report = check(&w.vault, None, clock.clone()).unwrap();
    assert_eq!(report.drift, None);
    assert_eq!(w.vault.read().object(beta).unwrap().title, "Beta");
}

#[test]
fn inconsistent_rows_and_conflicts_are_problems() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let gone = w.object("Gone", None);
    w.pump_out();
    w.vault.transact(|txn| txn.trash_object(gone)).unwrap();
    w.pump_out();
    let clock: Arc<dbme_core::ManualClock> = w.clock.clone();

    // A row for an object that does not exist, a live object whose row is trashed, a trashed
    // object whose row is live, and an open conflict naming a missing object.
    let ghost = Id::from_parts(9, 9);
    let mut row = w.vault.mirror_state(alpha).unwrap().unwrap();
    row.object_id = ghost;
    row.rel_path = "Ghost.md".into();
    row.path_key = dbme_core::files::paths::path_key("Ghost.md");
    w.vault.mirror_state_set(&row).unwrap();
    let mut row = w.vault.mirror_state(alpha).unwrap().unwrap();
    row.state = MirrorFileState::Trashed;
    w.vault.mirror_state_set(&row).unwrap();
    let mut row = w.vault.mirror_state(gone).unwrap().unwrap();
    row.state = MirrorFileState::Synced;
    w.vault.mirror_state_set(&row).unwrap();
    w.vault
        .conflict_add(NewConflict {
            object_id: Some(ghost),
            kind: Some(ConflictKind::MissingId),
            ..NewConflict::default()
        })
        .unwrap();
    w.retitle(alpha, "Alpha 2");
    w.mirror.request_full_rewrite(&mut w.vault).unwrap();

    let report = check(&w.vault, None, clock).unwrap();
    assert_eq!(report.problems.len(), 4, "{:#?}", report.problems);
    let text = report.problems.join("\n");
    assert!(text.contains(&format!("names object {ghost}, which does not exist")));
    assert!(text.contains("is live but its file row is trashed"));
    assert!(text.contains("is in the Trash but its file row is synced"));
    assert!(text.contains("open conflict"));
    assert!(report.dirty >= 1 && report.full_rewrite_needed);
    assert_eq!(report.open_conflicts, 1);
    assert_eq!(report.rows, 3);
}
