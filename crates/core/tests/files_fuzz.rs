// SPDX-License-Identifier: AGPL-3.0-or-later
//! `parse` never panics (M5 Task 2, spec §9): arbitrary strings and arbitrary bytes up to 64 KiB,
//! Markdown- and YAML-flavoured noise, random frontmatter structures nested up to 80 levels (a
//! nesting past [`MAX_YAML_DEPTH`] is an `Err`, never a panic or a stack overflow: the deep cases
//! run on a thread with a 256 KiB stack to show that the pre-scan, not the stack, decides), flow
//! brackets by the hundred, and byte-level mutations (flips, deletions, insertions, line swaps,
//! truncations) of files the serializer wrote. Every `Err` points inside the text; the input is
//! a `&str`, so an `Err` trivially leaves it unchanged.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use std::thread;

use dbme_core::files::{
    FileBlock, FileDoc, Frontmatter, LinkEntry, LinkResolver, MAX_YAML_DEPTH, NoResolver,
    ParseError, RelationRef, Schema, WikiLink, parse, serialize,
};
use dbme_core::model::{FieldKind, FieldRow, TypeRow};
use dbme_core::{ContentHash, Id};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

fn id(n: u128) -> Id {
    Id::from_parts(1_000, n)
}

/// A resolver that knows every name: whatever the target, it is object 1 at `Vault/Thing`, so
/// the mention paths of the reader run on arbitrary input.
struct Everything;

impl LinkResolver for Everything {
    fn resolve(&self, _target: &str) -> Option<Id> {
        Some(id(1))
    }

    fn path_of(&self, _id: Id) -> Option<String> {
        Some("Vault/Thing".to_owned())
    }

    fn attachment_name(&self, blob: &ContentHash) -> String {
        format!("{}.png", blob.hex())
    }
}

/// One type `T` with a field of every stored kind, keyed by the kind's name.
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
    let fields = FieldKind::ALL
        .into_iter()
        .filter(|k| k.is_stored())
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

/// An `Err` must point inside the text: a 1-based line no further than one past the last line
/// (the YAML scanner reports an unexpected end on the line after it) and a column within it.
fn check_position(text: &str, error: &ParseError) -> Result<(), TestCaseError> {
    let lines: Vec<&str> = text.split('\n').collect();
    prop_assert!(error.line >= 1, "line 0 in {error}");
    prop_assert!(
        error.line <= lines.len() + 1,
        "{error} points past the {} lines of:\n{text:?}",
        lines.len()
    );
    let width = lines.get(error.line - 1).map_or(0, |l| l.len());
    prop_assert!(
        error.col <= width,
        "{error} points past the {width} bytes of its line in:\n{text:?}"
    );
    Ok(())
}

/// Parses with both resolvers; the assertion is that this returns at all.
fn parse_both(text: &str) -> Result<(), TestCaseError> {
    let schema = schema();
    for resolver in [&NoResolver as &dyn LinkResolver, &Everything] {
        if let Err(error) = parse(text, &schema, resolver) {
            check_position(text, &error)?;
        }
    }
    Ok(())
}

/// The stack every input the pre-scan refuses must survive on: the YAML crate never sees it.
const REFUSED_STACK: usize = 256 * 1024;
/// The stack an admitted input gets. `saphyr::MarkedYaml::load_from_str` alone, in a debug
/// build, needs more than 256 KiB for 64 nested mappings (measured on 2026-11-02 with saphyr
/// 0.3.3: 48 levels load in 256 KiB, 64 do not, 128 do in 512 KiB, 250 in 1 MiB), so the cap of
/// [`MAX_YAML_DEPTH`] protects a 256 KiB stack only through what it refuses; this is the finding,
/// not something the pre-scan can change.
const ADMITTED_STACK: usize = 512 * 1024;

/// Runs `f` on a thread with a stack of `bytes`: a parse that needs more than that has let the
/// input, not the cap, decide its recursion.
fn on_stack<T: Send + 'static>(bytes: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    thread::Builder::new()
        .stack_size(bytes)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("the parse thread panicked")
}

/// The stack a frontmatter of the given measured nesting runs on.
fn stack_for(depth: usize) -> usize {
    if depth > MAX_YAML_DEPTH {
        REFUSED_STACK
    } else {
        ADMITTED_STACK
    }
}

// ---------------------------------------------------------------------------------------------
// Noise
// ---------------------------------------------------------------------------------------------

