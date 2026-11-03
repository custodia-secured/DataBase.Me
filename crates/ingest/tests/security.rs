// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hostile inputs for the importers (M6 acceptance, brief §10): malformed PDFs (truncated, a
//! recursive page tree, a page-tree bomb), a PNG that declares ten billion pixels, a CSV with
//! formula cells and one with a 1 MiB cell, a workbook whose sheet is named like a path, a nested
//! zip of zeros, and the files folder's own hostile fixtures through the engine. Each is refused
//! or contained: the plan and the apply return inside a bound, the vault verifies afterwards, the
//! file is attached whole, and nothing outside the import's own folders is read into the vault or
//! written to. The committed fixtures live in `fixtures/malicious-import/`, beside M5's
//! `fixtures/malicious/`, which the files folder's test copies whole and must not change.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{
    fixture, fixture_file, one_sheet_workbook, page_tree_bomb, png_header_only, recursive_pdf,
    two_pages_pdf, zip_store,
};
use dbme_core::model::AttachmentRole;
use dbme_core::{Clock, Id, ManualClock};
use dbme_ingest::tables::MAX_CELL_BYTES;
use dbme_ingest::{ImportOptions, ImportPlan, ImportRun, Importers, apply, plan};
use dbme_store::{Identity, Vault, VaultOptions};
use serde_json::json;

const NOW: i64 = 1_762_128_000_000;

/// A vault, a folder for the sources, and a folder beside them that nothing may touch.
struct Bench {
    dir: tempfile::TempDir,
    vault: Vault,
    sources: PathBuf,
    outside: PathBuf,
    before: BTreeMap<String, u64>,
}

/// The top-level entries of `dir`, each with how many files are under it.
fn snapshot(dir: &Path) -> BTreeMap<String, u64> {
    fn count(path: &Path) -> u64 {
        if path.is_dir() {
            fs::read_dir(path)
                .unwrap()
                .map(|e| count(&e.unwrap().path()))
                .sum()
        } else {
            1
        }
    }
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                count(&entry.path()),
            )
        })
        .collect()
}

impl Bench {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(NOW));
        let options = VaultOptions {
            identity: Identity::generate(clock.as_ref()).unwrap(),
            clock,
        };
        let vault = Vault::create(&dir.path().join("Test.dbme"), options).unwrap();
        let sources = dir.path().join("sources");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&sources).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("precious.txt"), b"keep").unwrap();
        let before = snapshot(dir.path());
        Self {
            dir,
            vault,
            sources,
            outside,
            before,
        }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.sources.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    /// Plans and applies `paths` inside `within`; the vault verifies afterwards.
    fn import(&mut self, paths: &[PathBuf], within: Duration) -> (ImportPlan, ImportRun) {
        let started = Instant::now();
        let plan = plan(
            &self.vault,
            paths,
            ImportOptions::default(),
            &Importers::v0(),
        )
        .unwrap();
        let run = apply(&mut self.vault, &plan).unwrap();
        let took = started.elapsed();
        assert!(took < within, "{paths:?} took {took:?}, over {within:?}");
        let report = self.vault.verify(false).unwrap();
        assert!(report.ops > 0);
        (plan, run)
    }

    /// The size of the original attached to `id`, from the blob store.
    fn original_size(&self, id: Id) -> u64 {
        let original = self
            .vault
            .read()
            .attachments(id)
            .unwrap()
            .into_iter()
            .find(|a| a.role == AttachmentRole::Original)
            .expect("an original attachment");
        self.vault
            .blobs()
            .size(&original.blob_hash)
            .unwrap()
            .expect("the blob is stored")
    }

    /// Nothing outside the vault and the sources changed: the folder beside them is as it was,
    /// and the temp folder holds the same entries.
    fn nothing_left_outside(&self) {
        assert_eq!(
            fs::read(self.outside.join("precious.txt")).unwrap(),
            b"keep"
        );
        let after = snapshot(self.dir.path());
        assert_eq!(
            after.keys().collect::<Vec<_>>(),
            self.before.keys().collect::<Vec<_>>(),
            "the temp folder's entries"
        );
        assert_eq!(after.get("outside"), self.before.get("outside"));
        assert_eq!(
            after.get("sources"),
            snapshot(self.dir.path()).get("sources")
        );
    }
}

