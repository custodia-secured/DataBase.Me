// SPDX-License-Identifier: AGPL-3.0-or-later
//! File → app (M5 Task 4b): what `pump_in` makes of files that appear, change, move and vanish.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::{World, new_world};
use dbme_core::Id;
use dbme_mirror::{FileEvent, Fs, GUARD_COUNT, Inbound};
use dbme_store::{ConflictKind, MirrorFileState, NewField, NewType};
use serde_json::json;

/// Enough unrelated files that one removal stays under the guard's ten percent.
fn filler(w: &mut World, n: usize) {
    for i in 0..n {
        w.object(&format!("Filler {i}"), None);
    }
}

fn rescan_in(w: &mut World) -> Inbound {
    let events = w.mirror.rescan(&w.vault).unwrap();
    w.mirror.pump_in(&mut w.vault, events).unwrap()
}

fn body_text(w: &World, id: Id) -> Vec<String> {
    w.vault
        .read()
        .blocks(id)
        .unwrap()
        .iter()
        .map(|b| {
            b.content_json["inline"]
                .as_array()
                .map(|runs| {
                    runs.iter()
                        .map(|r| r["text"].as_str().unwrap_or("").to_owned())
                        .collect::<String>()
                })
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn a_new_file_becomes_an_object_and_its_echo_is_ignored() {
    let mut w = new_world();
    w.mirror.fs_mut().put(
        "Ideas/Garden plan.md",
        b"---\ntypes: [Note]\ntags: [home]\n---\n\nPlant **beans** #spring\n\n- first\n- second\n",
    );
    w.mirror.fs_mut().put("Ideas.md", b"The ideas folder\n");
    let report = rescan_in(&mut w);
    assert_eq!(report.created.len(), 2, "{report:?}");
    assert!(report.conflicts.is_empty(), "{report:?}");

    let ideas = w.vault.read().objects_titled(None, "Ideas").unwrap();
    assert_eq!(ideas.len(), 1);
    let plan = w
        .vault
        .read()
        .objects_titled(Some(ideas[0].id), "Garden plan")
        .unwrap();
    assert_eq!(plan.len(), 1, "the file's folder is its parent");
    let plan = &plan[0];
    let types = w.vault.read().object_type_ids(plan.id).unwrap();
    let note = w
        .vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Note")
        .unwrap();
    assert_eq!(types, [note.id]);
    assert_eq!(
        body_text(&w, plan.id),
        ["Plant beans #spring", "first", "second"]
    );
    let tags: Vec<String> = w
        .vault
        .read()
        .relations_from(plan.id)
        .unwrap()
        .iter()
        .filter(|r| r.rel_type == "tagged")
        .map(|r| w.vault.read().object(r.dst_id).unwrap().title)
        .collect();
    assert_eq!(tags, ["home", "spring"]);

    // The file had no id and no anchors: it is written back with them, once.
    let state = w.vault.mirror_state(plan.id).unwrap().unwrap();
    assert_eq!(state.rel_path, "Ideas/Garden plan.md");
    assert_eq!(state.state, MirrorFileState::Synced);
    // Two files, plus the two Tag objects the tags made.
    assert_eq!(w.vault.mirror_dirty_count().unwrap(), 4);
    let out = w.pump_out();
    assert_eq!(out.written.len(), 4, "{out:?}");
    let text = w.text("Ideas/Garden plan.md");
    assert!(text.contains(&format!("id: {}", plan.id)), "{text}");
    assert!(text.contains("- first ^"), "{text}");

    // The rewrite is the app's own echo: a rescan finds nothing to do, nothing changes.
    let ops_before = w.vault.verify(false).unwrap().ops;
    let report = rescan_in(&mut w);
    assert_eq!(report, Inbound::default());
    let events = vec![FileEvent::Modified("Ideas/Garden plan.md".into())];
    let report = w.mirror.pump_in(&mut w.vault, events).unwrap();
    assert_eq!(report.echoes, ["Ideas/Garden plan.md"]);
    assert_eq!(w.vault.verify(false).unwrap().ops, ops_before);
    // The ops carry the mirror's device.
    let last = &w.vault.read().history(1).unwrap()[0].entry;
    assert_eq!(last.device_id, w.vault.mirror_identity().device_id);
}

#[test]
fn an_edit_merges_into_the_object_and_the_app_wins_a_true_conflict() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let p1 = w.paragraph(alpha, "one");
    let p2 = w.paragraph(alpha, "two");
    w.pump_out();
    let text = w.text("Alpha.md");

    // The file edits the second paragraph, adds a third, and sets a summary.
    let edited = text
        .replace("two ^", "two, edited ^")
        .replace("sort:", "summary: from the file\nsort:")
        + "\nthree\n";
    w.mirror.fs_mut().put("Alpha.md", edited.as_bytes());
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [alpha], "{report:?}");
    assert!(report.conflicts.is_empty(), "{report:?}");
    assert_eq!(body_text(&w, alpha), ["one", "two, edited", "three"]);
    assert_eq!(
        w.vault.read().block(p2).unwrap().content_json,
        json!({"inline": [{"text": "two, edited"}]})
    );
    assert_eq!(
        w.vault.read().object(alpha).unwrap().summary.as_deref(),
        Some("from the file")
    );
    // The new paragraph got an anchor, so the file is written back once and then quiet.
    assert_eq!(w.pump_out().written, [alpha]);
    assert_eq!(rescan_in(&mut w), Inbound::default());

    // Both sides change the first paragraph: the app's stays, the file's is in the inbox.
    w.vault
        .transact(|txn| {
            txn.update_block(p1, |b| {
                b.content_json = json!({"inline": [{"text": "one (app)"}]})
            })
        })
        .unwrap();
    let text = w.text("Alpha.md");
    w.mirror
        .fs_mut()
        .put("Alpha.md", text.replace("one ^", "one (file) ^").as_bytes());
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [alpha]);
    assert_eq!(report.conflicts.len(), 1, "{report:?}");
    assert_eq!(body_text(&w, alpha)[0], "one (app)");
    let conflict = w.vault.conflict(report.conflicts[0]).unwrap();
    assert_eq!(conflict.kind, ConflictKind::EditEdit);
    assert_eq!(conflict.object_id, Some(alpha));
    let copy = conflict.conflict_copy_id.unwrap();
    let copy_row = w.vault.read().object(copy).unwrap();
    assert_eq!(copy_row.title, "Alpha (conflict copy)");
    assert_eq!(body_text(&w, copy)[0], "one (file)");
    assert_eq!(
        w.vault.read().attachments(copy).unwrap().len(),
        1,
        "the raw bytes"
    );
    // The app's version is written over the file; the copy gets its own file.
    let out = w.pump_out();
    assert!(
        out.written.contains(&alpha) && out.written.contains(&copy),
        "{out:?}"
    );
    assert!(w.text("Alpha.md").contains("one (app) ^"));
    assert!(w.files().contains(&"Alpha (conflict copy).md".to_owned()));
    w.vault.verify(true).unwrap();
}

