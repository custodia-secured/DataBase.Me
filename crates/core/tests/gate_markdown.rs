// SPDX-License-Identifier: AGPL-3.0-or-later
//! Gate 1b (M5.md, Task 1): what pulldown-cmark 0.13 does with the Markdown the files folder
//! writes and reads back. Each test is a probe. It asserts the behaviour the crate shows, not the
//! behaviour the plan assumed, so the file is an executable record of the finding, and it prints
//! the event stream with byte ranges for the report (`--nocapture`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "gate probes: a failure here is the finding"
)]

use std::ops::Range;
use std::time::Instant;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

/// The options the files folder would parse with (M5.md, "Frontmatter" and "Body").
fn options() -> Options {
    Options::ENABLE_WIKILINKS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
}

/// An event in short form (see `short`) with the byte range it came from.
type Traced = (String, Range<usize>);

/// `Debug` with the noise cut: `Text("a")` for `Text(Borrowed("a"))`, `Heading(H1)` without the
/// empty attribute fields, `Link(WikiLink { has_pothole: false }, "Title")` with its target only.
fn short(event: &Event<'_>) -> String {
    match event {
        Event::Start(Tag::Heading { level, .. }) => format!("Start(Heading({level:?}))"),
        Event::Start(
            tag @ (Tag::Link {
                link_type,
                dest_url,
                ..
            }
            | Tag::Image {
                link_type,
                dest_url,
                ..
            }),
        ) => {
            let kind = if matches!(tag, Tag::Link { .. }) {
                "Link"
            } else {
                "Image"
            };
            format!("Start({kind}({link_type:?}, {:?}))", &**dest_url)
        }
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
            format!("Start(Fenced({:?}))", &**info)
        }
        Event::Start(Tag::MetadataBlock(kind)) => format!("Start(Metadata({kind:?}))"),
        Event::End(TagEnd::MetadataBlock(kind)) => format!("End(Metadata({kind:?}))"),
        Event::Text(text) => format!("Text({:?})", &**text),
        Event::Code(text) => format!("Code({:?})", &**text),
        Event::InlineHtml(html) => format!("InlineHtml({:?})", &**html),
        other => format!("{other:?}"),
    }
}

/// Every event of `src` under `opts`, in order, with its byte range.
fn trace_with(src: &str, opts: Options) -> Vec<Traced> {
    Parser::new_ext(src, opts)
        .into_offset_iter()
        .map(|(event, range)| (short(&event), range))
        .collect()
}

fn trace(src: &str) -> Vec<Traced> {
    trace_with(src, options())
}

/// The whole stream as one space-joined string, so a probe asserts a sequence in one line.
fn seq(src: &str) -> String {
    join(&trace(src))
}

fn join(traced: &[Traced]) -> String {
    traced
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// How many events of `src` render as `name`.
fn count(src: &str, name: &str) -> usize {
    trace(src).iter().filter(|(n, _)| n == name).count()
}

/// The same stream as typed events, for probes that match on fields.
fn events(src: &str) -> Vec<(Event<'_>, Range<usize>)> {
    Parser::new_ext(src, options()).into_offset_iter().collect()
}

/// The source slice under the `n`th event.
fn slice<'a>(src: &'a str, traced: &[Traced], n: usize) -> &'a str {
    &src[traced[n].1.clone()]
}

/// Prints a stream with the source slice under each range, for the report.
fn dump(title: &str, src: &str, traced: &[Traced]) {
    println!("--- {title}");
    for (name, range) in traced {
        let text = src.get(range.clone()).unwrap_or("<not a char boundary>");
        println!("  {range:?} {name} {text:?}");
    }
}

/// Every `Tag::Link` or `Tag::Image` in `src` as (is image, link type, dest_url, the text inside
/// it, the source slice of the whole link). Asserts that `title` and `id` are empty on each.
fn links(src: &str) -> Vec<(bool, LinkType, String, String, String)> {
    let mut out = Vec::new();
    let mut open: Option<(bool, LinkType, String, Range<usize>)> = None;
    let mut inner = String::new();
    for (event, range) in events(src) {
        match &event {
            Event::Start(tag @ (Tag::Link { .. } | Tag::Image { .. })) => {
                let (Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }
                | Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) = tag
                else {
                    unreachable!()
                };
                assert_eq!((&**title, &**id), ("", ""), "{tag:?}");
                let image = matches!(tag, Tag::Image { .. });
                open = Some((image, *link_type, dest_url.to_string(), range));
            }
            Event::Text(text) if open.is_some() => inner.push_str(text),
            Event::End(TagEnd::Link | TagEnd::Image) => {
                let (image, link_type, dest, range) = open.take().unwrap();
                let whole = src[range].to_owned();
                out.push((image, link_type, dest, std::mem::take(&mut inner), whole));
            }
            _ => {}
        }
    }
    out
}

