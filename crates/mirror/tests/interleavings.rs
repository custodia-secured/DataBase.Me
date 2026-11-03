// SPDX-License-Identifier: AGPL-3.0-or-later
//! The M5 acceptance property (brief §8): random interleavings of app and file edits lose zero
//! content. Each case is a fresh vault of a few pages with a few paragraphs each, mirrored to the
//! in-memory file system, and a random sequence of steps: an app edit of one paragraph, a file
//! edit of one paragraph, a `pump_out`, a sync (`rescan`, `pump_in`, `pump_out`, as the app's
//! "Sync now" does), and an undo. Every edit writes a fresh token, so each value names the side
//! and the step that wrote it.
//!
//! The expectation is a model of the folder's contract, not of its code: three copies of every
//! paragraph (the vault's, the file's, and the base both sides last agreed on), where a pump
//! writes the vault's copy to the file unless the file changed on disk since it was last read (then
//! the write waits), and a sync takes a changed file in by a three-way merge per paragraph, the
//! app's value winning a true conflict and the file's going to a conflict copy. An undo reverts
//! the app's last step when the paragraph still holds its value, and is `UndoBlocked` when the
//! folder changed it since (M5 Decision 2).
//!
//! At quiescence: every paragraph holds the model's value; every token a conflict sent to a copy
//! is in some conflict copy; every file holds its page's tokens; no block holds a value no side
//! wrote; every op carries the app's device or the folder's; `verify(true)` passes.
//!
//! `DBME_INTERLEAVINGS` sets the number of cases (200 by default; CI's release step runs
//! 10,000). A failure prints the case's seed and its steps; `DBME_INTERLEAVINGS_SEED` replays
//! one seed.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

mod common;

use std::collections::BTreeSet;
use std::fmt;
use std::time::{Duration, Instant};

use common::{World, new_world};
use dbme_core::Id;
use dbme_mirror::{Inbound, Outbound};
use dbme_store::StoreError;
use serde_json::json;

const DEFAULT_CASES: usize = 200;
const PAGES: usize = 3;
const PARAGRAPHS: usize = 3;
const MIN_STEPS: usize = 6;
const MAX_STEPS: usize = 40;

// ---------------------------------------------------------------------------------------------
// A small deterministic generator (splitmix64), so a seed names a case exactly.
// ---------------------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    AppEdit(usize, usize),
    FileEdit(usize, usize),
    PumpOut,
    Sync,
    Undo,
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppEdit(o, p) => write!(f, "app edits page {o} paragraph {p}"),
            Self::FileEdit(o, p) => write!(f, "file edits page {o} paragraph {p}"),
            Self::PumpOut => write!(f, "pump out"),
            Self::Sync => write!(f, "sync"),
            Self::Undo => write!(f, "undo"),
        }
    }
}

fn step(rng: &mut Rng) -> Step {
    match rng.below(10) {
        0..=2 => Step::AppEdit(rng.below(PAGES), rng.below(PARAGRAPHS)),
        3..=5 => Step::FileEdit(rng.below(PAGES), rng.below(PARAGRAPHS)),
        6 => Step::PumpOut,
        7..=8 => Step::Sync,
        _ => Step::Undo,
    }
}

// ---------------------------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Model {
    vault: Vec<Vec<String>>,
    file: Vec<Vec<String>>,
    base: Vec<Vec<String>>,
    /// Pages with an app change the folder has not written.
    dirty: Vec<bool>,
    /// Tokens a true conflict sent to a conflict copy.
    copies: Vec<String>,
    /// The app's undo stack: page, paragraph, the value before, the value written.
    undo: Vec<(usize, usize, String, String)>,
    /// Every value either side ever wrote.
    known: BTreeSet<String>,
}

impl Model {
    fn new(initial: &[Vec<String>]) -> Self {
        Self {
            vault: initial.to_vec(),
            file: initial.to_vec(),
            base: initial.to_vec(),
            dirty: vec![false; initial.len()],
            known: initial.iter().flatten().cloned().collect(),
            ..Self::default()
        }
    }

