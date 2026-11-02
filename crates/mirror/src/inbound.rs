// SPDX-License-Identifier: AGPL-3.0-or-later
//! File → app: [`Mirror::pump_in`] takes a batch of [`FileEvent`]s and commits what the files say
//! through the store, as a second device would (ADR-0006, ADR-0021, M5 Decisions 3, 12, 13, 16,
//! 17, 20b, 27).
//!
//! A batch is a set, not a sequence (ADR-0024): files that appeared or changed are taken in first,
//! each resolved by `id`, then by path, then by content hash against the files that disappeared,
//! and only then are the remaining disappearances applied as trashes. A `Removed` whose path still
//! exists is ignored. Too many removals or rewrites at once are a hold: one inbox entry, nothing
//! applied.

use std::collections::{BTreeSet, HashMap};

use dbme_core::files::paths::{file_stem, path_key};
use dbme_core::files::{self, FileDoc, Schema};
use dbme_core::model::AttachmentRole;
use dbme_core::{ContentHash, Id};
use dbme_store::{
    ConflictKind, MirrorFileState, MirrorState, NewAttachment, NewConflict, Position, StoreError,
    Vault,
};
use serde_json::{Value, json};

use crate::apply::{self, Placement};
use crate::engine::{FileEvent, doc_of, has_file, ignored, latest_seq, schema_of};
use crate::fs::Fs;
use crate::resolver::VaultResolver;
use crate::{Mirror, MirrorError, Result, TRASH_DIR};

/// Removals or rewrites in one batch from this count up are a hold (M5 Decision 3: fifty
/// deletions trigger the guard at any folder size).
pub const GUARD_COUNT: usize = 50;
/// Removals or rewrites above this share of the tracked files are a hold, with no floor.
pub const GUARD_PERCENT: usize = 10;

/// What one [`Mirror::pump_in`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inbound {
    /// Objects whose file's changes were applied.
    pub applied: Vec<Id>,
    /// Objects made for files that named no known object.
    pub created: Vec<Id>,
    /// Objects whose file was moved or renamed.
    pub moved: Vec<Id>,
    /// Objects trashed because their file is gone.
    pub trashed: Vec<Id>,
    /// Files whose bytes were what the vault last wrote or read: the app's own echo.
    pub echoes: Vec<String>,
    /// Inbox entries made, by id.
    pub conflicts: Vec<Id>,
    /// The `bulk_change_hold` entry, when the batch was held and nothing applied.
    pub held: Option<Id>,
}

/// Where a file's identity came from.
enum Identity {
    /// A known object.
    Existing(Id),
    /// An `id:` the vault has never seen: adopted.
    Adopt(Id),
    /// No id, no path, no hash match: a new object.
    New,
}

