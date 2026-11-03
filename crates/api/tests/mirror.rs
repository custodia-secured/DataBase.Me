// SPDX-License-Identifier: AGPL-3.0-or-later
//! The files folder's commands (M5 Task 6): enabling, syncing both ways, the inbox, the export,
//! the override file across openings, and the worker.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbme_api::{self as api, HoldChoice, IpcErrorKind, MirrorStatus, Session};
use dbme_core::{Id, SystemClock};
use dbme_store::{ConflictKind, Identity, NewObject, VaultOptions};

fn options() -> VaultOptions {
    let clock = Arc::new(SystemClock);
    VaultOptions {
        identity: Identity::generate(clock.as_ref()).unwrap(),
        clock,
    }
}

struct Counts {
    vault_changed: AtomicUsize,
    mirror_changed: AtomicUsize,
    last_status: Mutex<Option<MirrorStatus>>,
}

fn session(config: &Path) -> (Session, Arc<Counts>) {
    let counts = Arc::new(Counts {
        vault_changed: AtomicUsize::new(0),
        mirror_changed: AtomicUsize::new(0),
        last_status: Mutex::new(None),
    });
    let for_vault = Arc::clone(&counts);
    let for_mirror = Arc::clone(&counts);
    let session = Session::new(move |_| {
        for_vault.vault_changed.fetch_add(1, Ordering::SeqCst);
    })
    .with_mirror_notify(move |status| {
        for_mirror.mirror_changed.fetch_add(1, Ordering::SeqCst);
        *for_mirror.last_status.lock().unwrap() = Some(status.clone());
    })
    .with_config_dir(config);
    (session, counts)
}

fn create(session: &Session, title: &str) -> Id {
    api::object_create(
        session,
        NewObject {
            title: title.into(),
            ..NewObject::default()
        },
    )
    .unwrap()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_folder_is_enabled_synced_both_ways_and_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("Notes.dbme");
    let (session, counts) = session(&dir.path().join("config"));
    api::vault_create(&session, vault_path.to_str().unwrap(), options()).unwrap();
    let alpha = create(&session, "Alpha");

    // Off by default; sync refuses; enabling picks files/ inside the vault.
    let status = api::mirror_status(&session).unwrap();
    assert!(!status.enabled && status.folder.is_none());
    assert_eq!(
        api::mirror_sync_now(&session).unwrap_err().kind,
        IpcErrorKind::Rule
    );
    let status = api::mirror_enable(&session, None).unwrap();
    let folder = vault_path.join(api::DEFAULT_FOLDER);
    assert!(status.enabled && status.full_rewrite_needed);
    assert_eq!(status.folder.as_deref(), Some(folder.to_str().unwrap()));
    assert_eq!(counts.mirror_changed.load(Ordering::SeqCst), 1);

    // App → file.
    let sync = api::mirror_sync_now(&session).unwrap();
    assert!(sync.written >= 1, "{sync:?}");
    let alpha_file = folder.join("Alpha.md");
    assert!(alpha_file.is_file());
    let status = api::mirror_status(&session).unwrap();
    assert!(status.synced >= 1 && status.dirty == 0 && !status.full_rewrite_needed);

    // File → app: an edit, and a new file; the inbound batch is one vault-changed.
    let before = counts.vault_changed.load(Ordering::SeqCst);
    let mut text = fs::read_to_string(&alpha_file).unwrap();
    text.push_str("\nA line from the file.\n");
    fs::write(&alpha_file, text).unwrap();
    fs::write(folder.join("Beta.md"), "From a file\n").unwrap();
    let sync = api::mirror_sync_now(&session).unwrap();
    assert_eq!((sync.applied, sync.created), (1, 1), "{sync:?}");
    assert_eq!(counts.vault_changed.load(Ordering::SeqCst), before + 1);
    assert_eq!(api::blocks_list(&session, alpha).unwrap().len(), 1);
    let beta = api::object_search(&session, "Beta", 5).unwrap();
    assert_eq!(beta.len(), 1);
    assert!(api::conflicts_list(&session, true).unwrap().is_empty());

    // The export writes into the session's folder.
    let export = dir.path().join("export");
    let report = api::export_run(&session, export.display().to_string(), true).unwrap();
    assert!(report.ops.is_some() && export.join("ops.jsonl").is_file());

    // Off: the files stay, and the status says so.
    let status = api::mirror_disable(&session).unwrap();
    assert!(!status.enabled);
    assert!(alpha_file.is_file());
    assert!(api::mirror_status(&session).unwrap().folder.is_none());
}

