// SPDX-License-Identifier: AGPL-3.0-or-later
//! The fixpoint of the files format by property (M5 Task 2, spec §7 and §9): a generator over
//! every block kind (unknown kinds with children included), lists nested four deep with bullets,
//! numbers and todos mixed, every stored field kind against a schema of one type, reserved-key
//! collisions and non-identifier keys among the fields, `null` field values, identifier and
//! misfit envelope keys among the extras, relations and links, mentions with `shown`, `block` and `embed`
//! against a vault of known objects, and text drawn from a hostile alphabet. For every generated
//! document `d`: `parse(serialize(d)) == normalize(d)`, `serialize(parse(serialize(d))) ==
//! serialize(d)`, `normalize` is idempotent, and the text is LF-only, ends with exactly one
//! newline, has no `---` line outside the frontmatter and the code fences, and no line starting
//! with `^` that is not an anchor. A 1,000-block document's round trip is timed, informatively.
//!
//! What the generator leaves out, on purpose, because the format cannot tell the cases apart (so
//! no normal form could make them round-trip): a block whose anchor sits on its own line
//! (heading, quote, code, divider, unknown kind) always carries an id, else a following empty
//! anchored paragraph would be read as its anchor (Decision 11); an id-less paragraph or list
//! item always has some text, else it renders to nothing; and no text run holds `](`, since
//! `[text](url)` reads back as a link (spec §4).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use std::collections::BTreeMap;
use std::time::Instant;

use dbme_core::files::{
    FileBlock, FileDoc, Frontmatter, LinkEntry, LinkResolver, RESERVED_KEYS, RelationRef, Schema,
    WikiLink, normalize, parse, serialize,
};
use dbme_core::model::{FieldKind, FieldRow, Props, Sensitivity, TypeRow};
use dbme_core::{ContentHash, Id, SortKey};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------------------------------------
// The vault and the schema
// ---------------------------------------------------------------------------------------------

/// The objects the vault knows, by path without `.md`; object `n` (1-based) has `PATHS[n - 1]`.
const PATHS: [&str; 4] = [
    "Projects/Alpha",
    "People/Bravo Two",
    "Notes/2026/Charlie (draft)",
    "Archive/Délta — 名字",
];
/// Objects `1..=KNOWN` have files; `GHOST` is an object with none (written as its ULID).
const KNOWN: u128 = PATHS.len() as u128;
const GHOST: u128 = KNOWN + 1;

fn id(n: u128) -> Id {
    Id::from_parts(1_000, n)
}

struct Vault;

impl LinkResolver for Vault {
    fn resolve(&self, target: &str) -> Option<Id> {
        if let Some(i) = PATHS.iter().position(|p| *p == target) {
            return Some(id(i as u128 + 1));
        }
        let wanted = target.to_lowercase();
        let mut hits = PATHS
            .iter()
            .enumerate()
            .filter(|(_, p)| p.rsplit('/').next().unwrap().to_lowercase() == wanted);
        let (i, _) = hits.next()?;
        hits.next().is_none().then(|| id(i as u128 + 1))
    }

    fn path_of(&self, object: Id) -> Option<String> {
        PATHS
            .iter()
            .enumerate()
            .find(|(i, _)| id(*i as u128 + 1) == object)
            .map(|(_, p)| (*p).to_owned())
    }

    fn attachment_name(&self, blob: &ContentHash) -> String {
        format!("{}.png", blob.hex())
    }
}

/// The kinds whose values live in `props_json`, in FORMAT.md order.
fn stored_kinds() -> Vec<FieldKind> {
    FieldKind::ALL
        .into_iter()
        .filter(|k| k.is_stored())
        .collect()
}

