// SPDX-License-Identifier: AGPL-3.0-or-later
//! The engine: what the vault owes the folder ([`Mirror::pump_out`]) and what the folder's state
//! says happened outside ([`Mirror::rescan`]).

use std::collections::HashMap;
use std::sync::Arc;

use dbme_core::block::MENTIONS;
use dbme_core::files::paths::{self, file_stem, is_dot_path, path_key};
use dbme_core::files::{self, FileDoc, ObjectRows, Schema, TAGGED};
use dbme_core::model::{ObjectRow, Sensitivity, TypeRow};
use dbme_core::{Clock, ContentHash, Id, IdGenerator};
use dbme_store::{MirrorFileState, MirrorState, Reader, StoreError, Vault};

use crate::fs::Fs;
use crate::resolver::VaultResolver;
use crate::{MirrorError, Result, SCHEMA_FILE, TRASH_DIR};

/// How many times a write or rename that another program blocks is tried before the object is
/// left `pending` for the next change or rescan (ADR-0024). Never a drop.
pub const MAX_RETRIES: u32 = 10;

/// Something that happened in the folder, by path inside it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileEvent {
    /// A file appeared.
    Created(String),
    /// A file's bytes may have changed.
    Modified(String),
    /// A file is gone.
    Removed(String),
    /// A file moved, same folder or not.
    Renamed {
        /// Where it was.
        from: String,
        /// Where it is.
        to: String,
    },
    /// A folder appeared; its files may not have been reported (ADR-0024).
    DirCreated(String),
}

/// A source of [`FileEvent`]s: the platform watcher, or anything a test wants.
pub trait Watcher {
    /// The events since the last call, already debounced by the caller's window.
    fn poll(&mut self) -> Vec<FileEvent>;
}

/// What one [`Mirror::pump_out`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outbound {
    /// Objects whose file was written.
    pub written: Vec<Id>,
    /// Files that moved, `(from, to)`.
    pub renamed: Vec<(String, String)>,
    /// Objects whose file went to `.dbme/trash/`.
    pub trashed: Vec<Id>,
    /// Objects deleted for good, whose file is gone.
    pub removed: Vec<Id>,
    /// Sensitive objects kept out of the folder.
    pub excluded: Vec<Id>,
    /// Objects whose file already held the right bytes.
    pub unchanged: Vec<Id>,
    /// Objects whose write or rename another program blocked; they stay dirty while retries are
    /// left.
    pub pending: Vec<Id>,
    /// Whether `.dbme/schema.json` was written.
    pub schema_written: bool,
}

/// The files folder of one vault, over a file system.
pub struct Mirror<F: Fs> {
    pub(crate) fs: F,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) retries: HashMap<Id, u32>,
    /// For the blocks a person typed without an anchor, and for new objects.
    pub(crate) ids: IdGenerator,
}

impl<F: Fs> std::fmt::Debug for Mirror<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mirror").finish_non_exhaustive()
    }
}

/// Every `TypeRow` and `FieldRow` of the vault, as the file format needs them.
pub fn schema_of(vault: &Vault) -> Result<Schema> {
    let read = vault.read();
    let types = read.types()?;
    let mut fields = Vec::new();
    for t in &types {
        fields.extend(read.fields(t.id)?);
    }
    Ok(Schema::new(types, fields))
}

/// The document of an object as the vault holds it now, in normal form. The attachments' media
/// types are registered on `resolver` first, so their files get extensions.
pub(crate) fn doc_of(
    read: &Reader<'_>,
    row: &ObjectRow,
    schema: &Schema,
    resolver: &mut VaultResolver,
) -> Result<FileDoc> {
    let type_ids = read.object_type_ids(row.id)?;
    let mut types: Vec<TypeRow> = Vec::with_capacity(type_ids.len());
    for type_id in type_ids {
        types.push(read.type_row(type_id)?);
    }
    let type_refs: Vec<&TypeRow> = types.iter().collect();
    let blocks = read.blocks(row.id)?;
    let relations: Vec<_> = read
        .relations_from(row.id)?
        .into_iter()
        .filter(|r| r.rel_type != MENTIONS)
        .collect();
    let mut tags = Vec::new();
    for relation in relations.iter().filter(|r| r.rel_type == TAGGED) {
        if let Ok(tag) = read.object(relation.dst_id) {
            tags.push(tag.title);
        }
    }
    for attachment in read.attachments(row.id)? {
        resolver.add_blob(
            attachment.blob_hash,
            attachment.mime.as_deref(),
            attachment.filename.as_deref(),
        );
    }
    Ok(files::from_rows(
        &ObjectRows {
            object: row,
            types: &type_refs,
            blocks: &blocks,
            relations: &relations,
            tags: &tags,
        },
        schema,
        resolver,
    ))
}

