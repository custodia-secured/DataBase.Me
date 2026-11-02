// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rest of the engine (M5 Task 4c): attachments and the drop zone, cloud conflict copies, the
//! hold decision, the rebuild from files alone, and a write that never reaches the disk.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::sync::Arc;

use common::{World, new_world};
use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_core::{Id, ManualClock};
use dbme_mirror::{FileEvent, Fs, HoldDecision, Inbound, MemFs, Mirror, Outbound};
use dbme_store::{
    ConflictKind, MirrorFileState, NewAttachment, NewBlock, NewField, NewObject, NewRelation,
    NewType, Position, Vault, VaultOptions,
};
use serde_json::json;

fn rescan_in(w: &mut World) -> Inbound {
    let events = w.mirror.rescan(&w.vault).unwrap();
    w.mirror.pump_in(&mut w.vault, events).unwrap()
}

#[test]
fn attachments_are_written_once_and_never_read_as_objects() {
    let mut w = new_world();
    let photo = w.object("Photo", None);
    let blob = w.vault.blobs().put(b"PNG bytes").unwrap();
    w.vault
        .transact(|txn| {
            txn.attach(NewAttachment {
                object_id: photo,
                blob_hash: blob.hash.clone(),
                role: AttachmentRole::Original,
                filename: Some("cat.png".into()),
                mime: Some("image/png".into()),
            })?;
            txn.create_block(NewBlock {
                id: None,
                object_id: photo,
                parent_block_id: None,
                position: Position::Last,
                kind: "image".into(),
                content: json!({"blob": blob.hash.to_string(), "alt": "a cat"}),
            })
        })
        .unwrap();
    w.pump_out();
    let attachment = format!("attachments/{}.png", blob.hash.hex());
    assert_eq!(w.files(), ["Photo.md".to_owned(), attachment.clone()]);
    assert_eq!(w.mirror.fs().read(&attachment).unwrap(), b"PNG bytes");
    let text = w.text("Photo.md");
    assert!(text.contains(&format!("![[{attachment}|a cat]]")), "{text}");

    // Written once; never an event; a rescan is quiet.
    let writes = w.mirror.fs().writes();
    w.vault.mirror_dirty_mark(&[photo]).unwrap();
    w.pump_out();
    assert_eq!(w.mirror.fs().writes(), writes);
    assert_eq!(w.mirror.rescan(&w.vault).unwrap(), []);
    let report = w
        .mirror
        .pump_in(&mut w.vault, vec![FileEvent::Created(attachment)])
        .unwrap();
    assert_eq!(report, Inbound::default());
}

