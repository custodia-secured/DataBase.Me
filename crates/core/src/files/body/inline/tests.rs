// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests of the inline layer: every escaping rule written and read back through a paragraph
//! parse, marks, links, mentions, tags, the normal form, and a proptest over a hostile alphabet.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure is the finding"
)]

use proptest::prelude::*;
use pulldown_cmark::{Parser, Tag, TagEnd};
use serde_json::json;

use super::super::parser_options;
use super::*;
use crate::ContentHash;
use crate::files::NoResolver;

const ID: &str = "01JB3R7Q8Y5K3M2N4P6Q8R0S2T";
const ID2: &str = "01JB3R9C0D4E5F6G7H8J9K0M1N";

/// A vault with one object, `Projects/Alpha`, whose id is `ID`.
struct Vault;

impl LinkResolver for Vault {
    fn resolve(&self, target: &str) -> Option<Id> {
        matches!(target, "Projects/Alpha" | "Alpha").then(|| Id::parse(ID).unwrap())
    }

    fn path_of(&self, id: Id) -> Option<String> {
        (id == Id::parse(ID).unwrap()).then(|| "Projects/Alpha".to_owned())
    }

    fn attachment_name(&self, blob: &ContentHash) -> String {
        blob.hex().to_owned()
    }
}

fn write_with(runs: &[Value], resolver: &dyn LinkResolver, at_line_start: bool) -> String {
    let mut out = String::new();
    write_inline(runs, resolver, &mut out, at_line_start);
    out
}

fn write(runs: &[Value]) -> String {
    write_with(runs, &NoResolver, true)
}

/// Parses `md` as one paragraph and reads its inline events.
fn read_with(md: &str, resolver: &dyn LinkResolver) -> (Vec<Value>, Vec<Warning>, Vec<String>) {
    let source = format!("{md}\n");
    let events: Vec<_> = Parser::new_ext(&source, parser_options())
        .into_offset_iter()
        .collect();
    assert!(
        matches!(events.first(), Some((Event::Start(Tag::Paragraph), _))),
        "{md:?} does not start a paragraph: {:?}",
        events.first()
    );
    assert!(
        matches!(events.last(), Some((Event::End(TagEnd::Paragraph), _))),
        "{md:?} is more than one paragraph: {:?}",
        events.last()
    );
    let inner = &events[1..events.len() - 1];
    assert!(
        !inner
            .iter()
            .any(|(e, _)| matches!(e, Event::Start(Tag::Paragraph))),
        "{md:?} holds a second paragraph"
    );
    let mut warnings = Vec::new();
    let mut tags = TagSink::default();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let runs = read_inline(
        inner,
        &source,
        &line_starts,
        resolver,
        &mut warnings,
        &mut tags,
    );
    (runs, warnings, tags.order().to_vec())
}

fn read(md: &str) -> Vec<Value> {
    read_with(md, &NoResolver).0
}

fn normalized(runs: &[Value]) -> Vec<Value> {
    let mut content = json!({ "inline": runs });
    normalize_inline(&mut content);
    content["inline"].as_array().unwrap().clone()
}

/// Writes, reads back through a paragraph parse and checks the runs are the normal form.
fn round_trip_with(runs: &[Value], resolver: &dyn LinkResolver) -> String {
    let md = write_with(runs, resolver, true);
    let (back, _, _) = read_with(&md, resolver);
    assert_eq!(back, normalized(runs), "through {md:?}");
    md
}

fn round_trip(runs: &[Value]) -> String {
    round_trip_with(runs, &NoResolver)
}

fn text(t: &str) -> Value {
    json!({ "text": t })
}

#[test]
fn special_characters_are_escaped_where_they_would_change_the_parse() {
    let t = "\\ * _ ` ~ & <a <b </c <! <? <1 < 2 ! [ ] ( ) é — 中";
    let md = round_trip(&[text(t)]);
    for escaped in [
        "\\\\", "\\*", "\\_", "\\`", "\\~", "\\&", "\\<a", "\\</c", "\\<!", "\\<?",
    ] {
        assert!(md.contains(escaped), "{escaped:?} missing in {md:?}");
    }
    for plain in ["<1", "< 2", " ! ", " [ ] ( ) é — 中"] {
        assert!(md.contains(plain), "{plain:?} missing in {md:?}");
    }
    assert!(!md.contains("\\!") && !md.contains("\\["), "{md:?}");
    // Mid-line, the line-start set stays plain, and so does `#` (a body tag).
    assert_eq!(
        write(&[text("a - b + c > d | e = f 1. g 10) h # i ^j :k")]),
        "a - b + c > d | e = f 1. g 10) h # i ^j :k"
    );
}