impl<F: Fs> Mirror<F> {
    /// Applies a batch of events to the vault. Returns what was done; a problem with one file is
    /// an inbox entry, not an error.
    pub fn pump_in(&mut self, vault: &mut Vault, events: Vec<FileEvent>) -> Result<Inbound> {
        let mut report = Inbound::default();
        let mut changed: BTreeSet<String> = BTreeSet::new();
        let mut removed: BTreeSet<String> = BTreeSet::new();
        for event in events {
            match event {
                FileEvent::Created(p) | FileEvent::Modified(p) => {
                    if !ignored(&p) {
                        changed.insert(p);
                    }
                }
                FileEvent::Removed(p) => {
                    if !ignored(&p) {
                        removed.insert(p);
                    }
                }
                FileEvent::Renamed { from, to } => {
                    if !ignored(&from) {
                        removed.insert(from);
                    }
                    if !ignored(&to) {
                        changed.insert(to);
                    }
                }
                FileEvent::DirCreated(dir) => {
                    // The folder's files may not have been reported (ADR-0024): list them.
                    let prefix = format!("{dir}/");
                    for entry in self
                        .fs
                        .list()
                        .map_err(|e| MirrorError::io("listing", &dir, e))?
                    {
                        if entry.rel_path.starts_with(&prefix) && !ignored(&entry.rel_path) {
                            changed.insert(entry.rel_path);
                        }
                    }
                }
            }
        }
        // A file that is both gone and back is a change, not a removal; a removal whose path
        // still exists did not happen.
        removed.retain(|p| !changed.contains(p) && self.fs.stat(p).ok().flatten().is_none());
        changed.retain(|p| self.fs.stat(p).ok().flatten().is_some_and(|s| !s.is_dir));

        let states = vault.mirror_states(None)?;
        let tracked: HashMap<String, MirrorState> = states
            .iter()
            .filter(|s| has_file(s))
            .map(|s| (s.path_key.clone(), s.clone()))
            .collect();
        let removed_tracked: Vec<&MirrorState> = removed
            .iter()
            .filter_map(|p| tracked.get(&path_key(p)))
            .collect();
        let rewrites = changed
            .iter()
            .filter(|p| tracked.contains_key(&path_key(p)))
            .count();
        let held = if self.skip_guard {
            None
        } else {
            self.guard(
                vault,
                &tracked,
                &removed,
                removed_tracked.len(),
                &changed,
                rewrites,
            )?
        };
        if let Some(id) = held {
            report.held = Some(id);
            report.conflicts.push(id);
            return Ok(report);
        }

        let schema = schema_of(vault)?;
        let mut resolver = VaultResolver::new(&states);

        // Files whose `id:` the vault has never seen get their object first, bare, so that links
        // between files of one batch (a rebuild, a folder dropped in) find their targets.
        self.adopted.clear();
        for rel_path in &changed {
            if !rel_path.ends_with(".md") {
                continue;
            }
            let Ok(bytes) = self.fs.read(rel_path) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let Ok(parsed) = files::parse(text, &schema, &resolver) else {
                continue;
            };
            let Some(id) = parsed.doc.front.id else {
                continue;
            };
            if !matches!(vault.read().object(id), Err(StoreError::NotFound { .. })) {
                continue;
            }
            let title = stem_of(rel_path);
            let parent = self.parent_for(vault, rel_path, &tracked)?;
            let created = parsed.doc.front.created;
            vault.transact_external(|txn| {
                txn.create_object(dbme_store::NewObject {
                    id: Some(id),
                    created_at: created,
                    title,
                    parent_id: parent,
                    ..dbme_store::NewObject::default()
                })
            })?;
            self.adopted.insert(id);
        }
        let mut removed_by_hash: HashMap<ContentHash, &MirrorState> = removed_tracked
            .iter()
            .filter_map(|s| s.last_hash.clone().map(|h| (h, *s)))
            .collect();
        let mut taken_over: BTreeSet<String> = BTreeSet::new();

        for rel_path in &changed {
            self.take_in(
                vault,
                rel_path,
                &tracked,
                &mut removed_by_hash,
                &mut taken_over,
                &schema,
                &mut resolver,
                &mut report,
            )?;
        }

        for rel_path in &removed {
            if taken_over.contains(rel_path) {
                continue;
            }
            let Some(state) = tracked.get(&path_key(rel_path)) else {
                continue;
            };
            self.trash_missing(vault, state, &mut report)?;
        }
        Ok(report)
    }

    /// The bulk-change guard: `>= 50` or `> 10%` of the tracked files removed, or rewritten, in
    /// one batch holds the whole batch.
    fn guard(
        &self,
        vault: &mut Vault,
        tracked: &HashMap<String, MirrorState>,
        removed: &BTreeSet<String>,
        removed_tracked: usize,
        changed: &BTreeSet<String>,
        rewrites: usize,
    ) -> Result<Option<Id>> {
        let total = tracked.len();
        let over = |n: usize| n >= GUARD_COUNT || (total > 0 && n * 100 > total * GUARD_PERCENT);
        // The share rule is for removals (fifty files is over ten percent of any folder under
        // five hundred); rewrites are held by count alone, or one edit in a small folder would be.
        if !over(removed_tracked) && rewrites < GUARD_COUNT {
            return Ok(None);
        }
        let id = vault.conflict_add(NewConflict {
            kind: Some(ConflictKind::BulkChangeHold),
            details: json!({
                "removed": removed.iter().collect::<Vec<_>>(),
                "changed": changed.iter().collect::<Vec<_>>(),
                "tracked": total,
                "removed_tracked": removed_tracked,
                "rewrites": rewrites,
            }),
            ..NewConflict::default()
        })?;
        Ok(Some(id))
    }