/// One type `T` with one field of every stored kind, keyed by the kind's own name.
fn schema() -> Schema {
    let ty = TypeRow {
        id: id(900),
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
    let fields = stored_kinds()
        .into_iter()
        .enumerate()
        .map(|(i, kind)| FieldRow {
            id: id(1_000 + i as u128),
            type_id: id(900),
            key: kind.as_str().into(),
            label: kind.as_str().into(),
            kind,
            options_json: json!({}),
            required: false,
            sort_order: i as i64,
        })
        .collect();
    Schema::new(vec![ty], fields)
}

// ---------------------------------------------------------------------------------------------
// Alphabets and scalars
// ---------------------------------------------------------------------------------------------

/// Spec §9's hostile alphabet, plus digits and the punctuation the line-start rules name.
const HOSTILE: &[&str] = &[
    "*", "_", "~", "`", "#", "^", "-", "+", ">", "|", "[", "]", "(", ")", "!", "<", "&", "\\",
    "\n", "a", "b", "Z", "x", " ", "  ", "é", "—", "名", "1", "0", ".", ":", "=", "\"", "'", "\t",
    "/", "@", "%", "{", "}", ",", "?",
];

/// What a wikilink target or `shown` may hold: a file name's characters (`file_stem` forbids
/// `\ / : * ? " < > | # ^ [ ]` except the `/` separator), never `|`, `#`, `]` or a line break.
const TARGET: &[&str] = &[
    "a", "b", "Z", " ", "/", "(", ")", "é", "—", "名", "-", "_", ".", "'", "&", "+", ",", "1",
];

/// Characters of a link URL.
const URL: &[&str] = &[
    "a", "z", "1", ":", "/", ".", "?", "=", "&", "(", ")", "%", "#", "-", "_", "~", "*", " ", "é",
    "<", ">",
];

/// Characters of an image's alt text: no `]`, `|` or line break, which end the embed.
const ALT: &[&str] = &[
    "a", "b", " ", "é", "*", "_", "-", ".", "#", "`", "&", "\\", "<", "x", "(", ")", "~", "!", "[",
    "]", "|", "\n",
];

fn text_from(alphabet: &'static [&'static str], max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(alphabet), 0..max).prop_map(|v| v.concat())
}

fn hostile(max: usize) -> impl Strategy<Value = String> {
    text_from(HOSTILE, max)
}

/// A non-empty text from `alphabet`.
fn some_text(alphabet: &'static [&'static str], max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(alphabet), 1..max).prop_map(|v| v.concat())
}

/// A block or object id that never collides with the vault's objects.
fn arb_id() -> impl Strategy<Value = Id> {
    any::<u64>().prop_map(|n| Id::from_parts(1_000, u128::from(n) + 1_000_000))
}

/// A lower-case identifier that is neither reserved nor a field of the schema.
fn arb_key() -> impl Strategy<Value = String> {
    prop::sample::select(&["due", "status", "owner_name", "x1", "q", "zeta_9"][..])
        .prop_map(str::to_owned)
}

