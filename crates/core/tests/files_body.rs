// SPDX-License-Identifier: AGPL-3.0-or-later
//! The body of the files folder (M5 Task 2, spec §4, §7 and §9): every block kind written and
//! read back with its anchor in both placements, nesting to depth 4 under `- `, `1. ` and `10. `
//! with mixed kinds and non-list children, the lone-anchor rule, marks, links, mentions, body
//! tags, fences, the `dbme` fence and its fallback, tables and HTML kept as text, the line-start
//! escapes and CRLF input. For every hand-built document `d`: `parse(serialize(d)) == d` and
//! `serialize(parse(serialize(d))) == serialize(d)` through the public `dbme_core::files` API with
//! a resolver that knows three objects by path and title.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use dbme_core::files::{
    FileBlock, FileDoc, Frontmatter, LinkResolver, MAX_BLOCK_DEPTH, Parsed, Schema, normalize,
    parse, serialize,
};
use dbme_core::{ContentHash, Id};
use serde_json::{Value, json};

/// Object numbers the vault knows; `GHOST` is an object with no file.
const ALPHA: u128 = 1;
const BRAVO: u128 = 2;
const CHARLIE: u128 = 3;
const GHOST: u128 = 4;

fn id(n: u128) -> Id {
    Id::from_parts(1_000, n)
}

fn ulid(n: u128) -> String {
    id(n).to_string()
}

/// A vault of three objects, each known by its path (without `.md`) and by its title, the file's
/// basename. Attachments are named `<hex>.png`.
struct Vault;

const OBJECTS: [(u128, &str); 3] = [
    (ALPHA, "Projects/Alpha"),
    (BRAVO, "People/Bravo Two"),
    (CHARLIE, "Notes/2026/Charlie (draft)"),
];

impl LinkResolver for Vault {
    fn resolve(&self, target: &str) -> Option<Id> {
        if let Some((n, _)) = OBJECTS.iter().find(|(_, path)| *path == target) {
            return Some(id(*n));
        }
        let wanted = target.to_lowercase();
        let mut hits = OBJECTS
            .iter()
            .filter(|(_, path)| path.rsplit('/').next().unwrap().to_lowercase() == wanted);
        let hit = hits.next()?;
        hits.next().is_none().then(|| id(hit.0))
    }

    fn path_of(&self, id: Id) -> Option<String> {
        OBJECTS
            .iter()
            .find(|(n, _)| self::id(*n) == id)
            .map(|(_, path)| (*path).to_owned())
    }

    fn attachment_name(&self, blob: &ContentHash) -> String {
        format!("{}.png", blob.hex())
    }
}

fn schema() -> Schema {
    Schema::new(Vec::new(), Vec::new())
}

fn front() -> Frontmatter {
    Frontmatter {
        id: Some(id(900)),
        types: vec!["Note".to_owned()],
        ..Frontmatter::default()
    }
}

fn doc(body: Vec<FileBlock>) -> FileDoc {
    FileDoc {
        front: front(),
        body,
    }
}

fn blk(n: Option<u128>, kind: &str, content: Value, children: Vec<FileBlock>) -> FileBlock {
    FileBlock {
        id: n.map(id),
        kind: kind.to_owned(),
        content,
        children,
    }
}

/// `{"inline": runs}`.
fn inline(runs: Vec<Value>) -> Value {
    json!({ "inline": runs })
}

/// A plain text run.
fn t(text: &str) -> Value {
    json!({ "text": text })
}

/// A text run with the given marks set (`bold`, `italic`, `underline`, `strike`, `code`).
fn marked(text: &str, marks: &[&str]) -> Value {
    let mut run = serde_json::Map::new();
    run.insert("text".to_owned(), Value::String(text.to_owned()));
    for mark in marks {
        run.insert((*mark).to_owned(), Value::Bool(true));
    }
    Value::Object(run)
}

/// A text run that is a link, optionally marked.
fn linked(text: &str, url: &str, marks: &[&str]) -> Value {
    let mut run = marked(text, marks);
    run["link"] = Value::String(url.to_owned());
    run
}

/// A mention run.
fn mention(n: u128, shown: Option<&str>, block: Option<u128>, embed: bool) -> Value {
    let mut run = serde_json::Map::new();
    run.insert("mention".to_owned(), Value::String(ulid(n)));
    if let Some(shown) = shown {
        run.insert("shown".to_owned(), Value::String(shown.to_owned()));
    }
    if let Some(block) = block {
        run.insert("block".to_owned(), Value::String(ulid(block)));
    }
    if embed {
        run.insert("embed".to_owned(), Value::Bool(true));
    }
    Value::Object(run)
}

/// A paragraph of one plain text run.
fn para(n: Option<u128>, text: &str) -> FileBlock {
    blk(n, "paragraph", inline(vec![t(text)]), Vec::new())
}

fn bullet(n: Option<u128>, text: &str, children: Vec<FileBlock>) -> FileBlock {
    blk(n, "bullet", inline(vec![t(text)]), children)
}

fn numbered(n: Option<u128>, text: &str, children: Vec<FileBlock>) -> FileBlock {
    blk(n, "numbered", inline(vec![t(text)]), children)
}

fn todo(n: Option<u128>, checked: bool, text: &str, children: Vec<FileBlock>) -> FileBlock {
    blk(
        n,
        "todo",
        json!({ "checked": checked, "inline": [t(text)] }),
        children,
    )
}

fn heading(n: Option<u128>, level: u8, text: &str) -> FileBlock {
    blk(
        n,
        "heading",
        json!({ "level": level, "inline": [t(text)] }),
        Vec::new(),
    )
}

fn quote(n: Option<u128>, text: &str) -> FileBlock {
    blk(n, "quote", inline(vec![t(text)]), Vec::new())
}

fn code(n: Option<u128>, code: &str, language: Option<&str>) -> FileBlock {
    let content = match language {
        Some(language) => json!({ "code": code, "language": language }),
        None => json!({ "code": code }),
    };
    blk(n, "code", content, Vec::new())
}