// 1. Frontmatter (Decision 4, 10b; FORMAT.md §6.2).

#[test]
fn frontmatter_is_a_metadata_block_with_one_text_event() {
    let yaml = "id: 01HXYZ\ntitle: \"A: b\"\ntags: [x, y]\n";
    let src = format!("---\n{yaml}---\n\nBody text\n");
    let traced = trace(&src);
    dump("frontmatter", &src, &traced);
    assert_eq!(
        join(&traced),
        concat!(
            r#"Start(Metadata(YamlStyle)) Text("id: 01HXYZ\ntitle: \"A: b\"\ntags: [x, y]\n") "#,
            r#"End(Metadata(YamlStyle)) Start(Paragraph) Text("Body text") End(Paragraph)"#
        )
    );
    // The block range runs from the opening fence to the end of the closing fence, without its
    // newline. The raw YAML is a single Text (the first pass appends one text item per line and
    // the tree merges adjacent text) whose range is exactly the bytes between the fences.
    assert_eq!(slice(&src, &traced, 0), format!("---\n{yaml}---"));
    assert_eq!(slice(&src, &traced, 1), yaml);
    assert_eq!(traced[2].1, traced[0].1, "End carries Start's range");
    assert_eq!(
        slice(&src, &traced, 3),
        "Body text\n",
        "a paragraph keeps its newline"
    );
}

#[test]
fn a_metadata_block_is_recognised_anywhere_not_only_at_the_top() {
    // A `---` divider after a blank line, followed by a non-blank line and any later `---` line,
    // opens a metadata block: the scan runs at every indent-0 block start, not only at offset 0.
    let src = "Para\n\n---\nMore\n\n---\n\nEnd\n";
    let traced = trace(src);
    dump("mid-document ---", src, &traced);
    assert_eq!(
        join(&traced),
        concat!(
            r#"Start(Paragraph) Text("Para") End(Paragraph) "#,
            r#"Start(Metadata(YamlStyle)) Text("More\n\n") End(Metadata(YamlStyle)) "#,
            r#"Start(Paragraph) Text("End") End(Paragraph)"#
        )
    );
    // The same document with `***` dividers has two Rules and no metadata block, and so does
    // the `---` version with a blank line after each divider, as CommonMark has it.
    assert_eq!(count(&src.replace("---", "***"), "Rule"), 2);
    assert_eq!(count("Para\n\n---\n\nMore\n\n---\n\nEnd\n", "Rule"), 2);
    // Without a closing line a leading `---` is a Rule, as before.
    assert_eq!(
        seq("---\nid: x\n"),
        r#"Rule Start(Paragraph) Text("id: x") End(Paragraph)"#
    );
    // A blank first line disqualifies the block, and the closing `---` then underlines the key
    // line as a setext h2: a frontmatter block with a leading blank line is silently lost.
    assert_eq!(
        seq("---\n\nid: x\n---\n"),
        r#"Rule Start(Heading(H2)) Text("id: x") End(Heading(H2))"#
    );
}

#[test]
fn crlf_text_events_split_at_the_carriage_return() {
    // Frontmatter saved with CRLF: the YAML comes as two Text events per line, the bytes before
    // the `\r` and the `\n` after it, so the YAML must be sliced from the block range, never
    // concatenated from Text events.
    let src = "---\r\nid: x\r\ntitle: t\r\n---\r\n\r\nline one\r\nline two\r\n";
    let traced = trace(src);
    dump("crlf", src, &traced);
    assert_eq!(
        join(&traced),
        concat!(
            r#"Start(Metadata(YamlStyle)) Text("id: x") Text("\ntitle: t") Text("\n") "#,
            r#"End(Metadata(YamlStyle)) Start(Paragraph) Text("line one") SoftBreak "#,
            r#"Text("line two") End(Paragraph)"#
        )
    );
    // The Text events straddle lines (a line's `\n` merges with the next line's text) and their
    // concatenation is the YAML with `\r` dropped; the block range keeps the raw bytes.
    let yaml: String = traced[1..4].iter().map(|(_, r)| &src[r.clone()]).collect();
    assert_eq!(yaml, "id: x\ntitle: t\n");
    assert_eq!(slice(src, &traced, 0), "---\r\nid: x\r\ntitle: t\r\n---");
    assert_eq!(slice(src, &traced, 5), "line one\r\nline two\r\n");
    assert_eq!(
        slice(src, &traced, 7),
        "\r\n",
        "the SoftBreak covers the CRLF"
    );
}

