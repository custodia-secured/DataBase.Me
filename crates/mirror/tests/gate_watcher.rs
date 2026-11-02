// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1e (M5): what `notify` 8.2 with `notify-debouncer-full` 0.7 reports for the file
//! patterns the mirror has to recognise. Each test is a probe: it performs one scenario in a
//! temporary folder, prints every debounced event it received and asserts only what held on the
//! machine that ran it. The printed lists are the record; the gate report quotes them.
//! Collection waits up to 2 s for a first batch and ends after 1.5 s of quiet; anything that
//! depends on how events fall across batches is printed, not asserted.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, SystemTime};

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use tempfile::TempDir;

/// The debounce the engine's caller owns (M5.md Design: 300-500 ms).
const DEBOUNCE: Duration = Duration::from_millis(300);
/// How long a probe waits for its first batch.
const FIRST_BATCH: Duration = Duration::from_secs(2);
/// How much quiet ends a collection once a batch has arrived.
const QUIET: Duration = Duration::from_millis(1500);

/// One debounced event reduced to what the mirror keys on: the kind's debug form and the
/// paths relative to the watched root, with forward slashes.
type Seen = (String, Vec<String>);

/// A watched temporary folder. Dropping it stops the debouncer and removes the folder.
struct Watched {
    _dir: TempDir,
    root: PathBuf,
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    rx: Receiver<DebounceEventResult>,
}

