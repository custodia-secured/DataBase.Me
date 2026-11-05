// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme-api`: what the UI can ask of the open vault, as plain Rust functions over a [`Session`].
//!
//! The desktop shell wraps each function in a Tauri command (ADR-0014); the e2e test server calls
//! the same functions over HTTP (ADR-0018). Neither adds behaviour of its own, so the end-to-end
//! tests exercise what the app runs. Every function returns a [`Reply`]: the value, or an
//! [`IpcError`] the UI can act on. After every committed change the session's notifier receives
//! the [`Committed`] record, which the shell sends to the UI as `vault-changed`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::model::{
    AttachmentRole, AttachmentRow, BlockRow, FieldRow, ObjectRow, TypeRow, ViewRow,
};
use dbme_core::query::{AggregateQuery, Query};
use dbme_core::{ContentHash, CoreError, Id};
use dbme_store::{
    AggregateResult, Backlink, ColumnTarget, Committed, DataProfile, FacetBucket, GraphData,
    NewAttachment, NewColumn, NewField, NewObject, NewRelation, NewView, ObjectSummary, ObjectTree,
    Position, SearchHit, Sort, Source, StoreError, Txn, Vault, VaultOptions,
};

mod blocks;
mod import;
mod mirror;
mod settings;

pub use blocks::BlockChange;
pub use dbme_ingest::{ImportOptions, ImportPlan, ImportRun};
pub use import::{import_apply, import_plan, provenance_list};
pub use mirror::{
    DEFAULT_FOLDER, ExportReport, HoldChoice, MirrorStatus, MirrorSync, MirrorWorker,
    OVERRIDES_DIR, conflict_resolve, conflicts_list, export_run, mirror_disable, mirror_enable,
    mirror_hold_decide, mirror_status, mirror_sync_now, spawn_mirror_worker,
};
pub use settings::{
    Mode, SETTINGS_FILE, SETTINGS_VERSION, Settings, SettingsPatch, Theme, settings_get,
    settings_set,
};

/// What went wrong, in a form the UI can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum IpcErrorKind {
    /// No vault is open.
    NoVault,
    /// The requested row does not exist.
    NotFound,
    /// The change breaks a rule of the vault.
    Rule,
    /// Undo or redo would overwrite a later change.
    UndoBlocked,
    /// Another window or process has the vault open.
    Locked,
    /// The folder is not a vault, or not named `<name>.dbme`, or already in use.
    BadPath,
    /// The vault needs a newer version of the app.
    TooNew,
    /// A value breaks the data model.
    Invalid,
    /// Reading or writing failed.
    Storage,
}

/// An error returned by a command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct IpcError {
    /// What kind of problem it is.
    pub kind: IpcErrorKind,
    /// A sentence for people.
    pub message: String,
}