/// A JSON value for generic slots: scalars, arrays and objects two levels deep, no NaN.
fn arb_json(depth: u32) -> BoxedStrategy<Value> {
    let leaf = prop_oneof![
        1 => Just(Value::Null),
        2 => any::<bool>().prop_map(Value::Bool),
        3 => any::<i32>().prop_map(|i| json!(i)),
        1 => any::<i64>().prop_map(|i| json!(i)),
        2 => (-1.0e6f64..1.0e6).prop_map(|f| json!(f)),
        4 => hostile(12).prop_map(Value::String),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    prop_oneof![
        3 => leaf,
        1 => prop::collection::vec(arb_json(depth - 1), 0..4).prop_map(Value::Array),
        1 => arb_object(depth - 1),
    ]
    .boxed()
}

/// A JSON object with identifier-like and odd keys.
fn arb_object(depth: u32) -> BoxedStrategy<Value> {
    let keys = prop::sample::select(&["k", "n", "Zebra Key", "é", "list", "with: colon", ""][..]);
    prop::collection::btree_map(keys, arb_json(depth), 0..4)
        .prop_map(|m| {
            Value::Object(
                m.into_iter()
                    .map(|(k, v)| (k.to_owned(), v))
                    .collect::<Map<String, Value>>(),
            )
        })
        .boxed()
}

// ---------------------------------------------------------------------------------------------
// Inline runs
// ---------------------------------------------------------------------------------------------

/// A text run: hostile text, any subset of the five marks, sometimes a link.
fn text_run() -> impl Strategy<Value = Value> {
    (
        hostile(16),
        any::<u8>(),
        prop::option::weighted(0.15, some_text(URL, 16)),
    )
        .prop_map(|(text, bits, link)| {
            let mut run = Map::new();
            // `[text](url)` reads back as a link, which is its normal form (spec §4).
            run.insert("text".into(), Value::String(text.replace("](", "] (")));
            for (i, key) in ["bold", "italic", "underline", "strike", "code"]
                .into_iter()
                .enumerate()
            {
                if bits >> i & 1 == 1 {
                    run.insert(key.into(), Value::Bool(true));
                }
            }
            if let Some(url) = link {
                run.insert("link".into(), Value::String(url));
            }
            Value::Object(run)
        })
}

/// A mention of a known object or the ghost, with `shown`, `block` and `embed` at random.
fn mention_run() -> impl Strategy<Value = Value> {
    (
        1..=GHOST,
        prop::option::weighted(0.4, some_text(TARGET, 10)),
        prop::option::weighted(0.3, arb_id()),
        any::<bool>(),
    )
        .prop_map(|(n, shown, block, embed)| {
            let mut run = Map::new();
            run.insert("mention".into(), Value::String(id(n).to_string()));
            if let Some(shown) = shown {
                run.insert("shown".into(), Value::String(shown));
            }
            if let Some(block) = block {
                run.insert("block".into(), Value::String(block.to_string()));
            }
            if embed {
                run.insert("embed".into(), Value::Bool(true));
            }
            Value::Object(run)
        })
}

fn runs(max: usize) -> impl Strategy<Value = Vec<Value>> {
    prop::collection::vec(prop_oneof![5 => text_run(), 1 => mention_run()], 0..max)
}

// ---------------------------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------------------------

fn block_of(id: Option<Id>, kind: &str, content: Value, children: Vec<FileBlock>) -> FileBlock {
    FileBlock {
        id,
        kind: kind.to_owned(),
        content,
        children,
    }
}

/// A list item with its text, kind-specific keys and children.
fn item(
    id: Option<Id>,
    kind: &str,
    mut runs: Vec<Value>,
    checked: bool,
    children: Vec<FileBlock>,
) -> FileBlock {
    if id.is_none() {
        // An id-less empty item renders to a bare marker, which cannot carry a child.
        runs.insert(0, json!({ "text": "p" }));
    }
    let content = if kind == "todo" {
        json!({ "inline": runs, "checked": checked })
    } else {
        json!({ "inline": runs })
    };
    block_of(id, kind, content, children)
}

/// The kinds whose anchor sits on its own line always carry an id (see the module doc).
fn anchored_leaf() -> impl Strategy<Value = FileBlock> {
    let heading = (1..=6u64, runs(5)).prop_map(|(level, runs)| {
        block_of(
            None,
            "heading",
            json!({ "level": level, "inline": runs }),
            Vec::new(),
        )
    });
    let quote =
        runs(5).prop_map(|runs| block_of(None, "quote", json!({ "inline": runs }), Vec::new()));
    let language = prop::option::of(prop::sample::select(
        &["rust", "md", "a b", "x`y", "", "dbme", "é"][..],
    ));
    let code = (hostile(30), language).prop_map(|(code, language)| {
        let mut content = json!({ "code": code });
        if let Some(language) = language {
            content["language"] = Value::String(language.to_owned());
        }
        block_of(None, "code", content, Vec::new())
    });
    let divider = Just(block_of(None, "divider", json!({}), Vec::new()));
    (
        prop_oneof![3 => heading, 2 => quote, 2 => code, 1 => divider],
        arb_id(),
    )
        .prop_map(|(mut block, id)| {
            block.id = Some(id);
            block
        })
}

/// A paragraph or an image, with or without an id.
fn free_leaf() -> impl Strategy<Value = FileBlock> {
    let paragraph = (prop::option::weighted(0.8, arb_id()), runs(6)).prop_map(|(id, mut runs)| {
        if id.is_none() {
            // An id-less empty paragraph renders to nothing.
            runs.insert(0, json!({ "text": "p" }));
        }
        block_of(id, "paragraph", json!({ "inline": runs }), Vec::new())
    });
    let image = (
        prop::option::weighted(0.8, arb_id()),
        any::<[u8; 16]>(),
        prop::option::of(some_text(ALT, 10)),
    )
        .prop_map(|(id, bytes, alt)| {
            let mut content = json!({ "blob": ContentHash::of(&bytes).as_str() });
            if let Some(alt) = alt {
                content["alt"] = Value::String(alt);
            }
            block_of(id, "image", content, Vec::new())
        });
    prop_oneof![5 => paragraph, 1 => image]
}

/// Any block, with children when the kind takes them and `depth` allows (lists nest to four).
fn arb_block(depth: u32) -> BoxedStrategy<FileBlock> {
    let leaf = prop_oneof![3 => free_leaf().boxed(), 2 => anchored_leaf().boxed()];
    if depth >= 4 {
        return leaf.boxed();
    }
    let children = prop::collection::vec(arb_block(depth + 1), 0..=3);
    let list_item = (
        prop::option::weighted(0.8, arb_id()),
        prop::sample::select(&["bullet", "numbered", "todo"][..]),
        runs(5),
        any::<bool>(),
        children.clone(),
    )
        .prop_map(|(id, kind, runs, checked, children)| item(id, kind, runs, checked, children));
    let unknown = (
        arb_id(),
        prop::sample::select(&["chart", "kanban", "embed_x", "Weird Kind"][..]),
        arb_object(2),
        prop::collection::vec(arb_block(depth + 1), 0..=2),
    )
        .prop_map(|(id, kind, content, children)| block_of(Some(id), kind, content, children));
    prop_oneof![4 => leaf, 4 => list_item.boxed(), 1 => unknown.boxed()].boxed()
}

fn arb_body() -> impl Strategy<Value = Vec<FileBlock>> {
    prop::collection::vec(arb_block(0), 0..8)
}

// ---------------------------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------------------------

/// A value in the kind's JSON shape (FORMAT.md §3.3), hostile where the shape allows it.
fn field_value(kind: FieldKind) -> BoxedStrategy<Value> {
    let string = || hostile(14).prop_map(Value::String);
    let number = || {
        prop_oneof![
            2 => any::<i32>().prop_map(|i| json!(i)),
            1 => any::<i64>().prop_map(|i| json!(i)),
            2 => (-1.0e6f64..1.0e6).prop_map(|f| json!(f)),
        ]
    };
    match kind {
        FieldKind::Text
        | FieldKind::Url
        | FieldKind::Email
        | FieldKind::Phone
        | FieldKind::Select
        | FieldKind::Date
        | FieldKind::Datetime => string().boxed(),
        FieldKind::RichText => prop::collection::vec(arb_object(1), 0..3)
            .prop_map(Value::Array)
            .boxed(),
        FieldKind::Number => number().boxed(),
        FieldKind::Currency => (
            prop_oneof![
                2 => (0..100_000i64, 0..100u8).prop_map(|(a, c)| format!("{a}.{c:02}")),
                1 => hostile(8),
            ],
            prop::sample::select(&["EUR", "USD", "é"][..]),
        )
            .prop_map(|(amount, currency)| json!({ "amount": amount, "currency": currency }))
            .boxed(),
        FieldKind::Boolean => any::<bool>().prop_map(Value::Bool).boxed(),
        FieldKind::Duration | FieldKind::Rating => (0..1_000_000i64).prop_map(|i| json!(i)).boxed(),
        FieldKind::MultiSelect => prop::collection::vec(string(), 0..4)
            .prop_map(Value::Array)
            .boxed(),
        FieldKind::File => prop::collection::vec(any::<[u8; 8]>(), 0..3)
            .prop_map(|hashes| {
                Value::Array(
                    hashes
                        .iter()
                        .map(|b| Value::String(ContentHash::of(b).as_str().to_owned()))
                        .collect(),
                )
            })
            .boxed(),
        FieldKind::Geo => (number(), number())
            .prop_map(|(lat, lon)| json!({ "lat": lat, "lon": lon }))
            .boxed(),
        // `null` included: a null field is an absent field, which `normalize` establishes.
        FieldKind::Json => arb_json(2),
        FieldKind::Relation | FieldKind::Formula | FieldKind::Rollup => Just(Value::Null).boxed(),
    }
}

/// A generic value for a field the schema does not know, `null` included.
fn generic_value() -> impl Strategy<Value = Value> {
    arb_json(1)
}

/// User fields: a subset of the schema's kinds, then unknown identifiers, reserved-key
/// collisions and keys that are no identifier interleaved (a `BTreeMap` orders them by key), so
/// `normalize` has to sort them the way the reader will: reserved keys last (they are written
/// under `fields:`), non-identifiers among the extras.
fn arb_fields() -> impl Strategy<Value = Vec<(String, Value)>> {
    let typed = stored_kinds()
        .into_iter()
        .map(|kind| {
            prop::option::weighted(0.4, field_value(kind))
                .prop_map(move |v| v.map(|v| (kind.as_str().to_owned(), v)))
        })
        .collect::<Vec<_>>();
    let keys = prop_oneof![
        3 => arb_key(),
        2 => prop::sample::select(RESERVED_KEYS).prop_map(str::to_owned),
        1 => prop::sample::select(&["Due Date", "Status", "with: colon"][..]).prop_map(str::to_owned),
    ];
    let rest = prop::collection::btree_map(keys, generic_value(), 0..5);
    (typed, rest).prop_map(|(typed, rest)| typed.into_iter().flatten().chain(rest).collect())
}

fn arb_wikilink() -> impl Strategy<Value = WikiLink> {
    (
        some_text(TARGET, 12),
        prop::option::weighted(0.3, arb_id()),
        prop::option::weighted(0.3, some_text(TARGET, 8)),
    )
        .prop_map(|(target, block, shown)| WikiLink {
            target,
            block,
            shown,
        })
}

fn arb_relations() -> impl Strategy<Value = Vec<(String, Vec<RelationRef>)>> {
    let entry = (arb_id(), prop::option::of(arb_wikilink()))
        .prop_map(|(id, link)| RelationRef { id: Some(id), link });
    prop::collection::btree_map(arb_key(), prop::collection::vec(entry, 0..3), 0..3)
        .prop_map(|m| m.into_iter().collect())
}

fn arb_links() -> impl Strategy<Value = Vec<LinkEntry>> {
    let entry = (
        arb_id(),
        prop::option::of(arb_wikilink()),
        prop::sample::select(&["related", "cites", "blocks", "Weird Type"][..]),
        prop::option::of(hostile(10)),
        prop::collection::btree_map(arb_key(), arb_json(1), 0..3),
        prop::option::of(arb_id()),
    )
        .prop_map(|(id, link, rel_type, label, props, block)| LinkEntry {
            id: Some(id),
            link,
            rel_type: rel_type.to_owned(),
            label,
            props: props.into_iter().collect::<Props>(),
            block,
        });
    prop::collection::vec(entry, 0..3)
}

/// Extra keys: Obsidian's own and non-identifiers with any JSON value, `null` included; plus
/// unreserved identifiers (which the reader files as user fields), envelope keys with a value
/// that does not fit the slot (which the reader keeps as extras when the slot is unset, and the
/// writer drops when it is set) and a numeric `geo_lat`/`geo_lon` (the slot when both are there).
fn arb_extra() -> impl Strategy<Value = Vec<(String, Value)>> {
    let plain = prop::collection::btree_map(
        prop::sample::select(
            &[
                "aliases",
                "cssclasses",
                "Zebra Key",
                "Title",
                "my-key",
                "É",
                "with: colon",
                "123abc",
                "foo",
                "due",
            ][..],
        ),
        arb_json(2),
        0..4,
    )
    .prop_map(|m| {
        m.into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Vec<_>>()
    });
    let misfits = prop::collection::vec(0..9usize, 0..3).prop_map(|picks| {
        picks
            .into_iter()
            .map(|i| match i {
                0 => ("types", json!({"a": 1})),
                1 => ("id", json!("junk")),
                2 => ("created", json!("yesterday")),
                3 => ("updated", json!([1])),
                4 => ("sensitivity", json!("secret")),
                5 => ("fields", json!(3)),
                6 => ("links", json!({"a": 1})),
                7 => ("geo_lat", json!(48.85)),
                _ => ("geo_lon", json!(2)),
            })
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Vec<_>>()
    });
    (plain, misfits).prop_map(|(plain, misfits)| plain.into_iter().chain(misfits).collect())
}

/// Milliseconds within the years 1970..=9999, which format to four-digit years.
fn arb_ms() -> impl Strategy<Value = Option<i64>> {
    prop::option::of(0..=253_402_300_799_999i64)
}

fn arb_front() -> impl Strategy<Value = Frontmatter> {
    let envelope = (
        prop::option::weighted(0.8, arb_id()),
        prop::collection::vec(prop::sample::select(&["T", "Note", "Task"][..]), 1..3),
        prop::collection::vec(
            prop::sample::select(&["travel", "Admin", "x y", "123", "true", "é", "a#b"][..]),
            0..3,
        ),
        arb_ms(),
        arb_ms(),
        prop::option::of(hostile(16)),
        (arb_ms(), arb_ms(), arb_ms()),
        prop::option::of((-90.0f64..90.0, -180.0f64..180.0)),
        prop::sample::select(
            &[
                None,
                Some(Sensitivity::Normal),
                Some(Sensitivity::Sensitive),
            ][..],
        ),
        prop::option::of(prop::sample::select(&["a0", "a0V", "Zz"][..])),
    );
    let rest = (arb_fields(), arb_relations(), arb_links(), arb_extra());
    (envelope, rest).prop_map(
        |(
            (
                id,
                types,
                tags,
                created,
                updated,
                summary,
                (captured_at, event_start, event_end),
                geo,
                sensitivity,
                sort,
            ),
            (fields, relations, links, extra),
        )| Frontmatter {
            id,
            types: types.into_iter().map(str::to_owned).collect(),
            tags: tags.into_iter().map(str::to_owned).collect(),
            created,
            updated,
            summary,
            captured_at,
            event_start,
            event_end,
            geo,
            sensitivity,
            sort: sort.map(|s| SortKey::parse(s).unwrap()),
            fields,
            relations,
            links,
            extra,
        },
    )
}

fn arb_doc() -> impl Strategy<Value = FileDoc> {
    (arb_front(), arb_body()).prop_map(|(front, body)| FileDoc { front, body })
}

// ---------------------------------------------------------------------------------------------
// Checks
// ---------------------------------------------------------------------------------------------

/// The body of a serialized file: what follows the closing fence, or the whole text.
fn body_of(text: &str) -> &str {
    let Some(after) = text.strip_prefix("---\n") else {
        return text;
    };
    let mut pos = text.len() - after.len();
    while pos < text.len() {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
        if &text[pos..end] == "---" {
            return &text[(end + 1).min(text.len())..];
        }
        pos = end + 1;
    }
    text
}

/// The shape rules of a serialized file: LF only, exactly one trailing newline, and in the body
/// outside code fences no `---` line and no `^` line that is not an anchor.
fn check_shape(text: &str) -> Result<(), TestCaseError> {
    if text.is_empty() {
        return Ok(());
    }
    prop_assert!(!text.contains('\r'), "CR in:\n{text}");
    prop_assert!(
        text.ends_with('\n') && !text.ends_with("\n\n"),
        "ending of:\n{text:?}"
    );
    let mut fence: Option<usize> = None;
    for line in body_of(text).lines() {
        let trimmed = line.trim_start();
        let ticks = trimmed.len() - trimmed.trim_start_matches('`').len();
        match fence {
            Some(open) => {
                if ticks >= open && trimmed[ticks..].is_empty() {
                    fence = None;
                }
                continue;
            }
            None if ticks >= 3 => {
                fence = Some(ticks);
                continue;
            }
            None => {}
        }
        prop_assert_ne!(trimmed, "---", "a `---` line in the body of:\n{}", text);
        if let Some(rest) = trimmed.strip_prefix('^') {
            prop_assert!(
                rest.len() == 26 && Id::parse(rest).is_ok(),
                "a caret line that is no anchor: {line:?} in:\n{text}"
            );
        }
    }
    Ok(())
}

/// The three properties of one document plus the shape rules.
fn check_round_trip(doc: &FileDoc, schema: &Schema) -> Result<(), TestCaseError> {
    let text = serialize(doc, schema, &Vault);
    check_shape(&text)?;
    let mut normal = doc.clone();
    normalize(&mut normal);
    let mut twice = normal.clone();
    normalize(&mut twice);
    prop_assert_eq!(&twice, &normal, "normalize is not idempotent");
    let parsed = match parse(&text, schema, &Vault) {
        Ok(parsed) => parsed,
        Err(e) => return Err(TestCaseError::fail(format!("parse failed: {e}\n{text}"))),
    };
    prop_assert_eq!(
        &parsed.doc,
        &normal,
        "parse(serialize(d)) != normalize(d) for:\n{}",
        text
    );
    let again = serialize(&parsed.doc, schema, &Vault);
    prop_assert_eq!(
        &again,
        &text,
        "serialize(parse(serialize(d))) != serialize(d)"
    );
    prop_assert_eq!(
        serialize(&normal, schema, &Vault),
        text,
        "serialize(normalize(d)) != serialize(d)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

proptest! {
    #[test]
    fn documents_round_trip_through_their_text(doc in arb_doc()) {
        check_round_trip(&doc, &schema())?;
    }

    #[test]
    fn bodies_alone_round_trip(body in prop::collection::vec(arb_block(0), 1..12)) {
        let doc = FileDoc { front: Frontmatter::default(), body };
        check_round_trip(&doc, &schema())?;
    }

    #[test]
    fn frontmatter_alone_round_trips(front in arb_front()) {
        let doc = FileDoc { front, body: Vec::new() };
        check_round_trip(&doc, &schema())?;
    }

    #[test]
    fn inline_runs_round_trip_in_every_container(
        runs in runs(10),
        kind in prop::sample::select(&["paragraph", "heading", "quote", "bullet", "numbered", "todo"][..]),
    ) {
        let content = match kind {
            "heading" => json!({ "level": 2, "inline": runs }),
            "todo" => json!({ "inline": runs, "checked": true }),
            _ => json!({ "inline": runs }),
        };
        let block = block_of(Some(id(77)), kind, content, Vec::new());
        let doc = FileDoc { front: Frontmatter::default(), body: vec![block] };
        check_round_trip(&doc, &schema())?;
    }
}

/// A 1,000-block document round-trips; its time is reported, not asserted tightly (debug builds
/// vary), but a catastrophic slowdown fails.
#[test]
fn a_thousand_blocks_round_trip_quickly() {
    let runs = |i: usize| json!([{ "text": format!("block {i} with *stars* and [[x]]") }]);
    let mut body = Vec::new();
    for i in 0..1_000usize {
        let n = u128::try_from(i).unwrap() + 10;
        let block = match i % 7 {
            0 => block_of(
                Some(id(n)),
                "heading",
                json!({ "level": 2, "inline": runs(i) }),
                Vec::new(),
            ),
            1 | 2 => block_of(
                Some(id(n)),
                "paragraph",
                json!({ "inline": runs(i) }),
                Vec::new(),
            ),
            3 => block_of(
                Some(id(n)),
                "bullet",
                json!({ "inline": runs(i) }),
                vec![block_of(
                    Some(id(n + 5_000)),
                    "todo",
                    json!({ "inline": runs(i), "checked": true }),
                    Vec::new(),
                )],
            ),
            4 => block_of(
                Some(id(n)),
                "code",
                json!({ "code": "let x = 1;\n---", "language": "rust" }),
                Vec::new(),
            ),
            5 => block_of(
                Some(id(n)),
                "quote",
                json!({ "inline": runs(i) }),
                Vec::new(),
            ),
            _ => block_of(
                Some(id(n)),
                "chart",
                json!({ "series": [i, 2] }),
                Vec::new(),
            ),
        };
        body.push(block);
    }
    let mut doc = FileDoc {
        front: Frontmatter {
            id: Some(id(1)),
            types: vec!["T".into()],
            ..Frontmatter::default()
        },
        body,
    };
    normalize(&mut doc);
    let schema = schema();
    let start = Instant::now();
    let text = serialize(&doc, &schema, &Vault);
    let parsed = parse(&text, &schema, &Vault).unwrap();
    let elapsed = start.elapsed();
    eprintln!(
        "1,000 blocks ({} bytes): serialize + parse in {elapsed:?}",
        text.len()
    );
    assert_eq!(parsed.doc, doc);
    assert_eq!(serialize(&parsed.doc, &schema, &Vault), text);
    assert!(elapsed.as_secs() < 20, "{elapsed:?}");
}

#[test]
fn the_vault_resolves_paths_and_titles() {
    assert_eq!(Vault.resolve("Projects/Alpha"), Some(id(1)));
    assert_eq!(Vault.resolve("alpha"), Some(id(1)));
    assert_eq!(Vault.resolve("Bravo Two"), Some(id(2)));
    assert_eq!(Vault.resolve("Nope"), None);
    assert_eq!(Vault.path_of(id(4)).as_deref(), Some(PATHS[3]));
    assert_eq!(Vault.path_of(id(GHOST)), None);
    let keys: BTreeMap<&str, ()> = RESERVED_KEYS.iter().map(|k| (*k, ())).collect();
    assert!(keys.contains_key("aliases"));
}