impl Watched {
    /// Makes the folder, lets `before` populate it, then starts watching it recursively.
    fn start(before: impl FnOnce(&Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        // The backend reports paths under the path it was given; canonical in, canonical out.
        let root = dir.path().canonicalize().unwrap();
        before(&root);
        let (tx, rx) = mpsc::channel();
        let mut debouncer = new_debouncer(DEBOUNCE, None, tx).unwrap();
        debouncer.watch(&root, RecursiveMode::Recursive).unwrap();
        // Let the backend arm before the scenario starts.
        std::thread::sleep(Duration::from_millis(200));
        Self {
            _dir: dir,
            root,
            _debouncer: debouncer,
            rx,
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Drains batches until the channel has been quiet for `QUIET`. Returns the events in
    /// arrival order and how many batches carried them, and prints both under `label`.
    fn collect(&self, label: &str) -> (Vec<Seen>, usize) {
        let mut seen = Vec::new();
        let mut batches = 0;
        let mut wait = FIRST_BATCH;
        loop {
            match self.rx.recv_timeout(wait) {
                Ok(Ok(events)) => {
                    batches += 1;
                    for event in events {
                        let paths = event.paths.iter().map(|p| self.relative(p)).collect();
                        seen.push((format!("{:?}", event.kind), paths));
                    }
                    wait = QUIET;
                }
                Ok(Err(errors)) => panic!("{label}: watcher errors {errors:?}"),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => panic!("{label}: debouncer gone"),
            }
        }
        println!("{label}: {} event(s) in {batches} batch(es)", seen.len());
        for (kind, paths) in &seen {
            println!("  {kind} {paths:?}");
        }
        (seen, batches)
    }

    fn relative(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }
}

/// The kinds of every event that names `rel`, in order.
fn kinds_for<'a>(seen: &'a [Seen], rel: &str) -> Vec<&'a str> {
    seen.iter()
        .filter(|(_, paths)| paths.iter().any(|p| p == rel))
        .map(|(kind, _)| kind.as_str())
        .collect()
}

/// Every distinct path any event named.
fn paths_named(seen: &[Seen]) -> BTreeSet<&str> {
    seen.iter()
        .flat_map(|(_, paths)| paths.iter().map(String::as_str))
        .collect()
}

/// The mirror's own write pattern: a dot-prefixed temp file beside the target, then a rename.
fn atomic_write(root: &Path, title: &str, body: &str) -> String {
    let tmp = format!(".{title}.tmp-4242");
    fs::write(root.join(&tmp), body).unwrap();
    fs::rename(root.join(&tmp), root.join(title)).unwrap();
    tmp
}

/// A rename through cmd, which calls `MoveFileEx` the way Explorer and most editors do;
/// `std::fs::rename` goes through `FileRenameInfoEx` with POSIX semantics instead, so the two
/// can show the file system different things. `verb` is `move /Y` (replace allowed, `to` a
/// path) or `ren` (no replace, `to` a bare name).
#[cfg(windows)]
fn shell_rename(verb: &[&str], from: &Path, to: &str) {
    // cmd does not take the `\\?\` prefix canonicalize() adds.
    let from = from
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_owned();
    let status = std::process::Command::new("cmd")
        .arg("/C")
        .args(verb)
        .args([&from, to])
        .status()
        .unwrap();
    assert!(status.success(), "cmd {verb:?} failed: {status}");
}

// Probe 1: our own atomic write, first creating the target, then replacing it.
#[test]
fn atomic_write_new_then_replace() {
    let w = Watched::start(|_| {});
    let tmp = atomic_write(&w.root, "Title.md", "one");
    let (seen, _) = w.collect("atomic write, new target");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Create(Any)"]);
    assert!(kinds_for(&seen, &tmp).is_empty(), "{tmp} folded away");

    let tmp = atomic_write(&w.root, "Title.md", "two");
    let (seen, _) = w.collect("atomic write, replacing");
    // The replaced target reports as a remove followed by a create, never as a modify.
    assert_eq!(kinds_for(&seen, "Title.md"), ["Remove(Any)", "Create(Any)"]);
    assert!(kinds_for(&seen, &tmp).is_empty(), "{tmp} folded away");
}

// Probe 2: an editor's temp-and-rename, with a tilde backup name and a Vim swap name.
#[test]
fn editor_temp_and_rename() {
    let w = Watched::start(|root| fs::write(root.join("Title.md"), "old").unwrap());
    fs::write(w.path("Title.md~"), "tilde").unwrap();
    fs::rename(w.path("Title.md~"), w.path("Title.md")).unwrap();
    let (seen, _) = w.collect("rename Title.md~ over Title.md");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Remove(Any)", "Create(Any)"]);
    assert!(kinds_for(&seen, "Title.md~").is_empty());

    fs::write(w.path(".Title.md.swp"), "swap").unwrap();
    fs::rename(w.path(".Title.md.swp"), w.path("Title.md")).unwrap();
    let (seen, _) = w.collect("rename .Title.md.swp over Title.md");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Remove(Any)", "Create(Any)"]);
    assert!(kinds_for(&seen, ".Title.md.swp").is_empty());

    // The same replace through MoveFileEx, as an editor would do it.
    #[cfg(windows)]
    {
        fs::write(w.path("Title.md~"), "tilde 2").unwrap();
        let target = w.path("Title.md").to_string_lossy().replace(r"\\?\", "");
        shell_rename(&["move", "/Y"], &w.path("Title.md~"), &target);
        let (seen, _) = w.collect("cmd move /Y Title.md~ over Title.md");
        assert_eq!(kinds_for(&seen, "Title.md"), ["Remove(Any)", "Create(Any)"]);
        assert!(kinds_for(&seen, "Title.md~").is_empty());
    }
}

// Probe 3: writes that keep the path: truncate, append, same bytes, mtime only.
#[test]
fn in_place_writes_all_look_alike() {
    let w = Watched::start(|root| fs::write(root.join("Title.md"), "same").unwrap());

    File::create(w.path("Title.md"))
        .unwrap()
        .write_all(b"truncated")
        .unwrap();
    let (seen, _) = w.collect("truncate in place");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Modify(Any)"]);

    OpenOptions::new()
        .append(true)
        .open(w.path("Title.md"))
        .unwrap()
        .write_all(b" more")
        .unwrap();
    let (seen, _) = w.collect("append");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Modify(Any)"]);

    let bytes = fs::read(w.path("Title.md")).unwrap();
    fs::write(w.path("Title.md"), &bytes).unwrap();
    let (seen, _) = w.collect("rewrite with the same bytes");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Modify(Any)"]);

    File::options()
        .write(true)
        .open(w.path("Title.md"))
        .unwrap()
        .set_modified(SystemTime::now())
        .unwrap();
    let (seen, _) = w.collect("mtime-only touch");
    assert_eq!(kinds_for(&seen, "Title.md"), ["Modify(Any)"]);
}

// Probe 4: a rename inside one folder.
#[test]
fn rename_within_folder_is_one_event() {
    let w = Watched::start(|root| fs::write(root.join("A.md"), "a").unwrap());
    fs::rename(w.path("A.md"), w.path("B.md")).unwrap();
    let (seen, _) = w.collect("rename A.md -> B.md");
    assert_eq!(
        seen,
        [(
            "Modify(Name(Both))".to_owned(),
            vec!["A.md".to_owned(), "B.md".to_owned()]
        )]
    );
}

// Probe 5: a folder with three files moved into another folder, then a single file.
#[test]
fn moving_across_folders_is_remove_plus_create() {
    let w = Watched::start(|root| {
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("dst")).unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            fs::write(root.join("src").join(name), name).unwrap();
        }
        fs::write(root.join("loose.md"), "l").unwrap();
    });
    fs::rename(w.path("src"), w.path("dst/src")).unwrap();
    let (seen, _) = w.collect("move src/ into dst/");
    // Windows reports a move between parents as a remove and an add, never as a rename pair,
    // so the debouncer has nothing to stitch. The three files are not mentioned. The parent
    // that gained a child reports a modify of its own.
    assert_eq!(kinds_for(&seen, "src"), ["Remove(Any)"]);
    assert_eq!(kinds_for(&seen, "dst/src"), ["Create(Any)"]);
    assert_eq!(kinds_for(&seen, "dst"), ["Modify(Any)"]);
    assert_eq!(seen.len(), 3, "nothing for the files inside");