// 2. Wikilinks (Decision 10; FORMAT.md 368).

#[test]
fn wikilinks_keep_path_fragment_and_pothole() {
    let src = "[[Title]] [[path/Title]] [[path/Title|shown]] [[Title#^abc123]] [[Title#Heading]]";
    dump("wikilinks", src, &trace(src));
    // (has_pothole, dest_url, the Text inside, the source slice): dest_url is the raw text before
    // the pipe, fragment included; `shown` is the Text inside the Link; title and id are empty.
    let found = links(src);
    let seen: Vec<(bool, &str, &str, &str)> = found
        .iter()
        .map(|(image, link_type, dest, inner, whole)| {
            let LinkType::WikiLink { has_pothole } = link_type else {
                panic!("{link_type:?}")
            };
            assert!(!image);
            (*has_pothole, dest.as_str(), inner.as_str(), whole.as_str())
        })
        .collect();
    assert_eq!(
        seen,
        [
            (false, "Title", "Title", "[[Title]]"),
            (false, "path/Title", "path/Title", "[[path/Title]]"),
            (true, "path/Title", "shown", "[[path/Title|shown]]"),
            (false, "Title#^abc123", "Title#^abc123", "[[Title#^abc123]]"),
            (false, "Title#Heading", "Title#Heading", "[[Title#Heading]]"),
        ]
    );
    // The shown text's range is exactly the bytes after the pipe.
    let shown = events(src)
        .into_iter()
        .find(|(event, _)| matches!(event, Event::Text(t) if &**t == "shown"))
        .unwrap();
    assert_eq!(&src[shown.1], "shown");
}

// 3. Embeds (Decision 9): the gate's headline question.

#[test]
fn embeds_are_images_with_the_wikilink_type() {
    let src = "![[Title]] and ![[attachments/abc.png]] and ![[Title|alt]]";
    dump("embeds", src, &trace(src));
    // Every embed is a Tag::Image with LinkType::WikiLink; its range starts at the `!`, so no
    // stray "!" Text is left outside it, and the Text inside is the target (or the alt).
    for (image, _, _, _, whole) in links(src) {
        assert!(
            image && whole.starts_with("![[") && whole.ends_with("]]"),
            "{whole}"
        );
    }
    assert_eq!(
        seq(src),
        concat!(
            r#"Start(Paragraph) Start(Image(WikiLink { has_pothole: false }, "Title")) Text("Title") "#,
            r#"End(Image) Text(" and ") "#,
            r#"Start(Image(WikiLink { has_pothole: false }, "attachments/abc.png")) "#,
            r#"Text("attachments/abc.png") End(Image) Text(" and ") "#,
            r#"Start(Image(WikiLink { has_pothole: true }, "Title")) Text("alt") End(Image) "#,
            r#"End(Paragraph)"#
        )
    );
}

// 4. Anchors (Decision 8).

#[test]
fn trailing_anchors_stay_in_the_last_text_of_their_block() {
    assert_eq!(
        seq("Hello world ^01HXYZ\n"),
        r#"Start(Paragraph) Text("Hello world ^01HXYZ") End(Paragraph)"#
    );
    assert_eq!(
        seq("- item ^01HXYZ\n"),
        r#"Start(List(None)) Start(Item) Text("item ^01HXYZ") End(Item) End(List(false))"#
    );
    assert_eq!(
        seq("> quoted ^01HXYZ\n"),
        concat!(
            r#"Start(BlockQuote(None)) Start(Paragraph) Text("quoted ^01HXYZ") End(Paragraph) "#,
            r#"End(BlockQuote(None))"#
        )
    );
    // After inline marks the anchor is its own Text, still the last one in the block.
    assert_eq!(
        seq("see `code` ^01HXYZ\n"),
        r#"Start(Paragraph) Text("see ") Code("code") Text(" ^01HXYZ") End(Paragraph)"#
    );
    // A two-line paragraph: the anchor is the last Text after a SoftBreak, inside the block.
    assert_eq!(
        seq("line one\nline two ^01HXYZ\n"),
        r#"Start(Paragraph) Text("line one") SoftBreak Text("line two ^01HXYZ") End(Paragraph)"#
    );
}

