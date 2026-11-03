// SPDX-License-Identifier: AGPL-3.0-or-later
//! No echo loops (M5 acceptance, brief §8): the folder's own writes, renames and trashes come back
//! from the watcher as events the engine recognises as its own, making no op and no second write,
//! over a hundred cycles; and a "hostile editor" that reorders the frontmatter's keys and switches
//! the line endings, which a rescan cannot tell from an edit, makes no op either and at most one
//! normalising write, after which the folder is quiet. The counting file system is the witness:
//! every write through the seam is counted.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{World, new_world};
use dbme_mirror::{FileEvent, Inbound, Outbound};
use serde_json::json;

fn rescan_in(w: &mut World) -> Inbound {
    let events = w.mirror.rescan(&w.vault).unwrap();
    w.mirror.pump_in(&mut w.vault, events).unwrap()
}

fn ops(w: &World) -> u64 {
    w.vault.verify(false).unwrap().ops
}

#[test]
fn own_writes_renames_and_trashes_come_back_as_echoes_over_a_hundred_cycles() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let paragraph = w.paragraph(alpha, "v0");
    w.pump_out();

    for n in 1..=100 {
        w.tick();
        w.vault
            .transact(|txn| {
                txn.update_block(paragraph, |b| {
                    b.content_json = json!({"inline": [{"text": format!("v{n}")}]});
                })
            })
            .unwrap();
        let after_edit = ops(&w);
        let writes = w.mirror.fs().writes();
        let out = w.pump_out();
        assert_eq!(out.written, [alpha], "cycle {n}: {out:?}");
        assert_eq!(w.mirror.fs().writes(), writes + 1, "cycle {n}: one write");

        // The watcher reports the write; the engine knows the bytes as its own.
        let report = w
            .mirror
            .pump_in(&mut w.vault, vec![FileEvent::Modified("Alpha.md".into())])
            .unwrap();
        assert_eq!(report.echoes, ["Alpha.md"], "cycle {n}: {report:?}");
        assert!(report.applied.is_empty() && report.conflicts.is_empty());
        // A rescan has nothing to say, and nothing is written twice.
        assert_eq!(rescan_in(&mut w), Inbound::default(), "cycle {n}");
        assert_eq!(w.pump_out(), Outbound::default(), "cycle {n}");
        assert_eq!(
            w.mirror.fs().writes(),
            writes + 1,
            "cycle {n}: still one write"
        );
        assert_eq!(ops(&w), after_edit, "cycle {n}: the echo made no op");
    }

    // A rename: the watcher reports the pair; the new path holds the folder's own bytes.
    w.tick();
    w.retitle(alpha, "Alpha renamed");
    let after_edit = ops(&w);
    let out = w.pump_out();
    assert_eq!(
        out.renamed,
        [("Alpha.md".to_owned(), "Alpha renamed.md".to_owned())],
        "{out:?}"
    );
    let report = w
        .mirror
        .pump_in(
            &mut w.vault,
            vec![FileEvent::Renamed {
                from: "Alpha.md".into(),
                to: "Alpha renamed.md".into(),
            }],
        )
        .unwrap();
    assert_eq!(report.echoes, ["Alpha renamed.md"], "{report:?}");
    assert!(report.moved.is_empty() && report.trashed.is_empty() && report.created.is_empty());
    assert_eq!(rescan_in(&mut w), Inbound::default());
    assert_eq!(ops(&w), after_edit);

    // A trash: the file goes to `.dbme/trash/`; the watcher reports the removal and a creation
    // under a dot folder, and neither is an edit.
    w.tick();
    w.vault.transact(|txn| txn.trash_object(alpha)).unwrap();
    let after_trash = ops(&w);
    let out = w.pump_out();
    assert_eq!(out.trashed, [alpha], "{out:?}");
    let trash_path = w.vault.mirror_state(alpha).unwrap().unwrap().rel_path;
    assert!(trash_path.starts_with(".dbme/trash/"), "{trash_path}");
    let report = w
        .mirror
        .pump_in(
            &mut w.vault,
            vec![
                FileEvent::Removed("Alpha renamed.md".into()),
                FileEvent::Created(trash_path),
            ],
        )
        .unwrap();
    assert_eq!(report, Inbound::default(), "{report:?}");
    assert_eq!(rescan_in(&mut w), Inbound::default());
    assert_eq!(w.pump_out(), Outbound::default());
    assert_eq!(ops(&w), after_trash);
    w.vault.verify(true).unwrap();
}

/// The frontmatter's single-line keys in reverse order, and CRLF line endings throughout: the
/// same document, as another editor might save it.
fn hostile(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "---", "{text}");
    let end = lines[1..].iter().position(|l| *l == "---").unwrap() + 1;
    assert!(
        lines[1..end]
            .iter()
            .all(|l| !l.starts_with(' ') && !l.starts_with('-')),
        "the sample must have single-line keys only: {text}"
    );
    lines[1..end].reverse();
    let mut out: String = lines.iter().map(|l| format!("{l}\r\n")).collect();
    out.push_str("\r\n");
    out
}

#[test]
fn a_hostile_editor_that_reorders_keys_and_switches_line_endings_makes_no_op_and_no_loop() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.paragraph(alpha, "one");
    w.paragraph(alpha, "two");
    w.pump_out();
    let canonical = w.text("Alpha.md");
    let rewritten = hostile(&canonical);
    assert_ne!(rewritten, canonical);

    for cycle in 1..=10 {
        let before = ops(&w);
        let writes = w.mirror.fs().writes();
        w.mirror.fs_mut().put("Alpha.md", rewritten.as_bytes());
        let report = rescan_in(&mut w);
        assert!(report.conflicts.is_empty(), "cycle {cycle}: {report:?}");
        assert_eq!(
            ops(&w),
            before,
            "cycle {cycle}: the same document made an op"
        );
        // At most one write, the normalising one, and then quiet.
        let out = w.pump_out();
        assert!(out.written.len() <= 1, "cycle {cycle}: {out:?}");
        assert_eq!(rescan_in(&mut w), Inbound::default(), "cycle {cycle}");
        assert_eq!(w.pump_out(), Outbound::default(), "cycle {cycle}");
        assert!(
            w.mirror.fs().writes() <= writes + 1,
            "cycle {cycle}: a loop"
        );
        assert_eq!(ops(&w), before, "cycle {cycle}");
    }

    // The next write from the app is the canonical form again: LF, the keys in their order.
    w.tick();
    w.retitle(alpha, "Alpha");
    w.paragraph(alpha, "three");
    w.pump_out();
    let text = w.text("Alpha.md");
    assert!(!text.contains('\r'), "{text:?}");
    let canonical_keys: Vec<&str> = canonical
        .lines()
        .skip(1)
        .take_while(|l| *l != "---")
        .collect();
    let written_keys: Vec<&str> = text.lines().skip(1).take_while(|l| *l != "---").collect();
    let key_of = |l: &str| l.split(':').next().unwrap_or("").to_owned();
    assert_eq!(
        written_keys.iter().map(|l| key_of(l)).collect::<Vec<_>>(),
        canonical_keys.iter().map(|l| key_of(l)).collect::<Vec<_>>(),
        "{text}"
    );
    w.vault.verify(true).unwrap();
}