fn divider(n: Option<u128>) -> FileBlock {
    blk(n, "divider", json!({}), Vec::new())
}

/// The hex of the one attachment every image here embeds.
fn hex() -> String {
    "ab".repeat(32)
}

fn image(n: Option<u128>, alt: Option<&str>) -> FileBlock {
    let blob = format!("blake3:{}", hex());
    let content = match alt {
        Some(alt) => json!({ "blob": blob, "alt": alt }),
        None => json!({ "blob": blob }),
    };
    blk(n, "image", content, Vec::new())
}

fn chart(n: Option<u128>, children: Vec<FileBlock>) -> FileBlock {
    blk(
        n,
        "chart",
        json!({ "series": [1, 2.5, "x"], "title": "q #4 ```", "nested": { "ok": true } }),
        children,
    )
}

fn empty_paragraph(n: u128) -> FileBlock {
    blk(Some(n), "paragraph", inline(Vec::new()), Vec::new())
}

/// The decisive check: `d` is in normal form, `parse(serialize(d)) == d`, and `serialize` of
/// that is byte-identical. Returns the Markdown and what the parser saw.
fn round_trip(name: &str, doc: &FileDoc) -> (String, Parsed) {
    let mut normal = doc.clone();
    normalize(&mut normal);
    assert!(
        normal == *doc,
        "{name}: the hand-built doc is not in normal form\n{}",
        first_difference(&normal.body, &doc.body)
    );
    let md = serialize(doc, &schema(), &Vault);
    assert!(!md.contains('\r'), "{name}: CR written\n{md}");
    assert!(md.ends_with('\n'), "{name}: no final newline\n{md}");
    assert!(!md.ends_with("\n\n"), "{name}: two final newlines\n{md}");
    let parsed = parse(&md, &schema(), &Vault)
        .unwrap_or_else(|e| panic!("{name}: {e}\n--- markdown ---\n{md}"));
    assert!(
        parsed.doc == *doc,
        "{name}: parse(serialize(d)) != d\n{}\n--- markdown ---\n{md}",
        first_difference(&parsed.doc.body, &doc.body)
    );
    let again = serialize(&parsed.doc, &schema(), &Vault);
    assert_eq!(
        again, md,
        "{name}: serialize∘parse∘serialize is not the identity"
    );
    (md, parsed)
}

/// The first block, depth first, where `a` and `b` differ, as JSON: a readable failure.
fn first_difference(a: &[FileBlock], b: &[FileBlock]) -> String {
    if a.len() != b.len() {
        return format!("{} blocks on the left, {} on the right", a.len(), b.len());
    }
    for (x, y) in a.iter().zip(b) {
        if x == y {
            continue;
        }
        if x.id == y.id && x.kind == y.kind && x.content == y.content {
            return first_difference(&x.children, &y.children);
        }
        return format!(
            "left:\n{}\nright:\n{}",
            serde_json::to_string_pretty(x).unwrap(),
            serde_json::to_string_pretty(y).unwrap()
        );
    }
    "no block differs (the frontmatter?)".to_owned()
}

/// Parses body Markdown under the standard frontmatter.
fn parsed(md: &str) -> Parsed {
    let text = format!("{}\n{md}", serialize(&doc(Vec::new()), &schema(), &Vault));
    parse(&text, &schema(), &Vault).unwrap_or_else(|e| panic!("{e}\n--- markdown ---\n{text}"))
}

/// The one text of a paragraph of one run.
fn text_of(block: &FileBlock) -> &str {
    block.content["inline"][0]["text"].as_str().unwrap()
}

/// Every kind once with an anchor and once without, as top-level siblings.
fn every_kind(with_ids: bool) -> Vec<FileBlock> {
    let n = |k: u128| with_ids.then_some(k);
    vec![
        para(n(10), "A paragraph with é, — and 漢字."),
        heading(n(11), 1, "Top"),
        heading(n(12), 2, "Second"),
        heading(n(13), 6, "Deep"),
        bullet(n(14), "a bullet", Vec::new()),
        numbered(n(15), "a numbered", Vec::new()),
        todo(n(16), false, "open", Vec::new()),
        todo(n(17), true, "done", Vec::new()),
        quote(n(18), "first line\nsecond line\nafter a gap"),
        code(n(19), "fn main() {}\n    indented", Some("rust")),
        code(n(20), "no language", None),
        divider(n(21)),
        image(n(22), None),
        image(n(23), Some("the alt text")),
        chart(n(24), Vec::new()),
        para(n(25), "the end"),
    ]
}

#[test]
fn every_kind_round_trips_with_and_without_anchors() {
    let (md, parsed) = round_trip("anchored", &doc(every_kind(true)));
    assert!(md.contains(&format!(" ^{}\n", ulid(10))), "{md}");
    assert!(md.contains(&format!("\n\n^{}\n\n", ulid(11))), "{md}");
    assert!(md.contains(&format!("- a bullet ^{}\n", ulid(14))), "{md}");
    assert!(
        md.contains(&format!("1. a numbered ^{}\n", ulid(15))),
        "{md}"
    );
    assert!(md.contains(&format!("- [ ] open ^{}\n", ulid(16))), "{md}");
    assert!(md.contains(&format!("- [x] done ^{}\n", ulid(17))), "{md}");
    assert!(
        md.contains("> first line\n> second line\n> after a gap\n"),
        "{md}"
    );
    assert!(
        md.contains("```rust\nfn main() {}\n    indented\n```\n"),
        "{md}"
    );
    assert!(md.contains("\n***\n"), "{md}");
    let body = md.splitn(3, "---\n").nth(2).unwrap();
    assert!(
        !body.starts_with("---\n") && !body.contains("\n---\n"),
        "a --- line is never written in the body\n{md}"
    );
    assert!(
        md.contains(&format!("![[attachments/{}.png]] ^{}\n", hex(), ulid(22))),
        "{md}"
    );
    assert!(
        md.contains(&format!("![[attachments/{}.png|the alt text]]", hex())),
        "{md}"
    );
    assert!(
        md.contains("````dbme\n{\"children\":[],\"content\":{"),
        "{md}"
    );
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert!(parsed.body_tags.is_empty(), "{:?}", parsed.body_tags);

    let (md, _) = round_trip("anchorless", &doc(every_kind(false)));
    assert!(!md.contains('^'), "no anchor without an id\n{md}");
}

