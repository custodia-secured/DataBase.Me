// SPDX-License-Identifier: AGPL-3.0-or-later
//! The frontmatter of the files folder (M5 Task 2, spec §3 and §9): every field kind writes and
//! reads back, both shapes load to the same envelope, the pre-scan caps hold, hand-written
//! look-alikes keep their spelling, and what the serializer wrote is a fixpoint byte for byte.
//! Every document here has an empty body; the body is `files_body.rs`'s concern.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use dbme_core::files::{
    FileDoc, Frontmatter, LinkEntry, MAX_FRONTMATTER_BYTES, MAX_YAML_DEPTH, NoResolver, ParseError,
    Parsed, RESERVED_KEYS, RelationRef, Schema, WikiLink, parse, serialize,
};
use dbme_core::model::{FieldKind, FieldRow, Props, Sensitivity, TypeRow};
use dbme_core::{ContentHash, Id, SortKey};
use serde_json::{Value, json};

fn id(n: u128) -> Id {
    Id::from_parts(1_000, n)
}

fn type_row(n: u128, name: &str) -> TypeRow {
    TypeRow {
        id: id(n),
        name: name.into(),
        plural: None,
        icon: None,
        color: None,
        extends: None,
        description: None,
        is_system: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn field(n: u128, type_id: u128, key: &str, kind: FieldKind) -> FieldRow {
    FieldRow {
        id: id(n),
        type_id: id(type_id),
        key: key.into(),
        label: key.into(),
        kind,
        options_json: json!({}),
        required: false,
        sort_order: i64::try_from(n).unwrap(),
    }
}

/// The kinds a value of which is stored in `props_json`, in FORMAT.md order.
fn stored_kinds() -> Vec<FieldKind> {
    FieldKind::ALL
        .into_iter()
        .filter(|k| k.is_stored())
        .collect()
}

/// One type `T` with one field of every stored kind, each keyed by the kind's own name.
fn schema() -> Schema {
    let fields = stored_kinds()
        .into_iter()
        .enumerate()
        .map(|(i, kind)| field(100 + i as u128, 1, kind.as_str(), kind))
        .collect();
    Schema::new(vec![type_row(1, "T")], fields)
}

/// A value in the kind's JSON shape (FORMAT.md §3.3).
fn sample(kind: FieldKind) -> Value {
    match kind {
        FieldKind::Text => json!("hello: world"),
        FieldKind::RichText => json!([{"kind": "paragraph", "inline": [{"text": "x"}]}]),
        FieldKind::Number => json!(-2.5),
        FieldKind::Currency => json!({"amount": "12.50", "currency": "EUR"}),
        FieldKind::Boolean => json!(true),
        FieldKind::Date => json!("2026-11-30"),
        FieldKind::Datetime => json!("2026-10-25T08:21:11Z"),
        FieldKind::Duration => json!(3_600),
        FieldKind::Select => json!("open"),
        FieldKind::MultiSelect => json!(["a", "b"]),
        FieldKind::Url => json!("https://example.org/?a=1&b=2"),
        FieldKind::Email => json!("alex@example.org"),
        FieldKind::Phone => json!("+15551234"),
        FieldKind::Geo => json!({"lat": 1.5, "lon": 2}),
        FieldKind::File => json!([ContentHash::of(b"x").as_str()]),
        FieldKind::Rating => json!(4),
        FieldKind::Json => json!({"any": [1, "two", null, true, 2.5], "n": {"k": "v"}}),
        FieldKind::Relation | FieldKind::Formula | FieldKind::Rollup => Value::Null,
    }
}

fn link(target: &str, block: Option<Id>, shown: Option<&str>) -> WikiLink {
    WikiLink {
        target: target.into(),
        block,
        shown: shown.map(str::to_owned),
    }
}

/// Every envelope key set, every kind of user field, relations, links and extras.
fn full() -> Frontmatter {
    Frontmatter {
        id: Some(id(1)),
        types: vec!["T".into()],
        tags: vec!["travel".into(), "Admin".into()],
        created: Some(1_709_208_000_000),
        updated: Some(1_709_210_489_000),
        summary: Some("one line".into()),
        captured_at: Some(0),
        event_start: Some(951_782_400_000),
        event_end: Some(951_782_400_123),
        geo: Some((48.85, 2.35)),
        sensitivity: Some(Sensitivity::Sensitive),
        sort: Some(SortKey::parse("a0V").unwrap()),
        fields: stored_kinds()
            .into_iter()
            .map(|kind| (kind.as_str().to_owned(), sample(kind)))
            .collect(),
        relations: vec![(
            "assigned_to".into(),
            vec![
                RelationRef {
                    id: Some(id(2)),
                    link: Some(link("People/Alex Doe", None, None)),
                },
                RelationRef {
                    id: Some(id(3)),
                    link: None,
                },
            ],
        )],
        links: vec![
            LinkEntry {
                id: Some(id(4)),
                link: Some(link("Archive/Alpha", Some(id(5)), Some("shown"))),
                rel_type: "related".into(),
                label: Some("the earlier run".into()),
                props: Props::from([("weight".to_owned(), json!(2))]),
                block: Some(id(6)),
            },
            LinkEntry {
                id: Some(id(7)),
                link: None,
                rel_type: "cites".into(),
                label: None,
                props: Props::new(),
                block: None,
            },
        ],
        extra: vec![
            ("aliases".into(), json!(["Alpha"])),
            ("cssclasses".into(), json!(["wide"])),
            ("Zebra Key".into(), json!({"n": null, "list": [1, "x"]})),
        ],
    }
}

fn doc(front: Frontmatter) -> FileDoc {
    FileDoc {
        front,
        body: Vec::new(),
    }
}

fn write(front: &Frontmatter) -> String {
    serialize(&doc(front.clone()), &schema(), &NoResolver)
}

fn read(text: &str) -> Parsed {
    parse(text, &schema(), &NoResolver).unwrap()
}

fn read_front(text: &str) -> Frontmatter {
    read(text).doc.front
}

fn fail(text: &str) -> ParseError {
    parse(text, &schema(), &NoResolver).unwrap_err()
}

/// Asserts that `needles` appear in `text` in the order given.
fn in_order(text: &str, needles: &[&str]) {
    let mut from = 0;
    for needle in needles {
        let at = text[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing after byte {from} in:\n{text}"));
        // Move just past the start, so needles may share their boundary newline.
        from += at + 1;
    }
}

#[test]
fn every_field_kind_writes_and_reads_back() {
    for kind in stored_kinds() {
        let one = Frontmatter {
            types: vec!["T".into()],
            fields: vec![(kind.as_str().to_owned(), sample(kind))],
            ..Frontmatter::default()
        };
        let text = write(&one);
        let parsed = read(&text);
        assert_eq!(
            parsed.doc.front, one,
            "{kind:?} did not round-trip:\n{text}"
        );
        assert!(
            parsed.warnings.is_empty(),
            "{kind:?}: {:?}",
            parsed.warnings
        );
        assert_eq!(write(&parsed.doc.front), text, "{kind:?} is not a fixpoint");
    }
}

#[test]
fn the_full_envelope_is_a_fixpoint() {
    let full = full();
    let text = write(&full);
    assert!(text.starts_with("---\nid: "), "{text}");
    assert!(text.ends_with("\n---\n"), "{text}");
    assert!(!text.contains('\r'));
    in_order(
        &text,
        &[
            "\nid: ",
            "\ntypes:\n  - T\n",
            "\ntags:\n  - travel\n  - Admin\n",
            "\ncreated: ",
            "\nupdated: ",
            "\nsummary: one line\n",
            "\ncaptured_at: ",
            "\nevent_start: ",
            "\nevent_end: ",
            "\ngeo_lat: 48.85\n",
            "\ngeo_lon: 2.35\n",
            "\nsensitivity: sensitive\n",
            "\nsort: a0V\n",
            "\ntext: ",
            "\njson:\n",
            "\nrelations:\n  assigned_to:\n    - id: ",
            "link: \"[[People/Alex Doe]]\"\n",
            "\nlinks:\n  - id: ",
            "link: \"[[Archive/Alpha#^",
            "|shown]]\"\n",
            "type: related\n",
            "label: the earlier run\n",
            "props:\n",
            "weight: 2\n",
            "block: ",
            "type: cites\n",
            "\naliases:\n  - Alpha\n",
            "\ncssclasses:\n  - wide\n",
            "\nZebra Key:\n",
        ],
    );
    let parsed = read(&text);
    assert_eq!(parsed.warnings, []);
    assert_eq!(parsed.doc.front, full);
    assert_eq!(write(&parsed.doc.front), text);
}

#[test]
fn a_small_envelope_writes_exactly_this() {
    let front = Frontmatter {
        id: Some(id(1)),
        types: vec!["Task".into()],
        tags: vec!["travel".into(), "admin".into()],
        created: Some(1_709_208_000_000),
        updated: Some(1_709_210_489_000),
        sort: Some(SortKey::first()),
        fields: vec![
            ("due".into(), json!("2026-11-30")),
            ("status".into(), json!("Waiting")),
        ],
        ..Frontmatter::default()
    };
    let expected = format!(
        "---\nid: {}\ntypes:\n  - Task\ntags:\n  - travel\n  - admin\n\
         created: \"2024-02-29T12:00:00.000Z\"\nupdated: \"2024-02-29T12:41:29.000Z\"\n\
         sort: a0\ndue: 2026-11-30\nstatus: Waiting\n---\n",
        id(1)
    );
    assert_eq!(write(&front), expected);
    assert_eq!(read_front(&expected), front.clone());
    // `types` is always written, `tags` only when non-empty.
    let bare = Frontmatter {
        types: vec!["T".into()],
        ..Frontmatter::default()
    };
    assert_eq!(write(&bare), "---\ntypes:\n  - T\n---\n");
    assert_eq!(read_front("---\ntypes:\n  - T\n---\n"), bare);
}

#[test]
fn an_empty_envelope_writes_nothing_and_reads_back() {
    assert_eq!(write(&Frontmatter::default()), "");
    assert_eq!(serialize(&FileDoc::default(), &schema(), &NoResolver), "");
    let parsed = read("");
    assert_eq!(parsed.doc.front, Frontmatter::default());
    assert!(parsed.warnings.is_empty());
    // An empty block between fences, and a block that is only a comment.
    assert_eq!(read_front("---\n---\n"), Frontmatter::default());
    assert_eq!(
        read_front("---\n# nothing here\n---\n"),
        Frontmatter::default()
    );
}

#[test]
fn both_shapes_load_to_the_same_frontmatter() {
    let shape_a = "---\nid: X\ntypes: [T]\nfields:\n  date: 2026-11-30\n  select: open\n  \
                   boolean: true\n  number: 7\n---\n";
    let shape_b = "---\nid: X\ntypes: [T]\ndate: 2026-11-30\nselect: open\nboolean: true\n\
                   number: 7\n---\n";
    let a = read(shape_a);
    let b = read(shape_b);
    assert_eq!(a.doc.front, b.doc.front);
    assert_eq!(
        a.doc.front.fields,
        [
            ("date".to_owned(), json!("2026-11-30")),
            ("select".to_owned(), json!("open")),
            ("boolean".to_owned(), json!(true)),
            ("number".to_owned(), json!(7)),
        ]
    );
    // `id: X` is not a ULID: kept as an extra key with a warning, in both shapes.
    assert_eq!(a.doc.front.id, None);
    assert_eq!(a.doc.front.extra, [("id".to_owned(), json!("X"))]);
    assert_eq!(a.warnings.len(), 1);
    assert_eq!(a.warnings[0].line, 2);
    assert!(a.warnings[0].message.contains("ULID"), "{:?}", a.warnings);
    // A single type name as a scalar is the one-element list.
    assert_eq!(read_front("---\ntypes: T\n---\n").types, ["T"]);
    // Both shapes write the same bytes.
    assert_eq!(write(&a.doc.front), write(&b.doc.front));
}

#[test]
fn a_reserved_user_field_key_goes_under_fields() {
    let front = Frontmatter {
        types: vec!["T".into()],
        tags: vec!["real".into()],
        fields: vec![
            ("tags".into(), json!(["x"])),
            ("date".into(), json!("2026-11-30")),
            ("id".into(), json!("not an id")),
        ],
        ..Frontmatter::default()
    };
    let text = write(&front);
    assert_eq!(
        text,
        "---\ntypes:\n  - T\ntags:\n  - real\ndate: 2026-11-30\nfields:\n  tags:\n    - x\n  \
         id: not an id\n---\n"
    );
    let parsed = read(&text);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    // User fields come back in file order: `date` at the top level before `fields:`.
    let mut expected = front.clone();
    expected.fields = vec![
        ("date".into(), json!("2026-11-30")),
        ("tags".into(), json!(["x"])),
        ("id".into(), json!("not an id")),
    ];
    assert_eq!(parsed.doc.front, expected);
    assert_eq!(write(&parsed.doc.front), text);
    for key in RESERVED_KEYS {
        assert!(
            key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{key} is not a lower-case identifier"
        );
    }
}

#[test]
fn aliases_and_other_unknown_keys_round_trip_as_extra() {
    let hand_written = "---\ntypes: [T]\naliases: [Alpha, Beta]\ncssclasses: wide\n\
                        Zebra Key: {a: 1, b: [x, ~]}\nnot-an-identifier: 2\n_x: 3\n---\n";
    let parsed = read(hand_written);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.doc.front.extra,
        [
            ("aliases".to_owned(), json!(["Alpha", "Beta"])),
            ("cssclasses".to_owned(), json!("wide")),
            ("Zebra Key".to_owned(), json!({"a": 1, "b": ["x", null]})),
            ("not-an-identifier".to_owned(), json!(2)),
            ("_x".to_owned(), json!(3)),
        ]
    );
    assert!(parsed.doc.front.fields.is_empty());
    let text = write(&parsed.doc.front);
    in_order(
        &text,
        &[
            "types:",
            "aliases:\n  - Alpha\n  - Beta\n",
            "cssclasses: wide\n",
            "Zebra Key:\n  a: 1\n  b:\n    - x\n    - ~\n",
            "not-an-identifier: 2\n",
            "_x: 3\n",
        ],
    );
    assert_eq!(read_front(&text), parsed.doc.front);
    assert_eq!(write(&read_front(&text)), text);
}

#[test]
fn look_alike_scalars_decode_by_kind_from_their_spelling() {
    let schema = Schema::new(
        vec![type_row(1, "L")],
        vec![
            field(10, 1, "answer", FieldKind::Text),
            field(11, 1, "octal", FieldKind::Number),
            field(12, 1, "octal_text", FieldKind::Text),
            field(13, 1, "exp", FieldKind::Number),
            field(14, 1, "exp_text", FieldKind::Text),
            field(15, 1, "time", FieldKind::Text),
            field(16, 1, "flag", FieldKind::Boolean),
            field(17, 1, "flag_text", FieldKind::Boolean),
            field(18, 1, "quoted_number", FieldKind::Number),
            field(19, 1, "word_number", FieldKind::Number),
            field(20, 1, "seconds", FieldKind::Duration),
            field(21, 1, "stars", FieldKind::Rating),
            field(22, 1, "price", FieldKind::Currency),
            field(23, 1, "spot", FieldKind::Geo),
            field(24, 1, "choices", FieldKind::MultiSelect),
            field(25, 1, "phone", FieldKind::Phone),
        ],
    );
    let text = "---\ntypes: [L]\nanswer: no\noctal: 0755\noctal_text: 0755\nexp: 1e3\n\
                exp_text: 1e3\ntime: 08:21\nflag: true\nflag_text: \"true\"\n\
                quoted_number: \"12\"\nword_number: twelve\nseconds: \"90\"\nstars: 4.0\n\
                price: {amount: 12.50, currency: EUR}\nspot: {lat: 48.0, lon: 2}\n\
                choices: a\nphone: +15551234\nuntyped_no: no\nuntyped_int: 0755\n\
                untyped_exp: 1e3\nuntyped_time: 08:21\nuntyped_null: ~\nempty:\n---\n";
    let parsed = parse(text, &schema, &NoResolver).unwrap();
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.doc.front.fields,
        [
            ("answer".to_owned(), json!("no")),
            ("octal".to_owned(), json!(755)),
            ("octal_text".to_owned(), json!("0755")),
            ("exp".to_owned(), json!(1000.0)),
            ("exp_text".to_owned(), json!("1e3")),
            ("time".to_owned(), json!("08:21")),
            ("flag".to_owned(), json!(true)),
            ("flag_text".to_owned(), json!(true)),
            ("quoted_number".to_owned(), json!(12)),
            ("word_number".to_owned(), json!("twelve")),
            ("seconds".to_owned(), json!(90)),
            ("stars".to_owned(), json!(4)),
            (
                "price".to_owned(),
                json!({"amount": "12.50", "currency": "EUR"})
            ),
            ("spot".to_owned(), json!({"lat": 48.0, "lon": 2})),
            ("choices".to_owned(), json!(["a"])),
            ("phone".to_owned(), json!("+15551234")),
            // No field in the schema: generic decoding, by the Core Schema.
            ("untyped_no".to_owned(), json!("no")),
            ("untyped_int".to_owned(), json!(755)),
            ("untyped_exp".to_owned(), json!(1000.0)),
            ("untyped_time".to_owned(), json!("08:21")),
            // `~` and an empty value are null: the field is absent.
        ]
    );
    // What the serializer writes for these reads back the same, by kind.
    let out = serialize(&parsed.doc, &schema, &NoResolver);
    let again = parse(&out, &schema, &NoResolver).unwrap();
    assert_eq!(again.doc.front, parsed.doc.front, "{out}");
    assert_eq!(serialize(&again.doc, &schema, &NoResolver), out);
}

#[test]
fn an_unquoted_wikilink_is_the_string() {
    let text = format!(
        "---\ntypes: [T]\ntext: [[Title]]\nrelations:\n  assigned_to:\n    - id: {}\n      \
         link: [[People/Alex#^{}|shown]]\n    - [[Just/A Link]]\n    - {}\nlinks:\n  - link: \
         [[Archive/Alpha]]\n    type: related\n---\n",
        id(2),
        id(9),
        id(3)
    );
    let parsed = read(&text);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let front = &parsed.doc.front;
    assert_eq!(front.fields, [("text".to_owned(), json!("[[Title]]"))]);
    assert_eq!(
        front.relations,
        [(
            "assigned_to".to_owned(),
            vec![
                RelationRef {
                    id: Some(id(2)),
                    link: Some(link("People/Alex", Some(id(9)), Some("shown"))),
                },
                RelationRef {
                    id: None,
                    link: Some(link("Just/A Link", None, None)),
                },
                RelationRef {
                    id: Some(id(3)),
                    link: None,
                },
            ]
        )]
    );
    assert_eq!(front.links.len(), 1);
    assert_eq!(front.links[0].link, Some(link("Archive/Alpha", None, None)));
    assert_eq!(front.links[0].rel_type, "related");
    // Written back, the links are quoted and read as strings again.
    let out = write(front);
    assert!(out.contains("text: \"[[Title]]\"\n"), "{out}");
    assert!(out.contains("link: \"[[Just/A Link]]\"\n"), "{out}");
    assert_eq!(&read_front(&out), front);
    assert_eq!(write(&read_front(&out)), out);
    // A heading fragment is not a block: dropped with a warning.
    let heading = read("---\nrelations:\n  r:\n    - link: \"[[Doc#Intro]]\"\n---\n");
    assert_eq!(
        heading.doc.front.relations[0].1[0].link,
        Some(link("Doc", None, None))
    );
    assert_eq!(heading.warnings.len(), 1);
    assert_eq!(heading.warnings[0].line, 4);
}

#[test]
fn a_duplicate_key_is_an_error_with_a_position() {
    let error = fail("---\nid: x\na: 1\na: 2\n---\n");
    assert_eq!((error.line, error.col), (4, 0));
    assert!(error.message.contains("duplicated key"), "{error}");
    assert_eq!(
        error.to_string(),
        format!("line 4 col 0: {}", error.message)
    );
    let nested = fail("---\ntypes: [T]\nfields:\n  due: 1\n  due: 2\n---\n");
    assert_eq!((nested.line, nested.col), (5, 2));
    // Any other scanner error carries saphyr's marker too.
    let tab = fail("---\na:\n\tb: 1\n---\n");
    assert_eq!(tab.line, 3);
    let quote = fail("---\na: \"oops\n---\n");
    assert_eq!(quote.line, 2);
}

#[test]
fn the_size_cap_holds() {
    let big = format!("---\nk: {}\n---\n", "x".repeat(300 * 1024));
    let error = fail(&big);
    assert_eq!((error.line, error.col), (1, 0));
    assert!(error.message.contains("bytes"), "{error}");
    assert!(error.message.contains(&MAX_FRONTMATTER_BYTES.to_string()));
    // Just under the cap loads.
    let fine = format!("---\nk: {}\n---\n", "x".repeat(MAX_FRONTMATTER_BYTES - 8));
    assert_eq!(read_front(&fine).fields.len(), 1);
}

/// A chain of `levels` nested mappings, each indented `width` more than the last, as a file.
fn nested_mappings(levels: usize, width: usize, line_end: &str) -> String {
    let mut text = format!("---{line_end}");
    for level in 0..levels {
        text.push_str(&" ".repeat(width * level));
        text.push_str(&format!("k{level}:"));
        if level + 1 == levels {
            text.push_str(" x");
        }
        text.push_str(line_end);
    }
    text.push_str(&format!("---{line_end}"));
    text
}

#[test]
fn the_depth_cap_holds() {
    // A 70-level nested mapping: the line of the 65th mapping (level 64) is the first one too
    // deep; 64 levels load.
    let error = fail(&nested_mappings(70, 2, "\n"));
    assert_eq!(error.line, 2 + MAX_YAML_DEPTH);
    assert!(error.message.contains("deep"), "{error}");
    assert_eq!(
        read_front(&nested_mappings(MAX_YAML_DEPTH, 2, "\n")).fields[0].0,
        "k0"
    );
    // Nesting is counted in levels, not in spaces: one-space and four-space chains are refused
    // at the same level (hostile finding: `indent / 2` admitted 128 one-space levels).
    for width in [1, 3, 4] {
        let error = fail(&nested_mappings(MAX_YAML_DEPTH + 1, width, "\n"));
        assert_eq!(error.line, 2 + MAX_YAML_DEPTH, "width {width}");
        assert!(error.message.contains("deep"), "{error}");
        assert!(
            parse(
                &nested_mappings(MAX_YAML_DEPTH, width, "\n"),
                &schema(),
                &NoResolver
            )
            .is_ok()
        );
    }
    // CRLF counts like LF; a bare CR (which the YAML crate reads as a line break and the pre-scan
    // could not see) is refused outright, so 700 levels never reach the loader.
    assert_eq!(
        fail(&nested_mappings(MAX_YAML_DEPTH + 1, 2, "\r\n")).line,
        2 + MAX_YAML_DEPTH
    );
    let cr_only = {
        let mut text = String::from("---\n");
        for level in 0..700 {
            text.push_str(&" ".repeat(level));
            text.push_str(&format!(
                "k{level}:{}\r",
                if level == 699 { " v" } else { "" }
            ));
        }
        text.push_str("\n---\n");
        text
    };
    let error = fail(&cr_only);
    assert_eq!(error.line, 2);
    assert!(error.message.contains("carriage return"), "{error}");
    // A tag, an anchor, a tab or a line break between a key and its flow value hides nothing;
    // nor does a complex key.
    for prefix in ["k: !!seq ", "k: &a ", "k:\t", "k:\n  ", "? "] {
        let text = format!(
            "---\n{prefix}{}x{}\n---\n",
            "[".repeat(255),
            "]".repeat(255)
        );
        let error = fail(&text);
        assert!(error.message.contains("deep"), "{prefix:?}: {error}");
    }
    // A flow value of 63 levels under the root mapping is the cap exactly.
    let text = format!(
        "---\nk: !!seq &a {}x{}\n---\n",
        "[".repeat(63),
        "]".repeat(63)
    );
    assert_eq!(read_front(&text).fields.len(), 1);
    // A line of 70 `- ` runs.
    let error = fail(&format!("---\nk:\n  {}x\n---\n", "- ".repeat(70)));
    assert_eq!((error.line, error.col), (3, 2));
    // Exactly the cap loads: 1 indent level plus 63 runs.
    let fine = format!("---\nk:\n  {}x\n---\n", "- ".repeat(MAX_YAML_DEPTH - 1));
    assert_eq!(read_front(&fine).fields[0].0, "k");
    // Flow brackets count too, before saphyr's own limit of 255.
    let error = fail(&format!("---\nk: {}\n---\n", "[".repeat(100)));
    assert_eq!(error.line, 2);
    assert_eq!(
        fail(&format!("---\nk: {}\n---\n", "{".repeat(1_000))).line,
        2
    );
}

#[test]
fn two_documents_are_an_error() {
    // A `---` with trailing text is not our fence, but it starts a second YAML document.
    let error = fail("---\nid: x\n--- \nb: 2\n---\n");
    assert_eq!((error.line, error.col), (3, 0));
    assert!(error.message.contains("document"), "{error}");
    let error = fail("---\nid: x\n... # end\n---\n");
    assert_eq!((error.line, error.col), (3, 0));
    // The end marker as our closer is fine, and so is an indented `---` (a value).
    assert_eq!(read_front("---\ntypes: [T]\n...\n").types, ["T"]);
    assert_eq!(
        read_front("---\ntext: ---\nselect: ...\n---\n").fields,
        [
            ("text".to_owned(), json!("---")),
            ("select".to_owned(), json!("...")),
        ]
    );
}

#[test]
fn the_root_must_be_a_mapping() {
    let error = fail("---\n- a\n- b\n---\n");
    assert_eq!((error.line, error.col), (2, 0));
    assert!(error.message.contains("mapping"), "{error}");
    let error = fail("---\njust a line of text\n---\n");
    assert_eq!(error.line, 2);
    assert!(fail("---\n[a, b]\n---\n").message.contains("mapping"));
}

#[test]
fn closers_crlf_and_missing_frontmatter() {
    let lf = format!("---\nid: {}\ntypes: [T]\ntags: [a]\n---\nbody\n", id(1));
    let crlf = lf.replace('\n', "\r\n");
    let from_lf = read(&lf);
    let from_crlf = read(&crlf);
    assert_eq!(from_crlf.doc.front, from_lf.doc.front);
    assert_eq!(from_lf.doc.front.id, Some(id(1)));
    assert!(from_crlf.warnings.is_empty());
    // The serializer writes LF only.
    assert!(!write(&from_crlf.doc.front).contains('\r'));
    // `...` closes the block too.
    assert_eq!(read_front("---\ntypes: [T]\n...\nbody\n").types, ["T"]);
    // No frontmatter at all: the empty envelope.
    assert_eq!(read_front("hello\n").types, Vec::<String>::new());
    assert_eq!(read_front("hello\n"), Frontmatter::default());
    // `---` followed by a blank line is a rule, not a fence; so is `---` alone.
    assert_eq!(read_front("---\n\nid: x\n---\n"), Frontmatter::default());
    assert_eq!(read_front("---\n"), Frontmatter::default());
    assert_eq!(read_front(" ---\nid: x\n---\n"), Frontmatter::default());
    // An opened fence that never closes is no fence: the whole text is body, as pulldown reads
    // it (a `Rule`, then a paragraph), and nothing is refused. Indented and four-dash lines are
    // not closers.
    for (text, kinds) in [
        (
            "---\ntypes: [T]\nbody without a closer\n",
            vec!["divider", "paragraph"],
        ),
        // ` ---` under a text line is a setext heading underline.
        (
            "---\ntypes: [T]\n ---\n----\n",
            vec!["divider", "heading", "divider"],
        ),
        ("---\nfoo\n", vec!["divider", "paragraph"]),
        ("---\r\nfoo\r\n", vec!["divider", "paragraph"]),
        ("---\nfoo", vec!["divider", "paragraph"]),
    ] {
        let parsed = read(text);
        assert_eq!(parsed.doc.front, Frontmatter::default(), "{text:?}");
        let read_kinds: Vec<&str> = parsed.doc.body.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(read_kinds, kinds, "{text:?}");
        // The reference parser agrees: no metadata block, a rule first.
        let mut events = pulldown_cmark::Parser::new_ext(
            text,
            pulldown_cmark::Options::ENABLE_YAML_STYLE_METADATA_BLOCKS,
        );
        assert!(matches!(events.next(), Some(pulldown_cmark::Event::Rule)));
        assert!(
            !events.any(|e| matches!(
                e,
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::MetadataBlock(_))
            )),
            "{text:?}"
        );
    }
}

