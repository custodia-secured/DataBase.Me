// SPDX-License-Identifier: AGPL-3.0-or-later
//! The files folder's bookkeeping (migration 3, M5, ADR-0021): which file holds which object, what
//! was last synced, which objects need writing again, and the Conflicts inbox.
//!
//! None of this goes through the op log: it describes files on disk, not the vault's content, and
//! a rescan of the folder can rebuild it. Every function exists on [`Vault`] (its own SQLite
//! transaction) and on [`Txn`] (inside the transaction, so the mirror can record what it synced in
//! the same commit as the change itself).

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::{ContentHash, Id};

use crate::sql::{self, optional};
use crate::txn::Txn;
use crate::vault::Vault;
use crate::{Result, StoreError};

/// `mirror_meta` key: present (`"1"`) when every file must be written again, because a type or a
/// field changed.
pub const FULL_REWRITE_NEEDED: &str = "full_rewrite_needed";
/// `mirror_meta` key: `"1"` while the files folder is enabled for this vault.
pub const FOLDER_ENABLED: &str = "folder_enabled";
/// `mirror_meta` key: when the folder was last rescanned, in milliseconds since the Unix epoch.
pub const LAST_RESCAN_AT: &str = "last_rescan_at";

/// Where an object's file stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MirrorFileState {
    /// The file holds what the vault holds.
    Synced,
    /// The vault changed and the file has not been written yet, or a write is being retried.
    Pending,
    /// The file could not be read (a parse error) and the object's writes wait on the inbox.
    Held,
    /// The object is not written to the folder (a sensitive object, by default).
    Excluded,
    /// The object is in the Trash and its file under `.dbme/trash/`.
    Trashed,
}

impl MirrorFileState {
    /// The name stored in the `state` column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Pending => "pending",
            Self::Held => "held",
            Self::Excluded => "excluded",
            Self::Trashed => "trashed",
        }
    }

    /// The state with this name.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "synced" => Self::Synced,
            "pending" => Self::Pending,
            "held" => Self::Held,
            "excluded" => Self::Excluded,
            "trashed" => Self::Trashed,
            _ => return None,
        })
    }
}

/// One row of `mirror_state`: an object's file and what was last synced to it.
#[allow(missing_docs, reason = "the columns of `mirror_state`")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MirrorState {
    pub object_id: Id,
    /// Relative to the files folder, with forward slashes.
    pub rel_path: String,
    /// `dbme_core::files::paths::path_key` of `rel_path`: NFC, case-folded, one slash form.
    pub path_key: String,
    /// The hash of the bytes last written or read.
    pub last_hash: Option<ContentHash>,
    /// The Markdown last synced: the base of the next three-way merge.
    pub base_text: Option<String>,
    /// The `seq` of the last op the file reflects.
    pub synced_seq: Option<i64>,
    pub synced_at: Option<i64>,
    pub file_size: Option<i64>,
    pub file_mtime_ms: Option<i64>,
    pub state: MirrorFileState,
}

/// What kind of thing the person must resolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ConflictKind {
    /// The app and the file changed the same block or field, and the texts do not merge.
    EditEdit,
    /// One side edited an object the other side deleted.
    EditDelete,
    /// The file could not be parsed; its object's writes are held.
    ParseError,
    /// A value in the file does not fit its field's kind.
    InvalidValue,
    /// Two live files carry the same `id`.
    DuplicateId,
    /// A file in the folder could not be taken in as an object.
    DroppedFile,
    /// Too many files changed at once; nothing was applied.
    BulkChangeHold,
    /// A file the app knows by path has lost its `id` (informational).
    MissingId,
    /// A cloud drive left a conflict copy next to a file.
    CloudConflictFile,
}

impl ConflictKind {
    /// The name stored in the `kind` column.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EditEdit => "edit_edit",
            Self::EditDelete => "edit_delete",
            Self::ParseError => "parse_error",
            Self::InvalidValue => "invalid_value",
            Self::DuplicateId => "duplicate_id",
            Self::DroppedFile => "dropped_file",
            Self::BulkChangeHold => "bulk_change_hold",
            Self::MissingId => "missing_id",
            Self::CloudConflictFile => "cloud_conflict_file",
        }
    }

    /// The kind with this name.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "edit_edit" => Self::EditEdit,
            "edit_delete" => Self::EditDelete,
            "parse_error" => Self::ParseError,
            "invalid_value" => Self::InvalidValue,
            "duplicate_id" => Self::DuplicateId,
            "dropped_file" => Self::DroppedFile,
            "bulk_change_hold" => Self::BulkChangeHold,
            "missing_id" => Self::MissingId,
            "cloud_conflict_file" => Self::CloudConflictFile,
            _ => return None,
        })
    }
}

