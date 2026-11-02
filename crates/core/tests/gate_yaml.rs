// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1c (M5): what saphyr 0.3 does with the frontmatter. Each probe asserts the observed
//! behaviour, so this file is the record behind ADR-0023; the plan's assumptions are named in the
//! comments where a probe confirms or contradicts one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::borrow::Cow;
use std::process::Command;
use std::time::{Duration, Instant};

use saphyr::{
    LoadableYamlNode, Mapping, MarkedYaml, Scalar, ScalarStyle, Yaml, YamlEmitter, YamlOwned,
};

/// The first document of `src`.
fn doc(src: &str) -> Yaml<'_> {
    let mut docs = Yaml::load_from_str(src).unwrap();
    assert_eq!(docs.len(), 1, "{src:?} is one document");
    docs.remove(0)
}

fn s(v: &str) -> Yaml<'_> {
    Yaml::Value(Scalar::String(Cow::Borrowed(v)))
}

fn map<'a>(entries: &[(&'a str, Yaml<'a>)]) -> Yaml<'a> {
    let mut m = Mapping::new();
    for (k, v) in entries {
        m.insert(s(k), v.clone());
    }
    Yaml::Mapping(m)
}

/// The variant a loaded scalar landed in, as the plan names them.
fn kind(y: &Yaml<'_>) -> String {
    match y {
        Yaml::Value(Scalar::String(v)) => format!("String({v})"),
        Yaml::Value(Scalar::Boolean(b)) => format!("Boolean({b})"),
        Yaml::Value(Scalar::Integer(i)) => format!("Integer({i})"),
        Yaml::Value(Scalar::FloatingPoint(f)) => format!("Real({f})"),
        Yaml::Value(Scalar::Null) => "Null".into(),
        other => format!("{other:?}"),
    }
}

fn emit(y: &Yaml<'_>, multiline: bool, quoted: bool) -> String {
    let mut out = String::new();
    let mut emitter = YamlEmitter::new(&mut out);
    emitter.multiline(multiline);
    emitter.quoted_value(quoted);
    emitter.dump(y).unwrap();
    out
}

/// Probe 1. The plan assumes the YAML 1.2 Core Schema, so `no`, `on`, `yes`, `off`, dates and
/// times stay strings. Only `true`, `null`, `~`, numbers and `.inf` resolve.
#[test]
fn core_schema_scalars_resolve_as_the_plan_assumes() {
    let inputs = [
        "2026-11-30",
        "2026-11-30T08:21:00Z",
        "08:21",
        "no",
        "on",
        "yes",
        "off",
        "true",
        "null",
        "~",
        "0755",
        "1e3",
        "1_000",
        ".inf",
        "\"quoted\"",
        "'single'",
    ];
    let src: String = inputs.iter().map(|i| format!("- {i}\n")).collect();
    let seq = doc(&src);
    let kinds: Vec<String> = seq.as_vec().unwrap().iter().map(kind).collect();
    for (input, k) in inputs.iter().zip(&kinds) {
        println!("{input:>22} -> {k}");
    }
    assert_eq!(
        kinds,
        [
            "String(2026-11-30)",
            "String(2026-11-30T08:21:00Z)",
            "String(08:21)",
            "String(no)",
            "String(on)",
            "String(yes)",
            "String(off)",
            "Boolean(true)",
            "Null",
            "Null",
            // Core Schema reads a leading zero as decimal; the octal form is 0o755.
            "Integer(755)",
            "Real(1000)",
            // Underscores are YAML 1.1; in 1.2 this is a string.
            "String(1_000)",
            "Real(inf)",
            "String(quoted)",
            "String(single)",
        ]
    );
    // YamlOwned resolves the same way; it only drops the borrow.
    let owned = YamlOwned::load_from_str(&src).unwrap().remove(0);
    let owned: Vec<String> = owned
        .as_vec()
        .unwrap()
        .iter()
        .map(|o| kind(&o.into()))
        .collect();
    assert_eq!(owned, kinds);
}