#[test]
fn timestamps_parse_and_format_including_leap_day() {
    let parsed = read(
        "---\ncreated: 2024-02-29T12:00:00Z\nupdated: 2024-02-29T12:00:00.123Z\n\
         captured_at: 1970-01-01T00:00:00.000Z\nevent_start: 2000-02-29T00:00:00Z\n\
         event_end: 2100-03-01T00:00:00Z\n---\n",
    );
    assert!(parsed.warnings.is_empty());
    let front = &parsed.doc.front;
    assert_eq!(front.created, Some(1_709_208_000_000));
    assert_eq!(front.updated, Some(1_709_208_000_123));
    assert_eq!(front.captured_at, Some(0));
    assert_eq!(front.event_start, Some(951_782_400_000));
    assert_eq!(front.event_end, Some(4_107_542_400_000));
    let text = write(front);
    in_order(
        &text,
        &[
            "created: \"2024-02-29T12:00:00.000Z\"\n",
            "updated: \"2024-02-29T12:00:00.123Z\"\n",
            "captured_at: \"1970-01-01T00:00:00.000Z\"\n",
            "event_start: \"2000-02-29T00:00:00.000Z\"\n",
            "event_end: \"2100-03-01T00:00:00.000Z\"\n",
        ],
    );
    assert_eq!(read_front(&text), *front);
    // A timestamp that does not parse is kept as an extra key, with a warning, never fatal.
    let odd = read(
        "---\ncreated: 2023-02-29T00:00:00Z\nupdated: yesterday\ncaptured_at: 2026-10-25\n\
         event_start: 2026-10-25T08:21:11\nevent_end: [1]\n---\n",
    );
    assert_eq!(odd.doc.front.created, None);
    assert_eq!(
        odd.doc.front.extra,
        [
            ("created".to_owned(), json!("2023-02-29T00:00:00Z")),
            ("updated".to_owned(), json!("yesterday")),
            ("captured_at".to_owned(), json!("2026-10-25")),
            ("event_start".to_owned(), json!("2026-10-25T08:21:11")),
            ("event_end".to_owned(), json!([1])),
        ]
    );
    let lines: Vec<usize> = odd.warnings.iter().map(|w| w.line).collect();
    assert_eq!(lines, [2, 3, 4, 5, 6]);
    assert!(odd.warnings.iter().all(|w| w.message.contains("timestamp")));
}

