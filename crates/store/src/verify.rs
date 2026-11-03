// SPDX-License-Identifier: AGPL-3.0-or-later
//! Checking a vault from end to end.
//!
//! [`Vault::verify`] runs SQLite's integrity and foreign-key checks, checks the op log's hash
//! chain, replays the whole log into an empty in-memory database and compares every table with
//! the vault's, and checks that every blob the database records is in the blob store with the
//! right size (and, in a deep check, the right content).

use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;

use dbme_core::ContentHash;
use dbme_core::chain::ChainVerifier;

use crate::blobs::BlobStore;
use crate::log::{self, op_columns};
use crate::query::count;
use crate::{Result, Vault, migrate, rows, search, sql};

/// The tables ops produce, with the columns that order their rows.
const TABLES: [(&str, &str); 10] = [
    ("types", "id"),
    ("fields", "id"),
    ("objects", "id"),
    ("object_types", "object_id, type_id"),
    ("blocks", "id"),
    ("relations", "id"),
    ("blobs", "hash"),
    ("attachments", "object_id, blob_hash, role"),
    ("views", "id"),
    ("provenance", "id"),
];

/// What a check found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VerifyReport {
    /// The vault's schema version.
    pub schema_version: u32,
    /// How many ops passed the chain check.
    pub ops: u64,
    /// The hash of the last op that passed: the head of the chain.
    pub head: Option<ContentHash>,
    /// How many objects the vault holds.
    pub objects: u64,
    /// How many blobs the database records.
    pub blobs: u64,
    /// Everything wrong, in the order found. Empty for a healthy vault.
    pub problems: Vec<String>,
}

impl VerifyReport {
    /// `true` if nothing is wrong.
    pub fn is_ok(&self) -> bool {
        self.problems.is_empty()
    }
}

impl Vault {
    /// Checks the vault. `deep` also reads every blob back to check its content against its hash.
    /// Problems are reported, not returned as errors; an error means the check itself could not
    /// run.
    pub fn verify(&self, deep: bool) -> Result<VerifyReport> {
        verify(self.connection(), self.blobs(), deep)
    }
}