/// Markdown's structural characters, dense.
const MARKDOWN: &[&str] = &[
    "-",
    "*",
    "_",
    "#",
    ">",
    "`",
    "```",
    "~~",
    "[",
    "]",
    "[[",
    "]]",
    "![[",
    "(",
    ")",
    "!",
    "<",
    ">",
    "&",
    "\\",
    "|",
    "^",
    "1.",
    "10)",
    "- [ ] ",
    "- [x] ",
    "    ",
    "\t",
    "\n",
    "\n\n",
    "\r\n",
    " ",
    "a",
    "b",
    "x",
    "é",
    "名",
    "---",
    "***",
    "===",
    "<u>",
    "</u>",
    "<div>",
    "&amp;",
    "attachments/",
    ".png",
    "dbme",
    "{\"kind\":\"k\",\"content\":{}}",
    "^01JB3R7Q8Y5K3M2N4P6Q8R0S2T",
    " ^01JB3R7Q8Y5K3M2N4P6Q8R0S2T",
    "01JB3R7Q8Y5K3M2N4P6Q8R0S2T",
    "#tag",
    "http://x",
    "@",
];

/// YAML's structural characters, dense.
const YAML: &[&str] = &[
    "---",
    "...",
    "\n",
    ": ",
    ":",
    "- ",
    "  ",
    "    ",
    "[",
    "]",
    "{",
    "}",
    ",",
    "\"",
    "'",
    "|",
    "|-",
    ">",
    "#",
    "&",
    "*",
    "!",
    "%",
    "@",
    "`",
    "~",
    "a",
    "b",
    "é",
    "1",
    "0755",
    "1e3",
    "true",
    "null",
    "id",
    "types",
    "tags",
    "fields",
    "relations",
    "links",
    "created",
    "geo_lat",
    "sensitivity",
    "sort",
    "aliases",
    "link",
    "[[x]]",
    "2026-10-25T08:21:11.000Z",
    "\t",
    "\r",
];

fn noise(alphabet: &'static [&'static str], max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(alphabet), 0..max).prop_map(|v| v.concat())
}

/// Arbitrary text: unicode strings, Markdown noise, a YAML-noise frontmatter over a Markdown
/// body, and long random byte strings up to 64 KiB.
fn arb_text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => any::<String>(),
        3 => noise(MARKDOWN, 400),
        3 => (noise(YAML, 200), noise(MARKDOWN, 100))
            .prop_map(|(yaml, body)| format!("---\n{yaml}\n---\n{body}")),
        1 => prop::collection::vec(any::<u8>(), 0..65_536)
            .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
    ]
}

proptest! {
    #[test]
    fn arbitrary_text_never_panics(text in arb_text()) {
        parse_both(&text)?;
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..65_536)) {
        parse_both(&String::from_utf8_lossy(&bytes))?;
    }
}

// ---------------------------------------------------------------------------------------------
// Frontmatter structures
// ---------------------------------------------------------------------------------------------

/// One level of a nested YAML structure: a mapping key or a sequence item, each with a few
/// scalar siblings beside the branch that goes deeper.
#[derive(Clone, Debug)]
enum Step {
    Key { key: &'static str, siblings: u8 },
    Item { siblings: u8 },
}

/// Plain scalars the loader accepts without surprise.
const SCALARS: &[&str] = &[
    "x",
    "1",
    "true",
    "~",
    "a b",
    "'q'",
    "\"d\"",
    "[[Link]]",
    "é",
    "0755",
    "1e3",
    "2026-10-25T08:21:11.000Z",
    "{a: 1}",
    "[1, 2]",
    "",
];

/// Mapping keys: envelope keys among them, so every slot of the reader sees odd shapes.
const KEYS: &[&str] = &[
    "k0",
    "k1",
    "k2",
    "id",
    "types",
    "tags",
    "fields",
    "relations",
    "links",
    "created",
    "summary",
    "geo_lat",
    "geo_lon",
    "sensitivity",
    "sort",
    "aliases",
    "due",
    "Zebra Key",
];

fn arb_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => (prop::sample::select(KEYS), 0..3u8).prop_map(|(key, siblings)| Step::Key { key, siblings }),
        1 => (0..3u8).prop_map(|siblings| Step::Item { siblings }),
    ]
}

/// A chain of 1 to 80 steps, opening with a key (the root must be a mapping), and the scalar at
/// its end.
fn arb_structure() -> impl Strategy<Value = (Vec<Step>, &'static str)> {
    (
        prop::sample::select(KEYS),
        prop::collection::vec(arb_step(), 0..80),
        prop::sample::select(SCALARS),
    )
        .prop_map(|(key, mut steps, leaf)| {
            steps.insert(0, Step::Key { key, siblings: 0 });
            (steps, leaf)
        })
}