#[test]
fn envelope_keys_that_do_not_fit_are_kept_as_extra() {
    let parsed = read(
        "---\nid: nope\ntypes: {a: 1}\ntags: [[x, y]]\nsummary: [a]\ngeo_lat: 48.85\n\
         sensitivity: secret\nsort: \"!!\"\nfields: 3\nrelations: [1]\nlinks: {a: 1}\n---\n",
    );
    let front = &parsed.doc.front;
    assert_eq!(
        *front,
        Frontmatter {
            extra: vec![
                ("id".into(), json!("nope")),
                ("types".into(), json!({"a": 1})),
                ("tags".into(), json!([["x", "y"]])),
                ("summary".into(), json!(["a"])),
                ("sensitivity".into(), json!("secret")),
                ("sort".into(), json!("!!")),
                ("fields".into(), json!(3)),
                ("relations".into(), json!([1])),
                ("links".into(), json!({"a": 1})),
                ("geo_lat".into(), json!(48.85)),
            ],
            ..Frontmatter::default()
        }
    );
    assert_eq!(parsed.warnings.len(), 10);
    assert!(
        parsed
            .warnings
            .iter()
            .all(|w| w.message.contains("extra key"))
    );
    // `sensitivity: normal` is the absent default; `geo_lon` alone is kept too.
    let normal = read("---\nsensitivity: normal\ngeo_lon: 2\n---\n");
    assert_eq!(normal.doc.front.sensitivity, None);
    assert_eq!(normal.doc.front.geo, None);
    assert_eq!(normal.doc.front.extra, [("geo_lon".to_owned(), json!(2))]);
    assert_eq!(normal.warnings.len(), 1);
    // Written back, nothing is lost.
    let text = write(front);
    assert_eq!(read_front(&text), *front);
}