impl IpcError {
    /// An error of `kind`.
    pub fn new(kind: IpcErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<StoreError> for IpcError {
    fn from(error: StoreError) -> Self {
        let kind = match &error {
            StoreError::NotFound { .. } => IpcErrorKind::NotFound,
            StoreError::Rule(_) => IpcErrorKind::Rule,
            StoreError::UndoBlocked(_) => IpcErrorKind::UndoBlocked,
            StoreError::Locked(_) => IpcErrorKind::Locked,
            StoreError::NotAVault(_)
            | StoreError::AlreadyExists(_)
            | StoreError::BadVaultName(_) => IpcErrorKind::BadPath,
            StoreError::TooNew { .. } => IpcErrorKind::TooNew,
            StoreError::Core(CoreError::Conflict { .. }) => IpcErrorKind::UndoBlocked,
            StoreError::Core(_) => IpcErrorKind::Invalid,
            StoreError::Sql(_) | StoreError::Io { .. } | StoreError::MissingBlob(_) => {
                IpcErrorKind::Storage
            }
        };
        Self::new(kind, error.to_string())
    }
}

impl From<CoreError> for IpcError {
    fn from(error: CoreError) -> Self {
        StoreError::from(error).into()
    }
}

/// What every command returns.
pub type Reply<T> = Result<T, IpcError>;

/// About the open vault.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VaultInfo {
    /// The vault's ID.
    pub id: Id,
    /// Its folder.
    pub path: String,
    /// Whether there is a step to undo.
    pub can_undo: bool,
    /// Whether there is a step to redo.
    pub can_redo: bool,
}

/// The largest blob [`blob_read`] returns, and the largest [`attachment_add`] accepts: 32 MiB.
/// Larger files arrive with the ingestion pipeline (M6), which streams them.
pub const MAX_IPC_BLOB: usize = 32 * 1024 * 1024;

/// A sort key longer than this makes its siblings rebalance in the same step (ADR-0013). Inserting
/// at one spot grows a key by about a character every five inserts, so this is about 70 inserts.
pub const LONG_SORT_KEY: usize = 16;

type Notify = Box<dyn Fn(&Committed) + Send + Sync>;
type MirrorNotify = Box<dyn Fn(&MirrorStatus) + Send + Sync>;

/// The vault open in one window, if any, its files folder while that is on, and who to tell
/// about its changes.
pub struct Session {
    vault: Mutex<Option<Vault>>,
    /// The files folder's engine, while the folder is enabled. Locked after `vault`, never before.
    mirror: Mutex<Option<mirror::Running>>,
    notify: Notify,
    mirror_notify: MirrorNotify,
    /// Where the app keeps its settings, for the files folder's override file. `None` for a shell
    /// that keeps none.
    config_dir: Option<PathBuf>,
    /// The worker's wake-up channel, once one runs.
    waker: Mutex<Option<Sender<mirror::Wake>>>,
    /// The person's settings, read from the config folder on first use (M7).
    settings: Mutex<Option<Settings>>,
}

impl Session {
    /// A session with no vault open. `notify` receives every committed change.
    pub fn new(notify: impl Fn(&Committed) + Send + Sync + 'static) -> Self {
        Self {
            vault: Mutex::new(None),
            mirror: Mutex::new(None),
            notify: Box::new(notify),
            mirror_notify: Box::new(|_| {}),
            config_dir: None,
            waker: Mutex::new(None),
            settings: Mutex::new(None),
        }
    }

    /// Where the app keeps its settings: the files folder's override file goes there.
    #[must_use]
    pub fn with_config_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(dir.into());
        self
    }

    /// `notify` receives the files folder's status whenever it changes (`mirror-changed`).
    #[must_use]
    pub fn with_mirror_notify(
        mut self,
        notify: impl Fn(&MirrorStatus) + Send + Sync + 'static,
    ) -> Self {
        self.mirror_notify = Box::new(notify);
        self
    }

    fn lock(&self) -> MutexGuard<'_, Option<Vault>> {
        // A panic while the lock was held cannot leave the vault half-changed: SQLite rolls
        // back the unfinished transaction. So a poisoned lock is safe to use.
        self.vault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_mirror(&self) -> MutexGuard<'_, Option<mirror::Running>> {
        self.mirror
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_waker(&self) -> MutexGuard<'_, Option<Sender<mirror::Wake>>> {
        self.waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn lock_settings(&self) -> MutexGuard<'_, Option<Settings>> {
        self.settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The settings, read from the config folder the first time they are asked for.
    fn settings(&self) -> Settings {
        let mut guard = self.lock_settings();
        guard
            .get_or_insert_with(|| settings::load(self.config_dir.as_deref()))
            .clone()
    }

    /// Wakes the worker, if one runs, so it writes what the last commit changed.
    fn wake_mirror(&self) {
        if let Some(tx) = self.lock_waker().as_ref() {
            let _ = tx.send(mirror::Wake::Changed);
        }
    }

    /// Closes the open vault, if any, and stops its files folder.
    pub fn close(&self) {
        let mut vault = self.lock();
        *self.lock_mirror() = None;
        *vault = None;
    }

    /// Runs `use_vault` on the open vault.
    pub fn with<T>(&self, use_vault: impl FnOnce(&mut Vault) -> Result<T, StoreError>) -> Reply<T> {
        let mut guard = self.lock();
        let vault = guard
            .as_mut()
            .ok_or_else(|| IpcError::new(IpcErrorKind::NoVault, "no vault is open"))?;
        Ok(use_vault(vault)?)
    }

    /// Runs a change in one transaction, and tells the notifier what changed.
    fn transact<T>(&self, change: impl FnOnce(&mut Txn<'_>) -> Result<T, StoreError>) -> Reply<T> {
        self.change(|vault| vault.transact(change))
    }

    /// Tells the notifier about a change made outside [`Session::change`], and wakes the worker.
    pub(crate) fn notify_committed(&self, committed: &Committed) {
        (self.notify)(committed);
        self.wake_mirror();
    }

    /// Runs a change and tells the notifier what changed.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Vault) -> Result<(T, Option<Committed>), StoreError>,
    ) -> Reply<T> {
        let (value, committed) = self.with(change)?;
        if let Some(committed) = committed {
            (self.notify)(&committed);
            self.wake_mirror();
        }
        Ok(value)
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("vault", &self.lock().as_ref().map(Vault::id))
            .finish_non_exhaustive()
    }
}