/// Renders the chain as YAML: a key's branch goes two spaces deeper, an item's branch starts on
/// the item's own line (`- - k:`), siblings are scalars beside the branch. Sibling keys are made
/// unique per mapping, so the only reason the text could be refused is its depth.
fn render(steps: &[Step], leaf: &str, indent: usize, inline_first: bool, out: &mut String) {
    let pad = |first: bool, out: &mut String| {
        if !first || !inline_first {
            out.push_str(&" ".repeat(indent));
        }
    };
    let Some((step, rest)) = steps.split_first() else {
        out.push_str(leaf);
        out.push('\n');
        return;
    };
    match step {
        Step::Key { key, siblings } => {
            pad(true, out);
            out.push_str(key);
            out.push(':');
            if rest.is_empty() {
                out.push(' ');
                out.push_str(leaf);
                out.push('\n');
            } else {
                out.push('\n');
                render(rest, leaf, indent + 2, false, out);
            }
            for i in 0..*siblings {
                pad(false, out);
                out.push_str(&format!("sib{i}_{key}: {}\n", SCALARS[usize::from(i)]));
            }
        }
        Step::Item { siblings } => {
            pad(true, out);
            out.push_str("- ");
            render(rest, leaf, indent + 2, true, out);
            for i in 0..*siblings {
                pad(false, out);
                out.push_str(&format!("- {}\n", SCALARS[usize::from(i) + 3]));
            }
        }
    }
}

/// The nesting the pre-scan measures: the block collections open at a line's deepest node (a
/// stack of columns: a deeper line opens one, a `- ` run opens one per dash and a `key:` after
/// a dash one more), plus the deepest run of unclosed `[` and `{` in a value that starts with a
/// bracket (the scalars here hold no quoted brackets, so a plain count is exact). This is the
/// true YAML nesting of the structures `render` writes.
fn measured_depth(yaml: &str) -> usize {
    let mut stack: Vec<(usize, bool)> = Vec::new();
    let mut deepest = 0;
    for line in yaml.lines() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let mut rest = &line[indent..];
        if rest.trim().is_empty() {
            continue;
        }
        let mut col = indent;
        let mut dash_cols = Vec::new();
        while let Some(after) = rest.strip_prefix("- ") {
            dash_cols.push(col);
            col += 2;
            rest = after;
        }
        let is_seq = !dash_cols.is_empty();
        let value = if rest.starts_with(['[', '{']) {
            None
        } else {
            rest.find(": ")
                .map(|at| rest[at + 2..].trim_start())
                .or_else(|| rest.strip_suffix(':').map(|_| ""))
        };
        let is_key = value.is_some();
        while stack.last().is_some_and(|(c, _)| *c > indent) {
            stack.pop();
        }
        match stack.last().copied() {
            Some((c, seq)) if c == indent && seq == is_seq => {}
            Some((c, true)) if c == indent && !is_seq => {
                stack.pop();
                if stack.last() != Some(&(indent, false)) {
                    stack.push((indent, false));
                }
            }
            _ => stack.push((indent, is_seq)),
        }
        for c in dash_cols.iter().skip(1) {
            stack.push((*c, true));
        }
        if is_seq && is_key {
            stack.push((col, false));
        }
        let flow = value.unwrap_or(rest);
        let (mut open, mut peak) = (0usize, 0usize);
        if flow.starts_with(['[', '{']) {
            for c in flow.chars() {
                match c {
                    '[' | '{' => {
                        open += 1;
                        peak = peak.max(open);
                    }
                    ']' | '}' => open = open.saturating_sub(1),
                    _ => {}
                }
            }
        }
        deepest = deepest.max(stack.len() + peak);
    }
    deepest
}

