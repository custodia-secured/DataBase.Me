// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Mirror::import_from` (M6, ADR-0026): a folder taken into a vault as the files folder would
//! take it, under a parent, with the vault's own bookkeeping untouched, every write an undo step
//! and the inbox entries as warnings.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use dbme_core::{Clock, Id, ManualClock};
use dbme_mirror::{Mirror, StdFs};
use dbme_store::{Identity, NewObject, Vault, VaultOptions};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

fn vault(dir: &Path) -> Vault {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_128_000_000));
    let options = VaultOptions {
        identity: Identity::generate(clock.as_ref()).unwrap(),
        clock,
    };
    Vault::create(&dir.join("Test.dbme"), options).unwrap()
}

fn an_object(vault: &mut Vault, title: &str) -> Id {
    vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: title.into(),
                ..NewObject::default()
            })
        })
        .unwrap()
        .0
}

#[test]
fn a_folder_imports_under_a_parent_as_undo_steps_and_leaves_the_bookkeeping_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let home = an_object(&mut vault, "Home");
    let before = vault.verify(false).unwrap().objects;
    let fs = StdFs::new(Path::new(FIXTURES).join("obsidian")).unwrap();
    let mut mirror = Mirror::new(fs, vault.clock());

    let report = mirror.import_from(&mut vault, Some(home), None).unwrap();
    assert!(report.held.is_none());
    assert!(report.created.len() >= 8, "{report:?}");
    assert!(report.applied.is_empty(), "nothing was known before");
    assert_eq!(report.files.len(), report.created.len(), "{report:?}");
    let paths: Vec<&str> = report.files.iter().map(|(_, p)| p.as_str()).collect();
    assert!(
        paths.windows(2).all(|w| w[0] < w[1]),
        "sorted by path: {paths:?}"
    );
    assert!(
        paths.contains(&"Home.md") && paths.contains(&"Projects/Alpha.md"),
        "{paths:?}"
    );
    // The inbox entries became warnings: the cloud drives' copies, at least.
    assert!(vault.conflicts(false).unwrap().is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("cloud_conflict_file")),
        "{:?}",
        report.warnings
    );
    assert!(report.conflicts.len() == report.warnings.len());
    // The vault's own files folder knows nothing of it.
    assert!(vault.mirror_states(None).unwrap().is_empty());
    // Top-level files landed under the parent; nested ones under their folder's owner.
    let read = vault.read();
    let home_md = report.files.iter().find(|(_, p)| p == "Home.md").unwrap().0;
    assert_eq!(read.object(home_md).unwrap().parent_id, Some(home));
    let projects = report
        .files
        .iter()
        .find(|(_, p)| p == "Projects.md")
        .unwrap()
        .0;
    let alpha = report
        .files
        .iter()
        .find(|(_, p)| p == "Projects/Alpha.md")
        .unwrap()
        .0;
    assert_eq!(read.object(alpha).unwrap().parent_id, Some(projects));
    assert_eq!(read.object(projects).unwrap().parent_id, Some(home));
    // The fixture's own ids were adopted, and nothing was written back to the folder.
    assert!(
        read.object(Id::parse("01JB4F00000000000000H0ME01").unwrap())
            .is_ok()
    );
    assert!(!Path::new(FIXTURES).join("obsidian/.dbme").exists());
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);

    // Every write was an undo step: undoing them all leaves the vault as it was.
    let mut steps = 0;
    while vault.undo().unwrap().is_some() {
        steps += 1;
    }
    assert!(steps > 1, "one step per file, not one external write");
    assert_eq!(
        vault.verify(false).unwrap().objects,
        before - 1,
        "Home went too"
    );
}

#[test]
fn a_file_the_vault_knows_is_updated_and_only_limits_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let folder = Path::new(FIXTURES).join("obsidian");
    let mut mirror = Mirror::new(StdFs::new(&folder).unwrap(), vault.clock());
    let first = mirror.import_from(&mut vault, None, None).unwrap();
    let objects = vault.verify(false).unwrap().objects;

    // The same folder again: every file names an id the vault has, so nothing is made twice.
    let mut mirror = Mirror::new(StdFs::new(&folder).unwrap(), vault.clock());
    let second = mirror.import_from(&mut vault, None, None).unwrap();
    // Two files carry no `id:` (README.md, links.json): those come again; the rest update.
    assert_eq!(second.created.len(), 2, "{second:?}");
    assert_eq!(second.applied.len(), first.created.len() - 2);
    assert!(
        second.warnings.iter().all(|w| !w.contains("edit_edit")),
        "{:?}",
        second.warnings
    );
    assert_eq!(
        vault.verify(false).unwrap().objects,
        objects + 2,
        "no copies of the eight"
    );

    // `only`: one file of a folder of our own.
    let own = dir.path().join("notes");
    fs::create_dir(&own).unwrap();
    fs::write(own.join("One.md"), "# One\n\nfirst\n").unwrap();
    fs::write(own.join("Two.md"), "# Two\n\nsecond\n").unwrap();
    let mut mirror = Mirror::new(StdFs::new(&own).unwrap(), vault.clock());
    let only = ["Two.md".to_owned()];
    let report = mirror.import_from(&mut vault, None, Some(&only)).unwrap();
    assert_eq!(report.created.len(), 1);
    assert_eq!(report.files[0].1, "Two.md");
    assert_eq!(vault.read().object(report.created[0]).unwrap().title, "Two");

    // A parent that is not live is refused before anything happens.
    let gone = an_object(&mut vault, "Gone");
    vault.transact(|txn| txn.trash_object(gone)).unwrap();
    let before = vault.verify(false).unwrap().ops;
    let mut mirror = Mirror::new(StdFs::new(&own).unwrap(), vault.clock());
    assert!(mirror.import_from(&mut vault, Some(gone), None).is_err());
    assert_eq!(vault.verify(false).unwrap().ops, before);
}
