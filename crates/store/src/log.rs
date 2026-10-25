// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `ops` table: appending to the hash chain and reading it back (FORMAT.md §5).

use rusqlite::{Connection, OptionalExtension, params};

use dbme_core::ContentHash;
use dbme_core::chain::{OpEntry, OpRecord};

use crate::Result;
use crate::sql::{self, optional};

macro_rules! op_columns {
    () => {
        "seq, op_id, group_id, undo_of, actor_id, device_id, ts, kind, target_id, payload_json, prev_hash, hash, signature"
    };
}
pub(crate) use op_columns;

/// The hash of the last op, or `None` if the log is empty.
pub(crate) fn head(conn: &Connection) -> Result<Option<ContentHash>> {
    Ok(conn
        .prepare_cached("SELECT hash FROM ops ORDER BY seq DESC LIMIT 1")?
        .query_row([], |r| sql::hash(r, 0))
        .optional()?)
}

/// Appends `entry` after the op whose hash is `prev`. Returns the new op's `seq` and hash.
pub(crate) fn append(
    conn: &Connection,
    entry: &OpEntry,
    prev: Option<&ContentHash>,
) -> Result<(i64, ContentHash)> {
    let hash = entry.hash(prev);
    conn.prepare_cached(
        "INSERT INTO ops (op_id, group_id, undo_of, actor_id, device_id, ts, kind, target_id, payload_json, \
         prev_hash, hash, signature) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)",
    )?
    .execute(params![
        entry.op_id.to_string(),
        entry.group_id.to_string(),
        optional(entry.undo_of),
        entry.actor_id.to_string(),
        entry.device_id.to_string(),
        entry.ts,
        entry.kind,
        entry.target_id.to_string(),
        entry.payload_json,
        prev.map(ContentHash::as_str),
        hash.as_str(),
    ])?;
    Ok((conn.last_insert_rowid(), hash))
}

pub(crate) fn record(r: &rusqlite::Row<'_>) -> rusqlite::Result<OpRecord> {
    Ok(OpRecord {
        seq: r.get(0)?,
        entry: OpEntry {
            op_id: sql::id(r, 1)?,
            group_id: sql::id(r, 2)?,
            undo_of: sql::optional_id(r, 3)?,
            actor_id: sql::id(r, 4)?,
            device_id: sql::id(r, 5)?,
            ts: r.get(6)?,
            kind: r.get(7)?,
            target_id: sql::id(r, 8)?,
            payload_json: r.get(9)?,
        },
        prev_hash: sql::optional_hash(r, 10)?,
        hash: sql::hash(r, 11)?,
        signature: r.get(12)?,
    })
}

/// Calls `each` for every op with `seq` from `first` to `last`, in order, or in reverse order.
pub(crate) fn for_each(
    conn: &Connection,
    first: i64,
    last: i64,
    reverse: bool,
    mut each: impl FnMut(OpRecord) -> Result<()>,
) -> Result<()> {
    let sql = if reverse {
        concat!(
            "SELECT ",
            op_columns!(),
            " FROM ops WHERE seq BETWEEN ?1 AND ?2 ORDER BY seq DESC"
        )
    } else {
        concat!(
            "SELECT ",
            op_columns!(),
            " FROM ops WHERE seq BETWEEN ?1 AND ?2 ORDER BY seq"
        )
    };
    let mut statement = conn.prepare_cached(sql)?;
    let mut rows = statement.query([first, last])?;
    while let Some(row) = rows.next()? {
        each(record(row)?)?;
    }
    Ok(())
}