#[test]
fn malformed_pdfs_are_attached_whole_with_their_text_or_a_warning_inside_the_budget() {
    let mut bench = Bench::new();
    let whole = two_pages_pdf();
    let truncated = fixture_file(
        "malicious-import/truncated.pdf",
        &whole[..whole.len() * 3 / 5],
    );
    let recursive = fixture_file("malicious-import/recursive.pdf", &recursive_pdf());
    let bomb = fixture_file("malicious-import/page-tree-bomb.pdf", &page_tree_bomb());
    assert!(
        fs::metadata(&bomb).unwrap().len() < 100_000,
        "the bomb is small"
    );

    let paths = [truncated, recursive, bomb];
    // The text budget is 10 s per file; the apply and the verify take the rest.
    let (plan, run) = bench.import(&paths, Duration::from_secs(60));
    assert_eq!(plan.entries.len(), 3, "{:?}", plan.refused);
    assert_eq!(run.created.len(), 3);
    for (entry, id) in plan.entries.iter().zip(&run.created) {
        assert_eq!(entry.importer, "pdf");
        let object = &entry.extract.objects[0];
        assert_eq!(object.type_name, "Document");
        assert_eq!(object.attachments.len(), 1, "only the original");
        assert_eq!(
            bench.original_size(*id),
            fs::metadata(&entry.source.path).unwrap().len(),
            "{} is attached whole",
            entry.source.path.display()
        );
    }
    bench.nothing_left_outside();
}

#[test]
fn a_png_that_declares_ten_billion_pixels_is_a_file_with_a_warning_and_no_thumbnail() {
    let mut bench = Bench::new();
    let bomb = fixture_file(
        "malicious-import/png-bomb.png",
        &png_header_only(100_000, 100_000),
    );
    let (plan, run) = bench.import(std::slice::from_ref(&bomb), Duration::from_secs(10));
    let entry = &plan.entries[0];
    assert_eq!(entry.importer, "image");
    let object = &entry.extract.objects[0];
    assert_eq!(object.type_name, "Media");
    assert_eq!(object.attachments.len(), 1, "no thumbnail: {object:?}");
    assert!(
        entry.extract.warnings.iter().any(|w| w.contains("pixel")),
        "{:?}",
        entry.extract.warnings
    );
    assert_eq!(
        bench.original_size(run.created[0]),
        fs::metadata(&bomb).unwrap().len()
    );
    bench.nothing_left_outside();
}

#[test]
fn formula_cells_are_kept_as_text_and_counted_and_a_huge_cell_is_cut() {
    let mut bench = Bench::new();
    let formulas = fixture_file(
        "malicious-import/formulas.csv",
        concat!(
            "Name,Note,Amount\n",
            "Ada,\"=HYPERLINK(\"\"http://x\"\",\"\"go\"\")\",=1+1\n",
            "Bob,@SUM(A1:A2),+cmd|' /C calc'!A0\n",
            "Cy,-2+3,-5\n",
        )
        .as_bytes(),
    );
    let mut huge = b"Name,Blob\nA,".to_vec();
    huge.extend(std::iter::repeat_n(b'x', MAX_CELL_BYTES + 10));
    huge.extend_from_slice(b"\nB,small\n");
    let big_cell = bench.file("big-cell.csv", &huge);

    let (plan, run) = bench.import(&[formulas, big_cell.clone()], Duration::from_secs(20));
    assert_eq!(plan.entries.len(), 2, "{:?}", plan.refused);

    let table = &plan.entries[0].extract;
    assert!(
        table
            .warnings
            .iter()
            .any(|w| w.contains("5 cell(s) begin with =, @, + or - like a formula")),
        "{:?}",
        table.warnings
    );
    let rows = &table.objects[0].children;
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].props["note"],
        json!("=HYPERLINK(\"http://x\",\"go\")")
    );
    assert_eq!(rows[1].props["note"], json!("@SUM(A1:A2)"));
    assert_eq!(rows[1].props["amount"], json!("+cmd|' /C calc'!A0"));
    // Three cells, one a number: the column stays text and the number is text too.
    assert_eq!(rows[2].props["amount"], json!("-5"));

    let big = &plan.entries[1].extract;
    assert!(
        big.warnings.iter().any(|w| w.contains("1 cell(s) over")),
        "{:?}",
        big.warnings
    );
    let blob = big.objects[0].children[0].props["blob"].as_str().unwrap();
    assert_eq!(blob.len(), MAX_CELL_BYTES);
    assert_eq!(
        bench.original_size(run.created[1]),
        fs::metadata(&big_cell).unwrap().len()
    );
    bench.nothing_left_outside();
}

