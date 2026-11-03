// SPDX-License-Identifier: AGPL-3.0-or-later
//! The CSV importer never panics (M6 Task 9, brief §10): arbitrary bytes up to 64 KiB, CSV-shaped
//! noise, and byte-level mutations of `fixtures/csv/people.csv`. Every case returns inside a
//! bound; when it is `Ok`, the dataset holds one object per row, every row has a title, and every
//! property fits the kind its drafted field declares (the invariant the apply relies on). The
//! second layer is the `csv` cargo-fuzz target in `fuzz/`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

mod common;

use std::time::{Duration, Instant};

use common::fixture;
use dbme_core::Id;
use dbme_core::model::FieldRow;
use dbme_core::value::validate_field_value;
use dbme_ingest::probe;
use dbme_ingest::tables::KEPT_AS_TEXT;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use serde_json::json;

/// Each case returns inside this.
const BOUND: Duration = Duration::from_secs(5);

/// Runs the importer on `bytes` and checks the invariants of an `Ok`.
fn check(bytes: &[u8]) -> Result<(), TestCaseError> {
    let started = Instant::now();
    let outcome = probe::csv("fuzz.csv", bytes);
    let took = started.elapsed();
    prop_assert!(took < BOUND, "{} bytes took {took:?}", bytes.len());
    let Ok(extract) = outcome else {
        return Ok(());
    };
    prop_assert_eq!(extract.objects.len(), 1);
    prop_assert_eq!(extract.types.len(), 1);
    let dataset = &extract.objects[0];
    let draft = &extract.types[0];
    prop_assert!(!draft.name.is_empty());
    for row in &dataset.children {
        prop_assert!(!row.title.is_empty(), "{row:?}");
        prop_assert_eq!(row.type_name.as_str(), draft.name.as_str());
        for (key, value) in &row.props {
            if key == KEPT_AS_TEXT {
                prop_assert!(value.is_object(), "{key}: {value}");
                continue;
            }
            let field = draft.fields.iter().find(|f| &f.key == key);
            prop_assert!(
                field.is_some(),
                "a value for {key}, which no field declares"
            );
            let field = field.unwrap();
            let row = FieldRow {
                id: Id::from_parts(1, 1),
                type_id: Id::from_parts(1, 2),
                key: field.key.clone(),
                label: field.label.clone(),
                kind: field.kind,
                options_json: json!({}),
                required: false,
                sort_order: 0,
            };
            prop_assert!(
                validate_field_value(&row, value).is_ok(),
                "{key} = {value} does not fit {:?}",
                field.kind
            );
        }
    }
    Ok(())
}

/// CSV's structural characters, dense, with what the inference looks at.
const CSV: &[&str] = &[
    ",",
    ";",
    "\t",
    "\n",
    "\r\n",
    "\"",
    "\"\"",
    " ",
    "a",
    "é",
    "名",
    "1",
    "-1",
    "+",
    "-",
    "=",
    "@",
    "1.5e3",
    "0755",
    "true",
    "no",
    "2025-02-30",
    "2025-01-15",
    "2025-01-15T10:00:00Z",
    "n/a",
    "http://x",
    "a@b.c",
    "\u{FEFF}",
    "\0",
    "\\",
];

fn noise(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(CSV), 0..max).prop_map(|v| v.concat())
}

#[derive(Clone, Debug)]
enum Mutation {
    Flip { at: usize, byte: u8 },
    Delete { at: usize, len: usize },
    Insert { at: usize, bytes: Vec<u8> },
    Truncate { at: usize },
    Repeat { at: usize, times: usize },
}

fn arb_mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        3 => (any::<usize>(), any::<u8>()).prop_map(|(at, byte)| Mutation::Flip { at, byte }),
        2 => (any::<usize>(), 1..40usize).prop_map(|(at, len)| Mutation::Delete { at, len }),
        2 => (any::<usize>(), prop::collection::vec(any::<u8>(), 1..12))
            .prop_map(|(at, bytes)| Mutation::Insert { at, bytes }),
        1 => any::<usize>().prop_map(|at| Mutation::Truncate { at }),
        1 => (any::<usize>(), 2..50usize).prop_map(|(at, times)| Mutation::Repeat { at, times }),
    ]
}

fn mutate(bytes: &mut Vec<u8>, mutation: &Mutation) {
    if bytes.is_empty() {
        bytes.push(b',');
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
        Mutation::Repeat { at, times } => {
            let at = at % n;
            let line_end = bytes[at..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(n, |p| at + p + 1);
            let line = bytes[at..line_end].to_vec();
            for _ in 0..*times {
                bytes.extend_from_slice(&line);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 200,
        ..ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..65_536)) {
        check(&bytes)?;
    }

    #[test]
    fn csv_noise_never_panics(text in noise(600)) {
        check(text.as_bytes())?;
    }

    #[test]
    fn mutated_tables_never_panic(mutations in prop::collection::vec(arb_mutation(), 1..8)) {
        let mut bytes = std::fs::read(fixture("csv/people.csv")).unwrap();
        for mutation in &mutations {
            mutate(&mut bytes, mutation);
        }
        check(&bytes)?;
    }
}

#[test]
fn the_fixture_itself_passes_the_checks() {
    check(&std::fs::read(fixture("csv/people.csv")).unwrap()).unwrap();
    check(b"").unwrap();
    check(b"\n\n").unwrap();
    check(b"a").unwrap();
}
