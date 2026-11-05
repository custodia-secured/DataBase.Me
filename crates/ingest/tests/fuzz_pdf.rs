// SPDX-License-Identifier: AGPL-3.0-or-later
//! The PDF importer never panics (M6 Task 9, brief §10): byte-level mutations of the two-page
//! fixture, arbitrary bytes behind a `%PDF-1.4` header, and arbitrary bytes plain, each up to
//! 16 KiB. Every case returns inside its text budget plus a bound, and always as one `Document`
//! with the original attached: a file the parser refuses is a warning, never an error or a
//! dropped file (M6 design, "PDF text"). The second layer is the `pdf` cargo-fuzz target in
//! `fuzz/`.

#![allow(clippy::unwrap_used, reason = "tests: a failure here is the finding")]

mod common;

use std::time::{Duration, Instant};

use common::two_pages_pdf;
use dbme_ingest::probe;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

/// The text budget each case gets.
const BUDGET: Duration = Duration::from_secs(1);
/// What a case may take beyond it: the importer's grace before it leaves a stuck parser behind
/// (`PARSER_GRACE`, ten seconds), and a little for a busy machine.
const SLACK: Duration = Duration::from_secs(12);

fn check(bytes: &[u8]) -> Result<(), TestCaseError> {
    let started = Instant::now();
    let extract = probe::pdf("fuzz.pdf", bytes, BUDGET);
    let took = started.elapsed();
    prop_assert!(took < BUDGET + SLACK, "{} bytes took {took:?}", bytes.len());
    prop_assert_eq!(extract.objects.len(), 1);
    let object = &extract.objects[0];
    prop_assert_eq!(object.type_name.as_str(), "Document");
    prop_assert!(!object.title.is_empty());
    prop_assert_eq!(object.attachments.len(), 1, "the original and nothing else");
    prop_assert!(object.provenance.confidence > 0.0 && object.provenance.confidence <= 1.0);
    if let Some(pages) = object.props.get("page_count") {
        prop_assert!(pages.is_u64(), "{pages}");
    }
    for block in &object.blocks {
        prop_assert!(
            block.kind == "heading" || block.kind == "paragraph",
            "{block:?}"
        );
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum Mutation {
    Flip { at: usize, byte: u8 },
    Delete { at: usize, len: usize },
    Insert { at: usize, bytes: Vec<u8> },
    Truncate { at: usize },
    Swap { a: usize, b: usize, len: usize },
}

fn arb_mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        3 => (any::<usize>(), any::<u8>()).prop_map(|(at, byte)| Mutation::Flip { at, byte }),
        2 => (any::<usize>(), 1..64usize).prop_map(|(at, len)| Mutation::Delete { at, len }),
        2 => (any::<usize>(), prop::collection::vec(any::<u8>(), 1..16))
            .prop_map(|(at, bytes)| Mutation::Insert { at, bytes }),
        1 => any::<usize>().prop_map(|at| Mutation::Truncate { at }),
        1 => (any::<usize>(), any::<usize>(), 1..64usize)
            .prop_map(|(a, b, len)| Mutation::Swap { a, b, len }),
    ]
}

fn mutate(bytes: &mut Vec<u8>, mutation: &Mutation) {
    if bytes.is_empty() {
        bytes.push(b'%');
    }
    let n = bytes.len();
    match mutation {
        Mutation::Flip { at, byte } => bytes[at % n] = *byte,
        Mutation::Delete { at, len } => {
            let at = at % n;
            bytes.drain(at..(at + len).min(n));
        }
        Mutation::Insert { at, bytes: new } => {
            let at = at % (n + 1);
            bytes.splice(at..at, new.iter().copied());
        }
        Mutation::Truncate { at } => bytes.truncate(at % n),
        Mutation::Swap { a, b, len } => {
            let len = (*len).min(n / 2).max(1);
            let a = a % (n - len + 1);
            let b = b % (n - len + 1);
            let chunk: Vec<u8> = bytes[a..a + len].to_vec();
            let other: Vec<u8> = bytes[b..b + len].to_vec();
            bytes[a..a + len].copy_from_slice(&other);
            bytes[b..b + len].copy_from_slice(&chunk);
        }
    }
}

/// PDF syntax, dense: the tokens a parser branches on.
const PDF: &[&str] = &[
    "<<",
    ">>",
    "/Type",
    "/Pages",
    "/Page",
    "/Kids",
    "/Count",
    "/Parent",
    "/Contents",
    "/Length",
    "/Filter",
    "/FlateDecode",
    "/Root",
    "/Info",
    "/Title",
    "/Font",
    "/Resources",
    " 0 R",
    " 0 obj",
    "endobj",
    "stream\n",
    "endstream",
    "xref",
    "trailer",
    "startxref",
    "%%EOF",
    "[",
    "]",
    "(",
    ")",
    "<",
    ">",
    "\\",
    "\n",
    " ",
    "1",
    "2",
    "3",
    "9999999999",
    "-1",
    "0.5",
    "BT",
    "ET",
    "Tj",
    "Tf",
    "Td",
    "true",
    "null",
    "#",
    "é",
    "\0",
    "\u{FEFF}",
    "(Hello)",
    "<FEFF0041>",
];

fn pdf_noise(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop::sample::select(PDF), 0..max)
        .prop_map(|v| format!("%PDF-1.4\n{}", v.concat()).into_bytes())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 48,
        ..ProptestConfig::default()
    })]

    #[test]
    fn mutated_pdfs_never_panic(mutations in prop::collection::vec(arb_mutation(), 1..8)) {
        let mut bytes = two_pages_pdf();
        for mutation in &mutations {
            mutate(&mut bytes, mutation);
        }
        check(&bytes)?;
    }

    #[test]
    fn pdf_noise_never_panics(bytes in pdf_noise(300)) {
        check(&bytes)?;
    }

    #[test]
    fn arbitrary_bytes_never_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..16_384),
        header in any::<bool>(),
    ) {
        let bytes = if header {
            [b"%PDF-1.4\n".as_slice(), &bytes].concat()
        } else {
            bytes
        };
        check(&bytes)?;
    }
}

#[test]
fn the_fixture_itself_passes_the_checks() {
    check(&two_pages_pdf()).unwrap();
    check(b"").unwrap();
    check(b"%PDF-1.4\n").unwrap();
}