    /// Takes one file that appeared or changed into the vault.
    #[allow(clippy::too_many_arguments, reason = "one batch's working state")]
    fn take_in(
        &mut self,
        vault: &mut Vault,
        rel_path: &str,
        tracked: &HashMap<String, MirrorState>,
        removed_by_hash: &mut HashMap<ContentHash, &MirrorState>,
        taken_over: &mut BTreeSet<String>,
        schema: &Schema,
        resolver: &mut VaultResolver,
        report: &mut Inbound,
    ) -> Result<()> {
        let key = path_key(rel_path);
        let bytes = match self.fs.read(rel_path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(MirrorError::io("reading", rel_path, e)),
        };
        let hash = ContentHash::of(&bytes);
        let stat = self
            .fs
            .stat(rel_path)
            .map_err(|e| MirrorError::io("reading", rel_path, e))?;
        let at_path = tracked.get(&key);

        // The app's own write coming back: nothing to do but note the file's size and time.
        if let Some(state) = at_path
            && state.last_hash.as_ref() == Some(&hash)
        {
            let mut state = state.clone();
            state.file_size = stat.map(|s| s.size as i64);
            state.file_mtime_ms = stat.map(|s| s.mtime_ms);
            if state.state == MirrorFileState::Held {
                state.state = MirrorFileState::Synced;
            }
            vault.mirror_state_set(&state)?;
            report.echoes.push(rel_path.to_owned());
            return Ok(());
        }

        // A cloud drive's conflicting copy is for the person, not an object (Decision 1i).
        let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
        let dir = rel_path.rsplit_once('/').map_or("", |(d, _)| d);
        let sibling_exists = |sibling: &str| -> bool {
            let path = if dir.is_empty() {
                sibling.to_owned()
            } else {
                format!("{dir}/{sibling}")
            };
            self.fs.stat(&path).ok().flatten().is_some()
        };
        if at_path.is_none()
            && let Some(found) = dbme_core::files::paths::cloud_conflict(name, &sibling_exists)
        {
            let id = vault.conflict_add(NewConflict {
                rel_path: Some(rel_path.to_owned()),
                kind: Some(ConflictKind::CloudConflictFile),
                details: serde_json::to_value(&found).unwrap_or(Value::Null),
                ..NewConflict::default()
            })?;
            report.conflicts.push(id);
            return Ok(());
        }

        if !rel_path.ends_with(".md") {
            return self.drop_in(vault, rel_path, &bytes, hash, stat, at_path, report);
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(e) => {
                let id =
                    self.parse_error(vault, rel_path, at_path, 1, 0, &format!("not UTF-8: {e}"))?;
                report.conflicts.push(id);
                return Ok(());
            }
        };
        let parsed = match files::parse(text, schema, resolver) {
            Ok(parsed) => parsed,
            Err(e) => {
                let id = self.parse_error(vault, rel_path, at_path, e.line, e.col, &e.message)?;
                report.conflicts.push(id);
                return Ok(());
            }
        };

        // Identity: id, then path, then content hash against what disappeared, then new.
        let mut from_state: Option<MirrorState> = at_path.cloned();
        let mut missing_id = false;
        let identity = match parsed.doc.front.id {
            Some(id) => match vault.read().object(id) {
                Ok(row) => {
                    let own = vault.mirror_state(id)?;
                    let other_live_file = own.as_ref().is_some_and(|s| {
                        s.path_key != key
                            && matches!(
                                s.state,
                                MirrorFileState::Synced
                                    | MirrorFileState::Pending
                                    | MirrorFileState::Held
                            )
                            && self.fs.stat(&s.rel_path).ok().flatten().is_some()
                            && !removed_by_hash.values().any(|r| r.object_id == id)
                    });
                    if other_live_file && row.deleted_at.is_none() {
                        // Two live files claim one object: the newcomer is re-issued.
                        let conflict = vault.conflict_add(NewConflict {
                            object_id: Some(id),
                            rel_path: Some(rel_path.to_owned()),
                            kind: Some(ConflictKind::DuplicateId),
                            details: json!({"also_at": own.as_ref().map(|s| &s.rel_path)}),
                            ..NewConflict::default()
                        })?;
                        report.conflicts.push(conflict);
                        from_state = None;
                        Identity::New
                    } else {
                        if row.deleted_at.is_some() {
                            vault.transact_external(|txn| txn.restore_object(id))?;
                        }
                        if let Some(s) = own.as_ref().filter(|s| s.path_key != key) {
                            // The file moved: the old path is this object's no more.
                            taken_over.insert(s.rel_path.clone());
                            from_state = Some(s.clone());
                        }
                        Identity::Existing(id)
                    }
                }
                Err(StoreError::NotFound { .. }) => Identity::Adopt(id),
                Err(e) => return Err(e.into()),
            },
            None => match at_path {
                Some(state) => {
                    missing_id = true;
                    Identity::Existing(state.object_id)
                }
                None => match removed_by_hash.remove(&hash) {
                    Some(gone) => {
                        taken_over.insert(gone.rel_path.clone());
                        from_state = Some(gone.clone());
                        Identity::Existing(gone.object_id)
                    }
                    None => Identity::New,
                },
            },
        };

        // Title and place from the path.
        let stem = rel_path
            .rsplit('/')
            .next()
            .and_then(|n| n.strip_suffix(".md"))
            .unwrap_or(rel_path)
            .to_owned();
        let parent = self.parent_for(vault, rel_path, tracked)?;
        let mut tags = parsed.doc.front.tags.clone();
        for tag in &parsed.body_tags {
            if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                tags.push(tag.clone());
            }
        }

