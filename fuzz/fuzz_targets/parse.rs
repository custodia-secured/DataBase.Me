// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme_core::files::parse` on arbitrary bytes (M5 acceptance: fuzzing of the Markdown and
//! frontmatter parser). The property is the same as the proptest layer's: it returns, `Ok` or
//! `Err`, and never panics or overflows the stack; an `Err` points inside the text. The schema
//! has one type with a field of every stored kind, so every decoder runs.

#![no_main]

use std::sync::LazyLock;

use dbme_core::Id;
use dbme_core::files::{NoResolver, Schema, parse};
use dbme_core::model::{FieldKind, FieldRow, TypeRow};
use libfuzzer_sys::fuzz_target;
use serde_json::json;

static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    let type_id = Id::from_parts(1_000, 900);
    let ty = TypeRow {
        id: type_id,
        name: "T".into(),
        plural: None,
        icon: None,
        color: None,
        extends: None,
        description: None,
        is_system: false,
        created_at: 1,
        updated_at: 1,
    };
    let fields = FieldKind::ALL
        .into_iter()
        .filter(|k| k.is_stored())
        .enumerate()
        .map(|(i, kind)| FieldRow {
            id: Id::from_parts(1_000, 1_000 + i as u128),
            type_id,
            key: kind.as_str().into(),
            label: kind.as_str().into(),
            kind,
            options_json: json!({}),
            required: false,
            sort_order: i as i64,
        })
        .collect();
    Schema::new(vec![ty], fields)
});

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Err(error) = parse(text, &SCHEMA, &NoResolver) {
        let lines = text.split('\n').count();
        assert!(
            error.line >= 1 && error.line <= lines + 1,
            "{error} points outside the {lines} lines"
        );
    }
});
