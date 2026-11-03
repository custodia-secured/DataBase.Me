// SPDX-License-Identifier: AGPL-3.0-or-later
//! Blocks → Markdown: every kind, anchors, lists by marker width, children, fences and the
//! `dbme` fence for unknown kinds (spec §4, "Writing").
//!
//! Every block renders to a list of lines (never a trailing newline) through [`block_lines`];
//! [`join_siblings`] puts one blank line between siblings except inside a tight run of list
//! items of one list, and [`write_blocks`] ends the body with exactly one `\n`. Anchors sit at
//! the end of the last line for paragraphs, list items and images, and on a line of their own
//! (after a blank line) for every other kind, so that the reader's lone-anchor rule finds them.

use serde_json::{Map, Value};

use super::super::{FileBlock, LinkResolver};
use super::{anchor_on_own_line, inline, marker_width};
use crate::{ContentHash, Id};

/// A kind whose siblings of the same list family stay on adjacent lines: `Bullet` is shared by
/// `bullet` and `todo`, `Numbered` is `numbered` alone, `None` is every other kind.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListFamily {
    None,
    Bullet,
    Numbered,
}

fn family(kind: &str) -> ListFamily {
    match kind {
        "bullet" | "todo" => ListFamily::Bullet,
        "numbered" => ListFamily::Numbered,
        _ => ListFamily::None,
    }
}

/// Writes blocks as Markdown: one blank line between blocks (none between consecutive items of
/// one list), the body ending with exactly one `\n`, empty when there are no blocks.
pub(super) fn write_blocks(blocks: &[FileBlock], resolver: &dyn LinkResolver) -> String {
    let lines = join_siblings(blocks, resolver);
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Renders one block's text as the merge compares it: no anchor, no children, LF, no trailing
/// newline (ADR-0025). A numbered item is numbered `1.`.
pub(super) fn render_block_text(
    kind: &str,
    content: &Value,
    resolver: &dyn LinkResolver,
) -> String {
    let block = FileBlock {
        id: None,
        kind: kind.to_owned(),
        content: content.clone(),
        children: Vec::new(),
    };
    body_lines(&block, 1, resolver).join("\n")
}

/// The lines of a run of siblings: adjacent for list items of one family, otherwise separated by
/// one blank line. Numbered items count from 1 within their run. A block that renders to no
/// line at all (an empty paragraph without an id) leaves no trace.
fn join_siblings(blocks: &[FileBlock], resolver: &dyn LinkResolver) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut previous = ListFamily::None;
    let mut number = 0usize;
    for block in blocks {
        let current = family(&block.kind);
        number = if current == ListFamily::Numbered {
            number + 1
        } else {
            0
        };
        let lines = block_lines(block, number, resolver);
        if lines.is_empty() {
            continue;
        }
        if !out.is_empty() && (current == ListFamily::None || current != previous) {
            out.push(String::new());
        }
        out.extend(lines);
        previous = current;
    }
    out
}

/// The lines of one block with its anchor and its children. `number` is the item's ordinal
/// when the kind is `numbered`.
fn block_lines(block: &FileBlock, number: usize, resolver: &dyn LinkResolver) -> Vec<String> {
    let kind = block.kind.as_str();
    let mut lines = body_lines(block, number, resolver);
    if let Some(id) = block.id {
        if anchor_on_own_line(kind) {
            lines.push(String::new());
            lines.push(format!("^{id}"));
        } else {
            append_anchor(&mut lines, id);
        }
    }
    // An unknown kind carries its children inside its fence (a fence cannot nest).
    if !known(kind) || block.children.is_empty() || lines.is_empty() {
        return lines;
    }
    let children = join_siblings(&block.children, resolver);
    if children.is_empty() {
        return lines;
    }
    // A nested list item follows the item line directly; any other child is a continuation
    // paragraph after a blank line. A non-list parent has no marker, so its children follow it
    // as blocks of their own.
    let indent = " ".repeat(marker_width(kind, number));
    let first_is_item = block
        .children
        .first()
        .is_some_and(|c| is_list_item(&c.kind));
    if !(is_list_item(kind) && first_is_item) {
        lines.push(String::new());
    }
    for line in children {
        if line.is_empty() {
            lines.push(line);
        } else {
            lines.push(format!("{indent}{line}"));
        }
    }
    lines
}