#[test]
fn renames_and_moves_follow_the_path_and_the_id() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let beta = w.object("Beta", None);
    w.paragraph(alpha, "text");
    filler(&mut w, 20);
    w.pump_out();

    // A rename is a new title.
    w.mirror
        .fs_mut()
        .rename("Alpha.md", "Alpha renamed.md")
        .unwrap();
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
    assert_eq!(report.moved, [alpha], "{report:?}");
    assert!(report.trashed.is_empty() && report.created.is_empty());
    assert_eq!(w.vault.read().object(alpha).unwrap().title, "Alpha renamed");
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().rel_path,
        "Alpha renamed.md"
    );

    // A move into another file's folder is a reparent, seen as remove plus create.
    let text = w.text("Alpha renamed.md");
    w.mirror.fs_mut().remove("Alpha renamed.md").unwrap();
    w.mirror
        .fs_mut()
        .put("Beta/Alpha renamed.md", text.as_bytes());
    let report = rescan_in(&mut w);
    assert_eq!(report.moved, [alpha], "{report:?}");
    assert!(report.trashed.is_empty(), "{report:?}");
    assert_eq!(w.vault.read().object(alpha).unwrap().parent_id, Some(beta));

    // A file without an id that reappears elsewhere with the same bytes is the same object.
    let text = w.text("Beta/Alpha renamed.md");
    let stripped: String = text
        .lines()
        .filter(|l| !l.starts_with("id: "))
        .map(|l| format!("{l}\n"))
        .collect();
    w.mirror
        .fs_mut()
        .put("Beta/Alpha renamed.md", stripped.as_bytes());
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [alpha]);
    assert!(
        w.vault
            .conflicts(true)
            .unwrap()
            .iter()
            .any(|c| c.kind == ConflictKind::MissingId)
    );
    w.pump_out();
    let text = w.text("Beta/Alpha renamed.md");
    assert!(
        text.contains(&format!("id: {alpha}")),
        "the id is written back: {text}"
    );
    let anonymous: String = text
        .lines()
        .filter(|l| !l.starts_with("id: "))
        .map(|l| format!("{l}\n"))
        .collect();
    // Hash-match: drop the id, write it under a new name, remove the old.
    w.mirror
        .fs_mut()
        .put("Beta/Alpha renamed.md", anonymous.as_bytes());
    rescan_in(&mut w);
    w.pump_out();
    let state = w.vault.mirror_state(alpha).unwrap().unwrap();
    let bytes = w.mirror.fs().read(&state.rel_path).unwrap();
    w.mirror.fs_mut().remove(&state.rel_path).unwrap();
    w.mirror.fs_mut().put("Elsewhere.md", &bytes);
    let report = rescan_in(&mut w);
    assert_eq!(report.moved, [alpha], "{report:?}");
    assert!(
        report.trashed.is_empty() && report.created.is_empty(),
        "{report:?}"
    );
    assert_eq!(w.vault.read().object(alpha).unwrap().parent_id, None);
}

