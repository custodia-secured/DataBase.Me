// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Obsidian fixture keeps its links and frontmatter (M6 acceptance): the folder imported
//! under a parent gives every link in `links.json` what it names and the tags their Tag objects;
//! every object has its provenance; a second import of the same folder makes nothing twice; a
//! single `.md` file imports alone; a picture inside the folder is handed to the image importer
//! after the engine attached it; the vault's own files folder is untouched.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use dbme_core::model::AttachmentRole;
use dbme_core::{Clock, Id, ManualClock};
use dbme_ingest::{ImportOptions, Importers, apply, plan};
use dbme_store::{NewObject, Vault, VaultOptions};
use serde::Deserialize;

use common::{fixture, photo_jpeg};

fn vault(dir: &Path) -> Vault {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_762_128_000_000));
    let options = VaultOptions {
        identity: dbme_store::Identity::generate(clock.as_ref()).unwrap(),
        clock,
    };
    Vault::create(&dir.join("Test.dbme"), options).unwrap()
}

#[derive(Deserialize)]
struct Manifest {
    links: Vec<Link>,
    tags: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct Link {
    from_id: Option<Id>,
    link: String,
    to: String,
    to_id: Option<Id>,
    kind: String,
    shown: Option<String>,
    block: Option<Id>,
    label: Option<String>,
}

#[test]
fn the_obsidian_fixture_keeps_its_links_frontmatter_and_tags_with_provenance_on_every_object() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let folder = fixture("obsidian");
    let plan = plan(
        &vault,
        std::slice::from_ref(&folder),
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(plan.entries[0].importer, "markdown");
    let counted = plan.entries[0].extract.folder.as_ref().unwrap();
    assert!(counted.markdown_files >= 8, "{counted:?}");
    assert_eq!(plan.objects, counted.markdown_files + counted.other_files);
    assert!(
        plan.entries[0].extract.objects.is_empty(),
        "the engine makes them at apply"
    );
    let before = vault.verify(false).unwrap().ops;

    let run = apply(&mut vault, &plan).unwrap();
    assert!(
        run.transactions > 2,
        "the receipt, the files, the provenance: {run:?}"
    );
    assert!(run.created.len() >= 8, "{run:?}");
    let receipt = run.receipt.unwrap();
    let read = vault.read();
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(folder.join("links.json")).unwrap()).unwrap();