/// Appends ` ^<id>` to the last line, or makes the anchor the only line of an empty block.
fn append_anchor(lines: &mut Vec<String>, id: Id) {
    match lines.last_mut() {
        Some(last) if !last.is_empty() => {
            last.push_str(" ^");
            last.push_str(&id.to_string());
        }
        Some(last) => *last = format!("^{id}"),
        None => lines.push(format!("^{id}")),
    }
}

/// The lines of a block's own Markdown: no anchor, no children.
fn body_lines(block: &FileBlock, number: usize, resolver: &dyn LinkResolver) -> Vec<String> {
    let content = &block.content;
    match block.kind.as_str() {
        "paragraph" => {
            let text = inline_text(content, resolver, "", true);
            if text.is_empty() {
                Vec::new()
            } else {
                split_lines(&text)
            }
        }
        "heading" => vec![heading_line(content, resolver)],
        "bullet" => item_lines("- ", 2, content, resolver),
        "numbered" => {
            let marker = format!("{}. ", number.max(1));
            item_lines(&marker, marker.len(), content, resolver)
        }
        "todo" => {
            let checked = content.get("checked").and_then(Value::as_bool) == Some(true);
            let marker = if checked { "- [x] " } else { "- [ ] " };
            item_lines(marker, 2, content, resolver)
        }
        "quote" => inline_text(content, resolver, "> ", true)
            .split('\n')
            .map(|line| {
                if line.is_empty() {
                    ">".to_owned()
                } else {
                    format!("> {line}")
                }
            })
            .collect(),
        "code" => code_lines(content),
        "divider" => vec!["***".to_owned()],
        "image" => match super::read::demote_image(content) {
            // A blob that is no content hash cannot be an embed's name (it would sit outside
            // every escape): it is written as the paragraph its normal form is.
            Some(paragraph) => split_lines(&inline_text(&paragraph, resolver, "", true)),
            None => vec![image_line(content, resolver)],
        },
        _ => dbme_lines(block),
    }
}

/// The inline runs of `content` written after `prefix` (the flanking context of the first run;
/// the prefix itself is not part of the result).
fn inline_text(
    content: &Value,
    resolver: &dyn LinkResolver,
    prefix: &str,
    at_line_start: bool,
) -> String {
    let mut out = prefix.to_owned();
    if let Some(runs) = content.get("inline").and_then(Value::as_array) {
        inline::write_inline(runs, resolver, &mut out, at_line_start);
    }
    out.split_off(prefix.len())
}

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_owned).collect()
}

/// A list item's lines: the marker and the text, the text's further lines indented by `width`
/// (the marker's width, so they sit under the content). The marker's trailing space is dropped
/// when the text is empty (`-`, `1.`, `- [ ]`). A text that starts with a task box (`[ ]`,
/// `[x]`, `[X]` before whitespace or the end) would turn the item into a todo: its `[` is
/// escaped.
fn item_lines(
    marker: &str,
    width: usize,
    content: &Value,
    resolver: &dyn LinkResolver,
) -> Vec<String> {
    let text = inline_text(content, resolver, marker, true);
    if text.is_empty() {
        return vec![marker.trim_end().to_owned()];
    }
    let indent = " ".repeat(width);
    let mut lines = split_lines(&text);
    for line in lines.iter_mut().skip(1) {
        if !line.is_empty() {
            line.insert_str(0, &indent);
        }
    }
    let boxed = text.len() >= 3
        && text.as_bytes()[0] == b'['
        && matches!(text.as_bytes()[1], b' ' | b'x' | b'X')
        && text.as_bytes()[2] == b']'
        && text[3..].chars().next().is_none_or(char::is_whitespace);
    lines[0] = if boxed {
        format!("{marker}\\{}", lines[0])
    } else {
        format!("{marker}{}", lines[0])
    };
    lines
}