    fs::rename(w.path("loose.md"), w.path("dst/loose.md")).unwrap();
    let (seen, _) = w.collect("move loose.md into dst/");
    assert_eq!(kinds_for(&seen, "loose.md"), ["Remove(Any)"]);
    assert_eq!(kinds_for(&seen, "dst/loose.md"), ["Create(Any)"]);
    assert_eq!(kinds_for(&seen, "dst"), ["Modify(Any)"]);
}

// Probe 6a: one file deleted.
#[test]
fn deleting_a_file() {
    let w = Watched::start(|root| fs::write(root.join("Gone.md"), "x").unwrap());
    fs::remove_file(w.path("Gone.md")).unwrap();
    let (seen, _) = w.collect("delete Gone.md");
    assert_eq!(
        seen,
        [("Remove(Any)".to_owned(), vec!["Gone.md".to_owned()])]
    );
}

// Probe 6b: a folder with files deleted.
#[test]
fn deleting_a_folder_with_files() {
    let w = Watched::start(|root| {
        fs::create_dir(root.join("sub")).unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            fs::write(root.join("sub").join(name), name).unwrap();
        }
    });
    fs::remove_dir_all(w.path("sub")).unwrap();
    let (seen, _) = w.collect("remove_dir_all sub/");
    // The folder's removal swallows its children's: the debouncer drops every queued event
    // under a removed path, so the engine learns which files went only from its own state.
    assert_eq!(seen, [("Remove(Any)".to_owned(), vec!["sub".to_owned()])]);
}

// Probe 6d: a write into an existing folder; in probe 5 the folder receiving a move reported a
// modify of its own, here it does not.
#[test]
fn a_write_inside_a_folder_reports_the_file_only() {
    let w = Watched::start(|root| fs::create_dir(root.join("sub")).unwrap());
    fs::write(w.path("sub/new.md"), "n").unwrap();
    let (seen, _) = w.collect("write sub/new.md");
    assert_eq!(
        seen,
        [("Create(Any)".to_owned(), vec!["sub/new.md".to_owned()])]
    );
}

// Probe 6c: a folder created and filled at once, with recursion armed beforehand.
#[test]
fn files_inside_a_new_folder_are_reported() {
    let w = Watched::start(|_| {});
    fs::create_dir(w.path("new")).unwrap();
    for name in ["a.md", "b.md", "c.md"] {
        fs::write(w.path("new").join(name), name).unwrap();
    }
    let (seen, _) = w.collect("create new/ then three files");
    for name in ["new", "new/a.md", "new/b.md", "new/c.md"] {
        assert_eq!(kinds_for(&seen, name), ["Create(Any)"], "{name}");
    }
}

// Probe 7: a rename onto a file someone holds open. Plain std, no watcher.
#[test]
fn rename_over_an_open_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Target.md");
    let other = dir.path().join("Other.md");
    fs::write(&target, "target").unwrap();
    fs::write(&other, "other").unwrap();

    // Rust's File::open shares read, write and delete, as most editors' readers do.
    let held = File::open(&target).unwrap();
    let first = fs::rename(&other, &target);
    println!("rename over a File::open handle: {first:?}");
    assert!(first.is_ok(), "std's default sharing lets it through");
    drop(held);
    assert_eq!(fs::read_to_string(&target).unwrap(), "other");

    // A handle that refuses to share delete is what blocks a rename.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        fs::write(&other, "again").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&target)
            .unwrap();
        let blocked = fs::rename(&other, &target);
        println!("rename over a FILE_SHARE_READ-only handle: {blocked:?}");
        let error = blocked.expect_err("the rename is refused while the handle is held");
        assert_eq!(error.raw_os_error(), Some(5), "ERROR_ACCESS_DENIED, not 32");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        drop(held);
        let retried = fs::rename(&other, &target);
        println!("retry after dropping the handle: {retried:?}");
        assert!(retried.is_ok());
        assert_eq!(fs::read_to_string(&target).unwrap(), "again");
    }
}