proptest! {
    /// Random structures to 80 levels: `Err` exactly when the measured nesting passes the cap,
    /// never a panic; refused depths on a 256 KiB stack (see [`ADMITTED_STACK`]).
    #[test]
    fn nested_frontmatter_is_refused_past_the_cap_and_never_overflows(
        (steps, leaf) in arb_structure(),
    ) {
        let mut yaml = String::new();
        render(&steps, leaf, 0, false, &mut yaml);
        let depth = measured_depth(&yaml);
        let text = format!("---\n{yaml}---\nbody\n");
        let outcome = {
            let text = text.clone();
            on_stack(stack_for(depth), move || {
                parse(&text, &schema(), &NoResolver).map(|p| p.doc)
            })
        };
        match outcome {
            Ok(_) => prop_assert!(depth <= MAX_YAML_DEPTH, "depth {depth} loaded:\n{text}"),
            Err(error) => {
                check_position(&text, &error)?;
                prop_assert!(
                    depth > MAX_YAML_DEPTH,
                    "depth {depth} refused: {error}\n{text}"
                );
                prop_assert!(error.message.contains("deep"), "{error}");
            }
        }
    }

    /// Flow brackets by the hundred, closed on the line or left open; refused depths on a
    /// 256 KiB stack (see [`ADMITTED_STACK`]).
    #[test]
    fn flow_nesting_never_overflows(
        openers in 1..=100usize,
        bracket in prop::sample::select(&["[", "{"][..]),
        closed in any::<bool>(),
        split in any::<bool>(),
    ) {
        let (open, close, inner) = if bracket == "[" {
            ("[", "]", "x")
        } else {
            ("{", "}", "a: 1")
        };
        let mut yaml = String::from("k: ");
        for i in 0..openers {
            yaml.push_str(open);
            if split && i % 7 == 6 {
                yaml.push('\n');
            }
        }
        yaml.push_str(inner);
        if closed {
            yaml.push_str(&close.repeat(openers));
        }
        yaml.push('\n');
        let text = format!("---\n{yaml}---\n");
        // The root mapping is one level; the brackets all open before any closes, so the
        // nesting peaks at `openers + 1` whether or not the line closes them again.
        let depth = openers + 1;
        let outcome = {
            let text = text.clone();
            on_stack(stack_for(depth), move || {
                parse(&text, &schema(), &NoResolver).map(|p| p.doc)
            })
        };
        if let Err(error) = &outcome {
            check_position(&text, error)?;
        }
        // An unclosed run is a scan error besides, and so is a flow collection continued at the
        // key's own indentation.
        if depth > MAX_YAML_DEPTH || !closed {
            prop_assert!(outcome.is_err(), "{}", text);
        } else if !split {
            prop_assert!(outcome.is_ok(), "{:?}\n{}", outcome.err(), text);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Mutations of files the serializer wrote
// ---------------------------------------------------------------------------------------------

const WORDS: &[&str] = &[
    "a", "b", " ", "\n", "*", "`", "#", "[", "]", "(", ")", "-", ">", "^", "|", "<", "&", "\\",
    "é", "1.", "---",
];

fn words(max: usize) -> impl Strategy<Value = String> {
    noise(WORDS, max)
}

fn runs() -> impl Strategy<Value = Value> {
    prop::collection::vec(
        prop_oneof![
            4 => (words(10), any::<u8>()).prop_map(|(text, bits)| {
                let mut run = json!({ "text": text });
                for (i, key) in ["bold", "italic", "code", "strike"].into_iter().enumerate() {
                    if bits >> i & 1 == 1 {
                        run[key] = Value::Bool(true);
                    }
                }
                if bits >> 6 & 1 == 1 {
                    run["link"] = Value::String("http://x/(y)".into());
                }
                run
            }),
            1 => (1..3u128, any::<bool>()).prop_map(|(n, embed)| {
                let mut run = json!({ "mention": id(n).to_string() });
                if embed {
                    run["embed"] = Value::Bool(true);
                }
                run
            }),
        ],
        0..4,
    )
    .prop_map(Value::Array)
}

/// A block of any kind, list items with one level of children.
fn arb_block(depth: u32) -> BoxedStrategy<FileBlock> {
    let anchor = prop::option::of(any::<u64>().prop_map(|n| id(u128::from(n) + 100)));
    let leaf = (
        anchor.clone(),
        prop::sample::select(
            &[
                "paragraph",
                "heading",
                "quote",
                "code",
                "divider",
                "image",
                "chart",
            ][..],
        ),
        runs(),
        words(20),
    )
        .prop_map(|(id, kind, inline, text)| {
            let content = match kind {
                "heading" => json!({ "level": 2, "inline": inline }),
                "code" => json!({ "code": text, "language": "rs" }),
                "divider" => json!({}),
                "image" => {
                    json!({ "blob": ContentHash::of(text.as_bytes()).as_str(), "alt": "cat" })
                }
                "chart" => json!({ "series": [1, 2], "title": text }),
                _ => json!({ "inline": inline }),
            };
            FileBlock {
                id,
                kind: kind.to_owned(),
                content,
                children: Vec::new(),
            }
        });
    if depth >= 2 {
        return leaf.boxed();
    }
    let item = (
        anchor,
        prop::sample::select(&["bullet", "numbered", "todo"][..]),
        runs(),
        prop::collection::vec(arb_block(depth + 1), 0..3),
    )
        .prop_map(|(id, kind, inline, children)| FileBlock {
            id,
            kind: kind.to_owned(),
            content: if kind == "todo" {
                json!({ "inline": inline, "checked": true })
            } else {
                json!({ "inline": inline })
            },
            children,
        });
    prop_oneof![2 => leaf, 1 => item].boxed()
}

fn arb_front() -> impl Strategy<Value = Frontmatter> {
    (
        prop::option::of(1..5u128),
        words(12),
        prop::option::of(0..2_000_000_000_000i64),
        prop::collection::vec(
            (
                prop::sample::select(&["text", "number", "due", "tags"][..]),
                words(8),
            ),
            0..4,
        ),
        prop::option::of(words(8)),
    )
        .prop_map(|(id_n, summary, created, fields, target)| Frontmatter {
            id: id_n.map(id),
            types: vec!["T".into()],
            tags: vec!["travel".into()],
            created,
            summary: Some(summary),
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.to_owned(), json!(v)))
                .collect(),
            relations: vec![(
                "due".into(),
                vec![RelationRef {
                    id: Some(id(7)),
                    link: target.map(|t| WikiLink {
                        target: t,
                        block: None,
                        shown: None,
                    }),
                }],
            )],
            links: vec![LinkEntry {
                id: Some(id(8)),
                link: None,
                rel_type: "related".into(),
                label: None,
                props: Default::default(),
                block: Some(id(9)),
            }],
            extra: vec![("aliases".into(), json!(["A"]))],
            ..Frontmatter::default()
        })
}

/// A serialized file the serializer wrote.
fn arb_file() -> impl Strategy<Value = String> {
    (arb_front(), prop::collection::vec(arb_block(0), 0..6))
        .prop_map(|(front, body)| serialize(&FileDoc { front, body }, &schema(), &Everything))
}

/// One byte-level or line-level change to a file.
#[derive(Clone, Debug)]
enum Mutation {
    Flip { at: usize, byte: u8 },
    Delete { at: usize, len: usize },
    Insert { at: usize, bytes: Vec<u8> },
    SwapLines { a: usize, b: usize },
    DuplicateLine { at: usize },
    Truncate { at: usize },
}

fn arb_mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        3 => (any::<usize>(), any::<u8>()).prop_map(|(at, byte)| Mutation::Flip { at, byte }),
        2 => (any::<usize>(), 1..40usize).prop_map(|(at, len)| Mutation::Delete { at, len }),
        2 => (any::<usize>(), prop::collection::vec(any::<u8>(), 1..12))
            .prop_map(|(at, bytes)| Mutation::Insert { at, bytes }),
        2 => (any::<usize>(), any::<usize>()).prop_map(|(a, b)| Mutation::SwapLines { a, b }),
        1 => any::<usize>().prop_map(|at| Mutation::DuplicateLine { at }),
        1 => any::<usize>().prop_map(|at| Mutation::Truncate { at }),
    ]
}