/// Probe 2. The plan marks duplicate keys "not verified": by default they are an error, placed at
/// the second key. `YamlLoader::allow_duplicate_keys(true)` would keep the last value, but it is
/// reachable only through `saphyr_parser::Parser`, which dbme-core does not depend on.
#[test]
fn a_duplicate_key_is_an_error_at_the_second_key() {
    let err = Yaml::load_from_str("a: 1\nb: 2\na: 3\n").unwrap_err();
    println!("duplicate key: {err}");
    assert_eq!(err.info(), "duplicated key in mapping");
    assert_eq!((err.marker().line(), err.marker().col()), (3, 0));
    // Nested mappings are checked too, and the error names the inner key.
    let nested = Yaml::load_from_str("fields:\n  due: 1\n  due: 2\n").unwrap_err();
    assert_eq!((nested.marker().line(), nested.marker().col()), (3, 2));
    // Keys compare by value, so `1` and `01` collide while `1` and `"1"` do not.
    assert!(Yaml::load_from_str("1: a\n01: b\n").is_err());
    assert!(Yaml::load_from_str("1: a\n\"1\": b\n").is_ok());
}

/// Probe 3. Mapping keys keep source order through load and emit (the LinkedHashMap claim).
#[test]
fn key_order_survives_load_and_emit() {
    let y = doc("z: 1\na: 2\nm: 3\n");
    let keys: Vec<&str> = y
        .as_mapping()
        .unwrap()
        .keys()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert_eq!(keys, ["z", "a", "m"]);
    assert_eq!(emit(&y, false, false), "---\nz: 1\na: 2\nm: 3");
}

/// Probe 4. Look-alike strings through the emitter, in all four option combinations. The design
/// says look-alike strings are quoted: the emitter does that itself for `no`, `on`, `08:21`, numbers
/// and booleans, but not for dates, which need no quoting under the Core Schema. The fixpoint holds
/// in every combination without any quoting of our own.
#[test]
fn look_alike_strings_round_trip_through_the_emitter() {
    let long = "a".repeat(300);
    let original = map(&[
        ("date", s("2026-11-30")),
        ("no", s("no")),
        ("on", s("on")),
        ("time", s("08:21")),
        ("int", s("123")),
        ("exp", s("1e3")),
        ("bool", s("true")),
        ("empty", s("")),
        ("leading", s(" leading")),
        ("colon", s("a: b")),
        ("dash", s("- x")),
        ("hash", s("#x")),
        ("lines", s("one\ntwo\nthree")),
        ("long", s(&long)),
        ("unicode", s("Ünïcödé — 日本語")),
    ]);
    for (multiline, quoted) in [(false, false), (true, false), (false, true), (true, true)] {
        let out = emit(&original, multiline, quoted);
        println!("--- multiline({multiline}) quoted_value({quoted}):\n{out}\n");
        assert_eq!(
            doc(&out),
            original,
            "fixpoint with multiline({multiline}) quoted({quoted})"
        );
    }
    // The shape the plan proposes (multiline on, quoted_value off), exactly.
    let plain = emit(&original, true, false);
    let expected = format!(
        "---\ndate: 2026-11-30\n\"no\": \"no\"\n\"on\": \"on\"\ntime: \"08:21\"\nint: \"123\"\n\
         exp: \"1e3\"\nbool: \"true\"\nempty: \"\"\nleading: \" leading\"\ncolon: \"a: b\"\n\
         dash: \"- x\"\nhash: \"#x\"\nlines: |-\n  one\n  two\n  three\nlong: {long}\n\
         unicode: Ünïcödé — 日本語"
    );
    assert_eq!(plain, expected);
    // quoted_value(true) wins over multiline(true): the newlines are escaped, not a block.
    assert!(emit(&original, true, true).contains("lines: \"one\\ntwo\\nthree\""));
    // Without multiline, newlines are escaped in a double-quoted scalar.
    assert!(emit(&original, false, false).contains("lines: \"one\\ntwo\\nthree\""));
    // quoted_value leaves keys alone, but a look-alike key is quoted in every combination.
    assert!(emit(&original, false, false).contains("\n\"no\": \"no\"\n"));
    assert!(emit(&original, false, true).contains("\n\"no\": \"no\"\n"));
    assert!(emit(&original, false, true).contains("\ndate: \"2026-11-30\"\n"));
    // Every dump starts with the document marker and ends without a newline.
    assert!(plain.starts_with("---\n") && !plain.ends_with('\n'));
}

