// SPDX-License-Identifier: AGPL-3.0-or-later
//! The M5 performance budget (brief §8): with 10,000 pages in the vault and the files folder on,
//! an edit in the app reaches its file, and an edit to a file reaches the app, each in under
//! 2 seconds, in a release build, on a real folder with the real watcher. The app→file figure is
//! one `pump_out` after the edit, as the worker runs it; the file→app figure runs from the write
//! on disk through the watcher's debounce and `pump_in` to the block holding the new text. The
//! first write of the 10,000 files is printed, not gated.
//!
//! Run with `cargo test --release -p dbme-mirror --test budget -- --nocapture`. Debug builds
//! skip the gate. CI gates it on Ubuntu and prints it on macOS (M5 Task 9).

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dbme_core::{Id, ManualClock};
use dbme_mirror::{Mirror, NotifyWatcher, StdFs};
use dbme_store::{Identity, NewBlock, NewObject, Position, Vault, VaultOptions};
use serde_json::json;

const OBJECTS: usize = 10_000;
const BUDGET: Duration = Duration::from_secs(2);
const DEBOUNCE: Duration = Duration::from_millis(300);

fn text_of(vault: &Vault, block: Id) -> String {
    vault.read().block(block).unwrap().content_json["inline"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

/// Feeds the watcher's events to the engine until `done` holds or `patience` has passed since
/// `started`.
fn until(
    watcher: &mut NotifyWatcher,
    mirror: &mut Mirror<StdFs>,
    vault: &mut Vault,
    started: Instant,
    patience: Duration,
    done: impl Fn(&Vault) -> bool,
) {
    while started.elapsed() < patience {
        let events = watcher.poll_timeout(Duration::from_millis(100));
        if !events.is_empty() {
            mirror.pump_in(vault, events).unwrap();
        }
        if done(vault) {
            break;
        }
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "the budget applies to release builds")]
fn an_edit_reaches_its_file_and_a_file_reaches_the_app_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("files");
    fs::create_dir_all(&root).unwrap();
    let clock = Arc::new(ManualClock::new(1_761_380_471_000));
    let mut vault = Vault::create(
        &dir.path().join("Budget.dbme"),
        VaultOptions {
            identity: Identity {
                actor_id: Id::from_parts(1, 1),
                device_id: Id::from_parts(1, 2),
            },
            clock: clock.clone(),
        },
    )
    .unwrap();
    let pages: Vec<(Id, Id)> = vault
        .transact(|txn| {
            (0..OBJECTS)
                .map(|n| {
                    let page = txn.create_object(NewObject {
                        title: format!("Page {n}"),
                        ..Default::default()
                    })?;
                    let paragraph = txn.create_block(NewBlock {
                        id: None,
                        object_id: page,
                        parent_block_id: None,
                        position: Position::Last,
                        kind: "paragraph".into(),
                        content: json!({"inline": [{"text": format!("text {n}")}]}),
                    })?;
                    Ok((page, paragraph))
                })
                .collect()
        })
        .unwrap()
        .0;

    let mut mirror = Mirror::new(StdFs::new(&root).unwrap(), clock.clone());
    let started = Instant::now();
    let out = mirror.pump_out(&mut vault).unwrap();
    assert_eq!(out.written.len(), OBJECTS, "{}", out.pending.len());
    eprintln!(
        "first write of {OBJECTS} files: {:?} (not gated)",
        started.elapsed()
    );

    // The watcher starts after the first write, as the app's does after enabling the folder.
    let mut watcher = NotifyWatcher::new(&root, DEBOUNCE).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let _ = watcher.poll_timeout(Duration::from_millis(100));

    // App → file.
    let (page, paragraph) = pages[OBJECTS / 2];
    let path = root.join(vault.mirror_state(page).unwrap().unwrap().rel_path);
    clock.advance(1);
    let started = Instant::now();
    vault
        .transact(|txn| {
            txn.update_block(paragraph, |b| {
                b.content_json = json!({"inline": [{"text": "from the app"}]});
            })
        })
        .unwrap();
    let out = mirror.pump_out(&mut vault).unwrap();
    assert_eq!(out.written, [page], "{out:?}");
    assert!(fs::read_to_string(&path).unwrap().contains("from the app"));
    let app_to_file = started.elapsed();
    eprintln!("app → file, {OBJECTS} pages: {app_to_file:?} (budget {BUDGET:?})");

    // The folder's own write comes back from the watcher as an echo; let it pass.
    let drain = Instant::now();
    until(
        &mut watcher,
        &mut mirror,
        &mut vault,
        drain,
        Duration::from_secs(2),
        |_| false,
    );
    assert_eq!(text_of(&vault, paragraph), "from the app");

    // File → app, timed from the write on disk.
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("from the app ^"), "{text}");
    clock.advance(1);
    let started = Instant::now();
    fs::write(&path, text.replace("from the app ^", "from the file ^")).unwrap();
    until(
        &mut watcher,
        &mut mirror,
        &mut vault,
        started,
        BUDGET + Duration::from_secs(3),
        |v| text_of(v, paragraph) == "from the file",
    );
    let file_to_app = started.elapsed();
    eprintln!(
        "file → app, {OBJECTS} pages: {file_to_app:?} (budget {BUDGET:?}, debounce {DEBOUNCE:?})"
    );
    assert_eq!(text_of(&vault, paragraph), "from the file");

    assert!(
        app_to_file < BUDGET,
        "app → file took {app_to_file:?}, over the {BUDGET:?} budget"
    );
    assert!(
        file_to_app < BUDGET,
        "file → app took {file_to_app:?}, over the {BUDGET:?} budget"
    );
    vault.verify(false).unwrap();
}