#[test]
fn a_sheet_named_like_a_path_is_a_type_name_and_nothing_more() {
    let mut bench = Bench::new();
    let workbook = fixture_file(
        "malicious-import/sheet-traversal.xlsx",
        &one_sheet_workbook("../x"),
    );
    let (plan, run) = bench.import(std::slice::from_ref(&workbook), Duration::from_secs(10));
    let entry = &plan.entries[0];
    assert_eq!(entry.importer, "xlsx");
    assert_eq!(entry.extract.types[0].name, "../x");
    let sheet = &entry.extract.objects[0].children[0];
    assert_eq!(sheet.title, "../x");
    assert!(sheet.provenance.source_uri.ends_with("#sheet=../x"));
    assert_eq!(sheet.children.len(), 1);
    assert_eq!(run.objects, 3);
    let types = bench.vault.read().types().unwrap();
    assert!(types.iter().any(|t| t.name == "../x"), "the type is a name");
    bench.nothing_left_outside();
}

#[test]
fn a_nested_zip_of_zeros_is_a_blob_of_its_own_size_and_nothing_is_unpacked() {
    let mut bench = Bench::new();
    let zeros = vec![0u8; 1 << 20];
    let inner = zip_store(&[("zeros.bin", zeros.as_slice())]);
    let middle = zip_store(&[("inner.zip", inner.as_slice())]);
    let outer = zip_store(&[("middle.zip", middle.as_slice())]);
    let bomb = bench.file("bomb.zip", &outer);

    let (plan, run) = bench.import(std::slice::from_ref(&bomb), Duration::from_secs(10));
    let entry = &plan.entries[0];
    assert_eq!(entry.importer, "file");
    let object = &entry.extract.objects[0];
    assert_eq!(object.type_name, "Document");
    assert!(object.children.is_empty() && object.blocks.is_empty());
    assert_eq!(object.attachments.len(), 1);
    assert_eq!(run.objects, 1);
    assert_eq!(bench.original_size(run.created[0]), outer.len() as u64);
    bench.nothing_left_outside();
}

#[test]
fn the_files_folders_hostile_fixtures_import_through_the_engine_and_stay_inside() {
    let mut bench = Bench::new();
    let folder = fixture("malicious");
    let (plan, run) = bench.import(std::slice::from_ref(&folder), Duration::from_secs(30));
    assert_eq!(plan.entries[0].importer, "markdown");
    let folder_plan = plan.entries[0].extract.folder.as_ref().unwrap();
    assert_eq!(
        folder_plan.markdown_files, 5,
        "four hostile files and the README"
    );
    // The two the parser refuses are warnings of the run; the other three are pages.
    assert!(
        run.warnings.iter().any(|w| w.contains("deep-yaml.md"))
            && run
                .warnings
                .iter()
                .any(|w| w.contains("oversized-frontmatter.md")),
        "{:?}",
        run.warnings
    );
    assert_eq!(run.objects, 3, "{run:?}");
    let read = bench.vault.read();
    for id in &run.created {
        let object = read.object(*id).unwrap();
        assert!(
            !object.title.contains(".."),
            "a title with a path in it: {object:?}"
        );
    }
    bench.nothing_left_outside();
}
