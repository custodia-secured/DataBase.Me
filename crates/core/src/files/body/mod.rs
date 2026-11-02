// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Vec<FileBlock>` ⇄ Markdown (pulldown-cmark, ADR-0022): anchors, nesting, wikilinks, embeds,
//! tags, escaping and the normal form. The contract of each function is fixed by `files/mod.rs`
//! and delegated to the submodules: [`inline`] turns runs into text and back, [`write`] renders
//! blocks, [`read`] parses them. The shared pieces live here: the parser options, the anchor
//! scanner, marker widths and the block-kind helpers.
//!
//! Traps for the block writer ([`write`]), which the inline layer cannot see:
//! - a heading's rendered text that ends with a `#` run after a space (or is only `#`s) would be
//!   read as a closing sequence: escape the first `#` of that run;
//! - a bullet whose text starts with `[ ]`, `[x]` or `[X]` and a space would be read as a todo:
//!   escape that `[`;
//! - `at_line_start` is `true` for paragraphs, list items and quote lines (their text starts a
//!   line inside the container, so `# x` or `- x` there would open a block) and `false` for
//!   headings;
//! - a lone `^id` line needs a blank line on both sides (ADR-0022 rule 3).

use pulldown_cmark::Options;
use serde_json::Value;

use super::{FileBlock, LinkResolver, MAX_BLOCK_DEPTH, ParseError, Schema, Warning};
use crate::Id;

mod inline;
mod read;
mod write;

/// What [`parse`] returns: the blocks, the body tags (lower-cased, deduplicated, in order) and
/// the warnings.
pub(crate) type ParsedBody = (Vec<FileBlock>, Vec<String>, Vec<Warning>);

/// Reads the body into blocks. `line_offset` is the 0-based line of the file where `body`
/// starts, so positions in errors and warnings are file lines.
pub(crate) fn parse(
    body: &str,
    line_offset: usize,
    schema: &Schema,
    resolver: &dyn LinkResolver,
) -> Result<ParsedBody, ParseError> {
    read::read_blocks(body, line_offset, schema, resolver)
}

/// Writes blocks as Markdown: one blank line between blocks (none between consecutive items of one
/// list), the body ending with exactly one `\n`, empty when there are no blocks.
pub(crate) fn write(blocks: &[FileBlock], resolver: &dyn LinkResolver) -> String {
    write::write_blocks(blocks, resolver)
}

/// Renders one block's text as the merge compares it: no anchor, no children, LF, no trailing
/// newline (ADR-0025).
#[allow(
    dead_code,
    reason = "called by merge.rs once the merge is implemented (M5 Task 2); remove then"
)]
pub(crate) fn render_block_text(
    kind: &str,
    content: &Value,
    resolver: &dyn LinkResolver,
) -> String {
    write::render_block_text(kind, content, resolver)
}

/// Reads a single block's text (as [`render_block_text`] wrote it) back into content of `kind`.
/// `None` when the text does not parse as one block of that kind.
#[allow(
    dead_code,
    reason = "called by merge.rs once the merge is implemented (M5 Task 2); remove then"
)]
pub(crate) fn parse_block_text(
    kind: &str,
    text: &str,
    resolver: &dyn LinkResolver,
) -> Option<Value> {
    read::parse_block_text(kind, text, resolver)
}

/// Puts blocks into normal form, recursively: inline runs through [`normalize_inline`], `code`
/// content without a trailing newline and with an absent empty `language`, headings on one line.
pub(crate) fn normalize(blocks: &mut Vec<FileBlock>) {
    read::normalize(blocks);
}

