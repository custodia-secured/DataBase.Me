// SPDX-License-Identifier: AGPL-3.0-or-later
//! Golden tests for the query compiler (ADR-0020): every file in `tests/golden/` holds a query and
//! the SQL and parameters it compiles to. A change to the compiler shows up as a diff here.
//!
//! Run with `DBME_WRITE_GOLDEN=1` to write what the compiler produces now, then read the diff.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::path::{Path, PathBuf};

use dbme_core::Id;
use dbme_core::query::Query;
use dbme_store::{Select, compile};
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum What {
    #[default]
    Ids,
    Count,
    FacetTypes,
    FacetField,
}

#[derive(Serialize, Deserialize)]
struct Case {
    /// What the case is about.
    #[serde(default)]
    about: String,
    query: Query,
    #[serde(default)]
    select: What,
    /// The field a `facet_field` case counts.
    #[serde(default)]
    field: String,
    /// The types of `query.from.types` with their subtypes, as the store would look them up.
    #[serde(default)]
    types: Vec<Id>,
    #[serde(default)]
    sql: String,
    #[serde(default)]
    params: Vec<Value>,
}

fn json_of(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Integer(i) => Value::from(*i),
        SqlValue::Real(f) => Value::from(*f),
        SqlValue::Text(t) => Value::String(t.clone()),
        SqlValue::Blob(_) => Value::String("<blob>".into()),
    }
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn every_golden_query_compiles_to_its_recorded_sql() {
    let write = std::env::var_os("DBME_WRITE_GOLDEN").is_some();
    let mut files: Vec<PathBuf> = std::fs::read_dir(golden_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 10, "the goldens are missing");
    let mut stale = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let mut case: Case = serde_json::from_str(&text).unwrap();
        let types = if case.types.is_empty() {
            case.query.from.types.clone()
        } else {
            case.types.clone()
        };
        let select = match case.select {
            What::Ids => Select::Ids,
            What::Count => Select::Count,
            What::FacetTypes => Select::FacetTypes,
            What::FacetField => Select::FacetField,
        };
        let compiled = compile(&case.query, &types, select, &case.field).unwrap();
        let params: Vec<Value> = compiled.params.iter().map(json_of).collect();
        if compiled.sql != case.sql || params != case.params {
            stale.push(path.file_name().unwrap().to_string_lossy().into_owned());
            if write {
                case.sql = compiled.sql;
                case.params = params;
                let mut text = serde_json::to_string_pretty(&case).unwrap();
                text.push('\n');
                std::fs::write(&path, text).unwrap();
            } else {
                eprintln!(
                    "{}:\n  recorded: {}\n  now:      {}\n  recorded params: {:?}\n  now params:      {params:?}",
                    path.display(),
                    case.sql,
                    compiled.sql,
                    case.params
                );
            }
        }
    }
    if write {
        eprintln!("wrote {} golden file(s)", stale.len());
    } else {
        assert!(
            stale.is_empty(),
            "stale goldens: {stale:?}; run with DBME_WRITE_GOLDEN=1 and read the diff"
        );
    }
}