/// One row of `conflicts`: something in the inbox.
#[allow(missing_docs, reason = "the columns of `conflicts`")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Conflict {
    pub id: Id,
    pub object_id: Option<Id>,
    pub rel_path: Option<String>,
    pub kind: ConflictKind,
    /// What the inbox shows: the kind decides the shape.
    pub details: Value,
    /// The Note that holds the file's version, for an `edit_edit`.
    pub conflict_copy_id: Option<Id>,
    /// How it was resolved, once it is.
    pub resolution: Option<String>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

/// A conflict to add to the inbox.
#[allow(missing_docs, reason = "the columns of `conflicts` the caller sets")]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewConflict {
    pub object_id: Option<Id>,
    pub rel_path: Option<String>,
    pub kind: Option<ConflictKind>,
    pub details: Value,
    pub conflict_copy_id: Option<Id>,
}

const STATE_COLUMNS: &str = "object_id, rel_path, path_key, last_hash, base_text, synced_seq, \
                             synced_at, file_size, file_mtime_ms, state";
const CONFLICT_COLUMNS: &str = "id, object_id, rel_path, kind, details_json, conflict_copy_id, \
                                resolution, created_at, resolved_at";

fn state_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<MirrorState> {
    Ok(MirrorState {
        object_id: sql::id(r, 0)?,
        rel_path: r.get(1)?,
        path_key: r.get(2)?,
        last_hash: sql::optional_hash(r, 3)?,
        base_text: r.get(4)?,
        synced_seq: r.get(5)?,
        synced_at: r.get(6)?,
        file_size: r.get(7)?,
        file_mtime_ms: r.get(8)?,
        state: sql::name(r, 9, MirrorFileState::parse)?,
    })
}

fn conflict_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Conflict> {
    Ok(Conflict {
        id: sql::id(r, 0)?,
        object_id: sql::optional_id(r, 1)?,
        rel_path: r.get(2)?,
        kind: sql::name(r, 3, ConflictKind::parse)?,
        details: sql::json(r, 4)?,
        conflict_copy_id: sql::optional_id(r, 5)?,
        resolution: r.get(6)?,
        created_at: r.get(7)?,
        resolved_at: r.get(8)?,
    })
}

fn collect<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    map: fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut statement = conn.prepare_cached(sql)?;
    let rows = statement.query_map(params, map)?;
    Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
}

// The functions over a connection. The wrappers below are the public surface.

pub(crate) fn state(conn: &Connection, object_id: Id) -> Result<Option<MirrorState>> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {STATE_COLUMNS} FROM mirror_state WHERE object_id = ?1"
        ))?
        .query_row([object_id.to_string()], state_row)
        .optional()?)
}

pub(crate) fn state_by_path_key(conn: &Connection, path_key: &str) -> Result<Option<MirrorState>> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {STATE_COLUMNS} FROM mirror_state WHERE path_key = ?1"
        ))?
        .query_row([path_key], state_row)
        .optional()?)
}

pub(crate) fn states(conn: &Connection, only: Option<MirrorFileState>) -> Result<Vec<MirrorState>> {
    collect(
        conn,
        &format!(
            "SELECT {STATE_COLUMNS} FROM mirror_state WHERE ?1 IS NULL OR state = ?1 \
             ORDER BY rel_path"
        ),
        [only.map(MirrorFileState::as_str)],
        state_row,
    )
}

