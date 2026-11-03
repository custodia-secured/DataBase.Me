// SPDX-License-Identifier: AGPL-3.0-or-later
//! App → file (M5 Task 4a): what `pump_out` writes, renames, trashes and leaves alone, and what
//! `rescan` derives from the folder.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use common::new_world;
use dbme_core::files::{self, NoResolver};
use dbme_core::model::Sensitivity;
use dbme_mirror::{FileEvent, Fs, MemFs, Mirror, StdFs};
use dbme_store::{FULL_REWRITE_NEEDED, MirrorFileState, NewObject};
use serde_json::json;

#[test]
fn objects_become_files_named_by_their_titles_and_places() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let notes = w.object("Notes: draft?", Some(alpha));
    let twin_a = w.object("Twin", None);
    let twin_b = w.object("Twin", None);
    w.paragraph(alpha, "Hello **world**");
    w.paragraph(notes, "a note");

    let report = w.pump_out();
    assert_eq!(report.written.len(), 4, "{report:?}");
    assert!(report.renamed.is_empty());
    assert_eq!(
        w.files(),
        [
            "Alpha.md",
            "Alpha/Notes- draft-.md",
            "Twin (2).md",
            "Twin.md"
        ]
    );
    let text = w.text("Alpha.md");
    assert!(text.starts_with("---\nid: "), "{text}");
    // A plain text run's asterisks are escaped, so they read back as text, not as bold.
    assert!(text.contains("\nHello \\*\\*world\\*\\* ^"), "{text}");
    let parsed = files::parse(
        &text,
        &dbme_mirror::schema_of(&w.vault).unwrap(),
        &NoResolver,
    )
    .unwrap();
    assert_eq!(parsed.doc.front.id, Some(alpha));
    assert_eq!(parsed.doc.body.len(), 1);

    // The bookkeeping rows match the files.
    let state = w.vault.mirror_state(notes).unwrap().unwrap();
    assert_eq!(state.rel_path, "Alpha/Notes- draft-.md");
    assert_eq!(state.state, MirrorFileState::Synced);
    assert_eq!(
        state.base_text.as_deref(),
        Some(w.text("Alpha/Notes- draft-.md").as_str())
    );
    assert!(state.last_hash.is_some() && state.file_size.is_some());
    assert_eq!(state.synced_at, Some(common::START));
    let a = w.vault.mirror_state(twin_a).unwrap().unwrap().rel_path;
    let b = w.vault.mirror_state(twin_b).unwrap().unwrap().rel_path;
    assert_eq!(
        {
            let mut both = vec![a, b];
            both.sort();
            both
        },
        ["Twin (2).md", "Twin.md"]
    );

    // Nothing dirty: nothing written. The same bytes: nothing written either.
    let report = w.pump_out();
    assert_eq!(report, dbme_mirror::Outbound::default());
    w.vault.mirror_dirty_mark(&[alpha]).unwrap();
    let report = w.pump_out();
    assert_eq!(report.unchanged, [alpha]);
    assert!(report.written.is_empty());
    assert_eq!(w.mirror.fs().writes(), 4);
}

#[test]
fn a_new_title_renames_the_file_and_its_children_follow() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let child = w.object("Child", Some(alpha));
    let grandchild = w.object("Grandchild", Some(child));
    w.pump_out();
    assert_eq!(
        w.files(),
        ["Alpha.md", "Alpha/Child.md", "Alpha/Child/Grandchild.md"]
    );
    let before = w.text("Alpha/Child/Grandchild.md");

    w.tick();
    w.retitle(alpha, "Beta");
    let report = w.pump_out();
    assert_eq!(
        w.files(),
        ["Beta.md", "Beta/Child.md", "Beta/Child/Grandchild.md"]
    );
    assert_eq!(report.renamed.len(), 3, "{report:?}");
    assert!(w.mirror.fs().dirs().iter().all(|d| !d.starts_with("Alpha")));
    // The children's bytes did not change, only their place.
    assert_eq!(w.text("Beta/Child/Grandchild.md"), before);
    assert_eq!(report.unchanged, [child, grandchild]);
    assert_eq!(
        w.vault.mirror_state(grandchild).unwrap().unwrap().rel_path,
        "Beta/Child/Grandchild.md"
    );

    // Moving an object moves its file.
    let other = w.object("Other", None);
    w.vault
        .transact(|txn| txn.move_object(child, Some(other), dbme_store::Position::Last))
        .unwrap();
    w.pump_out();
    assert_eq!(
        w.files(),
        [
            "Beta.md",
            "Other.md",
            "Other/Child.md",
            "Other/Child/Grandchild.md"
        ]
    );
}

