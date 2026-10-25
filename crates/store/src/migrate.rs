// SPDX-License-Identifier: AGPL-3.0-or-later
//! Connection settings and schema migrations (ADR-0015).
//!
//! The schema changes only through the numbered migrations below, applied in order, each in its
//! own transaction together with the `user_version` it brings the file to. Migrations are never
//! edited once released; a fix is a new migration.

use std::path::Path;

use rusqlite::Connection;

use crate::{Result, StoreError};

/// `PRAGMA application_id` of every vault: the ASCII bytes `DBME`.
pub const APPLICATION_ID: i32 = 0x4442_4D45;

/// The migrations, oldest first. Migration `n` (counting from 1) brings `user_version` to `n`.
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_core.sql")];

/// The schema version this build writes: the number of the last migration.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Settings every connection to a vault uses: write-ahead logging, `synchronous = NORMAL` (a crash
/// can never corrupt the vault; a power cut can lose the last moments of work), enforced foreign
/// keys, and a wait instead of an immediate error when another connection holds a lock.
pub(crate) fn configure(conn: &Connection) -> Result<()> {
    // The store uses a few dozen distinct statements; keep them all prepared.
    conn.set_prepared_statement_cache_capacity(128);
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // 32 MiB of page cache (negative: KiB), so a bulk transaction of 10,000 changes is not
    // spilled to the WAL before it commits; 5% faster in the M1 budget test (ADR-0015).
    conn.pragma_update(None, "cache_size", -32_768)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn application_id(conn: &Connection) -> Result<i32> {
    Ok(conn.pragma_query_value(None, "application_id", |row| row.get(0))?)
}

/// The schema version of an open vault.
pub(crate) fn user_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Checks that `conn` is a vault this build may write, then applies any missing migrations. An
/// empty database becomes a new vault.
pub(crate) fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let app_id = application_id(conn)?;
    let version = user_version(conn)?;
    if app_id == 0 && version == 0 {
        let tables: i64 =
            conn.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))?;
        if tables != 0 {
            return Err(StoreError::NotAVault(path.to_owned()));
        }
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    } else if app_id != APPLICATION_ID {
        return Err(StoreError::NotAVault(path.to_owned()));
    }
    check_version(version)?;
    for (number, sql) in (1..).zip(MIGRATIONS).skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", number)?;
        tx.commit()?;
    }
    Ok(())
}

/// Refuses vaults written by a newer build.
pub(crate) fn check_version(version: u32) -> Result<()> {
    if version > SCHEMA_VERSION {
        return Err(StoreError::TooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}