#[test]
fn links_and_relations_in_other_shapes() {
    let text = format!(
        "---\nrelations:\n  one: {}\n  none: ~\n  bad:\n    - {{x: 1}}\n    - 7\nlinks:\n  - id: {}\n    \
         link: \"[[A]]\"\n  - type: cites\n    colour: red\n  - just a string\n  - id: nope\n    \
         type: t\n---\n",
        id(2),
        id(4)
    );
    let parsed = read(&text);
    let front = &parsed.doc.front;
    assert_eq!(
        front.relations,
        [
            (
                "one".to_owned(),
                vec![RelationRef {
                    id: Some(id(2)),
                    link: None,
                }]
            ),
            ("none".to_owned(), vec![]),
            // An unknown key is ignored with a warning; a bare scalar that is not a ULID is a
            // link target, so nothing is lost.
            (
                "bad".to_owned(),
                vec![
                    RelationRef::default(),
                    RelationRef {
                        id: None,
                        link: Some(link("7", None, None)),
                    },
                ]
            ),
        ]
    );
    assert_eq!(
        front.links,
        [
            LinkEntry {
                id: Some(id(4)),
                link: Some(link("A", None, None)),
                rel_type: "related".into(),
                label: None,
                props: Props::new(),
                block: None,
            },
            LinkEntry {
                id: None,
                link: None,
                rel_type: "cites".into(),
                label: None,
                props: Props::new(),
                block: None,
            },
            LinkEntry {
                id: None,
                link: None,
                rel_type: "t".into(),
                label: None,
                props: Props::new(),
                block: None,
            },
        ]
    );
    let messages: Vec<&str> = parsed.warnings.iter().map(|w| w.message.as_str()).collect();
    assert_eq!(messages.len(), 5, "{messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`x` is not part of a relation"))
    );
    assert!(messages.iter().any(|m| m.contains("without a `type`")));
    assert!(messages.iter().any(|m| m.contains("`colour`")));
    assert!(messages.iter().any(|m| m.contains("must be a mapping")));
    assert!(messages.iter().any(|m| m.contains("not a ULID")));
    // Empty relation lists are written and read back as such.
    let out = write(front);
    assert!(out.contains("  none: []\n"), "{out}");
    assert_eq!(read_front(&out), *front);
    assert_eq!(write(&read_front(&out)), out);
}