#[test]
fn an_empty_body_is_the_frontmatter_alone() {
    let (md, _) = round_trip("empty", &doc(Vec::new()));
    assert!(md.ends_with("---\n"), "{md}");
    assert!(!md.ends_with("---\n\n"), "{md}");
}

/// The children every list item gets at the deepest level: one of each non-list kind.
fn leaf_children(base: u128) -> Vec<FileBlock> {
    vec![
        para(Some(base), "a child paragraph\nwith a second line"),
        code(Some(base + 1), "child code\n\n```\n---", Some("md")),
        quote(Some(base + 2), "child quote\nline two"),
        heading(Some(base + 3), 3, "child heading"),
        divider(Some(base + 4)),
        image(Some(base + 5), Some("child image")),
        chart(
            Some(base + 6),
            vec![para(Some(base + 7), "inside the fence")],
        ),
        empty_paragraph(base + 8),
        para(Some(base + 9), "last child"),
    ]
}

/// Depth 4 under `first`, then alternating kinds, with mixed and non-list children at every
/// level.
fn nested(first: &str, base: u128) -> FileBlock {
    let level4 = vec![
        bullet(
            Some(base + 40),
            "level four bullet",
            leaf_children(base + 400),
        ),
        todo(Some(base + 41), true, "level four todo", Vec::new()),
        numbered(Some(base + 42), "level four numbered", Vec::new()),
        numbered(Some(base + 43), "level four numbered two", Vec::new()),
    ];
    let level3 = vec![
        todo(Some(base + 30), false, "level three todo", level4),
        para(Some(base + 31), "a paragraph among level-three items"),
        bullet(
            Some(base + 32),
            "level three bullet after a paragraph",
            Vec::new(),
        ),
    ];
    let level2 = vec![
        numbered(Some(base + 20), "level two numbered", level3),
        numbered(Some(base + 21), "level two numbered again", Vec::new()),
        code(Some(base + 22), "level two code", None),
        bullet(Some(base + 23), "level two bullet", Vec::new()),
    ];
    blk(Some(base + 10), first, inline(vec![t("level one")]), level2)
}

#[test]
fn nesting_to_depth_four_under_every_marker_width() {
    let (md, _) = round_trip("under -", &doc(vec![nested("bullet", 1_000)]));
    assert!(md.contains("\n  1. level two numbered ^"), "{md}");
    assert!(md.contains("\n     - [ ] level three todo"), "{md}");
    assert!(md.contains("\n       - level four bullet"), "{md}");
    assert!(
        md.contains("\n         ````md\n         child code\n"),
        "{md}"
    );

    let (md, _) = round_trip("under 1.", &doc(vec![nested("numbered", 2_000)]));
    assert!(md.contains("\n\n1. level one ^"), "{md}");
    assert!(md.contains("\n   1. level two numbered ^"), "{md}");

    let mut ten = Vec::new();
    for k in 0..9 {
        ten.push(numbered(
            Some(3_100 + k),
            &format!("item {}", k + 1),
            Vec::new(),
        ));
    }
    ten.push(nested("numbered", 3_000));
    ten.push(numbered(Some(3_200), "eleven", Vec::new()));
    let (md, _) = round_trip("under 10.", &doc(ten));
    assert!(md.contains("\n9. item 9"), "{md}");
    assert!(md.contains("\n10. level one"), "{md}");
    assert!(md.contains("\n    1. level two numbered ^"), "{md}");
    assert!(md.contains("\n       - [ ] level three todo"), "{md}");
    assert!(md.contains("\n11. eleven"), "{md}");
}

#[test]
fn list_runs_count_from_one_and_switch_on_marker_change() {
    let body = vec![
        bullet(Some(1), "one", Vec::new()),
        bullet(None, "two", Vec::new()),
        numbered(Some(3), "first", Vec::new()),
        numbered(Some(4), "second", Vec::new()),
        bullet(Some(5), "three", Vec::new()),
        todo(Some(6), false, "four", Vec::new()),
        para(Some(7), "a paragraph"),
        numbered(Some(8), "first again", Vec::new()),
        numbered(None, "second again", Vec::new()),
        blk(Some(9), "bullet", inline(Vec::new()), Vec::new()),
        blk(None, "numbered", inline(Vec::new()), Vec::new()),
        blk(
            Some(10),
            "todo",
            json!({ "checked": false, "inline": [] }),
            Vec::new(),
        ),
    ];
    let (md, _) = round_trip("runs", &doc(body));
    assert!(
        md.contains(&format!(
            "- one ^{}\n- two\n\n1. first ^{}\n2. second ^{}\n\n- three",
            ulid(1),
            ulid(3),
            ulid(4)
        )),
        "{md}"
    );
    assert!(md.contains("\n- [ ] four"), "{md}");
    assert!(
        md.contains(&format!(
            "\n\n1. first again ^{}\n2. second again\n\n- ^{}\n\n1.\n\n- [ ] ^{}\n",
            ulid(8),
            ulid(9),
            ulid(10)
        )),
        "{md}"
    );
}