        let now = self.clock.now_ms();
        // The file's own version, for the inbox copy when the merge conflicts.
        let file_doc = parsed.doc.clone();
        let mut theirs = parsed.doc;
        let ids_assigned: bool;
        // Whether the file held something the vault refused (a bad value, an unknown type, a
        // link to nothing). The inbox has it; the file is then written from the app, or the
        // refused part would sit in the merge base as a change the app "made" for ever after.
        let rejected: bool;
        let mut conflicts: Vec<files::Conflict> = Vec::new();
        let object_id = match identity {
            Identity::Existing(id) => {
                let row = vault.read().object(id)?;
                let base = match from_state.as_ref().and_then(|s| s.base_text.as_deref()) {
                    Some(base_text) => files::parse(base_text, schema, resolver)
                        .map(|p| p.doc)
                        .unwrap_or_default(),
                    // First enable, or no record: both sides changed against nothing (20b).
                    None => FileDoc::default(),
                };
                let ours = doc_of(&vault.read(), &row, schema, resolver)?;
                if !self.adopted.contains(&id) {
                    // `created` and `updated` are informational on the way in (FORMAT.md §6.2):
                    // the sides are aligned so a timestamp never counts as a conflict.
                    let mut base = base;
                    let mut file = theirs.clone();
                    for side in [&mut base, &mut file] {
                        side.front.created = ours.front.created;
                        side.front.updated = ours.front.updated;
                    }
                    let merged = files::merge(&base, &ours, &file, schema, resolver);
                    conflicts = merged.conflicts;
                    let mut doc = merged.doc;
                    take_link_details(&mut doc, &base, &ours, &file);
                    theirs = doc;
                }
                // A bare object made for this very file a moment ago has nothing of its own: the
                // file is taken as it is.
                let mut next = || self.ids.next(now).unwrap_or_else(|_| Id::from_parts(0, 0));
                let before = count_unanchored(&theirs.body);
                files::assign_ids(&mut theirs, &mut next);
                ids_assigned = before > 0;
                let current_stem = file_stem(&row.title);
                let title = (stem != current_stem && !is_dedupe_of(&stem, &current_stem))
                    .then(|| stem.clone());
                let moved = parent != row.parent_id;
                let placement = Placement {
                    title: title.clone(),
                    parent: moved.then_some(parent),
                };
                let doc = theirs.clone();
                let tags = tags.clone();
                let (applied, _) = vault.transact_external(|txn| {
                    apply::apply_doc(txn, id, &placement, &doc, &tags, schema)
                })?;
                rejected = self.note_invalid(vault, rel_path, Some(id), &applied, report)?;
                if title.is_some()
                    || moved
                    || from_state.as_ref().is_some_and(|s| s.path_key != key)
                {
                    // The object's own file is already where the person put it; its children's
                    // files are named after it and must follow (an external transaction marks
                    // nothing by itself).
                    let below: Vec<Id> = vault
                        .read()
                        .subtree(id)?
                        .into_iter()
                        .skip(1)
                        .map(|o| o.id)
                        .collect();
                    vault.mirror_dirty_mark(&below)?;
                    report.moved.push(id);
                }
                if self.adopted.contains(&id) {
                    report.created.push(id);
                } else {
                    report.applied.push(id);
                }
                id
            }
            Identity::Adopt(_) | Identity::New => {
                let adopt = match identity {
                    Identity::Adopt(id) => Some(id),
                    _ => None,
                };
                let mut next = || self.ids.next(now).unwrap_or_else(|_| Id::from_parts(0, 0));
                let before = count_unanchored(&theirs.body);
                files::assign_ids(&mut theirs, &mut next);
                ids_assigned = before > 0;
                let doc = theirs.clone();
                let tags = tags.clone();
                let stem = stem.clone();
                let ((id, applied), _) = vault.transact_external(|txn| {
                    apply::create_from_doc(txn, adopt, &stem, parent, &doc, &tags, schema)
                })?;
                rejected = self.note_invalid(vault, rel_path, Some(id), &applied, report)?;
                report.created.push(id);
                id
            }
        };