/// The `seq` of the vault's newest op, or `0`.
pub(crate) fn latest_seq(vault: &Vault) -> Result<i64> {
    Ok(vault.read().history(1)?.first().map_or(0, |r| r.seq))
}

impl<F: Fs> Mirror<F> {
    /// An engine over `fs`, stamping its bookkeeping with `clock`.
    pub fn new(fs: F, clock: Arc<dyn Clock>) -> Self {
        Self {
            fs,
            clock,
            retries: HashMap::new(),
            ids: IdGenerator::new(),
        }
    }

    /// The file system.
    pub fn fs(&self) -> &F {
        &self.fs
    }

    /// The file system, to change it.
    pub fn fs_mut(&mut self) -> &mut F {
        &mut self.fs
    }

    /// Marks every object dirty and asks for the schema file: the first enable, or a rebuild.
    pub fn request_full_rewrite(&self, vault: &mut Vault) -> Result<()> {
        vault.mirror_meta_set(dbme_store::FULL_REWRITE_NEEDED, Some("1"))?;
        Ok(())
    }

    /// Writes every object the store marked dirty since the last call: the file of a live object
    /// (renamed first when its title or place changed), a trashed object's file into
    /// `.dbme/trash/`, nothing for a sensitive object, and removes the file of an object deleted
    /// for good. When a type or field changed, every file is written and `.dbme/schema.json` with
    /// it. Parents are handled before their children, so a child's folder is named after its
    /// parent's final file. `mirror_state` is updated only after the file is on disk, so a crash
    /// leaves a stale row the next rescan resolves.
    pub fn pump_out(&mut self, vault: &mut Vault) -> Result<Outbound> {
        let mut report = Outbound::default();
        let full = vault
            .mirror_meta_get(dbme_store::FULL_REWRITE_NEEDED)?
            .is_some();
        let mut ids = vault.mirror_dirty_take()?;
        if full {
            ids = every_object(vault)?;
        }
        if ids.is_empty() && !full {
            return Ok(report);
        }
        let schema = schema_of(vault)?;
        let states = vault.mirror_states(None)?;
        let mut resolver = VaultResolver::new(&states);
        let mut by_key: HashMap<String, Id> = states
            .iter()
            .map(|s| (s.path_key.clone(), s.object_id))
            .collect();
        let mut depth_cache: HashMap<Id, usize> = HashMap::new();
        let mut rows: HashMap<Id, Option<ObjectRow>> = HashMap::new();
        for id in &ids {
            let row = match vault.read().object(*id) {
                Ok(row) => Some(row),
                Err(StoreError::NotFound { .. }) => None,
                Err(e) => return Err(e.into()),
            };
            rows.insert(*id, row);
        }
        ids.sort_by_key(|id| {
            let depth = depth_of(vault, *id, &mut depth_cache).unwrap_or(0);
            (depth, *id)
        });
        let latest_seq = latest_seq(vault)?;

        for id in ids {
            let state = vault.mirror_state(id)?;
            let Some(row) = rows.get(&id).cloned().flatten() else {
                // Deleted for good: the file goes.
                if let Some(state) = state {
                    self.fs
                        .remove(&state.rel_path)
                        .map_err(|e| MirrorError::io("removing", &state.rel_path, e))?;
                    by_key.remove(&state.path_key);
                    vault.mirror_state_remove(id)?;
                    resolver.forget(id);
                    report.removed.push(id);
                }
                continue;
            };
            if row.deleted_at.is_some() {
                self.trash(vault, &row, state, &mut by_key, &mut resolver, &mut report)?;
                continue;
            }
            if row.sensitivity == Sensitivity::Sensitive {
                self.exclude(vault, &row, state, &mut by_key, &mut resolver, &mut report)?;
                continue;
            }
            self.write_object(
                vault,
                &row,
                state,
                &schema,
                &mut by_key,
                &mut resolver,
                latest_seq,
                &mut report,
            )?;
        }

        if full {
            let text = serde_json::to_string_pretty(&schema.to_json())
                .map_err(dbme_core::CoreError::from)?;
            self.fs
                .write_atomic(SCHEMA_FILE, text.as_bytes())
                .map_err(|e| MirrorError::io("writing", SCHEMA_FILE, e))?;
            vault.mirror_meta_set(dbme_store::FULL_REWRITE_NEEDED, None)?;
            report.schema_written = true;
        }
        self.fs
            .remove_empty_dirs()
            .map_err(|e| MirrorError::io("pruning", "", e))?;
        Ok(report)
    }