pub(crate) fn set_state(conn: &Connection, row: &MirrorState) -> Result<()> {
    for (column, value) in [("rel_path", &row.rel_path), ("path_key", &row.path_key)] {
        let taken: Option<String> = conn
            .prepare_cached(&format!(
                "SELECT object_id FROM mirror_state WHERE {column} = ?1 AND object_id != ?2"
            ))?
            .query_row(params![value, row.object_id.to_string()], |r| r.get(0))
            .optional()?;
        if let Some(other) = taken {
            return Err(StoreError::Rule(format!(
                "the path {:?} already belongs to object {other}",
                row.rel_path
            )));
        }
    }
    conn.prepare_cached(&format!(
        "INSERT INTO mirror_state ({STATE_COLUMNS}) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT (object_id) DO UPDATE SET rel_path = excluded.rel_path, \
         path_key = excluded.path_key, last_hash = excluded.last_hash, \
         base_text = excluded.base_text, synced_seq = excluded.synced_seq, \
         synced_at = excluded.synced_at, file_size = excluded.file_size, \
         file_mtime_ms = excluded.file_mtime_ms, state = excluded.state"
    ))?
    .execute(params![
        row.object_id.to_string(),
        row.rel_path,
        row.path_key,
        row.last_hash.as_ref().map(ContentHash::as_str),
        row.base_text,
        row.synced_seq,
        row.synced_at,
        row.file_size,
        row.file_mtime_ms,
        row.state.as_str(),
    ])?;
    Ok(())
}

pub(crate) fn remove_state(conn: &Connection, object_id: Id) -> Result<bool> {
    Ok(conn
        .prepare_cached("DELETE FROM mirror_state WHERE object_id = ?1")?
        .execute([object_id.to_string()])?
        > 0)
}

pub(crate) fn mark_dirty<'a>(
    conn: &Connection,
    ids: impl IntoIterator<Item = &'a Id>,
) -> Result<()> {
    let mut statement =
        conn.prepare_cached("INSERT OR IGNORE INTO mirror_dirty (object_id) VALUES (?1)")?;
    for id in ids {
        statement.execute([id.to_string()])?;
    }
    Ok(())
}

/// The objects whose files change path or content when the objects in `moved` change title or
/// place: `moved` themselves, everything below them (their paths contain the parent's), and the
/// source of every relation pointing at any of those (its wikilink names the path). One query for
/// the whole set, so a bulk edit of 10,000 titles costs one walk, not 10,000 (ADR-0021).
pub(crate) fn below_and_linking<'a>(
    conn: &Connection,
    moved: impl IntoIterator<Item = &'a Id>,
) -> Result<Vec<Id>> {
    let seeds: Vec<String> = moved.into_iter().map(ToString::to_string).collect();
    collect(
        conn,
        "WITH RECURSIVE moved(id) AS (\
            SELECT value FROM json_each(?1) \
            UNION SELECT objects.id FROM objects JOIN moved ON objects.parent_id = moved.id) \
         SELECT id FROM moved \
         UNION SELECT src_id FROM relations WHERE dst_id IN (SELECT id FROM moved)",
        [sql::to_json(&seeds)?],
        |r| sql::id(r, 0),
    )
}

pub(crate) fn take_dirty(conn: &Connection) -> Result<Vec<Id>> {
    let mut ids = collect(conn, "SELECT object_id FROM mirror_dirty", [], |r| {
        sql::id(r, 0)
    })?;
    conn.execute_batch("DELETE FROM mirror_dirty")?;
    ids.sort_unstable();
    Ok(ids)
}

pub(crate) fn dirty_count(conn: &Connection) -> Result<u64> {
    crate::query::count(conn, "SELECT count(*) FROM mirror_dirty", [])
}

pub(crate) fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .prepare_cached("SELECT value FROM mirror_meta WHERE key = ?1")?
        .query_row([key], |r| r.get(0))
        .optional()?)
}

pub(crate) fn meta_set(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => conn
            .prepare_cached(
                "INSERT INTO mirror_meta (key, value) VALUES (?1, ?2) \
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            )?
            .execute([key, value])?,
        None => conn
            .prepare_cached("DELETE FROM mirror_meta WHERE key = ?1")?
            .execute([key])?,
    };
    Ok(())
}

pub(crate) fn add_conflict(conn: &Connection, id: Id, now: i64, new: &NewConflict) -> Result<Id> {
    let kind = new
        .kind
        .ok_or_else(|| StoreError::Rule("a conflict needs a kind".to_owned()))?;
    conn.prepare_cached(&format!(
        "INSERT INTO conflicts ({CONFLICT_COLUMNS}) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL)"
    ))?
    .execute(params![
        id.to_string(),
        optional(new.object_id),
        new.rel_path,
        kind.as_str(),
        sql::to_json(&new.details)?,
        optional(new.conflict_copy_id),
        now,
    ])?;
    Ok(id)
}

pub(crate) fn conflicts(conn: &Connection, open_only: bool) -> Result<Vec<Conflict>> {
    collect(
        conn,
        &format!(
            "SELECT {CONFLICT_COLUMNS} FROM conflicts \
             WHERE NOT ?1 OR resolved_at IS NULL ORDER BY created_at, id"
        ),
        [open_only],
        conflict_row,
    )
}