#[test]
fn empty_paragraphs_and_lone_anchors_tell_apart() {
    // An empty paragraph after each kind whose own anchor sits on its own line, after an anchored
    // paragraph, first in the body, twice in a row, and inside a list item.
    let body = vec![
        empty_paragraph(1),
        heading(Some(2), 2, "H"),
        empty_paragraph(3),
        code(Some(4), "c", None),
        empty_paragraph(5),
        quote(Some(6), "q"),
        empty_paragraph(7),
        divider(Some(8)),
        empty_paragraph(9),
        chart(Some(10), Vec::new()),
        empty_paragraph(11),
        para(Some(12), "anchored paragraph"),
        empty_paragraph(13),
        empty_paragraph(14),
        bullet(
            Some(15),
            "item",
            vec![empty_paragraph(16), para(Some(17), "child")],
        ),
        empty_paragraph(18),
    ];
    let (md, _) = round_trip("lone anchors", &doc(body));
    let expected_head = format!(
        "## H\n\n^{}\n\n^{}\n\n```\nc\n```\n\n^{}\n\n^{}\n\n> q\n\n^{}\n\n^{}\n\n***\n\n^{}\n\n^{}\n\n",
        ulid(2),
        ulid(3),
        ulid(4),
        ulid(5),
        ulid(6),
        ulid(7),
        ulid(8),
        ulid(9)
    );
    assert!(md.contains(&expected_head), "{md}");
    let expected_tail = format!(
        "anchored paragraph ^{}\n\n^{}\n\n^{}\n\n- item ^{}\n\n  ^{}\n\n  child ^{}\n\n^{}\n",
        ulid(12),
        ulid(13),
        ulid(14),
        ulid(15),
        ulid(16),
        ulid(17),
        ulid(18)
    );
    assert!(md.contains(&expected_tail), "{md}");

    // Hand-written: a lone anchor after an anchorless heading is the heading's anchor; after an
    // anchored one it is an empty paragraph; a lower-case id is accepted.
    let p = parsed(&format!("# H\n\n^{}\n", ulid(1).to_lowercase()));
    assert_eq!(p.doc.body, vec![heading(Some(1), 1, "H")]);
    let p = parsed(&format!(
        "^{}\n\n# H\n\n^{}\n\n^{}\n",
        ulid(1),
        ulid(2),
        ulid(3)
    ));
    assert_eq!(
        p.doc.body,
        vec![
            empty_paragraph(1),
            heading(Some(2), 1, "H"),
            empty_paragraph(3)
        ]
    );
    round_trip("reparsed lone anchors", &p.doc);
}

/// Every mark, combination, link and mention shape in one run list.
fn hostile_runs() -> Vec<Value> {
    vec![
        t("plain "),
        marked("bold", &["bold"]),
        t(" "),
        marked("italic", &["italic"]),
        t(" "),
        marked("under", &["underline"]),
        t(" "),
        marked("gone", &["strike"]),
        t(" "),
        marked("co`de` with `` ticks", &["code"]),
        t(" "),
        marked("bold italic", &["bold", "italic"]),
        t(", "),
        marked("all four", &["bold", "italic", "underline", "strike"]),
        t(", "),
        marked("bold code", &["bold", "code"]),
        t(" then "),
        linked("a link", "https://example.com/a(b) c?d=(e)&f=g", &[]),
        t(" and "),
        linked("bold link", "https://example.com/x y", &["bold"]),
        t(", "),
        linked("two", "https://example.com/two", &[]),
        linked("links", "https://example.com/links", &["italic"]),
        t(" "),
        mention(ALPHA, None, None, false),
        t(" "),
        mention(BRAVO, Some("Bravo!"), None, false),
        t(" "),
        mention(CHARLIE, None, Some(77), false),
        t(" "),
        mention(ALPHA, Some("shown & block"), Some(78), false),
        t(" "),
        mention(BRAVO, None, None, true),
        t(" "),
        mention(GHOST, Some("no file"), None, false),
        t(" `not code` *not italic* \\ <u>not a tag</u> &amp; ~x~ _y_ [[not a link]]"),
    ]
}

#[test]
fn marks_links_and_mentions_round_trip() {
    let runs = hostile_runs();
    let body = vec![
        blk(Some(1), "paragraph", inline(runs.clone()), Vec::new()),
        blk(Some(2), "bullet", inline(runs.clone()), Vec::new()),
        blk(
            Some(3),
            "heading",
            json!({ "level": 2, "inline": runs.clone() }),
            Vec::new(),
        ),
        blk(Some(4), "quote", inline(runs), Vec::new()),
        blk(
            Some(5),
            "paragraph",
            inline(vec![mention(ALPHA, None, None, true)]),
            Vec::new(),
        ),
    ];
    let (md, p) = round_trip("marks", &doc(body));
    assert!(
        md.contains("**bold** *italic* <u>under</u> ~~gone~~"),
        "{md}"
    );
    assert!(
        md.contains("[a link](https://example.com/a%28b%29%20c?d=%28e%29\\&f=g)"),
        "{md}"
    );
    assert!(
        md.contains("[**bold link**](https://example.com/x%20y)"),
        "{md}"
    );
    assert!(
        md.contains("[[Projects/Alpha]] [[People/Bravo Two|Bravo!]]"),
        "{md}"
    );
    let charlie = format!("[[Notes/2026/Charlie (draft)#^{}]]", ulid(77));
    assert!(md.contains(&charlie), "{md}");
    let alpha = format!("[[Projects/Alpha#^{}|shown \\& block]]", ulid(78));
    assert!(md.contains(&alpha), "{md}");
    assert!(md.contains("![[People/Bravo Two]]"), "{md}");
    let ghost = format!("[[{}|no file]]", ulid(GHOST));
    assert!(md.contains(&ghost), "{md}");
    assert!(md.contains("\\`not code\\` \\*not italic\\* \\\\ \\<u>not a tag\\</u> \\&amp; \\~x\\~ \\_y\\_ [[not a link]]"), "{md}");
    assert!(
        p.warnings.iter().any(|w| w.message.contains("not a link")),
        "{:?}",
        p.warnings
    );
}