    /// Derives events from the folder's current state against `mirror_state`: a file at a path no
    /// row has is `Created`, a file whose size or modification time differs from its row is
    /// `Modified`, a row whose file is gone is `Removed`. Dot paths are the app's or another
    /// tool's and are skipped. The events are sorted.
    pub fn rescan(&self, vault: &Vault) -> Result<Vec<FileEvent>> {
        let states = vault.mirror_states(None)?;
        let mut tracked: HashMap<String, &MirrorState> = states
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    MirrorFileState::Synced | MirrorFileState::Pending | MirrorFileState::Held
                )
            })
            .map(|s| (s.path_key.clone(), s))
            .collect();
        let mut events = Vec::new();
        for entry in self
            .fs
            .list()
            .map_err(|e| MirrorError::io("listing", "", e))?
        {
            if is_dot_path(&entry.rel_path) {
                continue;
            }
            match tracked.remove(&path_key(&entry.rel_path)) {
                None => events.push(FileEvent::Created(entry.rel_path)),
                Some(state) => {
                    let same = state.file_size == Some(entry.size as i64)
                        && state.file_mtime_ms == Some(entry.mtime_ms);
                    if !same {
                        events.push(FileEvent::Modified(entry.rel_path));
                    }
                }
            }
        }
        for state in tracked.into_values() {
            events.push(FileEvent::Removed(state.rel_path.clone()));
        }
        events.sort();
        Ok(events)
    }

    fn trash(
        &mut self,
        vault: &mut Vault,
        row: &ObjectRow,
        state: Option<MirrorState>,
        by_key: &mut HashMap<String, Id>,
        resolver: &mut VaultResolver,
        report: &mut Outbound,
    ) -> Result<()> {
        let Some(mut state) = state else {
            return Ok(());
        };
        if state.state == MirrorFileState::Trashed {
            return Ok(());
        }
        let target = format!("{TRASH_DIR}/{}.md", row.id);
        if state.state != MirrorFileState::Excluded {
            self.fs
                .remove(&target)
                .map_err(|e| MirrorError::io("removing", &target, e))?;
            if self.move_file(vault, row.id, &state.rel_path, &target, report)? {
                return Ok(());
            }
        }
        by_key.remove(&state.path_key);
        resolver.forget(row.id);
        state.rel_path = target.clone();
        state.path_key = path_key(&target);
        state.state = MirrorFileState::Trashed;
        by_key.insert(state.path_key.clone(), row.id);
        vault.mirror_state_set(&state)?;
        report.trashed.push(row.id);
        Ok(())
    }

    fn exclude(
        &mut self,
        vault: &mut Vault,
        row: &ObjectRow,
        state: Option<MirrorState>,
        by_key: &mut HashMap<String, Id>,
        resolver: &mut VaultResolver,
        report: &mut Outbound,
    ) -> Result<()> {
        let nominal = format!(".dbme/excluded/{}.md", row.id);
        let mut state = match state {
            Some(state) if state.state == MirrorFileState::Excluded => return Ok(()),
            Some(state) => {
                self.fs
                    .remove(&state.rel_path)
                    .map_err(|e| MirrorError::io("removing", &state.rel_path, e))?;
                by_key.remove(&state.path_key);
                resolver.forget(row.id);
                state
            }
            None => MirrorState {
                object_id: row.id,
                rel_path: nominal.clone(),
                path_key: path_key(&nominal),
                last_hash: None,
                base_text: None,
                synced_seq: None,
                synced_at: None,
                file_size: None,
                file_mtime_ms: None,
                state: MirrorFileState::Excluded,
            },
        };
        state.rel_path = nominal.clone();
        state.path_key = path_key(&nominal);
        state.last_hash = None;
        state.file_size = None;
        state.file_mtime_ms = None;
        state.state = MirrorFileState::Excluded;
        by_key.insert(state.path_key.clone(), row.id);
        vault.mirror_state_set(&state)?;
        report.excluded.push(row.id);
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "one pass's working state")]
    fn write_object(
        &mut self,
        vault: &mut Vault,
        row: &ObjectRow,
        state: Option<MirrorState>,
        schema: &Schema,
        by_key: &mut HashMap<String, Id>,
        resolver: &mut VaultResolver,
        latest_seq: i64,
        report: &mut Outbound,
    ) -> Result<()> {
        let read = vault.read();
        let dir = match row.parent_id {
            None => String::new(),
            Some(parent) => match vault.mirror_state(parent)? {
                Some(s)
                    if s.state != MirrorFileState::Trashed
                        && s.state != MirrorFileState::Excluded =>
                {
                    s.rel_path
                        .strip_suffix(".md")
                        .unwrap_or(&s.rel_path)
                        .to_owned()
                }
                // The parent has no file (sensitive, or not written yet): the child stands where
                // the parent would, so nothing is lost; the parent's own pass moves it.
                _ => String::new(),
            },
        };
        let current = state.as_ref().and_then(|s| {
            (s.state != MirrorFileState::Trashed && s.state != MirrorFileState::Excluded)
                .then(|| s.rel_path.clone())
        });
        let stem = file_stem(&row.title);
        let taken = |candidate: &str| -> bool {
            let rel = paths::join(&[dir.as_str()], candidate);
            let key = path_key(&rel);
            match by_key.get(&key) {
                Some(other) if *other != row.id => true,
                Some(_) => false,
                None => {
                    current.as_deref().is_none_or(|c| path_key(c) != key)
                        && self.fs.stat(&rel).ok().flatten().is_some()
                }
            }
        };
        let target = paths::join(&[dir.as_str()], &paths::dedupe(&stem, &taken));

        let doc = doc_of(&read, row, schema, resolver)?;
        // The file names its own path in no link, but other files name it: register the final
        // path before rendering anything that could link here (children render after).
        resolver.add(row.id, &target);
        let text = files::serialize(&doc, schema, resolver);
        let hash = ContentHash::of(text.as_bytes());

        // Move, if the path changed.
        if let Some(from) = state.as_ref().map(|s| s.rel_path.clone())
            && from != target
            && self
                .fs
                .stat(&from)
                .map_err(|e| MirrorError::io("reading", &from, e))?
                .is_some()
            && self.move_file(vault, row.id, &from, &target, report)?
        {
            return Ok(());
        }
        if let Some(s) = &state {
            by_key.remove(&s.path_key);
        }

        // Write, if the bytes changed.
        let on_disk = self
            .fs
            .stat(&target)
            .map_err(|e| MirrorError::io("reading", &target, e))?;
        let unchanged = on_disk.is_some()
            && state
                .as_ref()
                .is_some_and(|s| s.last_hash.as_ref() == Some(&hash));
        if unchanged {
            report.unchanged.push(row.id);
        } else {
            if let Err(e) = self.fs.write_atomic(&target, text.as_bytes()) {
                return self.blocked(vault, row.id, state, &target, by_key, report, "writing", e);
            }
            report.written.push(row.id);
        }
        self.retries.remove(&row.id);
        let stat = self
            .fs
            .stat(&target)
            .map_err(|e| MirrorError::io("reading", &target, e))?;
        let now = self.clock.now_ms();
        let new_state = MirrorState {
            object_id: row.id,
            rel_path: target.clone(),
            path_key: path_key(&target),
            last_hash: Some(hash),
            base_text: Some(text),
            synced_seq: Some(latest_seq),
            synced_at: Some(now),
            file_size: stat.map(|s| s.size as i64),
            file_mtime_ms: stat.map(|s| s.mtime_ms),
            state: MirrorFileState::Synced,
        };
        by_key.insert(new_state.path_key.clone(), row.id);
        vault.mirror_state_set(&new_state)?;
        Ok(())
    }

    /// Renames `from` to `to`. Returns `true` when the rename was blocked and the object was
    /// left pending (the caller stops there), `false` when it moved.
    fn move_file(
        &mut self,
        vault: &mut Vault,
        id: Id,
        from: &str,
        to: &str,
        report: &mut Outbound,
    ) -> Result<bool> {
        match self.fs.rename(from, to) {
            Ok(()) => {
                report.renamed.push((from.to_owned(), to.to_owned()));
                Ok(false)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => {
                let state = vault.mirror_state(id)?;
                let mut by_key = HashMap::new();
                self.blocked(vault, id, state, from, &mut by_key, report, "renaming", e)?;
                Ok(true)
            }
        }
    }

    /// A write or rename another program blocked: the object stays `pending`, is marked dirty
    /// again while retries are left, and is never dropped.
    #[allow(clippy::too_many_arguments, reason = "one pass's working state")]
    fn blocked(
        &mut self,
        vault: &mut Vault,
        id: Id,
        state: Option<MirrorState>,
        rel_path: &str,
        by_key: &mut HashMap<String, Id>,
        report: &mut Outbound,
        action: &'static str,
        error: std::io::Error,
    ) -> Result<()> {
        let tries = self.retries.entry(id).or_insert(0);
        *tries += 1;
        if *tries > MAX_RETRIES {
            return Err(MirrorError::io(action, rel_path, error));
        }
        let mut state = state.unwrap_or(MirrorState {
            object_id: id,
            rel_path: rel_path.to_owned(),
            path_key: path_key(rel_path),
            last_hash: None,
            base_text: None,
            synced_seq: None,
            synced_at: None,
            file_size: None,
            file_mtime_ms: None,
            state: MirrorFileState::Pending,
        });
        state.state = MirrorFileState::Pending;
        by_key.insert(state.path_key.clone(), id);
        vault.mirror_state_set(&state)?;
        if *tries < MAX_RETRIES {
            vault.mirror_dirty_mark(&[id])?;
        }
        report.pending.push(id);
        Ok(())
    }
}