/// Probe 5. The envelope shapes: `fields:` as a map, `relations:` as a map of sequences of maps,
/// `links:` as a sequence of maps, an empty map and an empty sequence, and a block sequence.
#[test]
fn nested_shapes_emit_in_block_style_and_round_trip() {
    let relation = map(&[
        ("id", s("01JCE0000000000000000000A1")),
        ("link", s("[[Projects/Alpha]]")),
    ]);
    let link = map(&[
        ("id", s("01JCE0000000000000000000B2")),
        ("link", s("[[Beta]]")),
        ("type", s("related")),
    ]);
    let original = map(&[
        (
            "fields",
            map(&[("due", s("2026-11-30")), ("status", s("open"))]),
        ),
        (
            "relations",
            map(&[("project", Yaml::Sequence(vec![relation]))]),
        ),
        ("links", Yaml::Sequence(vec![link])),
        ("empty_map", Yaml::Mapping(Mapping::new())),
        ("empty_seq", Yaml::Sequence(vec![])),
        ("tags", Yaml::Sequence(vec![s("a"), s("b")])),
    ]);
    let out = emit(&original, true, false);
    println!("{out}");
    assert_eq!(
        out,
        "---\n\
         fields:\n  due: 2026-11-30\n  status: open\n\
         relations:\n  project:\n    - id: 01JCE0000000000000000000A1\n      link: \"[[Projects/Alpha]]\"\n\
         links:\n  - id: 01JCE0000000000000000000B2\n    link: \"[[Beta]]\"\n    type: related\n\
         empty_map: {}\n\
         empty_seq: []\n\
         tags:\n  - a\n  - b"
    );
    assert_eq!(doc(&out), original);
    // A flow sequence in the input comes out in block style: the emitter normalises.
    let flow = doc("tags: [a, b]\n");
    assert_eq!(emit(&flow, true, false), "---\ntags:\n  - a\n  - b");
}

/// Probe 6. MarkedYaml carries a start and end marker per node (line 1-based, column 0-based,
/// plus a character index), and errors carry one marker: what the Conflicts inbox needs.
#[test]
fn marked_yaml_positions_nodes_and_errors() {
    let src = "id: abc\nfields:\n  due: 2026-11-30\n";
    let root = MarkedYaml::load_from_str(src).unwrap().remove(0);
    let at = |m: &MarkedYaml<'_>| {
        let (a, b) = (m.span.start, m.span.end);
        (a.line(), a.col(), b.line(), b.col())
    };
    println!("root {:?}", at(&root));
    assert_eq!(at(&root), (1, 0, 4, 0));
    let fields = root.data.as_mapping_get("fields").unwrap();
    let due = fields.data.as_mapping_get("due").unwrap();
    println!("fields {:?} due {:?}", at(fields), at(due));
    assert_eq!(at(fields), (3, 2, 4, 0));
    assert_eq!(at(due), (3, 7, 3, 17));
    assert_eq!(due.span.start.index(), 23);
    assert_eq!(due.data.as_str(), Some("2026-11-30"));
    // Keys carry markers too: the key node of `fields` starts at line 2.
    let key = root.data.as_mapping().unwrap().keys().nth(1).unwrap();
    assert_eq!(at(key), (2, 0, 2, 6));

    let syntax = MarkedYaml::load_from_str("a: [1, 2\nb: 3\n").unwrap_err();
    println!("syntax: {syntax}");
    // The unclosed `[` is blamed on the `:` of the next line, one past the colon.
    assert_eq!((syntax.marker().line(), syntax.marker().col()), (2, 1));
    assert!(syntax.info().contains("':' indicator"), "{syntax}");
    let dup = MarkedYaml::load_from_str("a: 1\na: 2\n").unwrap_err();
    assert_eq!((dup.marker().line(), dup.marker().col()), (2, 0));
}