    /// `pump_out` for one page: the vault's copy reaches the file unless the file changed since
    /// it was last read.
    fn pump_out(&mut self, o: usize) {
        if !self.dirty[o] || self.file[o] != self.base[o] {
            return;
        }
        self.file[o] = self.vault[o].clone();
        self.base[o] = self.vault[o].clone();
        self.dirty[o] = false;
    }

    /// A sync for one page: a changed file merges in, paragraph by paragraph; then the vault's
    /// copy is written if it differs from the file.
    fn sync(&mut self, o: usize) {
        if self.file[o] != self.base[o] {
            for p in 0..PARAGRAPHS {
                let (f, v, b) = (&self.file[o][p], &self.vault[o][p], &self.base[o][p]);
                let merged = if f == b || v == f {
                    v.clone()
                } else if v == b {
                    f.clone()
                } else {
                    self.copies.push(f.clone());
                    v.clone()
                };
                self.vault[o][p] = merged;
            }
            self.base[o] = self.file[o].clone();
        }
        if self.vault[o] != self.file[o] {
            self.file[o] = self.vault[o].clone();
            self.base[o] = self.vault[o].clone();
        }
        self.dirty[o] = false;
    }
}

// ---------------------------------------------------------------------------------------------
// One case
// ---------------------------------------------------------------------------------------------

struct Case {
    w: World,
    pages: Vec<Id>,
    paragraphs: Vec<Vec<Id>>,
    model: Model,
    steps: Vec<Step>,
    seed: u64,
    blocked_undos: usize,
    deferred_writes: usize,
}