#[test]
fn wikilinks_resolve_by_path_title_and_ulid_or_stay_text() {
    let md = format!(
        "see [[Alpha]], [[bravo two|B]], [[Notes/2026/Charlie (draft)]], [[{}]], [[Nowhere]] and [[Alpha#Heading]].\n",
        ulid(GHOST)
    );
    let p = parsed(&md);
    assert_eq!(
        p.doc.body,
        vec![blk(
            None,
            "paragraph",
            inline(vec![
                t("see "),
                mention(ALPHA, None, None, false),
                t(", "),
                mention(BRAVO, Some("B"), None, false),
                t(", "),
                mention(CHARLIE, None, None, false),
                t(", "),
                mention(GHOST, None, None, false),
                t(", [[Nowhere]] and "),
                mention(ALPHA, None, None, false),
                t("."),
            ]),
            Vec::new(),
        )]
    );
    assert_eq!(p.warnings.len(), 2, "{:?}", p.warnings);
    // Five frontmatter lines, a blank line, then the body.
    assert!(p.warnings.iter().all(|w| w.line == 7), "{:?}", p.warnings);
    round_trip("resolved mentions", &p.doc);
}

#[test]
fn body_tags_are_collected_with_their_exclusions() {
    let body = vec![
        para(
            Some(1),
            "#Alpha and #beta/gamma_1 (#Paren) #1 #22 a#b end#\n#second-line",
        ),
        blk(
            Some(2),
            "paragraph",
            inline(vec![
                marked("#code", &["code"]),
                t(" "),
                linked("#link", "https://example.com/#frag", &[]),
                t(" "),
                marked("#Bold", &["bold"]),
                t(" #alpha again"),
            ]),
            Vec::new(),
        ),
        bullet(Some(3), "#item", Vec::new()),
        heading(Some(4), 1, "#heading"),
        code(Some(5), "#not-a-tag", None),
        chart(Some(6), vec![para(Some(7), "#inside-fence")]),
    ];
    let (md, p) = round_trip("tags", &doc(body));
    assert_eq!(
        p.body_tags,
        [
            "alpha",
            "beta/gamma_1",
            "paren",
            "second-line",
            "bold",
            "item",
            "heading"
        ]
    );
    assert!(
        md.contains("\n\\#Alpha and #beta"),
        "a line-start # is escaped\n{md}"
    );
    assert!(md.contains("\n\\#second-line"), "{md}");
    // After the heading marker a `#` opens nothing, so it is not escaped and still reads as a tag.
    assert!(md.contains("# #heading\n"), "{md}");
    assert!(md.contains("- \\#item ^"), "{md}");
}

#[test]
fn code_blocks_keep_fences_dashes_and_languages() {
    let body = vec![
        code(Some(1), "```\n---\n````\n...\n~~~", Some("md")),
        code(Some(2), "", None),
        code(Some(3), "", Some("txt")),
        code(Some(4), "\n\nleading blanks", None),
        code(Some(5), "trailing spaces   \n  and indent", Some("c++")),
        code(Some(6), "{\"kind\": \"x\"}", Some("dbme-not")),
        code(
            Some(7),
            "a `dbme` code block that is not JSON",
            Some("dbme"),
        ),
    ];
    let (md, _) = round_trip("code", &doc(body));
    assert!(
        md.contains("`````md\n```\n---\n````\n...\n~~~\n`````\n"),
        "{md}"
    );
    assert!(
        md.contains("\n```\n```\n\n^"),
        "empty code is two fences\n{md}"
    );
    assert!(md.contains("\n```txt\n```\n"), "{md}");
    assert!(md.contains("\n```\n\n\nleading blanks\n```\n"), "{md}");
    assert!(
        md.contains("\n```dbme\na `dbme` code block that is not JSON\n```\n"),
        "{md}"
    );

    // A language with whitespace or a backtick, a trailing newline, CRLF: normalised, then stable.
    let mut odd = doc(vec![
        code(Some(1), "x\n\n", Some("rust ignore")),
        code(Some(2), "y", Some("a`b")),
        code(Some(3), "z\r\nw\r\n", Some("")),
    ]);
    let md = serialize(&odd, &schema(), &Vault);
    normalize(&mut odd);
    assert_eq!(
        odd.body,
        vec![
            code(Some(1), "x", Some("rust")),
            code(Some(2), "y", None),
            code(Some(3), "z\nw", None)
        ]
    );
    let reread = parse(&md, &schema(), &Vault).unwrap().doc;
    assert!(
        reread == odd,
        "{}\n{md}",
        first_difference(&reread.body, &odd.body)
    );
    round_trip("normalised code", &odd);
}

#[test]
fn dbme_fences_hold_unknown_kinds_and_fall_back_when_malformed() {
    let children = vec![
        para(Some(2), "child"),
        bullet(
            Some(3),
            "item",
            vec![todo(Some(4), true, "deep", Vec::new())],
        ),
        chart(Some(5), Vec::new()),
        blk(None, "widget", json!({ "a": null, "b": [] }), Vec::new()),
    ];
    let body = vec![
        chart(Some(1), children),
        blk(None, "callout", json!({ "text": "```\n---" }), Vec::new()),
    ];
    let (md, _) = round_trip("dbme", &doc(body));
    assert!(md.contains(&format!("\n````\n\n^{}\n", ulid(1))), "{md}");
    assert!(md.contains("````dbme\n{\"children\":[],\"content\":{\"text\":\"```\\n---\"},\"kind\":\"callout\"}\n````\n"), "{md}");

    for bad in [
        "{not json",
        "[]",
        "{\"kind\": 1}",
        "{\"kind\": \"x\"}",
        "{\"kind\": \"x\", \"content\": 3}",
    ] {
        let p = parsed(&format!("```dbme\n{bad}\n```\n"));
        assert_eq!(p.doc.body, vec![code(None, bad, Some("dbme"))], "{bad}");
        round_trip(bad, &p.doc);
    }
}