#[test]
fn a_gone_file_trashes_its_object_alone_and_adopts_unknown_ids() {
    let mut w = new_world();
    let parent = w.object("Parent", None);
    let child = w.object("Child", Some(parent));
    filler(&mut w, 20);
    w.pump_out();

    w.mirror.fs_mut().remove("Parent.md").unwrap();
    let report = rescan_in(&mut w);
    assert_eq!(report.trashed, [parent], "{report:?}");
    assert!(w.vault.read().object(parent).unwrap().deleted_at.is_some());
    let child_row = w.vault.read().object(child).unwrap();
    assert!(
        child_row.deleted_at.is_none(),
        "the child is reparented, not trashed"
    );
    assert_eq!(child_row.parent_id, None);
    assert_eq!(
        w.vault.mirror_state(parent).unwrap().unwrap().state,
        MirrorFileState::Trashed
    );
    // The child's file moves up on the next pass.
    w.pump_out();
    let files = w.files();
    assert!(files.contains(&"Child.md".to_owned()), "{files:?}");
    assert!(!files.iter().any(|f| f.starts_with("Parent")), "{files:?}");

    // A file whose id the vault never saw is adopted under that id; a trashed id is restored.
    let foreign = Id::from_parts(1_700_000_000_000, 42);
    w.mirror.fs_mut().put(
        "Imported.md",
        format!("---\nid: {foreign}\ncreated: 2024-01-02T03:04:05.000Z\n---\n\nhello\n").as_bytes(),
    );
    w.mirror.fs_mut().put(
        "Parent again.md",
        format!("---\nid: {parent}\n---\n\nback\n").as_bytes(),
    );
    let report = rescan_in(&mut w);
    assert_eq!(report.created, [foreign]);
    assert_eq!(report.applied, [parent]);
    let imported = w.vault.read().object(foreign).unwrap();
    assert_eq!(imported.created_at, 1_704_164_645_000);
    let parent_row = w.vault.read().object(parent).unwrap();
    assert!(parent_row.deleted_at.is_none());
    assert_eq!(parent_row.title, "Parent again");

    // A second live file claiming the same id is re-issued as a new object.
    w.mirror.fs_mut().put(
        "Copy.md",
        format!("---\nid: {foreign}\n---\n\ncopy\n").as_bytes(),
    );
    let report = rescan_in(&mut w);
    assert_eq!(report.created.len(), 1);
    assert_ne!(report.created[0], foreign);
    assert!(
        w.vault
            .conflicts(true)
            .unwrap()
            .iter()
            .any(|c| c.kind == ConflictKind::DuplicateId)
    );
    w.pump_out();
    assert!(
        w.text("Copy.md")
            .contains(&format!("id: {}", report.created[0]))
    );
    w.vault.verify(true).unwrap();
}