#[test]
fn line_starts_are_escaped_on_every_line() {
    let cases = [
        ("- x", "\\- x"),
        ("+ x", "\\+ x"),
        ("* x", "\\* x"),
        ("> x", "\\> x"),
        ("| a |", "\\| a |"),
        ("= x", "\\= x"),
        ("===", "\\==="),
        ("~~~", "\\~\\~\\~"),
        ("```", "\\`\\`\\`"),
        ("# x", "\\# x"),
        ("#tag", "\\#tag"),
        ("1. x", "1\\. x"),
        ("10) x", "10\\) x"),
        ("12", "12"),
        ("1x.", "1x."),
        ("^x", "\\^x"),
        (": x", "\\: x"),
        ("[x]: y", "[x]\\: y"),
        ("[x] y", "[x] y"),
    ];
    for (input, expected) in cases {
        assert_eq!(write(&[text(input)]), expected, "{input:?}");
        let second = format!("a\n{input}");
        assert_eq!(
            write(&[text(&second)]),
            format!("a\n{expected}"),
            "{second:?}"
        );
        round_trip(&[text(input)]);
        round_trip(&[text(&second)]);
    }
    // Not at a line start (a heading's text): nothing from that set is escaped.
    assert_eq!(write_with(&[text("- x")], &NoResolver, false), "- x");
    assert_eq!(
        write_with(&[text("1. x\n- y")], &NoResolver, false),
        "1. x\n\\- y"
    );
}

#[test]
fn a_caret_that_would_read_as_an_anchor_is_escaped() {
    let with_anchor = format!("a ^{ID}");
    assert_eq!(write(&[text(&with_anchor)]), format!("a \\^{ID}"));
    round_trip(&[text(&with_anchor)]);
    let two_lines = format!("a ^{ID}\nb");
    assert_eq!(write(&[text(&two_lines)]), format!("a \\^{ID}\nb"));
    round_trip(&[text(&two_lines)]);
    let lower = format!("^{}", ID.to_lowercase());
    assert_eq!(write(&[text(&lower)]), format!("\\{lower}"));
    assert_eq!(write(&[text("a ^notanid")]), "a ^notanid");
    assert_eq!(write(&[text(&format!("a ^{ID}x"))]), format!("a ^{ID}x"));
    assert_eq!(write(&[text(&format!("a^{ID}"))]), format!("a\\^{ID}"));
}

#[test]
fn marks_nest_in_order_and_combine() {
    let runs = [
        text("a "),
        json!({ "text": "b", "bold": true }),
        text(" "),
        json!({ "text": "c", "italic": true }),
        text(" "),
        json!({ "text": "d", "underline": true }),
        text(" "),
        json!({ "text": "e", "strike": true }),
        text(" "),
        json!({ "text": "f", "code": true }),
        text(" "),
        json!({ "text": "g", "bold": true, "italic": true }),
        text(" "),
        json!({ "text": "h", "bold": true, "italic": true, "underline": true, "strike": true, "code": true }),
    ];
    let md = round_trip(&runs);
    assert_eq!(
        md,
        "a **b** *c* <u>d</u> ~~e~~ `f` ***g*** ***<u>~~`h`~~</u>***"
    );
    assert_eq!(
        write(&[text("x"), json!({ "text": "y", "bold": true }), text("z")]),
        "x**y**z"
    );
    round_trip(&[text("x"), json!({ "text": "y", "bold": true }), text("z")]);
}