#[test]
fn a_dropped_file_is_an_object_with_the_bytes_attached() {
    let mut w = new_world();
    let album = w.object("Album", None);
    w.pump_out();
    w.mirror.fs_mut().put("Album/holiday.JPG", b"\xff\xd8 jpeg");
    w.mirror.fs_mut().put("notes.pdf", b"%PDF-1.4");
    let report = rescan_in(&mut w);
    assert_eq!(report.created.len(), 2, "{report:?}");
    assert!(report.conflicts.is_empty(), "{report:?}");

    let holiday = w
        .vault
        .read()
        .objects_titled(Some(album), "holiday")
        .unwrap();
    assert_eq!(holiday.len(), 1);
    let holiday = holiday[0].id;
    let types = w.vault.read().object_type_ids(holiday).unwrap();
    let media = w
        .vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Media")
        .unwrap();
    assert_eq!(types, [media.id]);
    let attachments = w.vault.read().attachments(holiday).unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].mime.as_deref(), Some("image/jpeg"));
    assert_eq!(attachments[0].filename.as_deref(), Some("holiday.JPG"));
    assert_eq!(
        w.vault.blobs().read(&attachments[0].blob_hash).unwrap(),
        b"\xff\xd8 jpeg"
    );
    let blocks = w.vault.read().blocks(holiday).unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind, "image");
    assert_eq!(
        blocks[0].content_json["blob"],
        json!(attachments[0].blob_hash.to_string())
    );
    let pdf = w.vault.read().objects_titled(None, "notes").unwrap()[0].id;
    let document = w
        .vault
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Document")
        .unwrap();
    assert_eq!(w.vault.read().object_type_ids(pdf).unwrap(), [document.id]);
    assert!(w.vault.read().blocks(pdf).unwrap().is_empty());

    // The file is the object's own: no Markdown is written for it, and a rescan is quiet.
    let state = w.vault.mirror_state(holiday).unwrap().unwrap();
    assert_eq!(state.rel_path, "Album/holiday.JPG");
    assert_eq!(state.state, MirrorFileState::Excluded);
    let out = w.pump_out();
    assert!(out.written.is_empty(), "{out:?}");
    assert_eq!(w.files(), ["Album.md", "Album/holiday.JPG", "notes.pdf"]);
    assert_eq!(rescan_in(&mut w), Inbound::default());

    // New bytes replace the attachment; a vanished file trashes the object.
    w.mirror.fs_mut().put("notes.pdf", b"%PDF-1.7 v2");
    let report = rescan_in(&mut w);
    assert_eq!(report.applied, [pdf]);
    let attachments = w.vault.read().attachments(pdf).unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(
        w.vault.blobs().read(&attachments[0].blob_hash).unwrap(),
        b"%PDF-1.7 v2"
    );
    for n in 0..20 {
        w.object(&format!("Filler {n}"), None);
    }
    w.pump_out();
    w.mirror.fs_mut().remove("notes.pdf").unwrap();
    let report = rescan_in(&mut w);
    assert_eq!(report.trashed, [pdf]);
    assert!(w.vault.read().object(pdf).unwrap().deleted_at.is_some());
    w.vault.verify(true).unwrap();
}

#[test]
fn a_cloud_drives_conflicting_copy_goes_to_the_inbox_not_the_vault() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.pump_out();
    let text = w.text("Alpha.md");
    w.mirror.fs_mut().put(
        "Alpha (conflicted copy 2026-11-02).md",
        text.replace("Alpha", "Alpha copy").as_bytes(),
    );
    w.mirror.fs_mut().put(
        "Alpha.sync-conflict-20261102-101500-ABCDEFG.md",
        text.as_bytes(),
    );
    let report = rescan_in(&mut w);
    assert!(report.created.is_empty(), "{report:?}");
    assert!(report.applied.is_empty(), "{report:?}");
    assert_eq!(report.conflicts.len(), 2);
    let open = w.vault.conflicts(true).unwrap();
    assert!(
        open.iter()
            .all(|c| c.kind == ConflictKind::CloudConflictFile)
    );
    assert!(
        open.iter().any(|c| c.details["vendor"] == json!("dropbox")),
        "{open:?}"
    );
    assert!(
        open.iter()
            .any(|c| c.details["vendor"] == json!("syncthing")),
        "{open:?}"
    );
    assert_eq!(w.vault.read().object_count().unwrap(), 1);
    let _ = alpha;
}

#[test]
fn a_hold_is_resolved_by_proceeding_or_restoring() {
    let mut w = new_world();
    let ids: Vec<Id> = (0..60).map(|n| w.object(&format!("N{n}"), None)).collect();
    w.pump_out();
    for n in 0..50 {
        w.mirror.fs_mut().remove(&format!("N{n}.md")).unwrap();
    }
    let held = rescan_in(&mut w).held.unwrap();

    // Restore: the vault wins, every held file comes back.
    let report = w
        .mirror
        .hold_decide(&mut w.vault, held, HoldDecision::Restore)
        .unwrap();
    assert_eq!(report, Inbound::default());
    assert!(w.vault.conflict(held).unwrap().resolved_at.is_some());
    let out = w.pump_out();
    assert_eq!(out.written.len(), 50, "{out:?}");
    assert_eq!(w.files().len(), 60);
    assert!(w.vault.read().object(ids[0]).unwrap().deleted_at.is_none());
    assert!(matches!(
        w.mirror
            .hold_decide(&mut w.vault, held, HoldDecision::Proceed),
        Err(dbme_mirror::MirrorError::Store(_))
    ));

    // Proceed: the removals were meant.
    for n in 0..50 {
        w.mirror.fs_mut().remove(&format!("N{n}.md")).unwrap();
    }
    let held = rescan_in(&mut w).held.unwrap();
    let report = w
        .mirror
        .hold_decide(&mut w.vault, held, HoldDecision::Proceed)
        .unwrap();
    assert_eq!(report.trashed.len(), 50, "{report:?}");
    assert!(report.held.is_none());
    assert!(w.vault.read().object(ids[0]).unwrap().deleted_at.is_some());
    assert!(w.vault.read().object(ids[59]).unwrap().deleted_at.is_none());
    assert_eq!(
        w.vault.conflict(held).unwrap().resolution.as_deref(),
        Some("proceed")
    );
}

