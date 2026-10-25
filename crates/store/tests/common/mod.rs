// SPDX-License-Identifier: AGPL-3.0-or-later
//! Helpers shared by the store's integration tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbme_core::{Id, ManualClock};
use dbme_store::{Identity, Vault, VaultOptions};

/// 2026-10-25T08:21:11Z.
pub const START: i64 = 1_761_380_471_000;

pub struct TestVault {
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
    pub clock: Arc<ManualClock>,
    pub vault: Vault,
}

pub fn identity() -> Identity {
    Identity {
        actor_id: Id::from_parts(1, 1),
        device_id: Id::from_parts(1, 2),
    }
}

pub fn options(clock: &Arc<ManualClock>) -> VaultOptions {
    VaultOptions {
        identity: identity(),
        clock: clock.clone(),
    }
}

pub fn new_vault() -> TestVault {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Test.dbme");
    let clock = Arc::new(ManualClock::new(START));
    let vault = Vault::create(&path, options(&clock)).unwrap();
    TestVault {
        dir,
        path,
        clock,
        vault,
    }
}

impl TestVault {
    /// Moves the clock on, so the next transaction has a later time.
    pub fn tick(&self) {
        self.clock.advance(1);
    }

    pub fn reopen(self) -> TestVault {
        let TestVault {
            dir,
            path,
            clock,
            vault,
        } = self;
        drop(vault);
        let vault = Vault::open(&path, options(&clock)).unwrap();
        TestVault {
            dir,
            path,
            clock,
            vault,
        }
    }

    pub fn assert_verifies(&self) {
        let report = self.vault.verify(true).unwrap();
        assert!(report.is_ok(), "{:#?}", report.problems);
    }

    /// A second, raw connection to the vault's database, standing in for another writer.
    pub fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.path.join(dbme_store::DATABASE_FILE)).unwrap()
    }
}

/// Every row of the tables ops produce, as text, for comparing whole vault states.
pub fn snapshot(path: &Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path.join(dbme_store::DATABASE_FILE)).unwrap();
    let mut out = Vec::new();
    for (table, order) in [
        ("types", "id"),
        ("fields", "id"),
        ("objects", "id"),
        ("object_types", "object_id, type_id"),
        ("blocks", "id"),
        ("relations", "id"),
        ("attachments", "object_id, blob_hash, role"),
    ] {
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let names: Vec<String> = statement.column_names().map(str::to_owned).collect();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let mut line = format!("{table}:");
            for (column, name) in names.iter().enumerate() {
                // updated_at moves forward on undo; it is not part of the state being compared.
                if name == "updated_at" {
                    continue;
                }
                let value: rusqlite::types::Value = row.get(column).unwrap();
                line.push_str(&format!(" {value:?}"));
            }
            out.push(line);
        }
    }
    out
}
