// SPDX-License-Identifier: AGPL-3.0-or-later
//! The M6 import budgets (M6 plan, "Budgets"; the brief names none, so these are informational
//! and gate nothing): 10,000 CSV rows imported in under 5 s, a 100-page text PDF in under 2 s,
//! and a 12-megapixel JPEG's thumbnail in under 500 ms, each as a plan and an apply into a fresh
//! vault, in a release build. The figures are printed; the assertions are on what was made, not
//! on time. CI runs this on Ubuntu with `--nocapture` and keeps the numbers in the log.
//!
//! Run with `cargo test --release -p dbme-ingest --test budget -- --nocapture`.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{HELVETICA, obj, page, pdf, text_content};
use dbme_core::{Clock, ManualClock};
use dbme_ingest::{ImportOptions, ImportRun, Importers, apply, plan};
use dbme_store::{Identity, Vault, VaultOptions};

const NOW: i64 = 1_762_128_000_000;

fn vault(dir: &Path) -> Vault {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(NOW));
    let options = VaultOptions {
        identity: Identity::generate(clock.as_ref()).unwrap(),
        clock,
    };
    Vault::create(&dir.join("Budget.dbme"), options).unwrap()
}

/// Plans and applies one source, and prints both times against the budget.
fn run(label: &str, vault: &mut Vault, source: PathBuf, budget: Duration) -> ImportRun {
    let started = Instant::now();
    let plan = plan(
        vault,
        std::slice::from_ref(&source),
        ImportOptions::default(),
        &Importers::v0(),
    )
    .unwrap();
    let planned = started.elapsed();
    let run = apply(vault, &plan).unwrap();
    let total = started.elapsed();
    let verdict = if total <= budget { "within" } else { "OVER" };
    println!(
        "budget {label}: plan {planned:?}, plan and apply {total:?}, {} objects in {} step(s); \
         {verdict} the {budget:?} budget ({})",
        run.objects,
        run.transactions,
        if cfg!(debug_assertions) {
            "debug build, informational"
        } else {
            "release build, informational"
        }
    );
    assert!(plan.refused.is_empty(), "{:?}", plan.refused);
    run
}

#[test]
fn ten_thousand_csv_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let mut csv = String::from("Name,Email,Joined,Score,Active,Notes\n");
    for n in 0..10_000u32 {
        let _ = writeln!(
            csv,
            "Person {n},p{n}@example.org,2025-{:02}-{:02},{}.{},{},Row {n} of the budget table",
            n % 12 + 1,
            n % 28 + 1,
            n % 100,
            n % 10,
            n % 2 == 0
        );
    }
    let source = dir.path().join("people.csv");
    fs::write(&source, csv).unwrap();
    let run = run(
        "10,000 CSV rows",
        &mut vault,
        source,
        Duration::from_secs(5),
    );
    assert_eq!(run.objects, 10_001, "the dataset and its rows");
    // A run splits at 10,000 objects but only between sources; one table is one step.
    assert_eq!(run.transactions, 1);
}

#[test]
fn a_hundred_page_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    // Objects: 1 the catalog, 2 the pages node, 3 the font, then a page and its content stream
    // per page: page n is object 4 + 2n, its content 5 + 2n.
    let pages = 100u32;
    // The font is object 3 here, where the shared helpers' `FONTS` expects 5.
    let fonts = "<< /Font << /F1 3 0 R >> >>";
    let mut objects = vec![
        obj("<< /Type /Catalog /Pages 2 0 R >>"),
        obj(&format!(
            "<< /Type /Pages /Kids [{}] /Count {pages} >>",
            (0..pages)
                .map(|n| format!("{} 0 R", 4 + 2 * n))
                .collect::<Vec<_>>()
                .join(" ")
        )),
        obj(HELVETICA),
    ];
    for n in 0..pages {
        objects.push(page(2, 5 + 2 * n, fonts));
        objects.push(text_content(&format!(
            "Page {} of the budget document, with a line of text to extract.",
            n + 1
        )));
    }
    let source = dir.path().join("hundred.pdf");
    fs::write(
        &source,
        pdf(&objects, Some("<< /Title (A hundred pages) >>")),
    )
    .unwrap();
    let run = run("100-page PDF", &mut vault, source, Duration::from_secs(2));
    assert_eq!(run.objects, 1);
    let read = vault.read();
    let blocks = read.blocks(run.created[0]).unwrap();
    assert_eq!(
        blocks.iter().filter(|b| b.kind == "heading").count(),
        pages as usize,
        "a heading per page"
    );
}

#[test]
fn a_twelve_megapixel_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = vault(dir.path());
    let (width, height) = (4000u32, 3000u32);
    let mut picture = image::RgbImage::new(width, height);
    for (x, y, pixel) in picture.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]);
    }
    let mut bytes = Vec::new();
    picture
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    let source = dir.path().join("large.jpg");
    fs::write(&source, &bytes).unwrap();
    println!("budget 12-megapixel JPEG: {} bytes encoded", bytes.len());
    let run = run(
        "12-megapixel JPEG",
        &mut vault,
        source,
        Duration::from_millis(500),
    );
    assert_eq!(run.objects, 1);
    let read = vault.read();
    let attachments = read.attachments(run.created[0]).unwrap();
    assert_eq!(attachments.len(), 2, "the original and the thumbnail");
}
