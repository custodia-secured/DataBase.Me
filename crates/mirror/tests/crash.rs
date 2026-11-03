// SPDX-License-Identifier: AGPL-3.0-or-later
//! A crash mid-write (M5 Task 4, the engine's promise at `pump_out`): the file is on disk before
//! its row is updated, so a crash between the two leaves a row the next rescan resolves without
//! an op or a conflict; a write that never reached the disk is finished after the vault is closed
//! and opened again, since the dirty table is in the vault; and the temporary file a crashed
//! atomic write leaves behind is a dot file the folder ignores.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{World, identity, new_world};
use dbme_core::ContentHash;
use dbme_mirror::{FileEvent, Fs, Inbound, Mirror, Outbound};
use dbme_store::{MirrorFileState, Vault, VaultOptions};
use serde_json::json;

fn rescan_in(w: &mut World) -> Inbound {
    let events = w.mirror.rescan(&w.vault).unwrap();
    w.mirror.pump_in(&mut w.vault, events).unwrap()
}

#[test]
fn a_crash_between_the_file_and_its_row_is_resolved_by_the_next_rescan() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let paragraph = w.paragraph(alpha, "first");
    w.pump_out();
    let old_row = w.vault.mirror_state(alpha).unwrap().unwrap();

    w.tick();
    w.vault
        .transact(|txn| {
            txn.update_block(paragraph, |b| {
                b.content_json = json!({"inline": [{"text": "second"}]});
            })
        })
        .unwrap();
    assert_eq!(w.pump_out().written, [alpha]);
    let new_text = w.text("Alpha.md");
    // The crash: the file is new, the row is still the old one.
    w.vault.mirror_state_set(&old_row).unwrap();
    let ops = w.vault.verify(false).unwrap().ops;

    let events = w.mirror.rescan(&w.vault).unwrap();
    assert_eq!(events, [FileEvent::Modified("Alpha.md".into())]);
    let report = w.mirror.pump_in(&mut w.vault, events).unwrap();
    assert_eq!(report.applied, [alpha], "{report:?}");
    assert!(report.conflicts.is_empty(), "{report:?}");
    assert_eq!(
        w.vault.verify(false).unwrap().ops,
        ops,
        "the file said what the vault knew"
    );
    let row = w.vault.mirror_state(alpha).unwrap().unwrap();
    assert_eq!(row.last_hash, Some(ContentHash::of(new_text.as_bytes())));
    assert_eq!(row.state, MirrorFileState::Synced);
    assert_eq!(w.pump_out(), Outbound::default());
    assert_eq!(w.text("Alpha.md"), new_text);
    w.vault.verify(true).unwrap();
}

#[test]
fn a_write_that_never_reached_the_disk_is_finished_after_the_vault_is_opened_again() {
    let w = new_world();
    let World {
        dir,
        path,
        clock,
        mut vault,
        mut mirror,
    } = w;
    let (alpha, paragraph) = vault
        .transact(|txn| {
            let alpha = txn.create_object(dbme_store::NewObject {
                title: "Alpha".into(),
                ..Default::default()
            })?;
            let paragraph = txn.create_block(dbme_store::NewBlock {
                id: None,
                object_id: alpha,
                parent_block_id: None,
                position: dbme_store::Position::Last,
                kind: "paragraph".into(),
                content: json!({"inline": [{"text": "first"}]}),
            })?;
            Ok((alpha, paragraph))
        })
        .unwrap()
        .0;
    mirror.pump_out(&mut vault).unwrap();
    clock.advance(1);
    vault
        .transact(|txn| {
            txn.update_block(paragraph, |b| {
                b.content_json = json!({"inline": [{"text": "second"}]});
            })
        })
        .unwrap();
    mirror.fs_mut().fail_next_writes(1);
    let out = mirror.pump_out(&mut vault).unwrap();
    assert_eq!(out.pending, [alpha], "{out:?}");
    assert_eq!(vault.mirror_dirty_count().unwrap(), 1);

    // The process ends here. The folder is still there; the vault is opened again.
    let folder = mirror.fs().clone();
    drop(mirror);
    drop(vault);
    let mut vault = Vault::open(
        &path,
        VaultOptions {
            identity: identity(),
            clock: clock.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        vault.mirror_dirty_count().unwrap(),
        1,
        "the dirty mark survived"
    );
    let mut mirror = Mirror::new(folder, clock.clone());
    let out = mirror.pump_out(&mut vault).unwrap();
    assert_eq!(out.written, [alpha], "{out:?}");
    assert!(mirror.fs().text("Alpha.md").unwrap().contains("second"));
    assert_eq!(
        vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Synced
    );
    assert_eq!(vault.mirror_dirty_count().unwrap(), 0);
    vault.verify(true).unwrap();
    drop(dir);
}

#[test]
fn the_temporary_file_of_a_crashed_write_is_ignored() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.paragraph(alpha, "text");
    w.pump_out();
    let ops = w.vault.verify(false).unwrap().ops;

    // What `StdFs::write_atomic` leaves when the process dies between the temporary file and
    // the rename, and what another tool keeps under a dot folder.
    w.mirror.fs_mut().put(".Alpha.md.tmp-1234-0", b"half a fi");
    w.mirror.fs_mut().put(".obsidian/workspace.json", b"{}");
    assert!(w.mirror.rescan(&w.vault).unwrap().is_empty());
    let report = w
        .mirror
        .pump_in(
            &mut w.vault,
            vec![
                FileEvent::Created(".Alpha.md.tmp-1234-0".into()),
                FileEvent::Created(".obsidian/workspace.json".into()),
            ],
        )
        .unwrap();
    assert_eq!(report, Inbound::default(), "{report:?}");
    assert_eq!(rescan_in(&mut w), Inbound::default());
    assert_eq!(w.vault.verify(false).unwrap().ops, ops);
    assert_eq!(w.vault.read().object_count().unwrap(), 1);
    assert!(
        w.mirror
            .fs()
            .stat(".Alpha.md.tmp-1234-0")
            .unwrap()
            .is_some()
    );
}