/// Probe 7. Malformed input: tabs, an unterminated quote, and a depth bomb in flow style. Flow
/// nesting is counted in a `u8`, so 256 open brackets fail fast as "recursion limit exceeded"
/// whether or not they are ever closed. There is no size limit; the input is read as it is parsed.
#[test]
fn malformed_input_errors_without_panicking() {
    let tab = Yaml::load_from_str("a:\n\tb: 1\n").unwrap_err();
    println!("tab: {tab}");
    // Markers point one past the offending character: the tab at column 0 is reported at 1.
    assert_eq!((tab.marker().line(), tab.marker().col()), (2, 1));
    let quote = Yaml::load_from_str("a: \"oops\n").unwrap_err();
    println!("quote: {quote}");
    assert!(quote.info().contains("quoted scalar"), "{quote}");

    let open = "[".repeat(1024 * 1024);
    let started = Instant::now();
    let bomb = Yaml::load_from_str(&open).unwrap_err();
    let elapsed = started.elapsed();
    println!("flow depth bomb: {bomb} after {elapsed:?}");
    assert_eq!(bomb.info(), "recursion limit exceeded");
    assert_eq!(bomb.marker().col(), 255);
    assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
    // Balanced to 1 MB each way it fails at the same spot, before reading the closers.
    let balanced = format!("{open}{}", "]".repeat(1024 * 1024));
    let bomb = Yaml::load_from_str(&balanced).unwrap_err();
    assert_eq!(bomb.marker().col(), 255);
    // 255 nested flow sequences still load.
    let fine = format!("{}{}", "[".repeat(255), "]".repeat(255));
    assert!(Yaml::load_from_str(&fine).is_ok());
}

const CHILD_ENV: &str = "DBME_GATE_YAML_CHILD";

/// Loads `- ` repeated `depth` times in a child process (this same test binary, filtered to the
/// one test, with the depth in the environment) and returns what it printed and whether it lived.
fn load_block_nesting_in_a_child(depth: usize) -> (bool, String) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "block_nesting_has_no_depth_limit_and_overflows_the_stack",
            "--nocapture",
        ])
        .env(CHILD_ENV, depth.to_string())
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let line = text
        .lines()
        .find(|l| l.starts_with("block depth") || l.contains("overflowed"))
        .unwrap_or("")
        .trim()
        .to_owned();
    println!("depth {depth:>7}: {:?} {line}", output.status.code());
    (output.status.success(), line)
}

/// Probe 7, block style. `- - - … x` nests block sequences with no counter, and saphyr-parser's
/// `load_node` → `load_sequence` → `load_node` recurses once per level, so the tree is as deep as
/// the line is long until the stack runs out: in this debug build between 800 and 900 levels on
/// the test thread's 2 MB stack, a 2 KB line. The process dies; no `Err` is returned. The flow
/// limit of 256 does not cover block style, so the fuzz layer's depth cap is ours to write.
#[test]
fn block_nesting_has_no_depth_limit_and_overflows_the_stack() {
    if let Some(depth) = std::env::var_os(CHILD_ENV) {
        let depth: usize = depth.to_str().unwrap().parse().unwrap();
        let bomb = format!("{}x", "- ".repeat(depth));
        let started = Instant::now();
        let result = Yaml::load_from_str(&bomb);
        println!(
            "block depth bomb: {:?} after {:?}",
            result.as_ref().map(Vec::len).map_err(ToString::to_string),
            started.elapsed()
        );
        drop(result);
        println!("block depth bomb dropped");
        return;
    }
    // The boundary moves with frame size (debug or release), so it is printed, not asserted.
    let survived: Vec<usize> = [500, 700, 800, 900, 1_000, 2_000]
        .into_iter()
        .filter(|&depth| load_block_nesting_in_a_child(depth).0)
        .collect();
    println!("block depths that loaded: {survived:?}");
    assert!(survived.contains(&500), "500 levels load in any build");
    let (fine, line) = load_block_nesting_in_a_child(100_000);
    assert!(!fine && line.contains("has overflowed its stack"), "{line}");
    let (fine, line) = load_block_nesting_in_a_child(512 * 1024);
    assert!(!fine && line.contains("has overflowed its stack"), "{line}");
}

