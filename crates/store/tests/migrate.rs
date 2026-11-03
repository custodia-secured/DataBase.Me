// SPDX-License-Identifier: AGPL-3.0-or-later
//! Migration 2 → 3 (M5): a vault made before the files folder existed opens, gains the four
//! bookkeeping tables, and verifies as before.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use dbme_core::{Clock, ManualClock};
use dbme_store::{Identity, NewObject, SCHEMA_VERSION, Vault, VaultOptions};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vaults/v2.dbme");

fn options() -> VaultOptions {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_100_000_000));
    VaultOptions {
        identity: Identity::generate(clock.as_ref()).unwrap(),
        clock,
    }
}

/// A working copy of the fixture: the repository's copy is never opened, since opening migrates.
fn copy_fixture(to: &Path) {
    fs::create_dir_all(to.join("blobs")).unwrap();
    for name in ["vault.sqlite", "FORMAT.md"] {
        fs::copy(Path::new(FIXTURE).join(name), to.join(name)).unwrap();
    }
}

#[test]
fn a_schema_version_2_vault_opens_migrates_and_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("V2.dbme");
    copy_fixture(&path);

    let mut vault = Vault::open(&path, options()).unwrap();
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
    assert_eq!(report.schema_version, SCHEMA_VERSION);
    assert_eq!(report.schema_version, 4);
    assert_eq!(
        report.ops, 25,
        "the built-in types, and nothing from the migration"
    );

    // The new tables are there and empty.
    assert!(vault.mirror_states(None).unwrap().is_empty());
    assert!(
        vault
            .read()
            .provenance_of_run(dbme_core::Id::from_parts(1, 1))
            .unwrap()
            .is_empty(),
        "migration 4's table"
    );
    assert_eq!(vault.mirror_dirty_count().unwrap(), 0);
    assert!(vault.conflicts(false).unwrap().is_empty());
    assert_eq!(
        vault.mirror_meta_get(dbme_store::FOLDER_ENABLED).unwrap(),
        None
    );

    // The migrated vault takes changes, and they mark the files folder's dirty table.
    vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "After the migration".into(),
                ..NewObject::default()
            })
        })
        .unwrap();
    assert_eq!(vault.mirror_dirty_count().unwrap(), 1);
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
    assert_eq!(report.ops, 26);

    // Opening it again finds it already migrated.
    drop(vault);
    let vault = Vault::open(&path, options()).unwrap();
    assert_eq!(vault.verify(false).unwrap().schema_version, 4);
}
