// SPDX-License-Identifier: AGPL-3.0-or-later
//! The platform watcher as the engine sees it (M5 Task 4d, ADR-0024): a probe on each OS. It
//! asserts only what every backend reports; shapes that differ by platform are printed.

#![allow(clippy::unwrap_used, reason = "a probe: a failure here is the finding")]

use std::fs;
use std::time::{Duration, Instant};

use dbme_mirror::{FileEvent, NotifyWatcher, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(300);
const PATIENCE: Duration = Duration::from_secs(6);

/// Polls until `done` is satisfied by the events seen so far, or patience runs out.
fn until(watcher: &mut NotifyWatcher, done: impl Fn(&[FileEvent]) -> bool) -> Vec<FileEvent> {
    let start = Instant::now();
    let mut seen = Vec::new();
    while start.elapsed() < PATIENCE {
        seen.extend(watcher.poll_timeout(Duration::from_millis(500)));
        if done(&seen) {
            break;
        }
    }
    seen
}

#[test]
fn the_watcher_reports_the_folder_s_changes_as_the_engine_s_events() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("files");
    fs::create_dir_all(root.join("Sub")).unwrap();
    let mut watcher = NotifyWatcher::new(&root, DEBOUNCE).unwrap();
    assert!(watcher.poll().is_empty());
    std::thread::sleep(Duration::from_millis(200));

    // A new file.
    fs::write(root.join("Sub/A.md"), "a\n").unwrap();
    let seen = until(&mut watcher, |s| {
        s.iter()
            .any(|e| matches!(e, FileEvent::Created(p) | FileEvent::Modified(p) if p == "Sub/A.md"))
    });
    println!("create: {seen:?}");
    assert!(
        seen.iter().any(
            |e| matches!(e, FileEvent::Created(p) | FileEvent::Modified(p) if p == "Sub/A.md")
        ),
        "{seen:?}"
    );

    // An edit.
    fs::write(root.join("Sub/A.md"), "a b\n").unwrap();
    let seen = until(&mut watcher, |s| {
        s.iter()
            .any(|e| matches!(e, FileEvent::Modified(p) | FileEvent::Created(p) if p == "Sub/A.md"))
    });
    println!("edit: {seen:?}");
    assert!(!seen.is_empty(), "an edit is reported");

    // A rename: one Renamed, or Removed plus Created, by platform.
    fs::rename(root.join("Sub/A.md"), root.join("Sub/B.md")).unwrap();
    let seen = until(&mut watcher, |s| {
        s.iter().any(|e| match e {
            FileEvent::Renamed { to, .. } => to == "Sub/B.md",
            FileEvent::Created(p) => p == "Sub/B.md",
            _ => false,
        })
    });
    println!("rename: {seen:?}");
    assert!(
        seen.iter().any(
            |e| matches!(e, FileEvent::Renamed { to, .. } if to == "Sub/B.md")
                || matches!(e, FileEvent::Created(p) if p == "Sub/B.md")
        ),
        "{seen:?}"
    );

    // A new folder is a DirCreated, so the engine lists it.
    fs::create_dir(root.join("New")).unwrap();
    let seen = until(&mut watcher, |s| {
        s.iter()
            .any(|e| matches!(e, FileEvent::DirCreated(p) if p == "New"))
    });
    println!("mkdir: {seen:?}");
    assert!(
        seen.iter()
            .any(|e| matches!(e, FileEvent::DirCreated(p) if p == "New")),
        "{seen:?}"
    );

    // A removal.
    fs::remove_file(root.join("Sub/B.md")).unwrap();
    let seen = until(&mut watcher, |s| {
        s.iter()
            .any(|e| matches!(e, FileEvent::Removed(p) if p == "Sub/B.md"))
    });
    println!("remove: {seen:?}");
    assert!(
        seen.iter()
            .any(|e| matches!(e, FileEvent::Removed(p) if p == "Sub/B.md")),
        "{seen:?}"
    );

    assert!(watcher.take_errors().is_empty());
    assert!(watcher.root().ends_with("files"));
    assert!(format!("{watcher:?}").contains("NotifyWatcher"));
    assert!(NotifyWatcher::new(&dir.path().join("missing"), DEBOUNCE).is_err());
}