/// Probe 8. Several documents and the end marker: every document is returned, an empty one as
/// `Null`. Our frontmatter is one document between the Markdown fences, so the parser must
/// reject a `Vec` longer than one.
#[test]
fn several_documents_are_all_returned() {
    let docs = Yaml::load_from_str("---\na: 1\n---\nb: 2\n").unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs[0].contains_mapping_key("a") && docs[1].contains_mapping_key("b"));
    let docs = Yaml::load_from_str("---\na: 1\n...\n").unwrap();
    assert_eq!(docs.len(), 1);
    let docs = Yaml::load_from_str("a: 1\n---\nb: 2\n").unwrap();
    assert_eq!(docs.len(), 2);
    let docs = Yaml::load_from_str("---\n...\n").unwrap();
    assert_eq!(docs, [Yaml::Value(Scalar::Null)]);
    assert!(Yaml::load_from_str("").unwrap().is_empty());
    // A frontmatter body with an unexpected `---` line inside it is a second document, not an error.
    let docs = Yaml::load_from_str("id: x\n---\n").unwrap();
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[1], Yaml::Value(Scalar::Null));
}

/// Probe 9. An unknown key with a flow value loads, emits (in block style) and loads again equal:
/// Decision 10b's verbatim round trip holds on the tree, not on the bytes.
#[test]
fn an_unknown_key_round_trips() {
    let first = doc("obsidianExtra: [1, 2]\nid: x\n");
    let out = emit(&first, true, false);
    assert_eq!(out, "---\nobsidianExtra:\n  - 1\n  - 2\nid: x");
    assert_eq!(doc(&out), first);
}

/// Probe 10. An unquoted wikilink is a nested flow sequence, not a string; the emitter quotes the
/// string `[[Title]]`, so our own output reads back as a string. Hand-written frontmatter needs the
/// quotes, which Obsidian itself writes.
#[test]
fn an_unquoted_wikilink_is_a_nested_sequence() {
    let y = doc("link: [[Title]]\n");
    let link = y.as_mapping_get("link").unwrap();
    assert_eq!(
        *link,
        Yaml::Sequence(vec![Yaml::Sequence(vec![s("Title")])])
    );
    // A `#`, a `^`, a `|` and a space all survive inside the inner scalar; none of them splits it.
    let y = doc("link: [[Projects/Alpha#^b1|shown here]]\n");
    assert_eq!(
        y.as_mapping_get("link").unwrap()[0][0].as_str(),
        Some("Projects/Alpha#^b1|shown here")
    );
    let out = emit(&map(&[("link", s("[[Title]]"))]), true, false);
    assert_eq!(out, "---\nlink: \"[[Title]]\"");
    assert_eq!(
        doc(&out).as_mapping_get("link").unwrap().as_str(),
        Some("[[Title]]")
    );
    // The quoted form in a file loads as the string.
    assert_eq!(
        doc("link: \"[[Title]]\"\n")
            .as_mapping_get("link")
            .unwrap()
            .as_str(),
        Some("[[Title]]")
    );
}

/// A `Representation` node in literal or folded style makes the emitter hit `todo!()`. Nothing we
/// build takes that path, since `load_from_str` parses eagerly, but the ADR should say so.
#[test]
fn emitting_a_literal_representation_panics() {
    let node = Yaml::Representation(Cow::Borrowed("x"), ScalarStyle::Literal, None);
    let hit = std::panic::catch_unwind(|| emit(&node, false, false));
    assert!(hit.is_err());
    let plain = Yaml::Representation(Cow::Borrowed("no"), ScalarStyle::Plain, None);
    // A plain representation is written verbatim: `no` is not quoted here.
    assert_eq!(emit(&plain, false, false), "---\nno");
}