#[test]
fn delimiters_fall_back_to_tags_where_commonmark_would_not_match_them() {
    // After a letter and before punctuation `**` cannot open.
    let md = round_trip(&[text("a"), json!({ "text": "!b", "bold": true })]);
    assert_eq!(md, "a<b>!b</b>");
    // Before a letter and after punctuation `*` cannot close.
    let md = round_trip(&[json!({ "text": "a!", "italic": true }), text("b")]);
    assert_eq!(md, "<i>a!</i>b");
    // Two runs of the same delimiter back to back would merge into one run.
    let md = round_trip(&[
        json!({ "text": "a", "bold": true }),
        json!({ "text": "b", "bold": true, "italic": true }),
    ]);
    assert_eq!(md, "<b>a</b>***b***");
    let md = round_trip(&[
        json!({ "text": "a", "strike": true }),
        json!({ "text": "b", "strike": true, "bold": true }),
    ]);
    assert_eq!(md, "~~a~~**~~b~~**");
    let md = round_trip(&[
        json!({ "text": "a", "bold": true }),
        json!({ "text": "b", "italic": true }),
    ]);
    assert_eq!(md, "<b>a</b>*b*");
    // An escaped `*` in the text before is punctuation too; the fallback is harmless.
    round_trip(&[text("*"), json!({ "text": "b", "bold": true }), text("*")]);
    round_trip(&[
        text("a"),
        json!({ "text": "\\b\\", "bold": true, "italic": true }),
        text("c"),
    ]);
    // Marks that start inside a code span or an underline: the inner delimiter is punctuation.
    round_trip(&[
        text("a"),
        json!({ "text": "b", "bold": true, "code": true }),
        text("c"),
    ]);
    round_trip(&[
        text("a"),
        json!({ "text": "b", "bold": true, "underline": true }),
        text("c"),
    ]);
}

#[test]
fn code_spans_use_longer_fences_and_padding() {
    let cases = [
        ("a", "`a`"),
        ("a`b", "``a`b``"),
        ("``", "``` `` ```"),
        ("`a", "`` `a ``"),
        ("a`", "`` a` ``"),
        (" a ", "`  a  `"),
        ("   ", "`   `"),
        (" a", "` a`"),
        ("*_&<a", "`*_&<a`"),
    ];
    for (code, expected) in cases {
        let run = json!({ "text": code, "code": true });
        assert_eq!(write(std::slice::from_ref(&run)), expected, "{code:?}");
        round_trip(&[run]);
    }
    // A newline inside a code span is a space in normal form, as CommonMark reads it.
    let md = round_trip(&[json!({ "text": "a\nb", "code": true })]);
    assert_eq!(md, "`a b`");
    assert_eq!(read("`a\nb`"), [json!({ "text": "a b", "code": true })]);
}

#[test]
fn links_wrap_marked_text_and_encode_the_url() {
    let runs = [
        json!({ "text": "a", "link": "http://x.y/p" }),
        json!({ "text": "b", "bold": true, "link": "http://x.y/p" }),
        text("c"),
        json!({ "text": "d", "link": "http://x.y/q" }),
    ];
    let md = round_trip(&runs);
    assert_eq!(md, "[a**b**](http://x.y/p)c[d](http://x.y/q)");
    let url = "u (v) w<x>y&z\\";
    let md = round_trip(&[json!({ "text": "t", "link": url })]);
    assert_eq!(md, "[t](u%20%28v%29%20w%3Cx%3Ey\\&z\\\\)");
    // Brackets inside link text would end it: escaped there, nowhere else.
    let md = round_trip(&[json!({ "text": "[a]", "link": "u" }), text("[b]")]);
    assert_eq!(md, "[\\[a\\]](u)[b]");
    // Read side: autolinks and images are links on their text; `%20` and friends are decoded.
    assert_eq!(
        read("<http://x.y> ![alt](p%20q)"),
        [
            json!({ "text": "http://x.y", "link": "http://x.y" }),
            text(" "),
            json!({ "text": "alt", "link": "p q" }),
        ]
    );
    // A `[[` that its own run does not close would swallow the marked run after it; a closed one
    // stays a wikilink (an unresolved one reads back as its text).
    let md = round_trip(&[
        text("[["),
        json!({ "text": "x", "bold": true }),
        text("]] [[y]]"),
    ]);
    assert_eq!(md, "\\[[**x**]] [[y]]");
    assert_eq!(
        write(&[
            text("[[a|"),
            json!({ "text": "b", "bold": true }),
            text("]]")
        ]),
        "\\[[a|**b**]]"
    );
    assert_eq!(write(&[text("[[a]] [[b")]), "[[a]] \\[[b");
    // Literal `[text](url)` in a text run is read as a link: the known limit of never escaping `[`.
    assert_eq!(
        read(&write(&[text("[a](b)")])),
        [json!({ "text": "a", "link": "b" })]
    );
}