#[test]
fn multi_line_strings_survive_as_block_scalars() {
    let front = Frontmatter {
        types: vec!["T".into()],
        summary: Some("one\ntwo\n\nthree".into()),
        fields: vec![
            ("text".into(), json!("ends with newlines\n\n")),
            ("select".into(), json!("  leading and trailing  ")),
            ("url".into(), json!("")),
        ],
        extra: vec![("Looks Like A Fence".into(), json!("a\n---\nb"))],
        ..Frontmatter::default()
    };
    let text = write(&front);
    assert!(
        text.contains("summary: |-\n  one\n  two\n  \n  three\n"),
        "{text}"
    );
    assert!(!text.contains("\n---\nb"), "{text}");
    assert!(text.ends_with("\n---\n"));
    let parsed = read(&text);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.doc.front, front);
    assert_eq!(write(&parsed.doc.front), text);
}

// ---------------------------------------------------------------------------------------------
// Regressions from the M5 Task 2 review (2026-11-02)
// ---------------------------------------------------------------------------------------------

fn normal(front: &Frontmatter) -> Frontmatter {
    let mut d = doc(front.clone());
    dbme_core::files::normalize(&mut d);
    d.front
}

/// `parse(serialize(d)) == normalize(d)`, `normalize` idempotent, and the bytes a fixpoint.
fn check_normal_form(front: &Frontmatter) -> Frontmatter {
    let text = write(front);
    let expected = normal(front);
    assert_eq!(normal(&expected), expected, "normalize is not idempotent");
    let parsed = read(&text);
    assert_eq!(parsed.doc.front, expected, "{text}");
    assert_eq!(write(&parsed.doc.front), text);
    expected
}