#[test]
fn a_folder_alone_rebuilds_every_object_field_relation_and_block() {
    let mut w = new_world();
    // A schema with a field, two objects linked both ways, tags, nested blocks, a child object.
    let (kind, due) = w
        .vault
        .transact(|txn| {
            let kind = txn.create_type(NewType {
                name: "Chore".into(),
                ..NewType::default()
            })?;
            let due = txn.create_field(NewField {
                type_id: kind,
                key: "due".into(),
                label: "Due".into(),
                kind: FieldKind::Date,
                options: json!({}),
                required: false,
            })?;
            txn.create_field(NewField {
                type_id: kind,
                key: "owner".into(),
                label: "Owner".into(),
                kind: FieldKind::Relation,
                options: json!({}),
                required: false,
            })?;
            Ok((kind, due))
        })
        .unwrap()
        .0;
    let _ = due;
    let owner_field = w
        .vault
        .read()
        .fields(kind)
        .unwrap()
        .into_iter()
        .find(|f| f.key == "owner")
        .unwrap()
        .id;
    let alpha = w
        .vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Alpha".into(),
                type_ids: vec![kind],
                props: Props::from_iter([("due".to_owned(), json!("2026-11-30"))]),
                summary: Some("first".into()),
                ..NewObject::default()
            })
        })
        .unwrap()
        .0;
    let beta = w.object("Beta", None);
    let child = w.object("Child", Some(alpha));
    w.vault
        .transact(|txn| {
            txn.create_relation(NewRelation {
                src_id: alpha,
                dst_id: beta,
                rel_type: "related".into(),
                field_id: Some(owner_field),
                src_block_id: None,
                label: None,
                props: Props::new(),
            })?;
            txn.create_relation(NewRelation {
                src_id: beta,
                dst_id: alpha,
                rel_type: "related".into(),
                field_id: None,
                src_block_id: None,
                label: Some("back".into()),
                props: Props::new(),
            })?;
            let item = txn.create_block(NewBlock {
                id: None,
                object_id: alpha,
                parent_block_id: None,
                position: Position::Last,
                kind: "bullet".into(),
                content: json!({"inline": [{"text": "see "}, {"mention": beta.to_string()}]}),
            })?;
            txn.create_block(NewBlock {
                id: None,
                object_id: alpha,
                parent_block_id: Some(item),
                position: Position::Last,
                kind: "todo".into(),
                content: json!({"inline": [{"text": "nested #chores"}], "checked": true}),
            })?;
            txn.create_block(NewBlock {
                id: None,
                object_id: beta,
                parent_block_id: None,
                position: Position::Last,
                kind: "code".into(),
                content: json!({"code": "let x = 1;\nx", "language": "rust"}),
            })
        })
        .unwrap();
    w.mirror.request_full_rewrite(&mut w.vault).unwrap();
    w.pump_out();
    // Tags made by the body `#chores` on the way in would be new objects; here none exist yet.

    // A fresh vault over the same folder.
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(common::START + 1000));
    let mut fresh = Vault::create(
        &dir.path().join("Fresh.dbme"),
        VaultOptions {
            identity: common::identity(),
            clock: clock.clone(),
        },
    )
    .unwrap();
    let fs = std::mem::take(w.mirror.fs_mut());
    let mut mirror = Mirror::new(fs, clock);
    let report = mirror.rebuild(&mut fresh).unwrap();
    assert!(report.held.is_none(), "{report:?}");
    assert!(report.conflicts.is_empty(), "{report:?}");

    // Same objects, same ids, same places.
    let same = |id: Id| {
        let a = w.vault.read().object(id).unwrap();
        let b = fresh.read().object(id).unwrap();
        assert_eq!(
            (&a.title, a.parent_id, &a.summary, &a.props_json),
            (&b.title, b.parent_id, &b.summary, &b.props_json),
            "{id}"
        );
    };
    same(alpha);
    same(beta);
    same(child);
    // Same types and fields, by name and key.
    let chore = fresh
        .read()
        .types()
        .unwrap()
        .into_iter()
        .find(|t| t.name == "Chore")
        .unwrap();
    let keys: Vec<String> = fresh
        .read()
        .fields(chore.id)
        .unwrap()
        .into_iter()
        .map(|f| f.key)
        .collect();
    assert_eq!(keys, ["due", "owner"]);
    assert_eq!(fresh.read().object_type_ids(alpha).unwrap(), [chore.id]);
    // Same blocks: ids, kinds, content, tree.
    let blocks = |v: &Vault, id: Id| -> Vec<(Id, Option<Id>, String, serde_json::Value)> {
        v.read()
            .blocks(id)
            .unwrap()
            .into_iter()
            .map(|b| (b.id, b.parent_block_id, b.kind, b.content_json))
            .collect()
    };
    assert_eq!(blocks(&w.vault, alpha), blocks(&fresh, alpha));
    assert_eq!(blocks(&w.vault, beta), blocks(&fresh, beta));
    // Same relations: source, target, type, field key, label; mentions re-derived from the body.
    let relations = |v: &Vault, id: Id| -> Vec<(Id, String, Option<String>, Option<String>)> {
        let read = v.read();
        let mut out: Vec<_> = read
            .relations_from(id)
            .unwrap()
            .into_iter()
            .filter(|r| r.rel_type != "tagged")
            .map(|r| {
                let field = r.field_id.map(|f| read.field(f).unwrap().key);
                (r.dst_id, r.rel_type, field, r.label)
            })
            .collect();
        out.sort();
        out
    };
    assert_eq!(relations(&w.vault, alpha), relations(&fresh, alpha));
    assert_eq!(relations(&w.vault, beta), relations(&fresh, beta));
    // The body tag became a Tag object in the fresh vault.
    let tags = fresh.read().objects_titled(None, "chores").unwrap();
    assert_eq!(tags.len(), 1);
    fresh.verify(true).unwrap();
    // The rebuilt vault is quiet against the folder: a rescan finds nothing to do.
    let events = mirror.rescan(&fresh).unwrap();
    let report = mirror.pump_in(&mut fresh, events).unwrap();
    assert!(
        report.applied.is_empty() && report.created.is_empty(),
        "{report:?}"
    );
}