/// `#{level} text` on one line: newlines in the runs become spaces first, and a trailing run of
/// `#`s after a space (or alone) would be read as a closing sequence, so its first `#` is
/// escaped. An empty heading is the markers alone.
fn heading_line(content: &Value, resolver: &dyn LinkResolver) -> String {
    let level = content
        .get("level")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, 6);
    let mut marks = "#".repeat(usize::try_from(level).unwrap_or(1));
    let mut one_line = content.clone();
    if let Some(runs) = one_line.get_mut("inline").and_then(Value::as_array_mut) {
        for run in runs.iter_mut() {
            if let Some(Value::String(text)) = run.get_mut("text") {
                *text = text.replace("\r\n", " ").replace(['\n', '\r'], " ");
            }
        }
    }
    marks.push(' ');
    let text = inline_text(&one_line, resolver, &marks, false);
    if text.is_empty() {
        marks.pop();
        return marks;
    }
    let trimmed = text.trim_end_matches('#');
    let closing = trimmed.len() < text.len()
        && trimmed
            .chars()
            .next_back()
            .is_none_or(|c| c == ' ' || c == '\t');
    if closing {
        format!("{marks}{trimmed}\\{}", &text[trimmed.len()..])
    } else {
        format!("{marks}{text}")
    }
}

/// A fence of `max(3, longest backtick run in text + 1)` backticks.
fn fence_for(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat((longest + 1).max(3))
}

/// The fenced code block, written in the code's normal form so that `parse(serialize(d))` is
/// `normalize(d)`: LF only, no trailing newline (the fence adds exactly the one the reader
/// strips), and the language's first word (dropped when it holds a backtick, which an info
/// string cannot carry).
fn code_lines(content: &Value) -> Vec<String> {
    let raw = content.get("code").and_then(Value::as_str).unwrap_or("");
    let code = raw.replace("\r\n", "\n").replace('\r', "\n");
    let code = code.trim_end_matches('\n');
    let language = content
        .get("language")
        .and_then(Value::as_str)
        .and_then(|l| l.split_whitespace().next())
        .filter(|l| !l.contains('`'))
        .unwrap_or("");
    let fence = fence_for(code);
    let mut lines = vec![format!("{fence}{language}")];
    if !code.is_empty() {
        lines.extend(split_lines(code));
    }
    lines.push(fence);
    lines
}