#[test]
fn an_extra_key_never_overwrites_a_set_envelope_slot() {
    let front = Frontmatter {
        id: Some(id(1)),
        types: vec!["T".into()],
        created: Some(1_700_000_000_000),
        extra: vec![
            ("types".into(), json!(5)),
            ("created".into(), json!("yesterday")),
            ("id".into(), json!("junk")),
            ("Zebra".into(), json!(1)),
        ],
        ..Frontmatter::default()
    };
    let text = write(&front);
    assert!(text.contains(&format!("id: {}\n", id(1))), "{text}");
    assert!(text.contains("types:\n  - T\n"), "{text}");
    assert!(
        text.contains("created: \"2023-11-14T22:13:20.000Z\"\n"),
        "{text}"
    );
    assert!(
        !text.contains("junk") && !text.contains("yesterday"),
        "{text}"
    );
    let normal_front = check_normal_form(&front);
    assert_eq!(normal_front.id, Some(id(1)));
    assert_eq!(normal_front.types, ["T"]);
    assert_eq!(normal_front.created, Some(1_700_000_000_000));
    assert_eq!(normal_front.extra, [("Zebra".to_owned(), json!(1))]);
    // With the slot unset, the extra survives: the Obsidian `created: 2026-10-25` spelling that
    // the parser files as an extra round-trips through a rebuild as long as the object has no
    // timestamp, and yields to the timestamp once it has one.
    let hand_written = read_front("---\ntypes: [T]\ncreated: 2026-10-25\n---\n");
    assert_eq!(
        hand_written.extra,
        [("created".to_owned(), json!("2026-10-25"))]
    );
    check_normal_form(&hand_written);
    let mut with_timestamp = hand_written.clone();
    with_timestamp.created = Some(0);
    let rebuilt = read_front(&write(&with_timestamp));
    assert_eq!(rebuilt.created, Some(0));
    assert!(rebuilt.extra.is_empty());
    // An empty `types` with a `types` extra writes the extra, so nothing is lost either way.
    let odd_types = read_front("---\ntypes: {a: 1}\n---\n");
    assert_eq!(odd_types.types, Vec::<String>::new());
    assert_eq!(odd_types.extra, [("types".to_owned(), json!({"a": 1}))]);
    check_normal_form(&odd_types);
}

