// SPDX-License-Identifier: AGPL-3.0-or-later
//! Killing the process during a write leaves a valid vault (M1 acceptance).
//!
//! The test runs its own test binary again as a writer process, which changes a vault as fast as
//! it can: objects, edits, moves, trash and permanent deletes, undos, and blobs with attachments.
//! The test kills the writer at a random moment, then opens the vault and requires a clean
//! deep verification: SQLite's checks, the hash chain, a full replay, and every blob. It does this
//! ten times on the same vault.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use dbme_core::SystemClock;
use dbme_core::model::AttachmentRole;
use dbme_store::{NewAttachment, NewObject, Position, StoreError, Vault, VaultOptions};

const VAULT_ENV: &str = "DBME_CRASH_TEST_VAULT";
const READY: &str = "writer ready";
const ROUNDS: usize = 10;

fn options() -> VaultOptions {
    VaultOptions {
        identity: common::identity(),
        clock: Arc::new(SystemClock),
    }
}

fn random_below(limit: u64) -> u64 {
    RandomState::new().hash_one(std::time::Instant::now()) % limit
}

/// Opens the vault, retrying for a while: the operating system can take a moment to release a
/// killed process's lock and file handles, especially on Windows.
fn open_after_kill(path: &Path) -> Vault {
    let mut attempts = 0;
    loop {
        match Vault::open(path, options()) {
            Ok(vault) => return vault,
            Err(StoreError::Locked(_) | StoreError::Sql(_) | StoreError::Io { .. })
                if attempts < 100 =>
            {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("cannot open the vault after a crash: {error}"),
        }
    }
}

#[test]
fn killing_a_writer_at_random_moments_leaves_a_valid_vault() {
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("Crash.dbme");
    drop(Vault::create(&path, options()).unwrap());
    let exe = std::env::current_exe().unwrap();
    let mut objects = Vec::new();
    for round in 0..ROUNDS {
        let mut child = Command::new(&exe)
            .args([
                "--exact",
                "writer",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(VAULT_ENV, &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = lines
            .by_ref()
            .map_while(Result::ok)
            .any(|line| line.contains(READY));
        assert!(ready, "round {round}: the writer did not start");
        std::thread::sleep(Duration::from_millis(20 + random_below(180)));
        child.kill().unwrap();
        child.wait().unwrap();

        let vault = open_after_kill(&path);
        let report = vault.verify(true).unwrap();
        assert!(report.is_ok(), "round {round}: {:#?}", report.problems);
        objects.push(report.objects);
    }
    eprintln!("objects after each kill: {objects:?}");
    assert!(
        objects.last() > objects.first(),
        "the writer made progress: {objects:?}"
    );
}

/// The writer process. It never finishes on its own; the test above kills it.
#[test]
#[ignore = "run by killing_a_writer_at_random_moments_leaves_a_valid_vault"]
fn writer() {
    let Some(path) = std::env::var_os(VAULT_ENV) else {
        return;
    };
    let mut vault = Vault::open(Path::new(&path), options()).unwrap();
    println!("{READY}");
    let mut n: u64 = 0;
    loop {
        n += 1;
        let blob = vault
            .blobs()
            .put(format!("blob {n} {}", random_below(u64::MAX)).as_bytes())
            .unwrap();
        let result = vault.transact(|txn| {
            let parent = txn.read().children(None)?.last().map(|o| o.id);
            let id = txn.create_object(NewObject {
                title: format!("object {n}"),
                parent_id: parent.filter(|_| n.is_multiple_of(3)),
                position: if n.is_multiple_of(2) {
                    Position::First
                } else {
                    Position::Last
                },
                ..Default::default()
            })?;
            txn.update_object(id, |o| {
                o.summary = Some("x".repeat(usize::try_from(n % 500).unwrap()))
            })?;
            txn.attach(NewAttachment {
                object_id: id,
                blob_hash: blob.hash.clone(),
                role: AttachmentRole::Original,
                filename: Some("blob.bin".into()),
                mime: None,
            })?;
            if n.is_multiple_of(5) {
                txn.trash_object(id)?;
                txn.delete_object(id)?;
            }
            Ok(())
        });
        result.unwrap();
        if n.is_multiple_of(7) {
            vault.undo().unwrap();
        }
    }
}