/// Every object the vault holds, live or in the Trash, parents before children.
fn every_object(vault: &Vault) -> Result<Vec<Id>> {
    let read = vault.read();
    let mut ids = Vec::new();
    for top in read.children(None)? {
        ids.extend(read.subtree(top.id)?.into_iter().map(|o| o.id));
    }
    for trashed in read.trash()? {
        ids.extend(read.subtree(trashed.id)?.into_iter().map(|o| o.id));
    }
    // Objects deleted for good that still have a file.
    for state in vault.mirror_states(None)? {
        if !ids.contains(&state.object_id)
            && matches!(
                read.object(state.object_id),
                Err(StoreError::NotFound { .. })
            )
        {
            ids.push(state.object_id);
        }
    }
    Ok(ids)
}

/// How many parents an object has; `0` for one that no longer exists.
fn depth_of(vault: &Vault, id: Id, cache: &mut HashMap<Id, usize>) -> Result<usize> {
    if let Some(depth) = cache.get(&id) {
        return Ok(*depth);
    }
    let depth = match vault.read().object(id) {
        Ok(row) => match row.parent_id {
            Some(parent) => depth_of(vault, parent, cache)? + 1,
            None => 0,
        },
        Err(StoreError::NotFound { .. }) => 0,
        Err(e) => return Err(e.into()),
    };
    cache.insert(id, depth);
    Ok(depth)
}