fn text_of(w: &World, block: Id) -> String {
    w.vault.read().block(block).unwrap().content_json["inline"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

impl Case {
    fn new(seed: u64) -> Self {
        let mut w = new_world();
        let mut pages = Vec::new();
        let mut paragraphs = Vec::new();
        let mut initial = Vec::new();
        for o in 0..PAGES {
            let page = w.object(&format!("Page {o}"), None);
            let mut ids = Vec::new();
            let mut tokens = Vec::new();
            for p in 0..PARAGRAPHS {
                let token = format!("start{o}{p}");
                ids.push(w.paragraph(page, &token));
                tokens.push(token);
            }
            pages.push(page);
            paragraphs.push(ids);
            initial.push(tokens);
        }
        let out = w.pump_out();
        assert_eq!(out.written.len(), PAGES, "{out:?}");
        Self {
            w,
            pages,
            paragraphs,
            model: Model::new(&initial),
            steps: Vec::new(),
            seed,
            blocked_undos: 0,
            deferred_writes: 0,
        }
    }

    fn fail(&self, what: &str) -> String {
        let steps: Vec<String> = self.steps.iter().map(ToString::to_string).collect();
        format!(
            "{what}\nseed {}: {} steps\n  {}\nfiles:\n{}",
            self.seed,
            steps.len(),
            steps.join("\n  "),
            self.w
                .files()
                .iter()
                .map(|p| format!("--- {p}\n{}", self.w.text(p)))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }

    fn path_of(&self, o: usize) -> String {
        self.w
            .vault
            .mirror_state(self.pages[o])
            .unwrap()
            .unwrap()
            .rel_path
    }

    fn apply(&mut self, step: Step) {
        let n = self.steps.len();
        self.steps.push(step);
        self.w.tick();
        match step {
            Step::AppEdit(o, p) => {
                let token = format!("app{n}");
                let block = self.paragraphs[o][p];
                self.w
                    .vault
                    .transact(|txn| {
                        txn.update_block(block, |b| {
                            b.content_json = json!({"inline": [{"text": token}]});
                        })
                    })
                    .unwrap();
                let token = format!("app{n}");
                let before = std::mem::replace(&mut self.model.vault[o][p], token.clone());
                self.model.undo.push((o, p, before, token.clone()));
                self.model.dirty[o] = true;
                self.model.known.insert(token);
            }
            Step::FileEdit(o, p) => {
                let token = format!("file{n}");
                let path = self.path_of(o);
                let anchor = format!(" ^{}", self.paragraphs[o][p]);
                let text = self.w.text(&path);
                let mut found = false;
                let edited: String = text
                    .split_inclusive('\n')
                    .map(|line| {
                        if line.trim_end_matches('\n').ends_with(&anchor) {
                            found = true;
                            format!("{token}{anchor}\n")
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect();
                assert!(
                    found,
                    "{}",
                    self.fail(&format!("no anchor {anchor} in {path}"))
                );
                self.w.mirror.fs_mut().put(&path, edited.as_bytes());
                self.model.file[o][p] = token.clone();
                self.model.known.insert(token);
            }
            Step::PumpOut => {
                let out = self.w.pump_out();
                self.check_out(&out);
                for o in 0..PAGES {
                    self.model.pump_out(o);
                }
            }
            Step::Sync => {
                self.sync();
            }
            Step::Undo => {
                let Some((o, p, before, token)) = self.model.undo.pop() else {
                    return;
                };
                match self.w.vault.undo() {
                    Ok(Some(_)) => {
                        assert_eq!(
                            self.model.vault[o][p],
                            token,
                            "{}",
                            self.fail(
                                "an undo went through although the folder changed the paragraph"
                            )
                        );
                        self.model.vault[o][p] = before;
                        self.model.dirty[o] = true;
                    }
                    Err(StoreError::UndoBlocked(_)) => {
                        self.blocked_undos += 1;
                        assert_ne!(
                            self.model.vault[o][p],
                            token,
                            "{}",
                            self.fail(
                                "an undo was blocked although the paragraph still held its value"
                            )
                        );
                    }
                    Ok(None) => panic!("{}", self.fail("nothing to undo")),
                    Err(e) => panic!("{}", self.fail(&format!("undo failed: {e}"))),
                }
            }
        }
    }

    fn sync(&mut self) -> (Inbound, Outbound) {
        let events = self.w.mirror.rescan(&self.w.vault).unwrap();
        let inbound = self.w.mirror.pump_in(&mut self.w.vault, events).unwrap();
        assert!(
            inbound.held.is_none(),
            "{}",
            self.fail(&format!("the guard held a small batch: {inbound:?}"))
        );
        let out = self.w.pump_out();
        self.check_out(&out);
        for o in 0..PAGES {
            self.model.sync(o);
        }
        (inbound, out)
    }

    fn check_out(&mut self, out: &Outbound) {
        self.deferred_writes += out.deferred.len();
        assert!(
            out.pending.is_empty() && out.trashed.is_empty() && out.removed.is_empty(),
            "{}",
            self.fail(&format!("an unexpected outbound: {out:?}"))
        );
    }

    /// Pumps both ways until nothing moves, then checks the vault and the folder against the
    /// model.
    fn settle_and_check(&mut self) {
        let mut quiet = false;
        for _ in 0..6 {
            let out = self.w.pump_out();
            self.check_out(&out);
            for o in 0..PAGES {
                self.model.pump_out(o);
            }
            let (inbound, out) = self.sync();
            if inbound == Inbound::default() && out.written.is_empty() && out.deferred.is_empty() {
                quiet = true;
                break;
            }
        }
        assert!(
            quiet,
            "{}",
            self.fail("the folder did not settle in six rounds")
        );

        // Every paragraph holds the model's value.
        for o in 0..PAGES {
            for p in 0..PARAGRAPHS {
                let actual = text_of(&self.w, self.paragraphs[o][p]);
                assert_eq!(
                    actual,
                    self.model.vault[o][p],
                    "{}",
                    self.fail(&format!("page {o} paragraph {p}"))
                );
            }
        }

        // Every block of every object, live or trashed, holds a value one side wrote; the
        // conflict copies between them hold every token a conflict sent there.
        let read = self.w.vault.read();
        let mut objects = read.children(None).unwrap();
        objects.extend(read.trash().unwrap());
        let mut in_copies: Vec<String> = Vec::new();
        for object in &objects {
            for block in read.blocks(object.id).unwrap() {
                let text = block.content_json["inline"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                assert!(
                    self.model.known.contains(&text),
                    "{}",
                    self.fail(&format!(
                        "{:?} holds {text:?}, which no side wrote",
                        object.title
                    ))
                );
                if object.title.contains("(conflict copy)") {
                    in_copies.push(text);
                }
            }
        }
        for token in &self.model.copies {
            assert!(
                in_copies.contains(token),
                "{}",
                self.fail(&format!("{token} was lost: no conflict copy holds it"))
            );
        }

        // The files hold what the vault holds.
        for o in 0..PAGES {
            let text = self.w.text(&self.path_of(o));
            for (p, token) in self.model.vault[o].iter().enumerate() {
                assert!(
                    text.contains(&format!("{token} ^{}", self.paragraphs[o][p])),
                    "{}",
                    self.fail(&format!("page {o}'s file lacks {token}"))
                );
            }
        }
        let all_text: String = self.w.files().iter().map(|p| self.w.text(p)).collect();
        for token in &self.model.copies {
            assert!(
                all_text.contains(token),
                "{}",
                self.fail(&format!("no file holds the conflict copy's {token}"))
            );
        }

        // Every op is the app's or the folder's.
        let app = common::identity().device_id;
        let folder = self.w.vault.mirror_identity().device_id;
        for record in read.history(u32::MAX).unwrap() {
            let device = record.entry.device_id;
            assert!(
                device == app || device == folder,
                "{}",
                self.fail(&format!("op {} carries device {device}", record.seq))
            );
        }
        self.w.vault.verify(true).unwrap();
    }
}

/// What the cases exercised, for the record: the property means little if the generator never
/// makes a conflict.
#[derive(Debug, Default)]
struct Tally {
    steps: usize,
    conflicts: usize,
    blocked_undos: usize,
    deferred_writes: usize,
}

fn run(seed: u64, tally: &mut Tally) {
    let mut rng = Rng(seed);
    let mut case = Case::new(seed);
    let count = MIN_STEPS + rng.below(MAX_STEPS - MIN_STEPS + 1);
    for _ in 0..count {
        let next = step(&mut rng);
        case.apply(next);
    }
    case.settle_and_check();
    tally.steps += count;
    tally.conflicts += case.model.copies.len();
    tally.blocked_undos += case.blocked_undos;
    tally.deferred_writes += case.deferred_writes;
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

#[test]
fn random_interleavings_of_app_and_file_edits_lose_nothing() {
    let mut tally = Tally::default();
    if let Some(seed) = env_number("DBME_INTERLEAVINGS_SEED") {
        run(seed, &mut tally);
        eprintln!("seed {seed}: settled; {tally:?}");
        return;
    }
    let cases = env_number("DBME_INTERLEAVINGS").map_or(DEFAULT_CASES, |n| n as usize);
    let started = Instant::now();
    let mut slowest = Duration::ZERO;
    for n in 0..cases {
        let seed = 0x5EED_0000_0000 + n as u64;
        let at = Instant::now();
        run(seed, &mut tally);
        slowest = slowest.max(at.elapsed());
    }
    let total = started.elapsed();
    // Gate 1f (M5): one interleavings case timed.
    eprintln!(
        "{cases} interleavings settled in {total:?}: {:?} a case on average, {slowest:?} the \
         slowest; {tally:?}",
        total / cases.max(1) as u32
    );
    assert!(
        cases < 50 || (tally.conflicts > 0 && tally.blocked_undos > 0 && tally.deferred_writes > 0),
        "the cases exercised too little: {tally:?}"
    );
}