// Probe 8: a rename that changes only the case of the name.
#[test]
fn case_only_rename_is_not_stitched() {
    let w = Watched::start(|root| fs::write(root.join("Note.md"), "n").unwrap());
    fs::rename(w.path("Note.md"), w.path("note.md")).unwrap();
    let (seen, _) = w.collect("std rename Note.md -> note.md");
    // Windows first reports the "replaced" target removed (it is the same file), which evicts
    // the cached file ID, so the From and To that follow cannot be matched and arrive raw.
    assert_eq!(
        seen,
        [
            ("Remove(Any)".to_owned(), vec!["Note.md".to_owned()]),
            ("Modify(Name(From))".to_owned(), vec!["Note.md".to_owned()]),
            ("Modify(Name(To))".to_owned(), vec!["note.md".to_owned()]),
        ]
    );

    // The same through MoveFileEx: with replace allowed (`move /Y`) and without (`ren`). Both
    // arrive the same way, so the leading remove is the file system's, not std's.
    #[cfg(windows)]
    {
        shell_rename(&["ren"], &w.path("note.md"), "NOTE.md");
        let (seen, _) = w.collect("cmd ren note.md NOTE.md");
        assert_eq!(
            seen,
            [
                ("Remove(Any)".to_owned(), vec!["note.md".to_owned()]),
                ("Modify(Name(From))".to_owned(), vec!["note.md".to_owned()]),
                ("Modify(Name(To))".to_owned(), vec!["NOTE.md".to_owned()]),
            ]
        );
        let target = w.path("Note.md").to_string_lossy().replace(r"\\?\", "");
        shell_rename(&["move", "/Y"], &w.path("NOTE.md"), &target);
        let (seen, _) = w.collect("cmd move /Y NOTE.md Note.md");
        assert_eq!(
            seen,
            [
                ("Remove(Any)".to_owned(), vec!["NOTE.md".to_owned()]),
                ("Modify(Name(From))".to_owned(), vec!["NOTE.md".to_owned()]),
                ("Modify(Name(To))".to_owned(), vec!["Note.md".to_owned()]),
            ]
        );
    }
}

// Probe 9: fifty files deleted in one loop. The batch count depends on timing: printed only.
#[test]
fn fifty_deletes_arrive_once_each() {
    let w = Watched::start(|root| {
        for i in 0..50 {
            fs::write(root.join(format!("n{i:02}.md")), "x").unwrap();
        }
    });
    for i in 0..50 {
        fs::remove_file(w.path(&format!("n{i:02}.md"))).unwrap();
    }
    let (seen, _batches) = w.collect("delete 50 files");
    assert_eq!(seen.len(), 50);
    assert!(seen.iter().all(|(kind, _)| kind == "Remove(Any)"));
    assert_eq!(paths_named(&seen).len(), 50);
}

// Probe 10: which path an own write carries, since that is what the echo ledger must match.
#[test]
fn own_write_events_name_the_final_path_only() {
    let w = Watched::start(|root| fs::write(root.join("Title.md"), "old").unwrap());
    let tmp = atomic_write(&w.root, "Title.md", "new");
    let (seen, _) = w.collect("own atomic replace");
    let named = paths_named(&seen);
    println!("  paths named: {named:?}");
    assert_eq!(named, BTreeSet::from(["Title.md"]), "never {tmp}");
}

// Probe 11: files that appear between the folder's creation and watch() are not reported.
#[test]
fn files_from_before_the_watch_are_silent() {
    let w = Watched::start(|root| fs::write(root.join("Early.md"), "e").unwrap());
    // One live change proves the channel works, so silence about Early.md means something.
    fs::write(w.path("Late.md"), "l").unwrap();
    let (seen, _) = w.collect("Early.md before watch, Late.md after");
    assert!(kinds_for(&seen, "Early.md").is_empty());
    assert_eq!(kinds_for(&seen, "Late.md"), ["Create(Any)"]);
}