        // Conflicts: the app's version stays; the file's goes to the inbox as a copy (16).
        if !conflicts.is_empty() {
            let copy =
                self.conflict_copy(vault, object_id, &stem, &bytes, &file_doc, &tags, schema)?;
            // Made by an external transaction, so nothing marked it: the copy needs its own file.
            vault.mirror_dirty_mark(&[copy])?;
            let id = vault.conflict_add(NewConflict {
                object_id: Some(object_id),
                rel_path: Some(rel_path.to_owned()),
                kind: Some(ConflictKind::EditEdit),
                details: json!({
                    "sites": conflicts.iter().map(|c| json!({
                        "site": format!("{:?}", c.site),
                        "base": c.base, "ours": c.ours, "theirs": c.theirs, "diff3": c.diff3,
                    })).collect::<Vec<_>>(),
                }),
                conflict_copy_id: Some(copy),
            })?;
            report.conflicts.push(id);
        }
        if missing_id {
            let id = vault.conflict_add(NewConflict {
                object_id: Some(object_id),
                rel_path: Some(rel_path.to_owned()),
                kind: Some(ConflictKind::MissingId),
                details: json!({"written_back": true}),
                ..NewConflict::default()
            })?;
            report.conflicts.push(id);
        }

        // The row: the file as read is the base of the next merge, unless the file must be
        // written back (an id to add, anchors to add, a conflict the app's version won), in which
        // case the next `pump_out` rewrites it and sets the base.
        let write_back = missing_id || ids_assigned || rejected || !conflicts.is_empty();
        if let Some(old) = from_state.as_ref().filter(|s| s.path_key != key) {
            resolver.forget(old.object_id);
            let _ = vault.mirror_state_remove(old.object_id);
        }
        vault.mirror_state_set(&MirrorState {
            object_id,
            rel_path: rel_path.to_owned(),
            path_key: key,
            last_hash: Some(hash),
            base_text: Some(text.to_owned()),
            synced_seq: Some(latest_seq(vault)?),
            synced_at: Some(now),
            file_size: stat.map(|s| s.size as i64),
            file_mtime_ms: stat.map(|s| s.mtime_ms),
            state: MirrorFileState::Synced,
        })?;
        resolver.add(object_id, rel_path);
        if write_back {
            vault.mirror_dirty_mark(&[object_id])?;
        }
        Ok(())
    }

    /// The object whose file owns the folder a path sits in: `A/B/C.md` belongs to the object at
    /// `A/B.md`, or, when there is no such file, to the one at `A.md`, or to the top level.
    fn parent_for(
        &self,
        vault: &Vault,
        rel_path: &str,
        tracked: &HashMap<String, MirrorState>,
    ) -> Result<Option<Id>> {
        let mut dir = rel_path.rsplit_once('/').map(|(d, _)| d);
        while let Some(d) = dir {
            // A folder's owner may have been taken in earlier in this very batch, so the vault
            // is asked when the snapshot does not know the path.
            let key = path_key(&format!("{d}.md"));
            let owner = match tracked.get(&key) {
                Some(state) => Some(state.clone()),
                None => vault.mirror_state_by_path_key(&key)?,
            };
            if let Some(state) = owner {
                let live = vault
                    .read()
                    .object(state.object_id)
                    .map(|o| o.deleted_at.is_none())
                    .unwrap_or(false);
                if live {
                    return Ok(Some(state.object_id));
                }
            }
            dir = d.rsplit_once('/').map(|(up, _)| up);
        }
        Ok(None)
    }

    /// A file that could not be read: an inbox entry, and the object's writes are held.
    fn parse_error(
        &self,
        vault: &mut Vault,
        rel_path: &str,
        state: Option<&MirrorState>,
        line: usize,
        col: usize,
        message: &str,
    ) -> Result<Id> {
        if let Some(state) = state {
            let mut held = state.clone();
            held.state = MirrorFileState::Held;
            vault.mirror_state_set(&held)?;
        }
        Ok(vault.conflict_add(NewConflict {
            object_id: state.map(|s| s.object_id),
            rel_path: Some(rel_path.to_owned()),
            kind: Some(ConflictKind::ParseError),
            details: json!({"line": line, "col": col, "message": message}),
            ..NewConflict::default()
        })?)
    }

    /// Inbox entries for values the file held that do not fit. Returns whether there were any.
    fn note_invalid(
        &self,
        vault: &mut Vault,
        rel_path: &str,
        object_id: Option<Id>,
        applied: &apply::Applied,
        report: &mut Inbound,
    ) -> Result<bool> {
        let mut details: Vec<Value> = applied
            .invalid
            .iter()
            .map(|(key, why)| json!({"key": key, "why": why}))
            .collect();
        details.extend(
            applied
                .unknown_types
                .iter()
                .map(|t| json!({"type": t, "why": "no such type"})),
        );
        details.extend(
            applied
                .missing_targets
                .iter()
                .map(|t| json!({"target": t.to_string(), "why": "not a live object"})),
        );
        if details.is_empty() {
            return Ok(false);
        }
        let id = vault.conflict_add(NewConflict {
            object_id,
            rel_path: Some(rel_path.to_owned()),
            kind: Some(ConflictKind::InvalidValue),
            details: Value::Array(details),
            ..NewConflict::default()
        })?;
        report.conflicts.push(id);
        Ok(true)
    }

    /// The file's version as a Note next to the object, with the raw bytes attached.
    #[allow(clippy::too_many_arguments, reason = "one file's working state")]
    fn conflict_copy(
        &mut self,
        vault: &mut Vault,
        object_id: Id,
        stem: &str,
        bytes: &[u8],
        theirs: &FileDoc,
        tags: &[String],
        schema: &Schema,
    ) -> Result<Id> {
        let blob = vault.blobs().put(bytes)?;
        let title = format!("{stem} (conflict copy)");
        let parent = vault.read().object(object_id)?.parent_id;
        let mut doc = theirs.clone();
        doc.front.id = None;
        doc.front.relations.clear();
        doc.front.links.clear();
        // The copy's blocks are new blocks: the originals' anchors belong to the object.
        strip_ids(&mut doc.body);
        let now = self.clock.now_ms();
        let mut next = || self.ids.next(now).unwrap_or_else(|_| Id::from_parts(0, 0));
        files::assign_ids(&mut doc, &mut next);
        let hash = blob.hash.clone();
        let ((copy, _), _) = vault.transact_external(|txn| {
            let (copy, applied) =
                apply::create_from_doc(txn, None, &title, parent, &doc, tags, schema)?;
            txn.attach(NewAttachment {
                object_id: copy,
                blob_hash: hash,
                role: AttachmentRole::Original,
                filename: Some(format!("{stem}.md")),
                mime: Some("text/markdown".to_owned()),
            })?;
            Ok((copy, applied))
        })?;
        Ok(copy)
    }

    /// A tracked file is gone: its object goes to the Trash, alone; its children, whose files
    /// are still there, move up to the grandparent first (Decision 27).
    fn trash_missing(
        &mut self,
        vault: &mut Vault,
        state: &MirrorState,
        report: &mut Inbound,
    ) -> Result<()> {
        let id = state.object_id;
        let row = match vault.read().object(id) {
            Ok(row) => row,
            Err(StoreError::NotFound { .. }) => {
                vault.mirror_state_remove(id)?;
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        if row.deleted_at.is_none() {
            let children: Vec<Id> = vault
                .read()
                .children(Some(id))?
                .iter()
                .map(|c| c.id)
                .collect();
            let grandparent = row.parent_id;
            let moved = children.clone();
            vault.transact_external(|txn| {
                for child in children {
                    txn.move_object(child, grandparent, Position::Last)?;
                }
                txn.trash_object(id)
            })?;
            // An external transaction marks nothing; the children's files must still move out of
            // a folder named after a trashed object, so they are marked by hand.
            vault.mirror_dirty_mark(&moved)?;
        }
        let target = format!("{TRASH_DIR}/{id}.md");
        vault.mirror_state_set(&MirrorState {
            rel_path: target.clone(),
            path_key: path_key(&target),
            state: MirrorFileState::Trashed,
            ..state.clone()
        })?;
        // The children's files now sit in a folder named after a trashed object; they are dirty
        // from the move and `pump_out` relocates them.
        report.trashed.push(id);
        Ok(())
    }
}

/// What to do with a held batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldDecision {
    /// Apply the held events after all: the removals and rewrites were meant.
    Proceed,
    /// Put the folder back the way the vault has it: every held file is written again.
    Restore,
}