#[test]
fn trash_restore_delete_and_sensitive_objects() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let child = w.object("Child", Some(alpha));
    w.pump_out();

    w.vault.transact(|txn| txn.trash_object(alpha)).unwrap();
    let report = w.pump_out();
    assert_eq!(report.trashed.len(), 2, "{report:?}");
    let trash_a = format!(".dbme/trash/{alpha}.md");
    let trash_c = format!(".dbme/trash/{child}.md");
    assert_eq!(w.files(), [trash_a.clone(), trash_c.clone()]);
    let state = w.vault.mirror_state(alpha).unwrap().unwrap();
    assert_eq!(
        (state.rel_path.as_str(), state.state),
        (trash_a.as_str(), MirrorFileState::Trashed)
    );

    // The path is free for another object while the first is in the Trash.
    let usurper = w.object("Alpha", None);
    w.pump_out();
    assert_eq!(
        w.vault.mirror_state(usurper).unwrap().unwrap().rel_path,
        "Alpha.md"
    );

    // Restoring brings the files back, deduped against the usurper.
    w.vault.transact(|txn| txn.restore_object(alpha)).unwrap();
    let report = w.pump_out();
    assert!(
        report
            .renamed
            .iter()
            .any(|(from, to)| from == &trash_a && to == "Alpha (2).md"),
        "{report:?}"
    );
    assert_eq!(
        w.files(),
        ["Alpha (2).md", "Alpha (2)/Child.md", "Alpha.md"]
    );
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Synced
    );

    // Deleting for good removes the file and the row.
    w.vault.transact(|txn| txn.trash_object(usurper)).unwrap();
    w.pump_out();
    w.vault.transact(|txn| txn.delete_object(usurper)).unwrap();
    let report = w.pump_out();
    assert_eq!(report.removed, [usurper]);
    assert_eq!(w.vault.mirror_state(usurper).unwrap(), None);
    assert_eq!(w.files(), ["Alpha (2).md", "Alpha (2)/Child.md"]);

    // A sensitive object has no file; one that becomes sensitive loses its file.
    let secret = w
        .vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Secret".into(),
                sensitivity: Sensitivity::Sensitive,
                ..NewObject::default()
            })
        })
        .unwrap()
        .0;
    w.vault
        .transact(|txn| txn.update_object(child, |o| o.sensitivity = Sensitivity::Sensitive))
        .unwrap();
    let report = w.pump_out();
    assert_eq!(report.excluded, [secret, child]);
    assert_eq!(w.files(), ["Alpha (2).md"]);
    assert_eq!(
        w.vault.mirror_state(secret).unwrap().unwrap().state,
        MirrorFileState::Excluded
    );
    assert_eq!(
        w.vault.mirror_state(child).unwrap().unwrap().state,
        MirrorFileState::Excluded
    );
    let report = w.pump_out();
    assert_eq!(report, dbme_mirror::Outbound::default());
}