#[test]
fn mentions_render_paths_and_fall_back_to_ids() {
    let id2 = Id::parse(ID2).unwrap();
    let runs = [
        json!({ "mention": ID }),
        text(" "),
        json!({ "mention": ID, "shown": "Al*pha|" }),
        text(" "),
        json!({ "mention": ID, "block": ID2 }),
        text(" "),
        json!({ "mention": ID, "embed": true }),
        text(" "),
        json!({ "mention": ID2 }),
        text(" "),
        json!({ "mention": ID2, "shown": "x", "block": ID, "embed": true }),
    ];
    let md = round_trip_with(&runs, &Vault);
    assert_eq!(
        md,
        format!(
            "[[Projects/Alpha]] [[Projects/Alpha|Al\\*pha]] [[Projects/Alpha#^{ID2}]] \
             ![[Projects/Alpha]] [[{ID2}]] ![[{ID2}#^{ID}|x]]"
        )
    );
    assert_eq!(id2.to_string(), ID2);
    // Resolution by name, a ULID target without a resolver, and what stays text.
    let (runs, warnings, _) = read_with(
        &format!(
            "[[Alpha]] [[{}]] [[Nope|shown]] ![[Nope]] [[Alpha#Heading]] [[a\\*b]]",
            ID2.to_lowercase()
        ),
        &Vault,
    );
    assert_eq!(
        runs,
        [
            json!({ "mention": ID }),
            text(" "),
            json!({ "mention": ID2 }),
            text(" [[Nope|shown]] ![[Nope]] "),
            json!({ "mention": ID }),
            text(" [[a*b]]"),
        ]
    );
    let messages: Vec<&str> = warnings.iter().map(|w| w.message.as_str()).collect();
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(messages[0].starts_with("[[Nope|shown]]"));
    assert!(messages[2].contains("Heading"));
    assert!(warnings.iter().all(|w| w.line == 1));
    // An unresolved wikilink written back keeps its escapes stable.
    assert_eq!(write(&[text("[[a*b]]")]), "[[a\\*b]]");
    assert_eq!(read(&format!("[[{ID}#^nope]]")), [json!({ "mention": ID })]);
}

