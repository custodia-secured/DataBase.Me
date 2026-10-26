// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme-api`: what the UI can ask of the open vault, as plain Rust functions over a [`Session`].
//!
//! The desktop shell wraps each function in a Tauri command (ADR-0014); the e2e test server calls
//! the same functions over HTTP (ADR-0018). Neither adds behaviour of its own, so the end-to-end
//! tests exercise what the app runs. Every function returns a [`Reply`]: the value, or an
//! [`IpcError`] the UI can act on. After every committed change the session's notifier receives
//! the [`Committed`] record, which the shell sends to the UI as `vault-changed`.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::model::{AttachmentRole, AttachmentRow, BlockRow, FieldRow, ObjectRow, TypeRow};
use dbme_core::{ContentHash, CoreError, Id};
use dbme_store::{
    Backlink, Committed, NewAttachment, NewField, NewObject, ObjectSummary, Position, Sort, Source,
    StoreError, Txn, Vault, VaultOptions,
};

mod blocks;

pub use blocks::BlockChange;

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

/// The vault open in one window, if any, and who to tell about its changes.
pub struct Session {
    vault: Mutex<Option<Vault>>,
    notify: Notify,
}

impl Session {
    /// A session with no vault open. `notify` receives every committed change.
    pub fn new(notify: impl Fn(&Committed) + Send + Sync + 'static) -> Self {
        Self {
            vault: Mutex::new(None),
            notify: Box::new(notify),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Vault>> {
        // A panic while the lock was held cannot leave the vault half-changed: SQLite rolls
        // back the unfinished transaction. So a poisoned lock is safe to use.
        self.vault
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Closes the open vault, if any.
    pub fn close(&self) {
        *self.lock() = None;
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

    /// Runs a change and tells the notifier what changed.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut Vault) -> Result<(T, Option<Committed>), StoreError>,
    ) -> Reply<T> {
        let (value, committed) = self.with(change)?;
        if let Some(committed) = committed {
            (self.notify)(&committed);
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
    // Close the old vault first: it may be the one being opened again.
    *guard = None;
    let vault = open()?;
    let info = info(&vault);
    // The ID only: paths can name people and projects (logs never hold user content).
    tracing::info!(vault = %info.id, "vault open");
    *guard = Some(vault);
    Ok(info)
}

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorKind::Invalid, message)
}

// The vault.

/// Creates a vault at `path` (a folder named `<name>.dbme`) and opens it.
pub fn vault_create(session: &Session, path: &str, options: VaultOptions) -> Reply<VaultInfo> {
    replace(session, || Vault::create(Path::new(path), options))
}

/// Opens the vault at `path`.
pub fn vault_open(session: &Session, path: &str, options: VaultOptions) -> Reply<VaultInfo> {
    replace(session, || Vault::open(Path::new(path), options))
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