#[test]
fn bad_files_go_to_the_inbox_and_hold_only_their_object() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.pump_out();
    w.mirror
        .fs_mut()
        .put("Alpha.md", b"---\nid: [unclosed\n---\nx\n");
    w.mirror.fs_mut().put("Binary.md", &[0xff, 0xfe, 0x00]);
    let report = rescan_in(&mut w);
    assert_eq!(report.conflicts.len(), 2, "{report:?}");
    assert!(report.applied.is_empty() && report.created.is_empty());
    let open = w.vault.conflicts(true).unwrap();
    assert!(open.iter().all(|c| c.kind == ConflictKind::ParseError));
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Held
    );

    // A value that does not fit its field is reported and the object keeps its own.
    let kind = w
        .vault
        .transact(|txn| {
            // Not "Task": the seed has one, and two types of one name would be looked up by id.
            let kind = txn.create_type(NewType {
                name: "Chore".into(),
                ..NewType::default()
            })?;
            txn.create_field(NewField {
                type_id: kind,
                key: "due".into(),
                label: "Due".into(),
                kind: dbme_core::model::FieldKind::Date,
                options: json!({}),
                required: false,
            })?;
            Ok(kind)
        })
        .unwrap()
        .0;
    w.vault.transact(|txn| txn.add_type(alpha, kind)).unwrap();
    // The app's write waits while the broken file's entry is open (the person may fix the file);
    // once the entry is dismissed, the app's version is written over it.
    let out = w.pump_out();
    assert_eq!(out.deferred, [alpha], "{out:?}");
    assert!(w.text("Alpha.md").contains("[unclosed"));
    for entry in open
        .iter()
        .filter(|c| c.rel_path.as_deref() == Some("Alpha.md"))
    {
        assert!(w.vault.conflict_resolve(entry.id, "dismissed").unwrap());
    }
    let out = w.pump_out();
    assert_eq!(out.written, [alpha], "{out:?}");
    let text = w.text("Alpha.md");
    w.mirror.fs_mut().put(
        "Alpha.md",
        text.replace("sort:", "due: not a date\ntypes_no: [Ghost]\nsort:")
            .as_bytes(),
    );
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [alpha]);
    let invalid: Vec<_> = w
        .vault
        .conflicts(true)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == ConflictKind::InvalidValue)
        .collect();
    assert_eq!(
        invalid.len(),
        1,
        "{invalid:?} {report:?} {:?} {:?}",
        w.vault.read().object(alpha).unwrap().props_json,
        w.vault.conflicts(false).unwrap()
    );
    assert_eq!(
        w.vault.read().object(alpha).unwrap().props_json.get("due"),
        None
    );
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Synced,
        "a good read lifts the hold"
    );
}

#[test]
fn too_many_removals_or_rewrites_at_once_are_held() {
    let mut w = new_world();
    let ids: Vec<Id> = (0..60).map(|n| w.object(&format!("N{n}"), None)).collect();
    w.pump_out();

    for n in 0..GUARD_COUNT {
        w.mirror.fs_mut().remove(&format!("N{n}.md")).unwrap();
    }
    let report = rescan_in(&mut w);
    assert!(report.held.is_some(), "{report:?}");
    assert!(report.trashed.is_empty());
    assert!(w.vault.read().object(ids[0]).unwrap().deleted_at.is_none());
    let hold = w.vault.conflict(report.held.unwrap()).unwrap();
    assert_eq!(hold.kind, ConflictKind::BulkChangeHold);
    assert_eq!(hold.details["removed_tracked"], json!(GUARD_COUNT));

    // Rewrites are held by count alone: seven edited of sixty is an ordinary batch, fifty is not.
    let mut w = new_world();
    let ids: Vec<Id> = (0..60).map(|n| w.object(&format!("N{n}"), None)).collect();
    w.pump_out();
    for n in 0..7 {
        let text = w.text(&format!("N{n}.md"));
        w.mirror
            .fs_mut()
            .put(&format!("N{n}.md"), format!("{text}\nmore\n").as_bytes());
    }
    let report = rescan_in(&mut w);
    assert!(report.held.is_none(), "{report:?}");
    assert_eq!(report.applied.len(), 7);
    for n in 7..7 + GUARD_COUNT {
        let text = w.text(&format!("N{n}.md"));
        w.mirror
            .fs_mut()
            .put(&format!("N{n}.md"), format!("{text}\nmore\n").as_bytes());
    }
    let report = rescan_in(&mut w);
    assert!(report.held.is_some(), "{report:?}");
    assert!(report.applied.is_empty());
    // Six of sixty is exactly ten percent: not over, applied.
    let mut w = new_world();
    for n in 0..60 {
        w.object(&format!("N{n}"), None);
    }
    w.pump_out();
    for n in 0..6 {
        w.mirror.fs_mut().remove(&format!("N{n}.md")).unwrap();
    }
    let report = rescan_in(&mut w);
    assert!(report.held.is_none(), "{report:?}");
    assert_eq!(report.trashed.len(), 6);
    let _ = ids;
}

#[test]
fn a_folder_deleted_by_hand_is_counted_from_the_rows_not_the_events() {
    let mut w = new_world();
    let top = w.object("Top", None);
    let kids: Vec<Id> = (0..3)
        .map(|n| w.object(&format!("K{n}"), Some(top)))
        .collect();
    filler(&mut w, 40);
    w.pump_out();
    for n in 0..3 {
        w.mirror.fs_mut().remove(&format!("Top/K{n}.md")).unwrap();
    }
    // The watcher reported only the folder; the engine finds the files from mirror_state.
    let report = rescan_in(&mut w);
    assert_eq!(report.trashed.len(), 3, "{report:?}");
    for kid in kids {
        assert!(w.vault.read().object(kid).unwrap().deleted_at.is_some());
    }
    assert!(w.vault.read().object(top).unwrap().deleted_at.is_none());
}