fn verify(conn: &Connection, blobs: &BlobStore, deep: bool) -> Result<VerifyReport> {
    // Objects changed since the last search are not yet in the index, by design; a check of the
    // index starts by bringing it up to date.
    search::flush(conn)?;
    // One read transaction, so every check sees the same state.
    let snapshot = conn.unchecked_transaction()?;
    let mut report = VerifyReport {
        schema_version: migrate::user_version(&snapshot)?,
        ..VerifyReport::default()
    };

    let messages: Vec<String> = snapshot
        .prepare("PRAGMA integrity_check")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if messages != ["ok"] {
        report.problems.extend(
            messages
                .into_iter()
                .map(|m| format!("SQLite integrity check: {m}")),
        );
    }
    let broken: Vec<String> = snapshot
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |r| {
            let table: String = r.get(0)?;
            let rowid: Option<i64> = r.get(1)?;
            let parent: String = r.get(2)?;
            Ok(format!(
                "a row of {table} (rowid {rowid:?}) refers to a row of {parent} that does not exist"
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    report.problems.extend(broken);

    let mut replay = Connection::open_in_memory()?;
    migrate::migrate(&mut replay, Path::new(":memory:"))?;
    replay.pragma_update(None, "foreign_keys", true)?;
    let replayed = replay.transaction()?;
    let mut verifier = ChainVerifier::new();
    let mut complete = true;
    let mut statement =
        snapshot.prepare(concat!("SELECT ", op_columns!(), " FROM ops ORDER BY seq"))?;
    let mut records = statement.query([])?;
    while let Some(row) = records.next()? {
        let problem = match log::record(row) {
            Err(e) => Some(format!("op log: an op cannot be read: {e}")),
            Ok(record) => match verifier.check(&record) {
                Err(e) => Some(format!("op log: {e}")),
                Ok(()) => match record
                    .entry
                    .op()
                    .map_err(Into::into)
                    .and_then(|op| rows::apply(&replayed, &op, record.entry.ts))
                {
                    Err(e) => Some(format!("op log: op {} does not replay: {e}", record.seq)),
                    Ok(()) => None,
                },
            },
        };
        if let Some(problem) = problem {
            report.problems.push(problem);
            complete = false;
            break;
        }
    }
    report.ops = verifier.count();
    report.head = verifier.head().cloned();
    if complete {
        for (table, order) in TABLES {
            if let Some(problem) = compare(&snapshot, &replayed, table, order)? {
                report.problems.push(problem);
            }
        }
    }

    report.problems.extend(search_problems(&snapshot, deep)?);

    report.objects = count(&snapshot, "SELECT count(*) FROM objects", [])?;
    let mut statement = snapshot.prepare("SELECT hash, size FROM blobs ORDER BY hash")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        report.blobs += 1;
        let hash = match sql::hash(row, 0) {
            Ok(hash) => hash,
            Err(e) => {
                report
                    .problems
                    .push(format!("blobs: a hash cannot be read: {e}"));
                continue;
            }
        };
        let size: i64 = row.get(1)?;
        match blobs.size(&hash)? {
            None => report
                .problems
                .push(format!("blob {hash} is missing from the blob store")),
            Some(found) if i64::try_from(found).ok() != Some(size) => {
                report.problems.push(format!(
                    "blob {hash} has {found} bytes in the blob store, not {size}"
                ));
            }
            Some(_) if deep && !blobs.check(&hash)? => {
                report.problems.push(format!(
                    "blob {hash} is damaged: its content no longer has its hash"
                ));
            }
            Some(_) => {}
        }
    }
    Ok(report)
}

/// Checks the search index against the objects: every live object has a row and nothing else
/// does, and, in a deep check, every row holds the text the object has now.
fn search_problems(conn: &Connection, deep: bool) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let mut statement = conn.prepare(
        "SELECT o.id, st.object_id FROM objects o \
         LEFT JOIN search_text st ON st.object_id = o.id \
         WHERE (o.deleted_at IS NULL) != (st.object_id IS NOT NULL)",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let indexed: Option<String> = row.get(1)?;
        problems.push(match indexed {
            Some(_) => format!("search: object {id} is in the Trash but still indexed"),
            None => format!("search: live object {id} is not indexed"),
        });
    }
    let orphans = count(
        conn,
        "SELECT count(*) FROM search_text st WHERE NOT EXISTS (SELECT 1 FROM objects o WHERE o.id = st.object_id)",
        [],
    )?;
    if orphans > 0 {
        problems.push(format!("search: {orphans} indexed objects no longer exist"));
    }
    if deep {
        let mut statement = conn.prepare(concat!(
            "SELECT ",
            rows::object_columns!(),
            ", (SELECT title FROM search_text st WHERE st.object_id = o.id), \
             (SELECT body FROM search_text st WHERE st.object_id = o.id), \
             (SELECT props_text FROM search_text st WHERE st.object_id = o.id) \
             FROM objects o WHERE o.deleted_at IS NULL ORDER BY o.id"
        ))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let object = rows::object_row(row)?;
            // An object with no row at all was reported above.
            let Some(title) = row.get::<_, Option<String>>(15)? else {
                continue;
            };
            let stored = search::SearchText {
                title,
                body: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
                props_text: row.get::<_, Option<String>>(17)?.unwrap_or_default(),
            };
            if search::text_of(conn, &object)? != stored {
                problems.push(format!(
                    "search: the index holds old text for object {}",
                    object.id
                ));
            }
        }
    }
    Ok(problems)
}

/// Compares one table of the vault with the same table rebuilt from the op log.
fn compare(
    vault: &Connection,
    replayed: &Connection,
    table: &str,
    order: &str,
) -> Result<Option<String>> {
    let sql = format!("SELECT * FROM {table} ORDER BY {order}");
    let mut ours = vault.prepare(&sql)?;
    let mut theirs = replayed.prepare(&sql)?;
    let columns: Vec<String> = ours.column_names().map(str::to_owned).collect();
    let mut ours = ours.query([])?;
    let mut theirs = theirs.query([])?;
    let mut index = 0_u64;
    loop {
        match (ours.next()?, theirs.next()?) {
            (None, None) => return Ok(None),
            (Some(a), Some(b)) => {
                for (column, name) in columns.iter().enumerate() {
                    if a.get_ref(column)? != b.get_ref(column)? {
                        return Ok(Some(format!(
                            "{table}: row {index} does not match the op log (column {name})"
                        )));
                    }
                }
            }
            (Some(_), None) => {
                return Ok(Some(format!("{table} has rows the op log does not create")));
            }
            (None, Some(_)) => {
                return Ok(Some(format!("{table} is missing rows the op log creates")));
            }
        }
        index += 1;
    }
}