#[test]
fn normalize_puts_the_frontmatter_into_normal_form() {
    // A reserved-key field before a plain one, a null field, a non-identifier field key and an
    // identifier extra key: each is sorted the way the reader will read it.
    let front = Frontmatter {
        types: vec!["T".into()],
        fields: vec![
            ("tags".into(), json!("x")),
            ("due".into(), Value::Null),
            ("date".into(), json!("2026-11-30")),
            ("Due Date".into(), json!("friday")),
            ("summary".into(), json!("s")),
            ("status".into(), json!("open")),
        ],
        extra: vec![
            ("foo".into(), json!(1)),
            ("Zebra Key".into(), json!(2)),
            ("bar".into(), Value::Null),
            ("Zebra Key".into(), json!(3)),
        ],
        ..Frontmatter::default()
    };
    let expected = check_normal_form(&front);
    assert_eq!(
        expected.fields,
        [
            ("date".to_owned(), json!("2026-11-30")),
            ("status".to_owned(), json!("open")),
            ("foo".to_owned(), json!(1)),
            ("tags".to_owned(), json!("x")),
            ("summary".to_owned(), json!("s")),
        ]
    );
    assert_eq!(
        expected.extra,
        [
            ("Zebra Key".to_owned(), json!(2)),
            ("Due Date".to_owned(), json!("friday")),
        ]
    );
    // A numeric `geo_lat` alone sits last among the extras; with its `geo_lon` it is the slot.
    let lat_alone = Frontmatter {
        types: vec!["T".into()],
        extra: vec![("geo_lat".into(), json!(48.85)), ("Z".into(), json!(1))],
        ..Frontmatter::default()
    };
    let expected = check_normal_form(&lat_alone);
    assert_eq!(expected.extra[1].0, "geo_lat");
    let both = Frontmatter {
        types: vec!["T".into()],
        extra: vec![
            ("geo_lat".into(), json!(48.85)),
            ("geo_lon".into(), json!(2)),
        ],
        ..Frontmatter::default()
    };
    let expected = check_normal_form(&both);
    assert_eq!(expected.geo, Some((48.85, 2.0)));
    assert!(expected.extra.is_empty());
}

#[test]
fn many_frontmatter_warnings_cost_no_rescan() {
    // 29k malformed `links` entries, two warnings each, in a block just under the size cap:
    // positions come from a line table, not a walk from the start per warning.
    let entries = (MAX_FRONTMATTER_BYTES - 7) / 9;
    let yaml = "links:\n".to_owned() + &"- {a: 1}\n".repeat(entries);
    let text = format!("---\n{yaml}---\n");
    let start = std::time::Instant::now();
    let parsed = read(&text);
    let elapsed = start.elapsed();
    assert_eq!(parsed.warnings.len(), 2 * entries);
    assert_eq!(parsed.warnings[0].line, 3);
    assert_eq!(parsed.warnings.last().unwrap().line, 2 + entries);
    assert!(elapsed.as_secs() < 30, "{elapsed:?}");
}