    for link in &manifest.links {
        let from = link.from_id.unwrap();
        let what = format!("{} in {}", link.link, from);
        match link.kind.as_str() {
            "mention" | "embed" => {
                let to = link.to_id.unwrap();
                let runs: Vec<serde_json::Value> = read
                    .blocks(from)
                    .unwrap()
                    .into_iter()
                    .flat_map(|b| {
                        b.content_json["inline"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter(|run| run["mention"] == to.to_string())
                    .collect();
                assert!(
                    runs.iter().any(|run| {
                        run["shown"].as_str() == link.shown.as_deref()
                            && run["block"].as_str() == link.block.map(|b| b.to_string()).as_deref()
                            && (run["embed"] == true) == (link.kind == "embed")
                    }),
                    "{what}: no such run among {runs:?}"
                );
            }
            "relation" => {
                let to = link.to_id.unwrap();
                let relation = read
                    .relations_from(from)
                    .unwrap()
                    .into_iter()
                    .find(|r| r.dst_id == to && r.field_id.is_none() && r.rel_type == "related")
                    .unwrap_or_else(|| panic!("{what}: no relation"));
                assert_eq!(relation.label, link.label, "{what}");
            }
            "attachment" => {
                // A picture Obsidian named stays text in the note; the picture itself is not
                // among the engine's objects (`attachments/` is the folder's own).
                let text: String = read
                    .blocks(from)
                    .unwrap()
                    .into_iter()
                    .flat_map(|b| {
                        b.content_json["inline"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter_map(|run| run["text"].as_str().map(str::to_owned))
                    .collect();
                assert!(text.contains(&link.link), "{what}: {text}");
                assert!(fixture("obsidian").join(&link.to).is_file());
            }
            other => panic!("{what}: unknown kind {other}"),
        }
    }
    let by_path: HashMap<&str, Id> = manifest
        .links
        .iter()
        .filter_map(|l| Some((l.to.as_str(), l.to_id?)))
        .collect();
    for (tag, files) in &manifest.tags {
        for file in files {
            let id = by_path[file.as_str()];
            let tags: BTreeSet<String> = read
                .relations_from(id)
                .unwrap()
                .into_iter()
                .filter(|r| r.rel_type == "tagged")
                .map(|r| read.object(r.dst_id).unwrap().title)
                .collect();
            assert!(tags.contains(tag), "{file}: tags {tags:?} lack {tag}");
        }
    }

    // Frontmatter: the fixture's ids are the objects' ids, and a field it carries is a field.
    let home = read
        .object(Id::parse("01JB4F00000000000000H0ME01").unwrap())
        .unwrap();
    assert_eq!(
        home.parent_id,
        Some(receipt),
        "top-level files land under the receipt"
    );
    let alex = read
        .object(Id::parse("01JB4F00000000000000A1EX01").unwrap())
        .unwrap();
    assert_eq!(alex.title, "Alex Doe", "the frontmatter id is the object");

    // Provenance on every object the run made, naming the run and the file.
    for id in &run.created {
        let provenance = read.provenance_of(*id).unwrap();
        assert_eq!(provenance.len(), 1, "{id}");
        assert_eq!(provenance[0].source_kind, "import:markdown");
        assert_eq!(provenance[0].import_run_id, Some(run.run_id));
        assert!(
            provenance[0]
                .source_uri
                .as_deref()
                .unwrap()
                .contains("/obsidian/")
        );
    }
    assert_eq!(
        read.provenance_of_run(run.run_id).unwrap().len(),
        run.created.len() + 1,
        "the receipt's too"
    );
    // The receipt carries the engine's warnings (the cloud drives' copies).
    assert!(
        run.warnings
            .iter()
            .any(|w| w.contains("cloud_conflict_file")),
        "{:?}",
        run.warnings
    );
    // The vault's own files folder knows nothing of it, and the fixture is unchanged.
    assert!(vault.mirror_states(None).unwrap().is_empty());
    assert!(vault.conflicts(false).unwrap().is_empty());
    assert!(!folder.join(".dbme").exists());
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);

    // A second import of the same folder: every id is known, so nothing is made twice.
    let again = dbme_ingest::plan(
        &vault,
        &[folder],
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    let objects = vault.verify(false).unwrap().objects;
    let second = apply(&mut vault, &again).unwrap();
    // The two files without an `id:` (README.md, links.json) come again; the eight with one
    // are updated, not copied.
    assert_eq!(second.created.len(), 2, "{second:?}");
    assert_eq!(
        vault.verify(false).unwrap().objects,
        objects + 3,
        "the receipt and the two"
    );

    // Undoing every step takes it all back.
    let mut steps = 0;
    while vault.undo().unwrap().is_some() {
        steps += 1;
    }
    assert!(steps >= run.transactions);
    assert!(vault.verify(false).unwrap().ops > before);
    assert!(vault.read().object(home.id).is_err());
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}

#[test]
fn a_single_markdown_file_imports_alone_and_a_picture_in_a_folder_is_enriched() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let (home, _) = vault
        .transact(|txn| {
            txn.create_object(NewObject {
                title: "Home".into(),
                ..NewObject::default()
            })
        })
        .unwrap();
    let notes = dir.path().join("notes");
    fs::create_dir(&notes).unwrap();
    fs::write(notes.join("One.md"), "# One\n\nSee [[Two]].\n").unwrap();
    fs::write(notes.join("Two.md"), "---\ntags: [fresh]\n---\n\n# Two\n").unwrap();
    fs::write(notes.join("photo.jpg"), photo_jpeg()).unwrap();

    // One file, under a named parent: nothing else from the folder.
    let options = ImportOptions {
        parent: Some(home),
        ..ImportOptions::default()
    };
    let plan = plan(&vault, &[notes.join("Two.md")], options, &Importers::v0()).unwrap();
    let folder = plan.entries[0].extract.folder.as_ref().unwrap();
    assert_eq!(folder.only.as_deref(), Some(&["Two.md".to_owned()][..]));
    assert_eq!(folder.markdown_files, 1);
    let run = apply(&mut vault, &plan).unwrap();
    assert_eq!((run.created.len(), run.receipt), (1, None));
    let two = run.created[0];
    assert_eq!(vault.read().object(two).unwrap().title, "Two");
    assert_eq!(vault.read().object(two).unwrap().parent_id, Some(home));
    assert_eq!(
        vault.read().provenance_of(two).unwrap()[0].source_kind,
        "import:markdown"
    );
    assert!(
        vault
            .read()
            .relations_from(two)
            .unwrap()
            .iter()
            .any(|r| r.rel_type == "tagged")
    );

    // The whole folder: One links to the Two the vault already has, by its file; the picture
    // becomes a Media with the file attached, then the image importer adds its thumbnail, size
    // and EXIF.
    let whole = dbme_ingest::plan(
        &vault,
        std::slice::from_ref(&notes),
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    assert_eq!(
        whole.entries[0]
            .extract
            .folder
            .as_ref()
            .unwrap()
            .other_files,
        1
    );
    let run = apply(&mut vault, &whole).unwrap();
    let read = vault.read();
    let picture = run
        .created
        .iter()
        .copied()
        .find(|id| read.object(*id).unwrap().title == "photo")
        .expect("the picture's object");
    let attachments = read.attachments(picture).unwrap();
    assert!(
        attachments
            .iter()
            .any(|a| a.role == AttachmentRole::Original)
    );
    assert!(
        attachments
            .iter()
            .any(|a| a.role == AttachmentRole::Thumbnail),
        "{attachments:?}"
    );
    let row = read.object(picture).unwrap();
    assert_eq!(row.props_json["camera_make"], "Dbme");
    assert_eq!(row.props_json["width"], 64);
    assert!(row.captured_at.is_some());
    assert_eq!(
        read.blocks(picture)
            .unwrap()
            .iter()
            .filter(|b| b.kind == "image")
            .count(),
        1,
        "one image block, not two"
    );
    let provenance = read.provenance_of(picture).unwrap();
    assert_eq!(provenance[0].source_kind, "import:markdown");
    assert_eq!(
        provenance[0].original_blob,
        Some(
            attachments
                .iter()
                .find(|a| a.role == AttachmentRole::Original)
                .unwrap()
                .blob_hash
                .clone()
        )
    );
    let report = vault.verify(true).unwrap();
    assert!(report.is_ok(), "{:#?}", report.problems);
}