pub(crate) fn conflict(conn: &Connection, id: Id) -> Result<Conflict> {
    conn.prepare_cached(&format!(
        "SELECT {CONFLICT_COLUMNS} FROM conflicts WHERE id = ?1"
    ))?
    .query_row([id.to_string()], conflict_row)
    .optional()?
    .ok_or_else(|| StoreError::not_found("conflict", id))
}

pub(crate) fn resolve_conflict(
    conn: &Connection,
    id: Id,
    now: i64,
    resolution: &str,
) -> Result<bool> {
    conflict(conn, id)?;
    Ok(conn
        .prepare_cached(
            "UPDATE conflicts SET resolution = ?2, resolved_at = ?3 \
             WHERE id = ?1 AND resolved_at IS NULL",
        )?
        .execute(params![id.to_string(), resolution, now])?
        > 0)
}

pub(crate) fn reset(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM mirror_state; DELETE FROM mirror_dirty; \
         DELETE FROM mirror_meta; DELETE FROM conflicts",
    )?;
    Ok(())
}

macro_rules! wrappers {
    ($conn:expr, $self_:ident, $now:expr, $new_id:expr) => {
        /// An object's file, if the folder holds or held one.
        pub fn mirror_state(&$self_, object_id: Id) -> Result<Option<MirrorState>> {
            state($conn, object_id)
        }

        /// The object whose file has this path key.
        pub fn mirror_state_by_path_key(&$self_, path_key: &str) -> Result<Option<MirrorState>> {
            state_by_path_key($conn, path_key)
        }

        /// Every object's file, or those in one state, by path.
        pub fn mirror_states(&$self_, only: Option<MirrorFileState>) -> Result<Vec<MirrorState>> {
            states($conn, only)
        }

        /// Records an object's file, replacing what was recorded. A path another object's file
        /// has is refused.
        pub fn mirror_state_set(&mut $self_, row: &MirrorState) -> Result<()> {
            set_state($conn, row)
        }

        /// Forgets an object's file. Returns `false` if none was recorded.
        pub fn mirror_state_remove(&mut $self_, object_id: Id) -> Result<bool> {
            remove_state($conn, object_id)
        }

        /// The objects whose files must be written again, sorted, and clears the list.
        pub fn mirror_dirty_take(&mut $self_) -> Result<Vec<Id>> {
            take_dirty($conn)
        }

        /// Marks objects whose files must be written again.
        pub fn mirror_dirty_mark(&mut $self_, ids: &[Id]) -> Result<()> {
            mark_dirty($conn, ids)
        }

        /// How many objects wait to be written.
        pub fn mirror_dirty_count(&$self_) -> Result<u64> {
            dirty_count($conn)
        }

        /// A fact about the folder, by key.
        pub fn mirror_meta_get(&$self_, key: &str) -> Result<Option<String>> {
            meta_get($conn, key)
        }

        /// Sets a fact about the folder; `None` removes it.
        pub fn mirror_meta_set(&mut $self_, key: &str, value: Option<&str>) -> Result<()> {
            meta_set($conn, key, value)
        }

        /// Puts something in the Conflicts inbox.
        pub fn conflict_add(&mut $self_, new: NewConflict) -> Result<Id> {
            let id = $new_id;
            add_conflict($conn, id, $now, &new)
        }

        /// The inbox, oldest first: everything, or only what is still open.
        pub fn conflicts(&$self_, open_only: bool) -> Result<Vec<Conflict>> {
            conflicts($conn, open_only)
        }

        /// One inbox entry.
        pub fn conflict(&$self_, id: Id) -> Result<Conflict> {
            conflict($conn, id)
        }

        /// Marks an inbox entry resolved. Returns `false` if it already was.
        pub fn conflict_resolve(&mut $self_, id: Id, resolution: &str) -> Result<bool> {
            resolve_conflict($conn, id, $now, resolution)
        }

        /// Empties the four bookkeeping tables: the folder is disabled or about to be rebuilt.
        pub fn mirror_reset(&mut $self_) -> Result<()> {
            reset($conn)
        }
    };
}

impl Vault {
    wrappers!(self.connection(), self, self.now_ms(), self.next_id()?);
}

impl Txn<'_> {
    wrappers!(self.conn(), self, self.now(), self.new_id()?);
}