#[test]
fn own_line_anchors_become_paragraphs_except_after_tables_lists_and_quotes() {
    let anchor = r#"Start(Paragraph) Text("^01HXYZ") End(Paragraph)"#;
    assert_eq!(
        seq("# Head\n^01HXYZ\n"),
        format!(r#"Start(Heading(H1)) Text("Head") End(Heading(H1)) {anchor}"#)
    );
    assert_eq!(
        seq("```\ncode\n```\n^01HXYZ\n"),
        format!(r#"Start(Fenced("")) Text("code\n") End(CodeBlock) {anchor}"#)
    );
    assert_eq!(seq("***\n^01HXYZ\n"), format!("Rule {anchor}"));
    // Right after a table the anchor line is swallowed as a table row: one cell holding the
    // anchor and an empty second cell. A blank line between them gives it its own paragraph.
    let table_src = "| a | b |\n|---|---|\n| 1 | 2 |\n^01HXYZ\n";
    let table = trace(table_src);
    dump("table + anchor", table_src, &table);
    assert_eq!(
        join(&table[table.len() - 8..]),
        concat!(
            r#"Start(TableRow) Start(TableCell) Text("^01HXYZ") End(TableCell) "#,
            r#"Start(TableCell) End(TableCell) End(TableRow) End(Table)"#
        )
    );
    assert_eq!(slice(table_src, &table, table.len() - 8), "^01HXYZ\n");
    assert!(seq("| a | b |\n|---|---|\n| 1 | 2 |\n\n^01HXYZ\n").ends_with(anchor));
    // Right after a list item or a quote line it is a lazy continuation of that paragraph.
    assert_eq!(
        seq("- item\n^01HXYZ\n"),
        concat!(
            r#"Start(List(None)) Start(Item) Text("item") SoftBreak Text("^01HXYZ") End(Item) "#,
            r#"End(List(false))"#
        )
    );
    assert_eq!(
        seq("> quoted\n^01HXYZ\n"),
        concat!(
            r#"Start(BlockQuote(None)) Start(Paragraph) Text("quoted") SoftBreak Text("^01HXYZ") "#,
            r#"End(Paragraph) End(BlockQuote(None))"#
        )
    );
}

// 5. Block kinds (Decision 11b and the body rules).

#[test]
fn nested_lists_indent_two_spaces_and_lazy_lines_continue_the_item() {
    let src = "- a\n  - b\n    - c\n- d\n  still d\n";
    let traced = trace(src);
    dump("nested lists", src, &traced);
    assert_eq!(
        join(&traced),
        concat!(
            r#"Start(List(None)) Start(Item) Text("a") Start(List(None)) Start(Item) Text("b") "#,
            r#"Start(List(None)) Start(Item) Text("c") End(Item) End(List(false)) End(Item) "#,
            r#"End(List(false)) End(Item) Start(Item) Text("d") SoftBreak Text("still d") "#,
            r#"End(Item) End(List(false))"#
        )
    );
    // Tight lists emit no Paragraph; an Item range starts at its marker and covers its nested list.
    assert_eq!(slice(src, &traced, 1), "- a\n  - b\n    - c\n");
    assert_eq!(slice(src, &traced, 4), "- b\n    - c\n");
    assert_eq!(
        seq("- [ ] x\n- [x] y\n"),
        concat!(
            r#"Start(List(None)) Start(Item) TaskListMarker(false) Text("x") End(Item) "#,
            r#"Start(Item) TaskListMarker(true) Text("y") End(Item) End(List(false))"#
        )
    );
    assert_eq!(
        seq("3. one\n4. two\n"),
        concat!(
            r#"Start(List(Some(3))) Start(Item) Text("one") End(Item) Start(Item) Text("two") "#,
            r#"End(Item) End(List(true))"#
        )
    );
}

#[test]
fn quotes_fences_rules_headings_and_tables_parse_as_expected() {
    assert_eq!(
        seq("> p1\n>\n> p2\n"),
        concat!(
            r#"Start(BlockQuote(None)) Start(Paragraph) Text("p1") End(Paragraph) "#,
            r#"Start(Paragraph) Text("p2") End(Paragraph) End(BlockQuote(None))"#
        )
    );
    // A four-backtick fence carries `---` and a three-backtick line as code, in one Text.
    let fence_src = "````dbme\n---\n```\nkind: chart\n````\n";
    let fence = trace(fence_src);
    dump("dbme fence", fence_src, &fence);
    assert_eq!(
        join(&fence),
        r#"Start(Fenced("dbme")) Text("---\n```\nkind: chart\n") End(CodeBlock)"#
    );
    // The fence range ends at the closing fence, without its newline (paragraphs include theirs).
    assert_eq!(slice(fence_src, &fence, 0), fence_src.trim_end());
    assert_eq!(slice(fence_src, &fence, 1), "---\n```\nkind: chart\n");
    // `***` after a paragraph is a Rule; `---` after a paragraph is a setext h2.
    assert_eq!(
        seq("para\n***\n"),
        r#"Start(Paragraph) Text("para") End(Paragraph) Rule"#
    );
    assert_eq!(
        seq("para\n---\n"),
        r#"Start(Heading(H2)) Text("para") End(Heading(H2))"#
    );
    let headings = seq("# 1\n## 2\n### 3\n#### 4\n##### 5\n###### 6\n####### 7\n");
    for level in 1..=6 {
        assert!(headings.contains(&format!("Start(Heading(H{level})) Text(\"{level}\")")));
    }
    assert!(headings.ends_with("Start(Paragraph) Text(\"####### 7\") End(Paragraph)"));
    assert_eq!(
        seq("| a | b |\n|---|:-:|\n| 1 | 2 |\n"),
        concat!(
            r#"Start(Table([None, Center])) Start(TableHead) Start(TableCell) Text("a") "#,
            r#"End(TableCell) Start(TableCell) Text("b") End(TableCell) End(TableHead) "#,
            r#"Start(TableRow) Start(TableCell) Text("1") End(TableCell) Start(TableCell) "#,
            r#"Text("2") End(TableCell) End(TableRow) End(Table)"#
        )
    );
}

// 6. Tags (Decision 5b): the parser gives nothing; our own scanner is needed.

#[test]
fn hash_tags_are_plain_text() {
    assert_eq!(
        seq("Hello #tag and #tag/sub and #1 and a#b\n"),
        r#"Start(Paragraph) Text("Hello #tag and #tag/sub and #1 and a#b") End(Paragraph)"#
    );
    // At line start: `#tag` (no space) is a paragraph, `# tag` is a heading.
    assert_eq!(
        seq("#tag\n"),
        "Start(Paragraph) Text(\"#tag\") End(Paragraph)"
    );
    assert_eq!(seq("#1\n"), "Start(Paragraph) Text(\"#1\") End(Paragraph)");
    assert_eq!(
        seq("# tag\n"),
        r#"Start(Heading(H1)) Text("tag") End(Heading(H1))"#
    );
}

// 7. Inline marks.

#[test]
fn inline_marks_strike_needs_its_option_and_u_is_inline_html() {
    let src = "**bold** *em* `code` [text](url) ~~strike~~ <u>x</u>\none\\\ntwo  \nthree\nfour\n";
    let traced = trace(src);
    dump("inline marks", src, &traced);
    assert_eq!(
        join(&traced),
        concat!(
            r#"Start(Paragraph) Start(Strong) Text("bold") End(Strong) Text(" ") "#,
            r#"Start(Emphasis) Text("em") End(Emphasis) Text(" ") Code("code") Text(" ") "#,
            r#"Start(Link(Inline, "url")) Text("text") End(Link) Text(" ~~strike~~ ") "#,
            r#"InlineHtml("<u>") Text("x") InlineHtml("</u>") SoftBreak Text("one") HardBreak "#,
            r#"Text("two") HardBreak Text("three") SoftBreak Text("four") End(Paragraph)"#
        )
    );
    // The hard break ranges cover the backslash or the two spaces, plus the newline.
    let hard: Vec<&str> = traced
        .iter()
        .filter(|(name, _)| name == "HardBreak")
        .map(|(_, range)| &src[range.clone()])
        .collect();
    assert_eq!(hard, ["\\\n", "  \n"]);
    let strike = trace_with("~~strike~~", options() | Options::ENABLE_STRIKETHROUGH);
    assert_eq!(
        join(&strike),
        r#"Start(Paragraph) Start(Strikethrough) Text("strike") End(Strikethrough) End(Paragraph)"#
    );
}

// 8. Hostile input: no panic, bounded time.

#[test]
fn hostile_input_parses_without_panic_in_bounded_time() {
    let cases: [(&str, String); 3] = [
        ("1 MB of [[", "[[".repeat(512 * 1024)),
        ("10,000 nested >", format!("{}x\n", "> ".repeat(10_000))),
        ("100 kB line", "a ".repeat(50 * 1024)),
    ];
    for (title, src) in &cases {
        let start = Instant::now();
        let count = Parser::new_ext(src, options()).into_offset_iter().count();
        let elapsed = start.elapsed();
        println!("{title}: {count} events in {} ms", elapsed.as_millis());
        assert!(elapsed.as_secs_f64() < 2.0, "{title}: {elapsed:?}");
        assert!(count > 0);
    }
    // The 1 MB of openers is one paragraph of Text events: not a single link, and one Text per
    // `[` (about a million events), so a hostile file costs events, not time.
    let traced = trace(&cases[0].1);
    assert!(
        !traced.iter().any(|(n, _)| n.starts_with("Start(Link")),
        "a link was made"
    );
    assert_eq!(traced[0].0, "Start(Paragraph)");
    assert_eq!(traced.len(), 2 * 512 * 1024 + 2);
    // The 10,000 quotes nest 10,000 BlockQuote tags around one paragraph.
    assert_eq!(count(&cases[1].1, "Start(BlockQuote(None))"), 10_000);
}

// 9. Ranges nest: a Text never falls outside its enclosing block's range.

const FIXTURE: &str = "---\nid: 01HXYZ\n---\n\n# Head\n^01A\n\nPara with [[Link|shown]], \
![[a.png]] and `code` ^01B\n\n- a ^01C\n  - [x] b ^01D\n    - c ^01E\n\n3. three ^01F\n\n\
> quoted\n> more ^01G\n\n````dbme\n---\n```\n````\n^01H\n\n***\n^01I\n\n| a | b |\n|---|---|\n\
| **1** | 2 |\n\n^01J\n\n#tag text<u>x</u>\\\nend\n";

#[test]
fn every_event_range_lies_inside_its_enclosing_block() {
    let traced = trace(FIXTURE);
    dump("fixture", FIXTURE, &traced);
    let mut stack: Vec<(String, Range<usize>)> = Vec::new();
    let mut sibling_end: Vec<usize> = vec![0];
    for (name, range) in &traced {
        assert!(
            range.start <= range.end && range.end <= FIXTURE.len(),
            "{name}"
        );
        if let Some((parent, outer)) = stack.last() {
            assert!(
                outer.start <= range.start && range.end <= outer.end,
                "{name} {range:?} outside {parent} {outer:?}"
            );
        }
        if name.starts_with("End(") {
            let (opened, open_range) = stack.pop().unwrap();
            assert_eq!(&open_range, range, "{opened} / {name}");
            sibling_end.pop();
            *sibling_end.last_mut().unwrap() = range.end;
        } else {
            // Siblings come in source order and never overlap.
            assert!(
                *sibling_end.last().unwrap() <= range.start,
                "{name} overlaps"
            );
            if name.starts_with("Start(") {
                stack.push((name.clone(), range.clone()));
                sibling_end.push(range.start);
            } else {
                *sibling_end.last_mut().unwrap() = range.end;
            }
        }
    }
    assert!(stack.is_empty(), "unbalanced: {stack:?}");
    // With LF endings every Text's range holds exactly the Text's bytes, so block text can be
    // sliced from ranges (CRLF splits a Text at the `\r`: see the crlf probe).
    for (event, range) in events(FIXTURE) {
        if let Event::Text(text) = event {
            assert_eq!(&FIXTURE[range.clone()], &*text);
        }
    }
    // The two "fenced" kinds the format relies on are told apart by their tags.
    assert_eq!(count(FIXTURE, "Start(Metadata(YamlStyle))"), 1);
    assert_eq!(count(FIXTURE, r#"Start(Fenced("dbme"))"#), 1);
    assert_eq!(slice(FIXTURE, &traced, 0), "---\nid: 01HXYZ\n---");
}