impl<F: Fs> Mirror<F> {
    /// Resolves a `bulk_change_hold`: either applies the held events with the guard off, or
    /// marks every held object for writing again so the folder returns to the vault's state.
    pub fn hold_decide(
        &mut self,
        vault: &mut Vault,
        conflict: Id,
        decision: HoldDecision,
    ) -> Result<Inbound> {
        let held = vault.conflict(conflict)?;
        if held.kind != ConflictKind::BulkChangeHold {
            return Err(StoreError::Rule(format!("conflict {conflict} is not a hold")).into());
        }
        if held.resolved_at.is_some() {
            return Err(StoreError::Rule(format!("hold {conflict} is already resolved")).into());
        }
        let paths = |key: &str| -> Vec<String> {
            held.details[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let (removed, changed) = (paths("removed"), paths("changed"));
        let report = match decision {
            HoldDecision::Proceed => {
                let mut events: Vec<FileEvent> =
                    removed.into_iter().map(FileEvent::Removed).collect();
                events.extend(changed.into_iter().map(FileEvent::Modified));
                self.skip_guard = true;
                let result = self.pump_in(vault, events);
                self.skip_guard = false;
                result?
            }
            HoldDecision::Restore => {
                let mut ids = Vec::new();
                for rel_path in removed.iter().chain(&changed) {
                    if let Some(state) = vault.mirror_state_by_path_key(&path_key(rel_path))? {
                        ids.push(state.object_id);
                    }
                }
                vault.mirror_dirty_mark(&ids)?;
                Inbound::default()
            }
        };
        vault.conflict_resolve(
            conflict,
            match decision {
                HoldDecision::Proceed => "proceed",
                HoldDecision::Restore => "restore",
            },
        )?;
        Ok(report)
    }

    /// Builds the vault's objects from the folder alone: the types and fields of
    /// `.dbme/schema.json` that the vault lacks are made first, then every file is taken in as
    /// new, with the guard off. On an empty vault this reproduces every object, field, relation
    /// and block the files carry.
    pub fn rebuild(&mut self, vault: &mut Vault) -> Result<Inbound> {
        if let Ok(bytes) = self.fs.read(crate::SCHEMA_FILE)
            && let Ok(json) = serde_json::from_slice::<Value>(&bytes)
            && let Ok(schema) = Schema::from_json(&json)
        {
            self.adopt_schema(vault, &schema)?;
        }
        let events: Vec<FileEvent> = self
            .fs
            .list()
            .map_err(|e| MirrorError::io("listing", "", e))?
            .into_iter()
            .filter(|e| !ignored(&e.rel_path))
            .map(|e| FileEvent::Created(e.rel_path))
            .collect();
        self.skip_guard = true;
        let result = self.pump_in(vault, events);
        self.skip_guard = false;
        result
    }

    /// Makes the types and fields of a schema file that the vault does not have, by name and key.
    fn adopt_schema(&self, vault: &mut Vault, schema: &Schema) -> Result<()> {
        let have = schema_of(vault)?;
        let mut missing_types = Vec::new();
        let mut missing_fields = Vec::new();
        for entry in &schema.types {
            match have.type_by_name(&entry.ty.name) {
                None => missing_types.push(entry.clone()),
                Some(existing) => {
                    let keys: Vec<String> = have
                        .fields_of(std::slice::from_ref(&existing.name))
                        .iter()
                        .map(|f| f.key.clone())
                        .collect();
                    for field in &entry.fields {
                        if !keys.contains(&field.key) {
                            missing_fields.push((existing.id, field.clone()));
                        }
                    }
                }
            }
        }
        if missing_types.is_empty() && missing_fields.is_empty() {
            return Ok(());
        }
        vault.transact_external(|txn| {
            for entry in missing_types {
                let type_id = txn.create_type(dbme_store::NewType {
                    name: entry.ty.name.clone(),
                    plural: entry.ty.plural.clone(),
                    icon: entry.ty.icon.clone(),
                    color: entry.ty.color.clone(),
                    extends: None,
                    description: entry.ty.description.clone(),
                })?;
                for field in entry.fields {
                    txn.create_field(dbme_store::NewField {
                        type_id,
                        key: field.key,
                        label: field.label,
                        kind: field.kind,
                        options: field.options_json,
                        required: field.required,
                    })?;
                }
            }
            for (type_id, field) in missing_fields {
                txn.create_field(dbme_store::NewField {
                    type_id,
                    key: field.key,
                    label: field.label,
                    kind: field.kind,
                    options: field.options_json,
                    required: field.required,
                })?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// A file that is not Markdown: the basic drop zone (Decision 15). It becomes an object named
    /// after the file, typed by its media type, with the bytes attached as a blob and, for a
    /// picture, an image block; nothing is extracted. The file stays where it is and is the
    /// object's own, so no `.md` is written for it.
    #[allow(clippy::too_many_arguments, reason = "one file's working state")]
    fn drop_in(
        &mut self,
        vault: &mut Vault,
        rel_path: &str,
        bytes: &[u8],
        hash: ContentHash,
        stat: Option<crate::fs::Stat>,
        at_path: Option<&MirrorState>,
        report: &mut Inbound,
    ) -> Result<()> {
        let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
        let (title, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => (stem.to_owned(), ext.to_ascii_lowercase()),
            _ => (name.to_owned(), String::new()),
        };
        let mime = mime_for(&ext);
        let is_image = mime.starts_with("image/");
        let blob = vault.blobs().put(bytes)?;
        let now = self.clock.now_ms();
        let object_id = match at_path {
            // The dropped file changed: the object keeps its identity and gets the new bytes.
            Some(state) => {
                let id = state.object_id;
                let attachments = vault.read().attachments(id)?;
                let blocks = vault.read().blocks(id)?;
                let new_hash = blob.hash.clone();
                vault.transact_external(|txn| {
                    for a in attachments {
                        if a.role == AttachmentRole::Original {
                            txn.detach(id, a.blob_hash, AttachmentRole::Original)?;
                        }
                    }
                    txn.attach(NewAttachment {
                        object_id: id,
                        blob_hash: new_hash.clone(),
                        role: AttachmentRole::Original,
                        filename: Some(name.to_owned()),
                        mime: Some(mime.to_owned()),
                    })?;
                    for block in blocks.iter().filter(|b| b.kind == "image") {
                        let new_hash = new_hash.clone();
                        txn.update_block(block.id, |b| {
                            b.content_json["blob"] = Value::String(new_hash.to_string());
                        })?;
                    }
                    Ok(())
                })?;
                id
            }
            None => {
                let parent = self.parent_for(vault, rel_path, &HashMap::new())?;
                let schema = schema_of(vault)?;
                let type_id = schema
                    .type_by_name(if is_image { "Media" } else { "Document" })
                    .map(|t| t.id);
                let new_hash = blob.hash.clone();
                let alt = title.clone();
                let (id, _) = vault.transact_external(|txn| {
                    let id = txn.create_object(dbme_store::NewObject {
                        title: title.clone(),
                        parent_id: parent,
                        type_ids: type_id.into_iter().collect(),
                        ..dbme_store::NewObject::default()
                    })?;
                    txn.attach(NewAttachment {
                        object_id: id,
                        blob_hash: new_hash.clone(),
                        role: AttachmentRole::Original,
                        filename: Some(name.to_owned()),
                        mime: Some(mime.to_owned()),
                    })?;
                    if is_image {
                        txn.create_block(dbme_store::NewBlock {
                            id: None,
                            object_id: id,
                            parent_block_id: None,
                            position: Position::Last,
                            kind: "image".to_owned(),
                            content: json!({"blob": new_hash.to_string(), "alt": alt}),
                        })?;
                    }
                    Ok(id)
                })?;
                report.created.push(id);
                id
            }
        };
        vault.mirror_state_set(&MirrorState {
            object_id,
            rel_path: rel_path.to_owned(),
            path_key: path_key(rel_path),
            last_hash: Some(hash),
            base_text: None,
            synced_seq: Some(latest_seq(vault)?),
            synced_at: Some(now),
            file_size: stat.map(|s| s.size as i64),
            file_mtime_ms: stat.map(|s| s.mtime_ms),
            // Excluded with a real path: the file is the object's own, and no `.md` is written.
            state: MirrorFileState::Excluded,
        })?;
        if at_path.is_some() {
            report.applied.push(object_id);
        }
        Ok(())
    }
}

/// The media type for a file extension; `application/octet-stream` when unknown.
fn mime_for(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// The file name without its folder and `.md`.
fn stem_of(rel_path: &str) -> String {
    rel_path
        .rsplit('/')
        .next()
        .and_then(|n| n.strip_suffix(".md"))
        .unwrap_or(rel_path)
        .to_owned()
}

/// The core merge compares `links` as a set of `(id, type, block)`, so a label or props edited in
/// the file alone would be lost: take the file's label and props for a link the app left as it
/// was.
fn take_link_details(merged: &mut FileDoc, base: &FileDoc, ours: &FileDoc, theirs: &FileDoc) {
    type Key = (Option<Id>, String, Option<Id>);
    type Details = (Option<String>, dbme_core::model::Props);
    let key = |l: &files::LinkEntry| -> Key { (l.id, l.rel_type.clone(), l.block) };
    let find = |doc: &FileDoc, k: &Key| -> Option<Details> {
        doc.front
            .links
            .iter()
            .find(|l| key(l) == *k)
            .map(|l| (l.label.clone(), l.props.clone()))
    };
    for link in &mut merged.front.links {
        let k = key(link);
        let Some(file) = find(theirs, &k) else {
            continue;
        };
        let in_base = find(base, &k);
        let app_changed = find(ours, &k).is_some_and(|o| Some(&o) != in_base.as_ref());
        if !app_changed && Some(&file) != in_base.as_ref() {
            link.label = file.0;
            link.props = file.1;
        }
    }
}

/// Takes every block's id away, so `assign_ids` gives fresh ones.
fn strip_ids(blocks: &mut [files::FileBlock]) {
    for block in blocks {
        block.id = None;
        strip_ids(&mut block.children);
    }
}

/// How many blocks have no anchor yet.
fn count_unanchored(blocks: &[files::FileBlock]) -> usize {
    blocks
        .iter()
        .map(|b| usize::from(b.id.is_none()) + count_unanchored(&b.children))
        .sum()
}

/// Whether `stem` is `current` with a ` (n)` dedupe suffix: not a new title.
fn is_dedupe_of(stem: &str, current: &str) -> bool {
    stem.strip_prefix(current)
        .and_then(|rest| rest.strip_prefix(" ("))
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}