/// The depth of the deepest block: 1 for a flat list, 0 for none. A worklist, so a deep tree
/// costs no stack.
pub(crate) fn depth_of(blocks: &[FileBlock]) -> usize {
    let mut deepest = 0;
    let mut stack: Vec<(&FileBlock, usize)> = blocks.iter().map(|b| (b, 1)).collect();
    while let Some((block, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        stack.extend(block.children.iter().map(|c| (c, depth + 1)));
    }
    deepest
}

/// Moves every subtree that would sit deeper than [`MAX_BLOCK_DEPTH`] to the top level, in
/// document order, so the tree is one the parser would read back (a block at the last allowed
/// depth has no children). A worklist, so a deep tree costs no stack.
pub(crate) fn flatten_deep(blocks: &mut Vec<FileBlock>) {
    let mut from = 0;
    loop {
        let mut overflow: Vec<FileBlock> = Vec::new();
        let mut stack: Vec<(&mut FileBlock, usize)> =
            blocks[from..].iter_mut().rev().map(|b| (b, 0)).collect();
        while let Some((block, depth)) = stack.pop() {
            if depth + 1 >= MAX_BLOCK_DEPTH {
                overflow.append(&mut block.children);
            } else {
                stack.extend(block.children.iter_mut().rev().map(|c| (c, depth + 1)));
            }
        }
        if overflow.is_empty() {
            return;
        }
        from = blocks.len();
        blocks.extend(overflow);
    }
}

/// Puts one block's inline runs into normal form: trims line edges, collapses `\n\n+` to `\n`,
/// drops empty text runs, merges adjacent text runs with identical marks and link.
pub(crate) fn normalize_inline(content: &mut Value) {
    inline::normalize_inline(content);
}

/// The options every body parse uses: wikilinks, tables, task lists and strikethrough, and never
/// the metadata option (ADR-0022 rule 1: a `---` pair inside the body must stay body).
pub(super) fn parser_options() -> Options {
    Options::ENABLE_WIKILINKS
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
}

/// The id a 26-character anchor names, if `text` is exactly one (lower case accepted).
fn anchor_id(text: &str) -> Option<Id> {
    if text.len() != 26 || !text.is_ascii() {
        return None;
    }
    Id::parse(text).ok()
}

/// Splits a trailing anchor off a block's text: `"item ^<id>"` gives `("item", Some(id))`, a
/// text that is only `^<id>` gives `("", Some(id))`, anything else comes back whole.
pub(super) fn split_anchor(text: &str) -> (&str, Option<Id>) {
    if let Some(id) = lone_anchor(text) {
        return ("", Some(id));
    }
    let n = text.len();
    if n >= 28
        && text.is_char_boundary(n - 28)
        && text.is_char_boundary(n - 26)
        && &text[n - 28..n - 26] == " ^"
        && let Some(id) = anchor_id(&text[n - 26..])
    {
        return (&text[..n - 28], Some(id));
    }
    (text, None)
}

/// The id of a line that is exactly `^<id>`.
pub(super) fn lone_anchor(text: &str) -> Option<Id> {
    text.strip_prefix('^').and_then(anchor_id)
}

/// How far a list item's children indent: the width of its marker. `- ` and `- [ ] ` give 2
/// (the task box is content), `1. ` gives 3, `10. ` gives 4; a kind that is no list item gives 0.
pub(super) fn marker_width(kind: &str, number: usize) -> usize {
    match kind {
        "bullet" | "todo" => 2,
        "numbered" => number.max(1).to_string().len() + 2,
        _ => 0,
    }
}

/// Whether a block of this kind carries its anchor on a line of its own (heading, quote, code,
/// divider and every unknown kind) rather than at the end of its text (paragraph, list items,
/// image).
pub(super) fn anchor_on_own_line(kind: &str) -> bool {
    !matches!(kind, "paragraph" | "bullet" | "numbered" | "todo" | "image")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JB3R7Q8Y5K3M2N4P6Q8R0S2T";

    #[test]
    fn anchors_are_split_off_in_both_positions() {
        let id = Id::parse(ID).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(split_anchor(&format!("item ^{ID}")), ("item", Some(id)));
        assert_eq!(
            split_anchor(&format!("^{}", ID.to_lowercase())),
            ("", Some(id))
        );
        assert_eq!(split_anchor("item ^nope"), ("item ^nope", None));
        assert_eq!(
            split_anchor(&format!("item^{ID}")),
            (format!("item^{ID}").as_str(), None)
        );
        assert_eq!(split_anchor(&format!("é ^{ID}")), ("é", Some(id)));
        assert_eq!(split_anchor(""), ("", None));
        assert_eq!(lone_anchor(&format!("^{ID}")), Some(id));
        assert_eq!(lone_anchor(&format!(" ^{ID}")), None);
        assert_eq!(lone_anchor("^81JB3R7Q8Y5K3M2N4P6Q8R0S2T"), None);
    }

    #[test]
    fn marker_widths_and_anchor_lines() {
        assert_eq!(marker_width("bullet", 0), 2);
        assert_eq!(marker_width("todo", 7), 2);
        assert_eq!(marker_width("numbered", 1), 3);
        assert_eq!(marker_width("numbered", 10), 4);
        assert_eq!(marker_width("numbered", 0), 3);
        assert_eq!(marker_width("paragraph", 1), 0);
        for kind in ["heading", "quote", "code", "divider", "chart"] {
            assert!(anchor_on_own_line(kind), "{kind}");
        }
        for kind in ["paragraph", "bullet", "numbered", "todo", "image"] {
            assert!(!anchor_on_own_line(kind), "{kind}");
        }
        assert!(parser_options().contains(Options::ENABLE_STRIKETHROUGH));
        assert!(!parser_options().contains(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS));
    }
}