#[test]
fn a_write_that_never_reaches_the_disk_leaves_the_old_file_and_the_row_alone() {
    let mut w = new_world();
    let alpha = w.object("Alpha", None);
    w.paragraph(alpha, "first");
    w.pump_out();
    let before = w.text("Alpha.md");
    let state_before = w.vault.mirror_state(alpha).unwrap().unwrap();

    w.paragraph(alpha, "second");
    w.mirror.fs_mut().fail_next_writes(1);
    let out = w.pump_out();
    assert_eq!(out.pending, [alpha], "{out:?}");
    assert_eq!(w.text("Alpha.md"), before, "the old bytes are intact");
    let state = w.vault.mirror_state(alpha).unwrap().unwrap();
    assert_eq!(
        state.last_hash, state_before.last_hash,
        "the row still describes the old file"
    );
    assert_eq!(state.state, MirrorFileState::Pending);
    assert_eq!(w.vault.mirror_dirty_count().unwrap(), 1);

    // The next pass completes it.
    let out = w.pump_out();
    assert_eq!(out.written, [alpha]);
    assert!(w.text("Alpha.md").contains("second"));
    assert_eq!(
        w.vault.mirror_state(alpha).unwrap().unwrap().state,
        MirrorFileState::Synced
    );
    let _: Outbound = w.pump_out();
    let _ = MemFs::new();
}