#[test]
fn tables_and_html_are_kept_as_text() {
    let table = "| a | b |\n| --- | --- |\n| 1 | 2 |";
    let html = "<div class=\"x\">\nhello\n</div>";
    let p = parsed(&format!(
        "{table}\n\n^{}\n\n{html}\n\n^{}\n",
        ulid(1),
        ulid(2)
    ));
    assert_eq!(p.doc.body.len(), 2, "{:?}", p.doc.body);
    assert_eq!(text_of(&p.doc.body[0]), table);
    assert_eq!(p.doc.body[0].id, Some(id(1)));
    assert_eq!(text_of(&p.doc.body[1]), html);
    assert_eq!(p.doc.body[1].id, Some(id(2)));
    let (md, _) = round_trip("table and html", &p.doc);
    assert!(md.contains("\\| a | b |\n\\| --- | --- |"), "{md}");
    assert!(md.contains("\\<div class=\"x\">\nhello\n\\</div>"), "{md}");
}

#[test]
fn line_starts_and_anchor_lookalikes_are_escaped() {
    let starts = [
        "- not a list",
        "+ not a list",
        "* not a list",
        "1. not numbered",
        "10) not numbered",
        "# not a heading",
        "## nor this",
        "> not a quote",
        "^ not an anchor",
        "| not a table",
        "= not a setext line",
        "~~~ not a fence",
        "``` not a fence",
        "***",
        "---",
        "___",
        "<div> not html",
        "[ ] not a todo",
        "    not indented code",
    ];
    let mut body: Vec<FileBlock> = starts
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.starts_with(' '))
        .map(|(i, s)| para(Some(i as u128 + 1), s))
        .collect();
    // Every line start inside one run (an indented line is not normal form: it is trimmed).
    let joined = starts
        .iter()
        .filter(|s| !s.starts_with(' '))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    body.push(para(Some(100), &joined));
    body.push(bullet(Some(101), &joined, Vec::new()));
    body.push(quote(Some(102), &joined));
    body.push(para(
        Some(103),
        &format!("ends like an anchor ^{}", ulid(5)),
    ));
    body.push(para(Some(104), &format!("^{}", ulid(6)).replace('^', "^")));
    body.push(bullet(Some(105), &format!("item ^{}", ulid(7)), Vec::new()));
    body.push(bullet(Some(106), "[ ] not a todo", Vec::new()));
    body.push(bullet(Some(107), "[x] nor this", Vec::new()));
    body.push(heading(Some(108), 2, "closing ##"));
    body.push(heading(Some(109), 2, "#"));
    body.push(para(Some(110), "a hard break\\\nnext"));
    let (md, _) = round_trip("escapes", &doc(body));
    assert!(md.contains("\n\\- not a list ^"), "{md}");
    assert!(md.contains("\n1\\. not numbered ^"), "{md}");
    assert!(md.contains("\n10\\) not numbered ^"), "{md}");
    assert!(md.contains("\n\\# not a heading ^"), "{md}");
    assert!(md.contains("\n\\> not a quote ^"), "{md}");
    assert!(md.contains("\n\\^ not an anchor ^"), "{md}");
    assert!(
        md.contains(&format!(
            "ends like an anchor \\^{} ^{}\n",
            ulid(5),
            ulid(103)
        )),
        "{md}"
    );
    assert!(
        md.contains(&format!("\\^{} ^{}\n", ulid(6), ulid(104))),
        "{md}"
    );
    assert!(
        md.contains(&format!("- item \\^{} ^{}\n", ulid(7), ulid(105))),
        "{md}"
    );
    assert!(md.contains("- \\[ ] not a todo ^"), "{md}");
    assert!(md.contains("## closing \\##\n"), "{md}");

    // The indented line is trimmed by normalisation, so it is not in normal form by itself.
    let mut indented = doc(vec![para(Some(1), "    not indented code\n  nor this  ")]);
    let md = serialize(&indented, &schema(), &Vault);
    normalize(&mut indented);
    assert_eq!(
        indented.body,
        vec![para(Some(1), "not indented code\nnor this")]
    );
    assert_eq!(parse(&md, &schema(), &Vault).unwrap().doc, indented);
}

#[test]
fn crlf_input_reads_like_lf() {
    let mut body = every_kind(true);
    body.push(nested("bullet", 5_000));
    body.push(blk(
        Some(6_000),
        "paragraph",
        inline(hostile_runs()),
        Vec::new(),
    ));
    let d = doc(body);
    let (md, _) = round_trip("lf", &d);
    let crlf = md.replace('\n', "\r\n");
    let p = parse(&crlf, &schema(), &Vault).unwrap();
    assert_eq!(p.doc, d);
    assert_eq!(serialize(&p.doc, &schema(), &Vault), md);
}

#[test]
fn parse_of_serialize_is_normalize_for_docs_outside_the_normal_form() {
    let docs = [
        doc(vec![heading(Some(1), 2, "two\nlines\r\nthree")]),
        doc(vec![blk(
            Some(2),
            "paragraph",
            inline(vec![
                t("  a "),
                t(""),
                t("b  \n\n\n  c  "),
                marked(" d ", &["bold"]),
            ]),
            Vec::new(),
        )]),
        doc(vec![blk(
            Some(3),
            "paragraph",
            inline(vec![
                t("same"),
                t(" marks"),
                marked("x", &["code"]),
                marked("y", &["code"]),
            ]),
            Vec::new(),
        )]),
        doc(vec![blk(
            Some(4),
            "paragraph",
            inline(vec![t("\n\nlead"), t("trail\n")]),
            Vec::new(),
        )]),
        doc(vec![blk(
            Some(5),
            "paragraph",
            inline(vec![
                json!({ "mention": ulid(ALPHA), "shown": "" }),
                json!({ "text": "x", "link": "" }),
            ]),
            Vec::new(),
        )]),
    ];
    for (i, d) in docs.iter().enumerate() {
        let md = serialize(d, &schema(), &Vault);
        let mut normal = d.clone();
        normalize(&mut normal);
        let mut twice = normal.clone();
        normalize(&mut twice);
        assert_eq!(twice, normal, "{i}: normalize is not idempotent");
        let p = parse(&md, &schema(), &Vault).unwrap_or_else(|e| panic!("{i}: {e}\n{md}"));
        assert!(
            p.doc == normal,
            "{i}: parse(serialize(d)) != normalize(d)\n{}\n{md}",
            first_difference(&p.doc.body, &normal.body)
        );
        round_trip(&format!("normalised {i}"), &normal);
    }
}