/// `![[attachments/<name>]]`, with `|alt` when the alt text is non-empty once the characters
/// that would end the embed (`]`, `|`, newlines) are dropped; the alt is escaped like a mention's
/// `shown` (a trailing `\` would otherwise swallow the `]]`). A blob that is no content hash
/// never reaches here (see `body_lines`); should it, the name is filtered like the alt, so it can
/// end neither the embed nor the line.
fn image_line(content: &Value, resolver: &dyn LinkResolver) -> String {
    let blob = content.get("blob").and_then(Value::as_str).unwrap_or("");
    let name = ContentHash::parse(blob).map_or_else(
        |_| {
            blob.chars()
                .filter(|c| !matches!(c, ']' | '|' | '\n' | '\r'))
                .collect()
        },
        |h| resolver.attachment_name(&h),
    );
    let alt: String = content
        .get("alt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .filter(|c| !matches!(c, ']' | '|' | '\n' | '\r'))
        .collect();
    let mut out = format!("![[attachments/{name}");
    if !alt.is_empty() {
        out.push('|');
        inline::write_shown(&alt, &mut out);
    }
    out.push_str("]]");
    out
}

/// An unknown kind as a `dbme` fence holding one JSON line
/// `{"children": [...], "content": {...}, "kind": "..."}` (keys in serde_json's order), the
/// children recursively as `{"children", "content", "id", "kind"}` objects. The block is written
/// in normal form, as the reader normalises what it reads from the fence: a fence carries no
/// Markdown that could normalise the children on the way back, so the writer does it here.
fn dbme_lines(block: &FileBlock) -> Vec<String> {
    let mut normal = vec![block.clone()];
    super::normalize(&mut normal);
    let Some(block) = normal.first() else {
        return Vec::new();
    };
    let mut object = Map::new();
    object.insert("kind".to_owned(), Value::String(block.kind.clone()));
    object.insert("content".to_owned(), block.content.clone());
    object.insert(
        "children".to_owned(),
        Value::Array(block.children.iter().map(child_json).collect()),
    );
    let json = Value::Object(object).to_string();
    let fence = fence_for(&json);
    vec![format!("{fence}dbme"), json, fence]
}

fn child_json(block: &FileBlock) -> Value {
    let mut object = Map::new();
    if let Some(id) = block.id {
        object.insert("id".to_owned(), Value::String(id.to_string()));
    }
    object.insert("kind".to_owned(), Value::String(block.kind.clone()));
    object.insert("content".to_owned(), block.content.clone());
    object.insert(
        "children".to_owned(),
        Value::Array(block.children.iter().map(child_json).collect()),
    );
    Value::Object(object)
}

fn is_list_item(kind: &str) -> bool {
    matches!(kind, "bullet" | "numbered" | "todo")
}

/// The kinds with a Markdown shape of their own; every other kind goes into a `dbme` fence.
fn known(kind: &str) -> bool {
    matches!(
        kind,
        "paragraph"
            | "heading"
            | "bullet"
            | "numbered"
            | "todo"
            | "quote"
            | "code"
            | "divider"
            | "image"
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests: a failure is the finding")]

    use serde_json::json;

    use super::super::super::NoResolver;
    use super::*;

    const ID: &str = "01JB3R7Q8Y5K3M2N4P6Q8R0S2T";
    const ID2: &str = "01JB3R9C0D4E5F6G7H8J9K0M1N";
    const ID3: &str = "01JB3RA1B2C3D4E5F6G7H8J9K0";

    /// A vault with two objects: `Projects/Alpha` (`ID`) and `People/Alex Doe` (`ID2`).
    struct Vault;

    impl LinkResolver for Vault {
        fn resolve(&self, target: &str) -> Option<Id> {
            match target {
                "Projects/Alpha" | "Alpha" => Some(id(ID)),
                "People/Alex Doe" | "Alex Doe" => Some(id(ID2)),
                _ => None,
            }
        }

        fn path_of(&self, object: Id) -> Option<String> {
            if object == id(ID) {
                Some("Projects/Alpha".to_owned())
            } else if object == id(ID2) {
                Some("People/Alex Doe".to_owned())
            } else {
                None
            }
        }

        fn attachment_name(&self, blob: &ContentHash) -> String {
            format!("{}.png", blob.hex())
        }
    }

    fn id(text: &str) -> Id {
        Id::parse(text).unwrap()
    }

    fn block(id_text: Option<&str>, kind: &str, content: Value) -> FileBlock {
        FileBlock {
            id: id_text.map(id),
            kind: kind.to_owned(),
            content,
            children: Vec::new(),
        }
    }

    fn with_children(mut block: FileBlock, children: Vec<FileBlock>) -> FileBlock {
        block.children = children;
        block
    }

    fn text(kind: &str, s: &str) -> FileBlock {
        block(Some(ID), kind, json!({"inline": [{"text": s}]}))
    }

    fn write(blocks: &[FileBlock]) -> String {
        write_blocks(blocks, &Vault)
    }

    #[test]
    fn empty_body_and_paragraph_anchors() {
        assert_eq!(write(&[]), "");
        assert_eq!(
            write(&[text("paragraph", "Hello")]),
            format!("Hello ^{ID}\n")
        );
        assert_eq!(
            write(&[block(
                None,
                "paragraph",
                json!({"inline": [{"text": "Hello"}]})
            )]),
            "Hello\n"
        );
        assert_eq!(
            write(&[text("paragraph", "one\ntwo")]),
            format!("one\ntwo ^{ID}\n")
        );
        assert_eq!(
            write(&[block(Some(ID), "paragraph", json!({"inline": []}))]),
            format!("^{ID}\n")
        );
        assert_eq!(
            write(&[block(None, "paragraph", json!({"inline": []}))]),
            ""
        );
        assert_eq!(
            write(&[
                text("paragraph", "a"),
                block(Some(ID2), "paragraph", json!({"inline": []}))
            ]),
            format!("a ^{ID}\n\n^{ID2}\n")
        );
    }

    #[test]
    fn paragraph_text_is_escaped_through_the_inline_layer() {
        assert_eq!(
            write(&[text(
                "paragraph",
                "- not a list\n# not a heading\n^not an anchor"
            )]),
            format!("\\- not a list\n\\# not a heading\n\\^not an anchor ^{ID}\n")
        );
        assert_eq!(
            write(&[block(
                None,
                "paragraph",
                json!({"inline": [{"text": "see "}, {"mention": ID2}, {"text": " and "}, {"mention": ID3}]})
            )]),
            format!("see [[People/Alex Doe]] and [[{ID3}]]\n")
        );
    }

    #[test]
    fn headings_are_one_line_with_the_anchor_below() {
        let heading = block(
            Some(ID),
            "heading",
            json!({"level": 2, "inline": [{"text": "Two\nlines"}]}),
        );
        assert_eq!(
            write(&[heading, text("paragraph", "after")]),
            format!("## Two lines\n\n^{ID}\n\nafter ^{ID}\n")
        );
        assert_eq!(
            write(&[block(
                None,
                "heading",
                json!({"level": 1, "inline": [{"text": "Topic #"}]})
            )]),
            "# Topic \\#\n"
        );
        assert_eq!(
            write(&[block(
                None,
                "heading",
                json!({"level": 3, "inline": [{"text": "###"}]})
            )]),
            "### \\###\n"
        );
        assert_eq!(
            write(&[block(
                None,
                "heading",
                json!({"level": 1, "inline": [{"text": "a#b#"}]})
            )]),
            "# a#b#\n"
        );
        assert_eq!(
            write(&[block(
                Some(ID),
                "heading",
                json!({"level": 9, "inline": []})
            )]),
            format!("######\n\n^{ID}\n")
        );
    }

    #[test]
    fn list_items_form_tight_runs_by_family() {
        let todo = |checked: bool, s: &str| {
            block(
                Some(ID2),
                "todo",
                json!({"checked": checked, "inline": [{"text": s}]}),
            )
        };
        assert_eq!(
            write(&[text("bullet", "a"), todo(false, "b"), todo(true, "c")]),
            format!("- a ^{ID}\n- [ ] b ^{ID2}\n- [x] c ^{ID2}\n")
        );
        assert_eq!(
            write(&[
                text("numbered", "a"),
                text("numbered", "b"),
                text("bullet", "c"),
                text("numbered", "d"),
            ]),
            format!("1. a ^{ID}\n2. b ^{ID}\n\n- c ^{ID}\n\n1. d ^{ID}\n")
        );
        assert_eq!(
            write(&[
                text("numbered", "a"),
                text("paragraph", "p"),
                text("numbered", "b")
            ]),
            format!("1. a ^{ID}\n\np ^{ID}\n\n1. b ^{ID}\n")
        );
        assert_eq!(
            write(&[
                block(Some(ID), "bullet", json!({"inline": []})),
                block(None, "bullet", json!({"inline": []})),
                block(Some(ID2), "todo", json!({"checked": false, "inline": []})),
                block(None, "numbered", json!({"inline": []})),
            ]),
            format!("- ^{ID}\n-\n- [ ] ^{ID2}\n\n1.\n")
        );
    }

    #[test]
    fn a_task_box_at_the_start_of_an_item_is_escaped() {
        assert_eq!(
            write(&[block(
                None,
                "bullet",
                json!({"inline": [{"text": "[ ] not a todo"}]})
            )]),
            "- \\[ ] not a todo\n"
        );
        assert_eq!(
            write(&[block(
                None,
                "numbered",
                json!({"inline": [{"text": "[X]"}]})
            )]),
            "1. \\[X]\n"
        );
        assert_eq!(
            write(&[block(None, "bullet", json!({"inline": [{"text": "[x]y"}]}))]),
            "- [x]y\n"
        );
        assert_eq!(
            write(&[block(
                None,
                "bullet",
                json!({"inline": [{"text": "[ok] fine"}]})
            )]),
            "- [ok] fine\n"
        );
    }

    #[test]
    fn children_indent_by_the_marker_width() {
        let nested = with_children(
            text("bullet", "a"),
            vec![with_children(text("bullet", "b"), vec![text("todo", "c")])],
        );
        assert_eq!(
            write(&[nested]),
            format!("- a ^{ID}\n  - b ^{ID}\n    - [ ] c ^{ID}\n")
        );
        let wrapped = with_children(text("numbered", "one\ntwo"), vec![text("todo", "x\ny")]);
        assert_eq!(
            write(&[wrapped]),
            format!("1. one\n   two ^{ID}\n   - [ ] x\n     y ^{ID}\n")
        );
        let mut items: Vec<FileBlock> = (0..10).map(|_| text("numbered", "n")).collect();
        items.push(with_children(
            text("numbered", "n"),
            vec![text("bullet", "deep")],
        ));
        items[0] = with_children(text("numbered", "n"), vec![text("bullet", "deep")]);
        let out = write(&items);
        assert!(out.starts_with(&format!("1. n ^{ID}\n   - deep ^{ID}\n2. n ^{ID}\n")));
        assert!(out.ends_with(&format!("11. n ^{ID}\n    - deep ^{ID}\n")));
    }

    #[test]
    fn a_non_list_child_is_a_continuation_after_a_blank_line() {
        let item = with_children(
            text("bullet", "a"),
            vec![
                text("paragraph", "para"),
                block(
                    Some(ID2),
                    "code",
                    json!({"code": "x\n\ny", "language": "rs"}),
                ),
                text("bullet", "b"),
            ],
        );
        assert_eq!(
            write(&[item, text("bullet", "next")]),
            format!(
                "- a ^{ID}\n\n  para ^{ID}\n\n  ```rs\n  x\n\n  y\n  ```\n\n  ^{ID2}\n\n  - b ^{ID}\n- next ^{ID}\n"
            )
        );
        // A paragraph is no list item: its children follow it as blocks of their own.
        let odd = with_children(text("paragraph", "p"), vec![text("bullet", "c")]);
        assert_eq!(write(&[odd]), format!("p ^{ID}\n\n- c ^{ID}\n"));
    }

    #[test]
    fn quotes_prefix_every_line() {
        assert_eq!(
            write(&[text("quote", "one\ntwo")]),
            format!("> one\n> two\n\n^{ID}\n")
        );
        assert_eq!(
            write(&[block(
                None,
                "quote",
                json!({"inline": [{"text": "a\n"}, {"text": "b", "code": true}, {"text": "\n\nc"}]})
            )]),
            "> a\n> `b`\n> c\n"
        );
        assert_eq!(
            write(&[block(Some(ID), "quote", json!({"inline": []}))]),
            format!(">\n\n^{ID}\n")
        );
    }

    #[test]
    fn code_fences_grow_past_the_longest_backtick_run() {
        let code = |code: &str, language: Value| {
            block(
                Some(ID),
                "code",
                json!({"code": code, "language": language}),
            )
        };
        assert_eq!(
            write(&[code("fn main() {}", json!("rust"))]),
            format!("```rust\nfn main() {{}}\n```\n\n^{ID}\n")
        );
        assert_eq!(
            write(&[code("a ``` b\n---", json!(" md "))]),
            format!("````md\na ``` b\n---\n````\n\n^{ID}\n")
        );
        assert_eq!(
            write(&[code("x", json!("a`b"))]),
            format!("```\nx\n```\n\n^{ID}\n")
        );
        assert_eq!(
            write(&[code("", json!(""))]),
            format!("```\n```\n\n^{ID}\n")
        );
        // The code is written in its normal form: trailing newlines and CRs do not survive, and
        // only the language's first word is the info string.
        assert_eq!(
            write(&[block(None, "code", json!({"code": "x\n\n"}))]),
            "```\nx\n```\n"
        );
        assert_eq!(
            write(&[code("a\r\nb\rc\n", json!("rust ignore"))]),
            format!("```rust\na\nb\nc\n```\n\n^{ID}\n")
        );
    }

    #[test]
    fn dividers_and_images() {
        let hex = "a".repeat(64);
        let blob = format!("blake3:{hex}");
        assert_eq!(
            write(&[
                block(Some(ID), "divider", json!({})),
                text("paragraph", "p")
            ]),
            format!("***\n\n^{ID}\n\np ^{ID}\n")
        );
        assert_eq!(
            write(&[block(Some(ID), "image", json!({"blob": blob}))]),
            format!("![[attachments/{hex}.png]] ^{ID}\n")
        );
        assert_eq!(
            write(&[block(
                None,
                "image",
                json!({"blob": blob, "alt": "A ]]cat|\nphoto"})
            )]),
            format!("![[attachments/{hex}.png|A catphoto]]\n")
        );
        assert_eq!(
            write_blocks(
                &[block(None, "image", json!({"blob": blob, "alt": ""}))],
                &NoResolver
            ),
            format!("![[attachments/{hex}]]\n")
        );
        assert_eq!(
            write(&[block(None, "image", json!({"blob": "nope"}))]),
            "![[attachments/nope]]\n"
        );
    }

    #[test]
    fn unknown_kinds_go_into_a_dbme_fence_with_their_children() {
        let chart = with_children(
            block(Some(ID), "chart", json!({"series": [1, 2]})),
            vec![with_children(
                text("bullet", "a"),
                vec![block(None, "divider", json!({}))],
            )],
        );
        let expected = format!(
            "```dbme\n{{\"children\":[{{\"children\":[{{\"children\":[],\"content\":{{}},\"kind\":\"divider\"}}],\
             \"content\":{{\"inline\":[{{\"text\":\"a\"}}]}},\"id\":\"{ID}\",\"kind\":\"bullet\"}}],\
             \"content\":{{\"series\":[1,2]}},\"kind\":\"chart\"}}\n```\n\n^{ID}\n"
        );
        assert_eq!(write(&[chart]), expected);
        assert_eq!(
            write(&[block(None, "x", json!({"t": "```"}))]),
            "````dbme\n{\"children\":[],\"content\":{\"t\":\"```\"},\"kind\":\"x\"}\n````\n"
        );
    }

    #[test]
    fn render_block_text_has_no_anchor_children_or_trailing_newline() {
        let render = |kind: &str, content: Value| render_block_text(kind, &content, &Vault);
        assert_eq!(
            render("paragraph", json!({"inline": [{"text": "a\nb"}]})),
            "a\nb"
        );
        assert_eq!(render("paragraph", json!({"inline": []})), "");
        assert_eq!(
            render("heading", json!({"level": 2, "inline": [{"text": "h"}]})),
            "## h"
        );
        assert_eq!(
            render("numbered", json!({"inline": [{"text": "n"}]})),
            "1. n"
        );
        assert_eq!(
            render("todo", json!({"checked": true, "inline": [{"text": "t"}]})),
            "- [x] t"
        );
        assert_eq!(render("quote", json!({"inline": [{"text": "q"}]})), "> q");
        assert_eq!(
            render("code", json!({"code": "x", "language": "rs"})),
            "```rs\nx\n```"
        );
        assert_eq!(render("divider", json!({})), "***");
        assert_eq!(
            render("chart", json!({"a": 1})),
            "```dbme\n{\"children\":[],\"content\":{\"a\":1},\"kind\":\"chart\"}\n```"
        );
    }
}