fn apply(bytes: &mut Vec<u8>, mutation: &Mutation) {
    if bytes.is_empty() {
        bytes.push(b'-');
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
        Mutation::SwapLines { a, b } => {
            let mut lines: Vec<Vec<u8>> =
                bytes.split(|c| *c == b'\n').map(<[u8]>::to_vec).collect();
            let (a, b) = (a % lines.len(), b % lines.len());
            lines.swap(a, b);
            *bytes = lines.join(&b'\n');
        }
        Mutation::DuplicateLine { at } => {
            let mut lines: Vec<Vec<u8>> =
                bytes.split(|c| *c == b'\n').map(<[u8]>::to_vec).collect();
            let at = at % lines.len();
            let line = lines[at].clone();
            lines.insert(at, line);
            *bytes = lines.join(&b'\n');
        }
        Mutation::Truncate { at } => bytes.truncate(at % n),
    }
}

proptest! {
    #[test]
    fn mutated_files_never_panic(
        file in arb_file(),
        mutations in prop::collection::vec(arb_mutation(), 1..6),
    ) {
        let mut bytes = file.into_bytes();
        for mutation in &mutations {
            apply(&mut bytes, mutation);
        }
        parse_both(&String::from_utf8_lossy(&bytes))?;
    }

    /// The files themselves parse, as a sanity check on the generator.
    #[test]
    fn unmutated_files_parse(file in arb_file()) {
        let parsed = parse(&file, &schema(), &Everything);
        prop_assert!(parsed.is_ok(), "{:?}\n{}", parsed.err(), file);
    }
}