#[test]
fn nesting_past_the_cap_is_an_error_with_a_position() {
    let mut md = String::new();
    for depth in 0..=MAX_BLOCK_DEPTH {
        md.push_str(&"  ".repeat(depth));
        md.push_str("- x\n");
    }
    let head = format!("{}\n", serialize(&doc(Vec::new()), &schema(), &Vault));
    let text = format!("{head}{md}");
    let err = parse(&text, &schema(), &Vault).unwrap_err();
    // The item at depth 0 is the first body line; the one at `MAX_BLOCK_DEPTH` is the offender.
    let first_body_line = head.matches('\n').count() + 1;
    assert_eq!(err.line, first_body_line + MAX_BLOCK_DEPTH, "{err}");
    assert!(err.message.contains("deep"), "{err}");

    let mut ok = String::new();
    for depth in 0..MAX_BLOCK_DEPTH {
        ok.push_str(&"  ".repeat(depth));
        ok.push_str("- x\n");
    }
    let p = parsed(&ok);
    assert_eq!(p.doc.body.len(), 1);
    round_trip("depth 64", &p.doc);
}

// ---------------------------------------------------------------------------------------------
// Regressions from the M5 Task 2 review (2026-11-02)
// ---------------------------------------------------------------------------------------------

/// The body of a parsed text with no frontmatter: the blocks alone.
fn body_only(md: &str) -> Vec<FileBlock> {
    parse(md, &schema(), &Vault)
        .unwrap_or_else(|e| panic!("{e}\n{md}"))
        .doc
        .body
}

#[test]
fn a_block_inside_a_quote_carries_no_quote_markers() {
    // A code block, a table and an HTML block inside `> `: pulldown's source range spans the raw
    // lines, which still carry the markers; the reader strips them. Nested quotes strip one per
    // level, and a quote inside a list item strips the item's indentation before its marker.
    let cases = [
        ("> ```\n> code\n> ```\n", "```\ncode\n```"),
        (
            "> | a | b |\n> |---|---|\n> | 1 | 2 |\n",
            "| a | b |\n|---|---|\n| 1 | 2 |",
        ),
        ("> <div>\n> x\n> </div>\n", "<div>\nx\n</div>"),
        ("> > ```\n> > code\n> > ```\n", "```\ncode\n```"),
        // A `>` that is code content inside the quote stays, once the container markers are off.
        (
            "> ```\n> > not a marker\n> ```\n",
            "```\n> not a marker\n```",
        ),
        ("> ```\n>code\n>```\n", "```\ncode\n```"),
    ];
    for (md, expected) in cases {
        let body = body_only(md);
        assert_eq!(body.len(), 1, "{md:?}: {body:#?}");
        assert_eq!(body[0].kind, "quote", "{md:?}");
        assert_eq!(text_of(&body[0]), expected, "{md:?}");
        // What it writes reads back the same, so the clean text is the normal form.
        let mut d = doc(body);
        normalize(&mut d);
        round_trip(md, &d);
    }
    let nested_in_item = body_only("- item\n\n  > ```\n  > code\n  > ```\n");
    assert_eq!(nested_in_item.len(), 1);
    assert_eq!(nested_in_item[0].children.len(), 1);
    assert_eq!(nested_in_item[0].children[0].kind, "quote");
    assert_eq!(text_of(&nested_in_item[0].children[0]), "```\ncode\n```");
}

#[test]
fn an_escaped_caret_is_text_not_an_anchor() {
    let anchor = ulid(7);
    // The writer escapes a caret that would read as an anchor; the reader must not strip it.
    let escaped = body_only(&format!("a \\^{anchor}\n"));
    assert_eq!(escaped, vec![para(None, &format!("a ^{anchor}"))]);
    let lone = body_only(&format!("\\^{anchor}\n"));
    assert_eq!(lone, vec![para(None, &format!("^{anchor}"))]);
    let in_bullet = body_only(&format!("- see \\^{anchor}\n"));
    assert_eq!(
        in_bullet,
        vec![bullet(None, &format!("see ^{anchor}"), vec![])]
    );
    // An anchor inside a mark is not one for Obsidian either: the id ends with the delimiter.
    for md in [
        format!("**see \\^{anchor}**\n"),
        format!("**see ^{anchor}**\n"),
        format!("*see ^{anchor}*\n"),
        format!("`see ^{anchor}`\n"),
    ] {
        let body = body_only(&md);
        assert_eq!(body.len(), 1, "{md:?}");
        assert_eq!(body[0].id, None, "{md:?}");
    }
    // Trailing text after a mark still carries the anchor.
    let after_mark = body_only(&format!("**see** this ^{anchor}\n"));
    assert_eq!(after_mark[0].id, Some(id(7)));
    assert_eq!(
        after_mark[0].content["inline"],
        json!([marked("see", &["bold"]), t(" this")])
    );
    // An escaped anchor after an embed makes it a paragraph, not an anchored image.
    let hash = ContentHash::of(b"pic");
    let embed = body_only(&format!(
        "![[attachments/{}.png]] \\^{anchor}\n",
        hash.hex()
    ));
    assert_eq!(embed[0].kind, "paragraph");
    assert_eq!(embed[0].id, None);
    let anchored = body_only(&format!("![[attachments/{}.png]] ^{anchor}\n", hash.hex()));
    assert_eq!(anchored[0].kind, "image");
    assert_eq!(anchored[0].id, Some(id(7)));
    // Round trips of id-less blocks whose text ends in, or is, an anchor.
    for d in [
        doc(vec![para(None, &format!("see ^{anchor}"))]),
        doc(vec![para(None, &format!("^{anchor}"))]),
        doc(vec![bullet(None, &format!("see ^{anchor}"), vec![])]),
        doc(vec![numbered(None, &format!("^{anchor}"), vec![])]),
        doc(vec![para(Some(8), &format!("see ^{anchor}"))]),
    ] {
        let (md, p) = round_trip("escaped anchor", &d);
        assert!(md.contains("\\^"), "{md}");
        assert_eq!(p.doc, d);
    }
}