fn info(vault: &Vault) -> VaultInfo {
    VaultInfo {
        id: vault.id(),
        path: vault.root().display().to_string(),
        can_undo: vault.can_undo(),
        can_redo: vault.can_redo(),
    }
}

/// Opens or creates a vault in place of the one open now, if any.
fn replace(
    session: &Session,
    open: impl FnOnce() -> Result<Vault, StoreError>,
) -> Reply<VaultInfo> {
    let mut guard = session.lock();
    let mut running = session.lock_mirror();
    // Close the old vault first: it may be the one being opened again.
    *running = None;
    *guard = None;
    let vault = open()?;
    let info = info(&vault);
    // The ID only: paths can name people and projects (logs never hold user content).
    tracing::info!(vault = %info.id, "vault open");
    // The files folder stays on across openings; a folder that cannot be used leaves it off
    // for this opening, and the status says so.
    if vault.mirror_meta_get(dbme_store::FOLDER_ENABLED)?.is_some() {
        match mirror::start(session.config_dir.as_deref(), &vault) {
            Ok(started) => *running = Some(started),
            Err(error) => {
                tracing::warn!(error = %error.message, "files folder: not started");
            }
        }
    }
    *guard = Some(vault);
    drop(running);
    drop(guard);
    session.wake_mirror();
    Ok(info)
}

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorKind::Invalid, message)
}

// The vault.

/// Creates a vault at `path` (a folder named `<name>.dbme`) and opens it.
pub fn vault_create(session: &Session, path: &str, options: VaultOptions) -> Reply<VaultInfo> {
    // A folder typed into onboarding may not exist yet (M7); the vault's parents are made first.
    if let Some(parent) = Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            IpcError::new(
                IpcErrorKind::Storage,
                format!("cannot create {}: {e}", parent.display()),
            )
        })?;
    }
    replace(session, || Vault::create(Path::new(path), options))
}

/// Opens the vault at `path`.
pub fn vault_open(session: &Session, path: &str, options: VaultOptions) -> Reply<VaultInfo> {
    replace(session, || Vault::open(Path::new(path), options))
}

/// What the diagnostic report says about the vault (M7): numbers, never content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VaultCounts {
    /// Objects, the Trash included.
    pub objects: u64,
    /// Blobs in the store.
    pub blobs: u64,
    /// The schema version the vault is at.
    pub schema_version: u32,
}

/// The open vault's counts, or `None` when no vault is open.
pub fn vault_counts(session: &Session) -> Reply<Option<VaultCounts>> {
    let guard = session.lock();
    let Some(vault) = guard.as_ref() else {
        return Ok(None);
    };
    let read = vault.read();
    Ok(Some(VaultCounts {
        objects: read.object_count()?,
        blobs: read.blob_count()?,
        schema_version: dbme_store::SCHEMA_VERSION,
    }))
}

/// Closes the open vault, if any.
pub fn vault_close(session: &Session) -> Reply<()> {
    session.close();
    Ok(())
}

/// The open vault, or `None`.
pub fn vault_info(session: &Session) -> Reply<Option<VaultInfo>> {
    Ok(session.lock().as_ref().map(info))
}

/// Undoes the last step. Returns `None` if there is nothing to undo.
pub fn undo(session: &Session) -> Reply<Option<Committed>> {
    session.change(|vault| vault.undo().map(|c| (c.clone(), c)))
}

/// Redoes the last undone step. Returns `None` if there is nothing to redo.
pub fn redo(session: &Session) -> Reply<Option<Committed>> {
    session.change(|vault| vault.redo().map(|c| (c.clone(), c)))
}

// Types and fields.

/// Every type, by name.
pub fn types_list(session: &Session) -> Reply<Vec<TypeRow>> {
    session.with(|vault| vault.read().types())
}

/// The fields the objects of `source` can show, one per key.
pub fn fields_list(session: &Session, source: Source) -> Reply<Vec<FieldRow>> {
    session.with(|vault| vault.read().fields_in(source))
}

/// Adds a field to a type and returns its ID.
pub fn field_create(session: &Session, field: NewField) -> Reply<Id> {
    session.transact(|txn| txn.create_field(field))
}