#[test]
fn an_override_folder_is_kept_in_the_config_and_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    let vault_path = dir.path().join("Notes.dbme");
    let elsewhere = dir.path().join("elsewhere");
    let (session, _) = session(&config);
    api::vault_create(&session, vault_path.to_str().unwrap(), options()).unwrap();
    let id = api::vault_info(&session).unwrap().unwrap().id;
    create(&session, "Alpha");

    let status = api::mirror_enable(&session, Some(elsewhere.display().to_string())).unwrap();
    assert_eq!(status.folder.as_deref(), Some(elsewhere.to_str().unwrap()));
    let override_file = config.join(api::OVERRIDES_DIR).join(format!("{id}.txt"));
    assert!(override_file.is_file());
    api::mirror_sync_now(&session).unwrap();
    assert!(elsewhere.join("Alpha.md").is_file());
    assert!(!vault_path.join(api::DEFAULT_FOLDER).exists());

    // Reopening starts the folder again where it was.
    api::vault_close(&session).unwrap();
    assert!(api::mirror_status(&session).is_err(), "no vault");
    api::vault_open(&session, vault_path.to_str().unwrap(), options()).unwrap();
    let status = api::mirror_status(&session).unwrap();
    assert!(status.enabled);
    assert_eq!(status.folder.as_deref(), Some(elsewhere.to_str().unwrap()));

    // Back to the default clears the override.
    api::mirror_enable(&session, None).unwrap();
    assert!(!override_file.exists());
    assert_eq!(
        api::mirror_status(&session).unwrap().folder.as_deref(),
        Some(vault_path.join(api::DEFAULT_FOLDER).to_str().unwrap())
    );

    // A shell with no settings can only use the default.
    let bare = Session::new(|_| {});
    let other = dir.path().join("Other.dbme");
    api::vault_create(&bare, other.to_str().unwrap(), options()).unwrap();
    assert_eq!(
        api::mirror_enable(&bare, Some(elsewhere.display().to_string()))
            .unwrap_err()
            .kind,
        IpcErrorKind::Rule
    );
    assert!(api::mirror_enable(&bare, None).unwrap().enabled);
}

