// SPDX-License-Identifier: AGPL-3.0-or-later
//! The edges of the inbound direction (M5 Task 4): files that name what the vault lacks, anchors
//! that repeat, folders that would loop, events the watcher shapes oddly, and the hold API misused.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{World, new_world};
use dbme_core::Id;
use dbme_mirror::{FileEvent, Fs, HoldDecision, MirrorError};
use dbme_store::{ConflictKind, NewConflict};

fn rescan_in(w: &mut World) -> dbme_mirror::Inbound {
    let events = w.mirror.rescan(&w.vault).unwrap();
    w.mirror.pump_in(&mut w.vault, events).unwrap()
}

fn invalid_details(w: &World) -> Vec<serde_json::Value> {
    w.vault
        .conflicts(true)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == ConflictKind::InvalidValue)
        .flat_map(|c| c.details.as_array().cloned().unwrap_or_default())
        .collect()
}

#[test]
fn what_the_vault_lacks_is_reported_and_the_rest_applied() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let beta = w.object("Beta", None);
    let gone = w.object("Gone", None);
    w.vault.transact(|txn| txn.trash_object(gone)).unwrap();
    w.pump_out();
    let text = w.text("Alpha.md");
    let ghost = Id::from_parts(7, 7);
    let edited = text.replace("types: []", "types: [Ghost, Note]").replace(
        "sort:",
        &format!(
            "relations:\n  nofield:\n    - id: {beta}\nlinks:\n  - id: {gone}\n    type: related\n  - id: {ghost}\n    type: related\n  - id: {beta}\n    type: related\n    label: first\nsort:"
        ),
    );
    w.mirror.fs_mut().put("Alpha.md", edited.as_bytes());
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [alpha], "{report:?}");
    let details = invalid_details(&w);
    assert!(details.iter().any(|d| d["type"] == "Ghost"), "{details:?}");
    assert!(details.iter().any(|d| d["key"] == "nofield"), "{details:?}");
    assert!(
        details.iter().any(|d| d["target"] == gone.to_string()),
        "{details:?}"
    );
    assert!(
        details.iter().any(|d| d["target"] == ghost.to_string()),
        "{details:?}"
    );
    // The known type and the live link were applied.
    let note = w
        .vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Note")
        .unwrap();
    assert_eq!(w.vault.read().object_type_ids(alpha).unwrap(), [note.id]);
    let links = w.vault.read().relations_from(alpha).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(
        (links[0].dst_id, links[0].label.as_deref()),
        (beta, Some("first"))
    );

    // Changing the label edits the relation in place; dropping the link trashes it.
    w.pump_out();
    let text = w.text("Alpha.md");
    w.mirror.fs_mut().put(
        "Alpha.md",
        text.replace("label: first", "label: second").as_bytes(),
    );
    rescan_in(&mut w);
    let links = w.vault.read().relations_from(alpha).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].label.as_deref(), Some("second"));
    w.pump_out();
    let text = w.text("Alpha.md");
    let without: String = text
        .lines()
        .filter(|l| !l.starts_with("links:") && !l.starts_with("  - id:") && !l.starts_with("    "))
        .map(|l| format!("{l}\n"))
        .collect();
    w.mirror.fs_mut().put("Alpha.md", without.as_bytes());
    let report = rescan_in(&mut w);
    let left = w.vault.read().relations_from(alpha).unwrap();
    assert!(left.is_empty(), "{report:?}\n{without}\n{left:?}");
    w.vault.verify(true).unwrap();
}

#[test]
fn repeated_anchors_keep_the_first_and_a_loop_is_refused() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let child = w.object("Child", Some(alpha));
    let p = w.paragraph(alpha, "one");
    w.pump_out();
    let text = w.text("Alpha.md");
    let doubled = format!("{text}\nagain ^{p}\n");
    w.mirror.fs_mut().put("Alpha.md", doubled.as_bytes());
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [alpha]);
    let blocks = w.vault.read().blocks(alpha).unwrap();
    // Two blocks claimed one anchor; exactly one block carries it afterwards.
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].id, p);

    // Alpha's file moved under its own child's folder: the parent would loop, so it stays.
    for n in 0..20 {
        w.object(&format!("Filler {n}"), None);
    }
    w.pump_out();
    let text = w.text("Alpha.md");
    w.mirror.fs_mut().remove("Alpha.md").unwrap();
    w.mirror
        .fs_mut()
        .put("Alpha/Child/Alpha.md", text.as_bytes());
    let report = rescan_in(&mut w);
    assert!(report.trashed.is_empty(), "{report:?}");
    assert_eq!(w.vault.read().object(alpha).unwrap().parent_id, None);
    assert_eq!(w.vault.read().object(child).unwrap().parent_id, Some(alpha));
}

#[test]
fn oddly_shaped_events_are_handled_as_a_set() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.pump_out();
    w.mirror.fs_mut().put("New/One.md", b"one\n");
    w.mirror.fs_mut().put("New/Deeper/Two.md", b"two\n");
    w.mirror.fs_mut().put("Loose.md", b"loose\n");
    let report = w
        .mirror
        .pump_in(
            &mut w.vault,
            vec![
                FileEvent::DirCreated("New".into()),
                FileEvent::Renamed {
                    from: "Never.md".into(),
                    to: "Loose.md".into(),
                },
                FileEvent::Removed("Alpha.md".into()),
                FileEvent::Modified("Missing.md".into()),
                FileEvent::Created(".obsidian/app.json".into()),
                FileEvent::Modified("New".into()),
            ],
        )
        .unwrap();
    assert_eq!(report.created.len(), 3, "{report:?}");
    assert!(
        report.trashed.is_empty(),
        "Alpha.md still exists, so it was not removed"
    );
    assert!(w.vault.read().object(alpha).unwrap().deleted_at.is_none());
    let one = w.vault.read().objects_titled(None, "One").unwrap();
    assert_eq!(one.len(), 1);
    let two = w.vault.read().objects_titled(None, "Two").unwrap();
    assert_eq!(
        two.len(),
        1,
        "a folder with no file beside it parents to its parent"
    );

    // A rebuild without a schema file still works, and a hold decision needs a hold.
    let report = w.mirror.rebuild(&mut w.vault).unwrap();
    assert!(report.held.is_none());
    let not_a_hold = w
        .vault
        .conflict_add(NewConflict {
            kind: Some(ConflictKind::MissingId),
            ..NewConflict::default()
        })
        .unwrap();
    assert!(matches!(
        w.mirror
            .hold_decide(&mut w.vault, not_a_hold, HoldDecision::Restore),
        Err(MirrorError::Store(_))
    ));
}
