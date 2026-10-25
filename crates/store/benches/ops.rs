// SPDX-License-Identifier: AGPL-3.0-or-later
//! Benchmarks of the paths every change takes: one change as its own transaction, a bulk
//! transaction, undo and redo, and a full verification.
//!
//! Run with `cargo bench -p dbme-store`.

#![allow(
    missing_docs,
    reason = "criterion's macros generate undocumented items"
)]
#![allow(clippy::unwrap_used, reason = "benchmarks may panic")]

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use dbme_core::{Id, ManualClock};
use dbme_store::{Identity, NewObject, Vault, VaultOptions};

fn new_vault() -> (tempfile::TempDir, Arc<ManualClock>, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(ManualClock::new(1_761_380_471_000));
    let identity = Identity {
        actor_id: Id::from_parts(1, 1),
        device_id: Id::from_parts(1, 2),
    };
    let vault = Vault::create(
        &dir.path().join("Bench.dbme"),
        VaultOptions {
            identity,
            clock: clock.clone(),
        },
    )
    .unwrap();
    (dir, clock, vault)
}

fn object(n: usize) -> NewObject {
    NewObject {
        title: format!("Object {n}"),
        ..Default::default()
    }
}

fn benches(c: &mut Criterion) {
    let (_dir, clock, mut vault) = new_vault();
    let mut n = 0;
    c.bench_function("create one object (own transaction)", |b| {
        b.iter(|| {
            n += 1;
            clock.advance(1);
            black_box(vault.transact(|txn| txn.create_object(object(n))).unwrap());
        });
    });
    let id = vault.read().children(None).unwrap()[0].id;
    c.bench_function("edit one object (own transaction)", |b| {
        b.iter(|| {
            n += 1;
            clock.advance(1);
            vault
                .transact(|txn| txn.update_object(id, |o| o.title = format!("Edit {n}")))
                .unwrap();
        });
    });
    c.bench_function("undo and redo one edit", |b| {
        b.iter(|| {
            clock.advance(1);
            vault.undo().unwrap();
            vault.redo().unwrap();
        });
    });

    c.bench_function("create 1,000 objects (one transaction)", |b| {
        b.iter_batched(
            new_vault,
            |(dir, clock, mut vault)| {
                clock.advance(1);
                vault
                    .transact(|txn| {
                        (0..1_000).try_for_each(|n| txn.create_object(object(n)).map(drop))
                    })
                    .unwrap();
                (dir, vault)
            },
            BatchSize::PerIteration,
        );
    });

    let (_dir, clock, mut vault) = new_vault();
    clock.advance(1);
    vault
        .transact(|txn| (0..1_000).try_for_each(|n| txn.create_object(object(n)).map(drop)))
        .unwrap();
    c.bench_function("verify a vault of 1,025 ops", |b| {
        b.iter(|| black_box(vault.verify(false).unwrap()))
    });
}

criterion_group!(store, benches);
criterion_main!(store);
