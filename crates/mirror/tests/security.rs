// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hostile files (M5 acceptance, brief §10): everything in `fixtures/malicious/`, plus a junction
//! out of the folder and a 240-character path made here, is rejected or contained, and nothing
//! outside the folder is read into the vault or written to.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbme_core::{Id, ManualClock};
use dbme_mirror::{Mirror, StdFs};
use dbme_store::{ConflictKind, Identity, Vault, VaultOptions};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

/// A junction (Windows) or symlink (elsewhere) at `link` pointing at `target`. Returns whether
/// it could be made: a junction needs no privilege, a symlink on Windows may.
fn link_dir(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

#[test]
fn hostile_files_are_contained_and_nothing_leaves_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("files");
    copy_tree(&Path::new(FIXTURES).join("malicious"), &folder);

    // What the folder must never reach: a sibling folder with a picture and a note.
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.png"), b"\x89PNG not really").unwrap();
    fs::write(outside.join("secret.md"), "A note outside the folder.\n").unwrap();
    let outside_before = snapshot(&outside);
    let linked = link_dir(&folder.join("escape"), &outside);

    // A file 240 characters deep.
    let segment = |c: char| std::iter::repeat_n(c, 60).collect::<String>();
    let deep: PathBuf = [segment('a'), segment('b'), segment('c')]
        .iter()
        .fold(folder.clone(), |p, s| p.join(s));
    fs::create_dir_all(&deep).unwrap();
    let leaf = format!("{}.md", std::iter::repeat_n('d', 54).collect::<String>());
    fs::write(deep.join(&leaf), "Deep down.\n").unwrap();
    let deep_rel = format!("{}/{}/{}/{leaf}", segment('a'), segment('b'), segment('c'));
    assert_eq!(deep_rel.len(), 240);

    let clock = Arc::new(ManualClock::new(1_762_090_000_000));
    let mut vault = Vault::create(
        &dir.path().join("Fresh.dbme"),
        VaultOptions {
            identity: Identity {
                actor_id: Id::from_parts(1, 1),
                device_id: Id::from_parts(1, 2),
            },
            clock: clock.clone(),
        },
    )
    .unwrap();
    let mut mirror = Mirror::new(StdFs::new(&folder).unwrap(), clock);
    let report = mirror.rebuild(&mut vault).unwrap();
    assert!(report.held.is_none(), "{report:?}");
    let read = vault.read();

    // The two unreadable files are inbox entries and nothing else.
    let inbox = vault.conflicts(true).unwrap();
    for name in ["oversized-frontmatter.md", "deep-yaml.md"] {
        assert!(
            inbox
                .iter()
                .any(|c| c.kind == ConflictKind::ParseError && c.rel_path.as_deref() == Some(name)),
            "{name} should be a parse error: {inbox:?}"
        );
        assert!(
            read.search_titles(name.trim_end_matches(".md"), 5)
                .unwrap()
                .is_empty()
        );
    }

    // Traversal: a page, with nothing read from outside and nothing attached.
    let traversal = read.search_titles("traversal", 5).unwrap();
    assert_eq!(traversal.len(), 1, "{traversal:?}");
    let traversal = traversal[0].object.id;
    assert!(read.attachments(traversal).unwrap().is_empty());
    assert!(
        read.search_titles("secret", 5).unwrap().is_empty(),
        "nothing outside the folder became a page"
    );
    assert!(
        read.blocks(traversal)
            .unwrap()
            .iter()
            .all(|b| b.kind != "image"),
        "an image block was made from a path outside the folder"
    );

    // Bad ids: a page with a fresh id; the bad parts are text.
    let bad = read.search_titles("bad-ids", 5).unwrap();
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(
        read.relations_from(bad[0].object.id)
            .unwrap()
            .iter()
            .all(|r| r.rel_type == "tagged")
    );

    // The deep file is a page, and the junction's content is not.
    assert_eq!(read.search_titles(&leaf[..20], 5).unwrap().len(), 1);
    if linked {
        assert!(read.search_titles("secret", 5).unwrap().is_empty());
    }

    // Writing everything back stays inside the folder.
    mirror.request_full_rewrite(&mut vault).unwrap();
    let out = mirror.pump_out(&mut vault).unwrap();
    assert!(out.pending.is_empty(), "{out:?}");
    for state in vault.mirror_states(None).unwrap() {
        assert!(
            !state.rel_path.split('/').any(|part| part == ".."),
            "{}",
            state.rel_path
        );
        assert!(!state.rel_path.starts_with("escape/"), "{}", state.rel_path);
        mirror.fs().full(&state.rel_path).unwrap();
    }
    assert_eq!(
        snapshot(&outside),
        outside_before,
        "the outside folder was touched"
    );
    assert!(vault.verify(true).unwrap().is_ok());
}