#[test]
fn an_image_blob_that_is_no_hash_cannot_inject_blocks() {
    let anchor = ulid(9);
    let blob = format!("x]]\n\n^{anchor}\n\n# injected heading\n\n```");
    let fence = json!({ "kind": "image", "content": { "blob": blob } }).to_string();
    let md = format!("```dbme\n{fence}\n```\n\nafter\n");
    let first = parse(&md, &schema(), &Vault).unwrap();
    // The fence's block is normalised on the way in: a non-hash blob is a paragraph of its text.
    assert_eq!(first.doc.body.len(), 2);
    assert_eq!(first.doc.body[0].kind, "paragraph");
    assert_eq!(first.doc.body[0].id, None);
    assert_eq!(first.doc.body[1], para(None, "after"));
    let out = serialize(&first.doc, &schema(), &Vault);
    let second = parse(&out, &schema(), &Vault).unwrap();
    assert_eq!(second.doc, first.doc, "{out}");
    assert!(
        second.doc.body.iter().all(|b| b.id != Some(id(9))),
        "the blob minted a block id:\n{out}"
    );
    // The same block built directly (the `from_rows` path with junk `content_json`).
    let mut d = doc(vec![
        blk(
            Some(1),
            "image",
            json!({ "blob": blob, "alt": "a|b" }),
            vec![],
        ),
        para(Some(2), "after"),
    ]);
    let out = serialize(&d, &schema(), &Vault);
    let p = parse(&out, &schema(), &Vault).unwrap();
    normalize(&mut d);
    assert_eq!(p.doc, d, "{out}");
    assert_eq!(d.body[0].kind, "paragraph");
    assert_eq!(d.body[1], para(Some(2), "after"));
    assert!(text_of(&d.body[0]).starts_with("![[attachments/x]]"));
    assert!(text_of(&d.body[0]).ends_with("|ab]]"));
    // A real hash stays an image.
    let hash = ContentHash::of(b"pic");
    let image = doc(vec![blk(
        Some(3),
        "image",
        json!({ "blob": hash.as_str() }),
        vec![],
    )]);
    round_trip("real image", &image);
}

#[test]
fn hostile_sizes_parse_and_serialize_in_linear_time() {
    // Each case was quadratic (minutes at the 16 MiB cap); a loose bound catches a return to it.
    let budget = std::time::Duration::from_secs(30);
    // One warning per paragraph: the warning's line comes from a line table, not a rescan.
    let wikilinks = "[[x]]\n\n".repeat((1 << 20) / 7);
    let start = std::time::Instant::now();
    let p = parse(&wikilinks, &schema(), &Vault).unwrap();
    assert!(p.warnings.len() > 100_000);
    assert!(start.elapsed() < budget, "wikilinks: {:?}", start.elapsed());
    // Many distinct tags: a set, not a scan of the list.
    let mut tags = String::new();
    for i in 0..120_000 {
        tags.push_str(&format!("#t{i} "));
        if i % 20 == 19 {
            tags.push_str("\n\n");
        }
    }
    let start = std::time::Instant::now();
    let p = parse(&tags, &schema(), &Vault).unwrap();
    assert_eq!(p.body_tags.len(), 120_000);
    assert!(start.elapsed() < budget, "tags: {:?}", start.elapsed());
    // A run of `[` and a line of `^`: the writer's escape checks are constant per character.
    for line in [
        format!("{}\n", "[".repeat(400_000)),
        format!("{}\n", "^".repeat(400_000)),
    ] {
        let p = parse(&line, &schema(), &Vault).unwrap();
        let start = std::time::Instant::now();
        let out = serialize(&p.doc, &schema(), &Vault);
        assert!(
            start.elapsed() < budget,
            "{:?}: {:?}",
            &line[..3],
            start.elapsed()
        );
        assert_eq!(parse(&out, &schema(), &Vault).unwrap().doc, p.doc);
    }
}

#[test]
fn a_tree_deeper_than_the_cap_is_flattened_not_refused() {
    // A chain of 200 bullets, each the child of the last, as a caller might build it: the
    // writer flattens what the parser could not read back, on a 1 MiB stack.
    let mut chain = bullet(Some(200), "x", vec![]);
    for n in (1..200).rev() {
        chain = bullet(Some(n), "x", vec![chain]);
    }
    let d = doc(vec![chain]);
    let out = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let out = serialize(&d, &schema(), &Vault);
            let p = parse(&out, &schema(), &Vault).unwrap_or_else(|e| panic!("{e}"));
            let mut normal = d;
            normalize(&mut normal);
            assert_eq!(p.doc, normal);
            assert!(normal.body.len() > 1, "nothing was flattened");
            let mut count = 0;
            let mut stack: Vec<(&FileBlock, usize)> = normal.body.iter().map(|b| (b, 1)).collect();
            while let Some((b, depth)) = stack.pop() {
                count += 1;
                assert!(depth <= MAX_BLOCK_DEPTH, "depth {depth}");
                stack.extend(b.children.iter().map(|c| (c, depth + 1)));
            }
            assert_eq!(count, 200, "a block was lost");
            out
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(out.contains("- x"));
}