/// The type a new column of a Table showing `source` goes on (ADR-0030).
pub fn column_target(session: &Session, source: Source) -> Reply<ColumnTarget> {
    session.with(|vault| vault.read().column_target(source))
}

/// Adds a column to a Table showing `source`, in one undo step, and returns the field's ID.
pub fn column_add(session: &Session, source: Source, column: NewColumn) -> Reply<Id> {
    session.transact(|txn| txn.add_column(source, column))
}

/// Renames a field. Returns `false` if it already had the label.
pub fn field_rename(session: &Session, id: Id, label: &str) -> Reply<bool> {
    session.transact(|txn| txn.rename_field(id, label))
}

/// Deletes a field. Values objects hold for it stay in their properties.
pub fn field_delete(session: &Session, id: Id) -> Reply<()> {
    session.transact(|txn| txn.delete_field(id))
}

// Reading objects.

/// One object.
pub fn object_get(session: &Session, id: Id) -> Reply<ObjectRow> {
    session.with(|vault| vault.read().object(id))
}

/// The live children of `parent`, or the top-level objects, in order.
pub fn object_children(session: &Session, parent: Option<Id>) -> Reply<Vec<ObjectRow>> {
    session.with(|vault| vault.read().children(parent))
}

/// The objects above `id`, from the top level down to its parent.
pub fn object_ancestors(session: &Session, id: Id) -> Reply<Vec<ObjectRow>> {
    session.with(|vault| vault.read().ancestors(id))
}

/// The summaries of `ids`, in the same order, skipping objects that do not exist.
pub fn object_summaries(session: &Session, ids: &[Id]) -> Reply<Vec<ObjectSummary>> {
    session.with(|vault| vault.read().summaries(ids))
}

/// Up to `limit` objects of `source` in `sort` order, starting at `offset`.
pub fn objects_page(
    session: &Session,
    source: Source,
    sort: &Sort,
    offset: u32,
    limit: u32,
) -> Reply<Vec<ObjectSummary>> {
    session.with(|vault| vault.read().page(source, sort, offset, limit))
}