#[test]
fn a_hold_is_decided_and_an_inbox_entry_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("Notes.dbme");
    let (session, counts) = session(&dir.path().join("config"));
    api::vault_create(&session, vault_path.to_str().unwrap(), options()).unwrap();
    let ids: Vec<Id> = (0..60)
        .map(|n| create(&session, &format!("N {n}")))
        .collect();
    api::mirror_enable(&session, None).unwrap();
    api::mirror_sync_now(&session).unwrap();
    let folder = vault_path.join(api::DEFAULT_FOLDER);

    // Deleting 50 files at once is held, and the hold shows in the status.
    for n in 0..50 {
        fs::remove_file(folder.join(format!("N {n}.md"))).unwrap();
    }
    let sync = api::mirror_sync_now(&session).unwrap();
    let hold = sync.held.expect("the removals are held");
    assert_eq!(sync.trashed_objects, 0);
    let status = api::mirror_status(&session).unwrap();
    assert_eq!(status.hold, Some(hold));
    assert_eq!(status.open_conflicts, 1);
    assert_eq!(
        api::conflict_resolve(&session, hold, "dismissed".into())
            .unwrap_err()
            .kind,
        IpcErrorKind::Rule,
        "a hold is decided, not resolved"
    );

    // Restore writes the files back from the vault.
    let before = counts.vault_changed.load(Ordering::SeqCst);
    let sync = api::mirror_hold_decide(&session, hold, HoldChoice::Restore).unwrap();
    assert!(sync.written >= 50, "{sync:?}");
    assert!(folder.join("N 0.md").is_file());
    assert_eq!(counts.vault_changed.load(Ordering::SeqCst), before);
    assert_eq!(api::mirror_status(&session).unwrap().hold, None);
    assert!(
        api::object_get(&session, ids[0])
            .unwrap()
            .deleted_at
            .is_none()
    );

    // Deleting them again and proceeding trashes the objects, as one vault-changed.
    for n in 0..50 {
        fs::remove_file(folder.join(format!("N {n}.md"))).unwrap();
    }
    let hold = api::mirror_sync_now(&session).unwrap().held.unwrap();
    let before = counts.vault_changed.load(Ordering::SeqCst);
    let sync = api::mirror_hold_decide(&session, hold, HoldChoice::Proceed).unwrap();
    assert_eq!(sync.trashed_objects, 50, "{sync:?}");
    assert_eq!(counts.vault_changed.load(Ordering::SeqCst), before + 1);
    assert!(
        api::object_get(&session, ids[0])
            .unwrap()
            .deleted_at
            .is_some()
    );

    // A parse error is an inbox entry, which the person resolves.
    fs::write(folder.join("N 55.md"), "---\nid: [not: valid\n---\n").unwrap();
    api::mirror_sync_now(&session).unwrap();
    let open = api::conflicts_list(&session, true).unwrap();
    let parse = open
        .iter()
        .find(|c| c.kind == ConflictKind::ParseError)
        .expect("a parse error in the inbox");
    assert!(api::conflict_resolve(&session, parse.id, "dismissed".into()).unwrap());
    assert!(!api::conflict_resolve(&session, parse.id, "dismissed".into()).unwrap());
    assert!(
        api::conflicts_list(&session, true)
            .unwrap()
            .iter()
            .all(|c| c.id != parse.id)
    );
    assert!(api::conflicts_list(&session, false).unwrap().len() >= 3);
}

#[test]
fn the_worker_writes_after_commits_and_reads_what_the_watcher_sees() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("Notes.dbme");
    let (session, counts) = session(&dir.path().join("config"));
    let session = Arc::new(session);
    let worker = api::spawn_mirror_worker(Arc::clone(&session));
    api::vault_create(&session, vault_path.to_str().unwrap(), options()).unwrap();
    api::mirror_enable(&session, None).unwrap();
    let folder = vault_path.join(api::DEFAULT_FOLDER);

    // A commit wakes the worker, which writes the file.
    create(&session, "Alpha");
    wait_for("Alpha.md", || folder.join("Alpha.md").is_file());
    wait_for("the status after the write", || {
        counts
            .last_status
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.dirty == 0 && s.synced >= 1)
    });

    // A file the watcher sees becomes an object. The worker arms the watcher on its next tick
    // after the folder is enabled, so give it one.
    std::thread::sleep(Duration::from_secs(1));
    fs::write(folder.join("Beta.md"), "Seen by the watcher\n").unwrap();
    wait_for("Beta in the vault", || {
        api::object_search(&session, "Beta", 5)
            .map(|hits| hits.len() == 1)
            .unwrap_or(false)
    });
    assert!(counts.vault_changed.load(Ordering::SeqCst) >= 2);

    // Dropping the worker stops its thread; the session still works without it.
    drop(worker);
    create(&session, "Gamma");
    api::mirror_sync_now(&session).unwrap();
    assert!(folder.join("Gamma.md").is_file());
}