#[test]
fn tags_are_collected_from_plain_text_only() {
    let md = "Hello #Tag and #tag/sub (#x_y-z) #1 a#b `#code` [#link](u) **#bold** #é #";
    let (runs, _, tags) = read_with(md, &NoResolver);
    assert_eq!(tags, ["tag", "tag/sub", "x_y-z", "bold"]);
    assert_eq!(runs[0], text("Hello #Tag and #tag/sub (#x_y-z) #1 a#b "));
    let mut again = Vec::new();
    let mut tags = TagSink::default();
    tags.insert("tag".to_owned());
    read_inline(&[], "", &[0], &NoResolver, &mut again, &mut tags);
    collect_tags("#Tag #new", &mut tags);
    assert_eq!(tags.order(), ["tag", "new"]);
    // Many distinct tags cost a set lookup each, not a scan of the list.
    let many: String = (0..20_000).map(|i| format!("#t{i} ")).collect();
    let mut sink = TagSink::default();
    let start = std::time::Instant::now();
    collect_tags(&many, &mut sink);
    assert_eq!(sink.order().len(), 20_000);
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

#[test]
fn normalize_inline_puts_runs_into_normal_form() {
    let mut content = json!({ "inline": [
        { "text": "  a  \n\n  b  " },
        { "text": "" },
        { "text": " c", "bold": true },
        { "text": "d", "bold": true, "italic": false },
        { "text": "x\ny", "code": true },
        { "text": "e", "link": "p%28q%29%20r\n" },
        { "text": "f", "link": "" },
        { "mention": ID, "shown": "a]|b\n", "block": "nope", "embed": false, "extra": 1 },
        { "mention": "nope" },
        { "mention": ID2, "shown": "" },
        { "bold": true },
        { "text": "\n\n" },
    ]});
    normalize_inline(&mut content);
    let runs = content["inline"].as_array().unwrap().clone();
    assert_eq!(
        runs,
        [
            text("a\nb   "),
            json!({ "text": "cd", "bold": true }),
            json!({ "text": "x y", "code": true }),
            json!({ "text": "e", "link": "p(q) r" }),
            text("f"),
            json!({ "mention": ID, "shown": "ab" }),
            json!({ "mention": ID2 }),
        ]
    );
    let mut again = json!({ "inline": runs.clone() });
    normalize_inline(&mut again);
    assert_eq!(again["inline"].as_array().unwrap(), &runs);
    // Whitespace at a line edge inside a mark is dropped; mid-line it moves out of the mark.
    assert_eq!(
        normalized(&[
            json!({ "text": " a ", "bold": true }),
            text("\n"),
            json!({ "text": " b", "italic": true })
        ]),
        [
            json!({ "text": "a", "bold": true }),
            text("\n"),
            json!({ "text": "b", "italic": true })
        ]
    );
    assert_eq!(
        normalized(&[
            text("a"),
            json!({ "text": " b ", "bold": true, "link": "u" }),
            text("c")
        ]),
        [
            text("a"),
            json!({ "text": " ", "link": "u" }),
            json!({ "text": "b", "bold": true, "link": "u" }),
            json!({ "text": " ", "link": "u" }),
            text("c")
        ]
    );
    // A content with no runs, or no `inline`, is left alone.
    let mut other = json!({ "code": "x" });
    normalize_inline(&mut other);
    assert_eq!(other, json!({ "code": "x" }));
    assert_eq!(write(&[text("   \n  ")]), "");
}

#[test]
fn html_breaks_and_odd_inline_events_read_back() {
    assert_eq!(
        read("<u>x</u> <span>y</span> <b>z</b>"),
        [
            json!({ "text": "x", "underline": true }),
            text(" <span>y</span> "),
            json!({ "text": "z", "bold": true }),
        ]
    );
    assert_eq!(read("a  \nb\\\nc\nd"), [text("a\nb\nc\nd")]);
    assert_eq!(read("a\r\nb"), [text("a\nb")]);
    assert_eq!(read("[ ] x"), [text("[ ] x")]);
    assert_eq!(read("&amp; &#35;x"), [text("& #x")]);
    assert_eq!(write(&[text("& #x")]), "\\& #x");
}

/// The hostile alphabet of the fixpoint tests: every character the escaper cares about, letters,
/// spaces, a line break and a few non-ASCII characters.
fn hostile_text() -> impl Strategy<Value = String> {
    const ALPHABET: &[char] = &[
        '*', '_', '~', '`', '#', '^', '-', '+', '>', '|', '[', ']', '(', ')', '!', '<', '&', '\\',
        '\n', 'a', 'b', 'c', 'A', '1', '2', ' ', ' ', 'é', '—', '中',
    ];
    prop::collection::vec(prop::sample::select(ALPHABET), 0..40)
        .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn hostile_text_round_trips_after_normalisation(t in hostile_text()) {
        let runs = normalized(&[text(&t)]);
        let md = write(&runs);
        // `[a](b)` in text reads as a link: the one shape the format does not preserve (§4).
        if md.contains("](") {
            return Ok(());
        }
        if runs.is_empty() {
            prop_assert_eq!(md, "");
            return Ok(());
        }
        let (back, _, _) = read_with(&md, &NoResolver);
        prop_assert_eq!(&back, &runs, "through {:?}", md);
        prop_assert_eq!(write(&back), md);
    }

    #[test]
    fn hostile_marked_runs_round_trip(
        a in hostile_text(),
        b in hostile_text(),
        c in hostile_text(),
        marks in 0u8..32,
    ) {
        let mut run = json!({ "text": b });
        for (bit, key) in ["bold", "italic", "underline", "strike", "code"].into_iter().enumerate() {
            if marks & (1 << bit) != 0 {
                run[key] = json!(true);
            }
        }
        let runs = normalized(&[text(&a), run, text(&c)]);
        let md = write(&runs);
        if md.contains("](") || runs.is_empty() {
            return Ok(());
        }
        let (back, _, _) = read_with(&md, &NoResolver);
        prop_assert_eq!(&back, &runs, "through {:?}", md);
    }
}