#[test]
fn a_type_change_rewrites_every_file_and_the_schema() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let beta = w.object("Beta", None);
    w.pump_out();
    assert_eq!(w.mirror.fs().stat(dbme_mirror::SCHEMA_FILE).unwrap(), None);

    w.vault
        .transact(|txn| {
            txn.create_type(dbme_store::NewType {
                name: "Kind".into(),
                ..Default::default()
            })
        })
        .unwrap();
    assert!(
        w.vault
            .mirror_meta_get(FULL_REWRITE_NEEDED)
            .unwrap()
            .is_some()
    );
    let report = w.pump_out();
    assert!(report.schema_written);
    assert_eq!(
        report.unchanged,
        [alpha, beta],
        "the files themselves did not change"
    );
    assert_eq!(w.vault.mirror_meta_get(FULL_REWRITE_NEEDED).unwrap(), None);
    let schema: serde_json::Value =
        serde_json::from_str(&w.text(dbme_mirror::SCHEMA_FILE)).unwrap();
    assert!(
        schema["types"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == json!("Kind")),
        "{schema}"
    );
    assert!(files::Schema::from_json(&schema).is_ok());

    // A first enable asks for the same.
    w.mirror.request_full_rewrite(&mut w.vault).unwrap();
    let report = w.pump_out();
    assert!(report.schema_written);
}

#[test]
fn a_blocked_rename_leaves_the_object_pending_and_retries() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.pump_out();
    w.retitle(alpha, "Beta");
    w.mirror.fs_mut().fail_next_renames(2);

    let report = w.pump_out();
    assert_eq!(report.pending, [alpha]);
    assert_eq!(w.files(), ["Alpha.md"]);
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Pending
    );
    assert_eq!(w.vault.mirror_dirty_count().unwrap(), 1, "still dirty");

    let report = w.pump_out();
    assert_eq!(report.pending, [alpha]);
    let report = w.pump_out();
    assert_eq!(
        report.renamed,
        [("Alpha.md".to_owned(), "Beta.md".to_owned())]
    );
    assert_eq!(w.files(), ["Beta.md"]);
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Synced
    );
    assert_eq!(w.vault.mirror_dirty_count().unwrap(), 0);

    // After the last retry the object stays pending and the error surfaces, but nothing is lost.
    w.retitle(alpha, "Gamma");
    w.mirror
        .fs_mut()
        .fail_next_renames(dbme_mirror::MAX_RETRIES + 1);
    for _ in 0..dbme_mirror::MAX_RETRIES - 1 {
        assert_eq!(w.pump_out().pending, [alpha]);
    }
    assert_eq!(w.pump_out().pending, [alpha]);
    assert_eq!(
        w.vault.mirror_dirty_count().unwrap(),
        0,
        "no more automatic retries"
    );
    w.vault.mirror_dirty_mark(&[alpha]).unwrap();
    assert!(w.mirror.pump_out(&mut w.vault).is_err());
    assert_eq!(w.files(), ["Beta.md"], "the file is still there");
}

#[test]
fn rescan_reads_the_folder_against_the_rows() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let beta = w.object("Beta", None);
    w.pump_out();
    assert_eq!(w.mirror.rescan(&w.vault).unwrap(), []);

    w.mirror
        .fs_mut()
        .put("Alpha.md", b"---\nid: x\n---\nedited\n");
    w.mirror.fs_mut().remove("Beta.md").unwrap();
    w.mirror.fs_mut().put("New/Thing.md", b"hello\n");
    w.mirror.fs_mut().put(".obsidian/workspace.json", b"{}");
    w.mirror.fs_mut().put(".dbme/schema.json", b"{}");
    w.mirror.fs_mut().put("photo.png", b"PNG");
    let events = w.mirror.rescan(&w.vault).unwrap();
    assert_eq!(
        events,
        [
            FileEvent::Created("New/Thing.md".into()),
            FileEvent::Created("photo.png".into()),
            FileEvent::Modified("Alpha.md".into()),
            FileEvent::Removed("Beta.md".into()),
        ]
    );
    let _ = (alpha, beta);
}