/// How many objects `source` lists.
pub fn objects_count(session: &Session, source: Source) -> Reply<u32> {
    let count = session.with(|vault| vault.read().count_in(source))?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Live objects whose title contains `text`, prefixes first; with no text, the most recently
/// changed.
pub fn object_search(session: &Session, text: &str, limit: u32) -> Reply<Vec<ObjectSummary>> {
    session.with(|vault| vault.read().search_titles(text, limit))
}

/// The live relations to an object, with their sources.
pub fn backlinks(session: &Session, id: Id) -> Reply<Vec<Backlink>> {
    session.with(|vault| vault.read().backlinks(id))
}

// Changing objects.

/// Creates an object and returns its ID.
pub fn object_create(session: &Session, object: NewObject) -> Reply<Id> {
    session.transact(|txn| {
        let parent = object.parent_id;
        let id = txn.create_object(object)?;
        rebalance_objects_if_long(txn, id, parent)?;
        Ok(id)
    })
}

/// Adds a row to a Table showing `source`: an empty object under `parent`, with the types a row
/// there has. Returns its ID.
pub fn row_add(session: &Session, source: Source, parent: Id) -> Reply<Id> {
    session.transact(|txn| {
        let id = txn.add_row(source, parent)?;
        rebalance_objects_if_long(txn, id, Some(parent))?;
        Ok(id)
    })
}

/// Renames an object. Returns `false` if the title was already `title`.
pub fn object_rename(session: &Session, id: Id, title: String) -> Reply<bool> {
    session.transact(|txn| txn.update_object(id, |o| o.title = title))
}

/// Sets the value of field `key` on an object, or clears it when `value` is `None`. Returns
/// `false` if nothing changed.
pub fn object_set_prop(
    session: &Session,
    id: Id,
    key: String,
    value: Option<Value>,
) -> Reply<bool> {
    if value.as_ref().is_some_and(Value::is_null) {
        return Err(invalid(
            "null is not a value; send no value to clear the field",
        ));
    }
    session.transact(|txn| {
        txn.update_object(id, |o| match value {
            Some(value) => {
                o.props_json.insert(key, value);
            }
            None => {
                o.props_json.remove(&key);
            }
        })
    })
}

/// Sets when an object's event starts and ends, in milliseconds, or clears them with `None`. One
/// change, so one undo puts both back. Returns `false` if nothing changed.
pub fn object_set_event(
    session: &Session,
    id: Id,
    start: Option<i64>,
    end: Option<i64>,
) -> Reply<bool> {
    session.transact(|txn| {
        txn.update_object(id, |o| {
            o.event_start = start;
            o.event_end = end;
        })
    })
}

/// Moves an object under `parent` (`None`: the top level), at `position` among its new
/// siblings. Returns `false` if it did not move.
pub fn object_move(
    session: &Session,
    id: Id,
    parent: Option<Id>,
    position: Position,
) -> Reply<bool> {
    session.transact(|txn| {
        let moved = txn.move_object(id, parent, position)?;
        rebalance_objects_if_long(txn, id, parent)?;
        Ok(moved)
    })
}

fn rebalance_objects_if_long(
    txn: &mut Txn<'_>,
    id: Id,
    parent: Option<Id>,
) -> Result<(), StoreError> {
    if txn.read().object(id)?.sort_key.as_str().len() > LONG_SORT_KEY {
        txn.rebalance_objects(parent)?;
    }
    Ok(())
}

/// Moves an object, and everything below it, to the Trash.
pub fn object_trash(session: &Session, id: Id) -> Reply<()> {
    session.transact(|txn| txn.trash_object(id))
}

/// Takes an object out of the Trash.
pub fn object_restore(session: &Session, id: Id) -> Reply<()> {
    session.transact(|txn| txn.restore_object(id))
}

/// Gives an object another type. Returns `false` if it already had it.
pub fn object_add_type(session: &Session, id: Id, type_id: Id) -> Reply<bool> {
    session.transact(|txn| txn.add_type(id, type_id))
}

/// Takes a type away from an object. Returns `false` if it did not have it.
pub fn object_remove_type(session: &Session, id: Id, type_id: Id) -> Reply<bool> {
    session.transact(|txn| txn.remove_type(id, type_id))
}

// The mind map (ADR-0019).

/// The type of the relations drawn on the mind map.
pub const RELATED: &str = "related";

/// `root` and up to `limit` objects in all from below it, with the relations among them.
pub fn object_tree(session: &Session, root: Id, limit: u32) -> Reply<ObjectTree> {
    session.with(|vault| vault.read().tree(root, limit))
}

/// Relates `src` to `dst` and returns the relation. If they are already related that way, it
/// changes nothing and returns the relation there is. An object can't relate to itself.
pub fn relation_create(session: &Session, src: Id, dst: Id) -> Reply<Id> {
    session.transact(|txn| {
        if src == dst {
            return Err(StoreError::Rule(
                "an object can't be related to itself".into(),
            ));
        }
        let existing = txn.read().relations_from(src)?.into_iter().find(|r| {
            r.dst_id == dst
                && r.rel_type == RELATED
                && r.field_id.is_none()
                && r.src_block_id.is_none()
        });
        if let Some(relation) = existing {
            return Ok(relation.id);
        }
        txn.create_relation(NewRelation {
            src_id: src,
            dst_id: dst,
            rel_type: RELATED.into(),
            field_id: None,
            src_block_id: None,
            label: None,
            props: Default::default(),
        })
    })
}

/// Moves a relation to the Trash. Returns `false` if it was already there. A relation that a block
/// or a field holds belongs to it, and changes only with it.
pub fn relation_trash(session: &Session, id: Id) -> Reply<bool> {
    session.transact(|txn| {
        let relation = txn.read().relation(id)?;
        if relation.field_id.is_some() || relation.src_block_id.is_some() {
            return Err(StoreError::Rule(
                "this relation belongs to a block or a field; change it there".into(),
            ));
        }
        txn.trash_relation(id)
    })
}

// Search and query (ADR-0020).

/// Up to `limit` live objects matching `text` anywhere in their title, body or field values, best
/// match first, each with a snippet. With no words, the most recently changed objects.
pub fn search(session: &Session, text: &str, limit: u32) -> Reply<Vec<SearchHit>> {
    session.with(|vault| vault.read().search(text, limit))
}

/// The results of a query, in its order, from its offset and up to its limit.
pub fn objects_query(session: &Session, query: &Query) -> Reply<Vec<ObjectSummary>> {
    session.with(|vault| vault.read().query(query))
}

/// How many results a query has in all.
pub fn objects_query_count(session: &Session, query: &Query) -> Reply<u32> {
    let count = session.with(|vault| vault.read().query_count(query))?;
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// The values of `field` among all of a query's results and how many have each, most common
/// first. `field` is a field key, or `$type` for the results' types.
pub fn facets(session: &Session, query: &Query, field: &str) -> Reply<Vec<FacetBucket>> {
    session.with(|vault| vault.read().facets(query, field))
}

/// Figures per group over a query's results, computed in the store (M8, ADR-0032).
pub fn aggregate(session: &Session, aggregate: &AggregateQuery) -> Reply<AggregateResult> {
    session.with(|vault| vault.read().aggregate(aggregate))
}

/// The shape of a query's results, read from a sample, for the view recommender (M8).
pub fn data_profile(session: &Session, query: &Query, fields: &[String]) -> Reply<DataProfile> {
    session.with(|vault| vault.read().data_profile(query, fields))
}

/// A query's results as a graph, up to `limit` nodes, with the live relations and parent links
/// among them (M8, ADR-0034).
pub fn graph_read(session: &Session, query: &Query, limit: u32) -> Reply<GraphData> {
    session.with(|vault| vault.read().graph(query, limit))
}

/// Every saved view, by name.
pub fn views_list(session: &Session) -> Reply<Vec<ViewRow>> {
    session.with(|vault| vault.read().views())
}

/// A saved view.
pub fn view_get(session: &Session, id: Id) -> Reply<ViewRow> {
    session.with(|vault| vault.read().view(id))
}

/// Saves a view and returns its ID.
pub fn view_save(session: &Session, view: NewView) -> Reply<Id> {
    session.transact(|txn| txn.create_view(view))
}

/// Replaces a saved view's name, renderer, query, settings and owner. Returns `false` if nothing
/// changed.
pub fn view_update(session: &Session, id: Id, view: NewView) -> Reply<bool> {
    session.transact(|txn| {
        txn.update_view(id, |row| {
            row.name = view.name;
            row.renderer = view.renderer;
            row.query_json = view.query;
            row.config_json = view.config;
            row.owner_object_id = view.owner_object_id;
        })
    })
}

/// Deletes a saved view. Undo brings it back.
pub fn view_delete(session: &Session, id: Id) -> Reply<()> {
    session.transact(|txn| txn.delete_view(id))
}

// Blocks.

/// All blocks of an object, in tree order.
pub fn blocks_list(session: &Session, object_id: Id) -> Reply<Vec<BlockRow>> {
    session.with(|vault| vault.read().blocks(object_id))
}

/// Applies an editor's changes to one object's blocks, in order, as one undo step (ADR-0016).
pub fn blocks_apply(session: &Session, object_id: Id, changes: Vec<BlockChange>) -> Reply<()> {
    session.transact(|txn| blocks::apply(txn, object_id, changes))
}

// Attachments and blobs.

/// An object's attachments.
pub fn attachments_list(session: &Session, object_id: Id) -> Reply<Vec<AttachmentRow>> {
    session.with(|vault| vault.read().attachments(object_id))
}

/// Stores `data` (base64) in the blob store and attaches it to an object as an original. Returns
/// its hash. Attaching the same bytes again changes nothing.
pub fn attachment_add(
    session: &Session,
    object_id: Id,
    data: &str,
    filename: Option<String>,
    mime: Option<String>,
) -> Reply<ContentHash> {
    let bytes = BASE64
        .decode(data)
        .map_err(|e| invalid(format!("the data is not base64: {e}")))?;
    if bytes.len() > MAX_IPC_BLOB {
        return Err(invalid("the file is larger than 32 MiB"));
    }
    session.change(|vault| {
        let hash = vault.blobs().put(&bytes)?.hash;
        let attached = vault
            .read()
            .attachments(object_id)?
            .iter()
            .any(|a| a.blob_hash == hash && a.role == AttachmentRole::Original);
        if attached {
            return Ok((hash, None));
        }
        let (_, committed) = vault.transact(|txn| {
            txn.attach(NewAttachment {
                object_id,
                blob_hash: hash.clone(),
                role: AttachmentRole::Original,
                filename,
                mime,
            })
        })?;
        Ok((hash, committed))
    })
}

/// A blob's bytes, as base64.
pub fn blob_read(session: &Session, hash: &ContentHash) -> Reply<String> {
    session.with(|vault| {
        let size = vault
            .blobs()
            .size(hash)?
            .ok_or_else(|| StoreError::MissingBlob(hash.clone()))?;
        if size > MAX_IPC_BLOB as u64 {
            return Err(StoreError::Rule("the blob is larger than 32 MiB".into()));
        }
        Ok(BASE64.encode(vault.blobs().read(hash)?))
    })
}