#[test]
fn the_real_file_system_behaves_like_the_memory_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut fs = StdFs::new(dir.path().join("files")).unwrap();
    assert!(fs.list().unwrap().is_empty());
    fs.write_atomic("A/B.md", b"one").unwrap();
    fs.write_atomic("A/B.md", b"two").unwrap();
    assert_eq!(fs.read("A/B.md").unwrap(), b"two");
    let listed = fs.list().unwrap();
    assert_eq!(listed.len(), 1, "no temp file left: {listed:?}");
    assert_eq!(listed[0].rel_path, "A/B.md");
    assert_eq!(listed[0].size, 3);
    assert!(listed[0].mtime_ms > 0);
    let stat = fs.stat("A/B.md").unwrap().unwrap();
    assert!(!stat.is_dir && stat.size == 3);
    assert!(fs.stat("A").unwrap().unwrap().is_dir);
    assert_eq!(fs.stat("missing").unwrap(), None);

    fs.rename("A/B.md", "C/D.md").unwrap();
    assert!(fs.rename("C/D.md", "C/D.md").is_err());
    fs.write_atomic("C/E.md", b"e").unwrap();
    assert!(
        fs.rename("C/E.md", "C/D.md").is_err(),
        "never over an existing file"
    );
    fs.remove("C/E.md").unwrap();
    fs.remove("C/E.md").unwrap();
    fs.remove_empty_dirs().unwrap();
    assert!(!dir.path().join("files/A").exists());
    assert!(dir.path().join("files/C/D.md").exists());
    assert!(fs.full("../x").is_err());
    assert!(fs.full("/abs").is_err());
    assert!(fs.read("nope.md").is_err());

    // A symbolic link is neither followed nor listed.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path(), dir.path().join("files/loop")).unwrap();
        assert_eq!(fs.list().unwrap().len(), 1);
    }

    let mut mem = MemFs::new();
    mem.write_atomic("A/B.md", b"x").unwrap();
    assert!(mem.rename("A/B.md", "A/B.md").is_err());
    assert!(mem.rename("nope", "x").is_err());
    mem.mkdir("Empty/Deep");
    assert!(mem.stat("Empty/Deep").unwrap().unwrap().is_dir);
    mem.remove_empty_dirs().unwrap();
    assert_eq!(mem.dirs(), ["A"]);
    let _ = Mirror::new(mem, std::sync::Arc::new(dbme_core::ManualClock::new(0)));
}

#[test]
fn a_file_edited_by_hand_is_not_written_over_before_it_is_read() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    let p1 = w.paragraph(alpha, "one");
    let p2 = w.paragraph(alpha, "two");
    w.pump_out();

    // The file changes on disk, unread; then the app edits another paragraph.
    let text = w.text("Alpha.md");
    w.mirror
        .fs_mut()
        .put("Alpha.md", text.replace("two ^", "two (file) ^").as_bytes());
    w.tick();
    w.vault
        .transact(|txn| {
            txn.update_block(p1, |b| {
                b.content_json = json!({"inline": [{"text": "one (app)"}]})
            })
        })
        .unwrap();

    // The write waits: the file keeps its edit, the object stays dirty.
    let out = w.pump_out();
    assert_eq!(out.deferred, [alpha], "{out:?}");
    assert!(out.written.is_empty(), "{out:?}");
    assert!(w.text("Alpha.md").contains("two (file) ^"));
    assert_eq!(w.vault.mirror_dirty_count().unwrap(), 1);
    assert_eq!(w.pump_out().deferred, [alpha], "still waiting");

    // A rescan takes the file in: the merge carries both edits, and the next pass writes them.
    let events = w.mirror.rescan(&w.vault).unwrap();
    assert_eq!(events, [FileEvent::Modified("Alpha.md".into())]);
    let report = w.mirror.pump_in(&mut w.vault, events).unwrap();
    assert_eq!(report.applied, [alpha], "{report:?}");
    assert!(report.conflicts.is_empty(), "{report:?}");
    assert_eq!(
        w.vault.read().block(p2).unwrap().content_json,
        json!({"inline": [{"text": "two (file)"}]})
    );
    let out = w.pump_out();
    assert_eq!(out.written, [alpha], "{out:?}");
    let text = w.text("Alpha.md");
    assert!(
        text.contains("one (app) ^") && text.contains("two (file) ^"),
        "{text}"
    );
    assert_eq!(w.pump_out(), dbme_mirror::Outbound::default());
    w.vault.verify(true).unwrap();
}
