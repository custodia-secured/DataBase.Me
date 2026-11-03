// SPDX-License-Identifier: AGPL-3.0-or-later
//! Markdown → blocks (spec §4, "Reading", and §7): every kind, the lone-anchor rule, lists by
//! marker width, the depth cap, tables and HTML kept verbatim, plus the block-level normal form.
//!
//! The parser walks the events of `pulldown_cmark::Parser::into_offset_iter` as a slice with a
//! cursor: a block is the events between its `Start` and its matching `End`, its text is read by
//! [`inline::read_inline`], and what the inline layer cannot see (an anchor, a task marker, a
//! nested list) is handled here. Recursion follows the block tree only, and a sequence of blocks
//! nested `MAX_BLOCK_DEPTH` deep is refused before it is entered, so the recursion is bounded by
//! the cap, never by the input. Quotes are flattened without recursion (10,000 nested `>` is one
//! quote), so they never count against the cap.

use std::collections::HashSet;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};
use serde_json::{Map, Value, json};

use super::super::{FileBlock, LinkResolver, MAX_BLOCK_DEPTH, ParseError, Schema, Warning};
use super::{ParsedBody, anchor_on_own_line, inline, lone_anchor, parser_options, split_anchor};
use crate::{ContentHash, Id};

/// One parsed event with the byte range it came from.
type Ev<'a> = (Event<'a>, Range<usize>);

/// What reading one top-level event yields.
enum Item {
    /// Nothing (a stray `End`).
    None,
    /// One block.
    One(FileBlock),
    /// A paragraph holding a table's or an HTML block's source verbatim: its anchor, like a
    /// table's in Obsidian, belongs on its own line.
    Verbatim(FileBlock),
    /// The items of a list, which are siblings at this level.
    Many(Vec<FileBlock>),
    /// A paragraph whose whole text is `^<id>`: resolved by the lone-anchor rule.
    Anchor(Id),
}

/// Reads the body into blocks. `line_offset` is the 0-based line of the file where `body`
/// starts, so positions in errors and warnings are file lines.
pub(super) fn read_blocks(
    body: &str,
    line_offset: usize,
    _schema: &Schema,
    resolver: &dyn LinkResolver,
) -> Result<ParsedBody, ParseError> {
    read_body(body, line_offset, resolver)
}

/// [`read_blocks`] without the schema, which the body does not need.
fn read_body(
    body: &str,
    line_offset: usize,
    resolver: &dyn LinkResolver,
) -> Result<ParsedBody, ParseError> {
    let events: Vec<Ev<'_>> = Parser::new_ext(body, parser_options())
        .into_offset_iter()
        .collect();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(body.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let mut reader = Reader {
        events: &events,
        source: body,
        line_starts,
        line_offset,
        resolver,
        warnings: Vec::new(),
        tags: TagSink::default(),
    };
    let blocks = reader.sequence(0, events.len(), 0)?;
    let mut warnings = reader.warnings;
    for warning in &mut warnings {
        warning.line += line_offset;
    }
    Ok((blocks, reader.tags.order, warnings))
}

/// The body tags in order of first appearance, with a set beside the list so that a body of many
/// distinct tags costs no quadratic scan.
#[derive(Default)]
pub(super) struct TagSink {
    order: Vec<String>,
    seen: HashSet<String>,
}

impl TagSink {
    /// Adds a lower-cased tag once.
    pub(super) fn insert(&mut self, tag: String) {
        if self.seen.insert(tag.clone()) {
            self.order.push(tag);
        }
    }

    /// The tags in order of first appearance.
    #[cfg(test)]
    pub(super) fn order(&self) -> &[String] {
        &self.order
    }
}

/// The cursor over one body's events.
struct Reader<'e, 'a> {
    events: &'e [Ev<'a>],
    source: &'a str,
    /// The byte offset of the first byte of every line of `source`; `line_starts[0] == 0`.
    line_starts: Vec<usize>,
    line_offset: usize,
    resolver: &'e dyn LinkResolver,
    /// Lines are 1-based within `source`; [`read_body`] adds the offset at the end.
    warnings: Vec<Warning>,
    tags: TagSink,
}

/// Whether a tag opens a block (as opposed to an inline span).
fn is_block_tag(tag: &Tag<'_>) -> bool {
    !matches!(
        tag,
        Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. }
            | Tag::Image { .. }
    )
}

/// Whether a tag end closes a block.
fn is_block_end(tag: &TagEnd) -> bool {
    !matches!(
        tag,
        TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link
            | TagEnd::Image
    )
}

/// Whether an event belongs to inline content rather than to the block structure.
fn is_inline_event(event: &Event<'_>) -> bool {
    match event {
        Event::Start(tag) => !is_block_tag(tag),
        Event::End(tag) => !is_block_end(tag),
        Event::Rule | Event::TaskListMarker(_) => false,
        _ => true,
    }
}

/// A plain text run.
fn text_run(text: &str) -> Value {
    json!({ "text": text })
}

/// A block with no anchor and no children.
fn plain(kind: &str, content: Value) -> FileBlock {
    FileBlock {
        id: None,
        kind: kind.to_owned(),
        content,
        children: Vec::new(),
    }
}

/// The empty paragraph that a lone anchor becomes when nothing takes it (Decision 11).
fn empty_paragraph(id: Id) -> FileBlock {
    FileBlock {
        id: Some(id),
        kind: "paragraph".to_owned(),
        content: json!({ "inline": [] }),
        children: Vec::new(),
    }
}

/// Strips a trailing ` ^<id>` off the last run when that run is plain or marked text (not code,
/// not a link) and returns the id. The runs are renormalised when something was cut.
fn strip_anchor(runs: &mut Vec<Value>) -> Option<Id> {
    let last = runs.last()?.as_object()?;
    if last.contains_key("code") || last.contains_key("link") {
        return None;
    }
    let (rest, id) = split_anchor(last.get("text")?.as_str()?);
    let id = id?;
    let rest = rest.to_owned();
    if let Some(Value::Object(map)) = runs.last_mut() {
        map.insert("text".to_owned(), Value::String(rest));
    }
    inline::normalize_values(runs);
    Some(id)
}

/// The id when `runs` is exactly one plain text run reading `^<id>`.
fn lone_anchor_runs(runs: &[Value]) -> Option<Id> {
    let [run] = runs else { return None };
    let map = run.as_object()?;
    if map.len() != 1 {
        return None;
    }
    lone_anchor(map.get("text")?.as_str()?)
}

/// `attachments/<64 hex><ext>` → `blake3:<hex>`; anything else is not an image block's target.
fn attachment_blob(dest: &str) -> Option<String> {
    let name = dest.strip_prefix("attachments/")?;
    if name.len() < 64 || !name.is_char_boundary(64) {
        return None;
    }
    let (hex, ext) = name.split_at(64);
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) || !(ext.is_empty() || ext.starts_with('.')) {
        return None;
    }
    Some(format!("blake3:{}", hex.to_ascii_lowercase()))
}

impl<'e, 'a> Reader<'e, 'a> {
    /// A parse error at a byte offset: its file line (by binary search over the line table) and
    /// 0-based column.
    fn error(&self, offset: usize, message: String) -> ParseError {
        let offset = offset.min(self.source.len());
        let line = inline::line_of(&self.line_starts, offset);
        let start = self.line_starts.get(line - 1).copied().unwrap_or(0);
        ParseError {
            line: line + self.line_offset,
            col: offset.saturating_sub(start),
            message,
        }
    }

    /// The raw source of the inline events `start..end`, without trailing line breaks and
    /// whitespace; empty when the range is. The range of an escaped character starts after its
    /// backslash, so when the byte before the first event is one the slice is widened to show it.
    fn raw(&self, start: usize, end: usize) -> &'a str {
        let end = end.min(self.events.len());
        if start >= end {
            return "";
        }
        let mut from = self.events[start].1.start.min(self.source.len());
        if from > 0 && self.source.as_bytes()[from - 1] == b'\\' {
            from -= 1;
        }
        let to = self.events[end - 1].1.end.max(from).min(self.source.len());
        self.source.get(from..to).unwrap_or("").trim_end()
    }

    /// The anchor the raw source of the inline events `start..end` ends with: a last line that
    /// is exactly `^<id>` or ends with ` ^<id>`. The decoded runs cannot tell `\^` from `^`,
    /// so the source decides: the writer escapes a caret that would read as an anchor, and a
    /// `^<id>` inside a mark or a link ends with the mark's delimiter, not with the id.
    fn raw_anchor(&self, start: usize, end: usize) -> Option<Id> {
        let last_line = self.raw(start, end).rsplit('\n').next().unwrap_or("");
        split_anchor(last_line.trim_end()).1
    }

    /// Whether the raw source of the inline events `start..end` is exactly `^<id>`.
    fn raw_lone_anchor(&self, start: usize, end: usize) -> Option<Id> {
        lone_anchor(self.raw(start, end).trim())
    }

    /// [`strip_anchor`] when the raw source carries the anchor (see [`Self::raw_anchor`]).
    fn anchor_of(&self, start: usize, end: usize, runs: &mut Vec<Value>) -> Option<Id> {
        self.raw_anchor(start, end)?;
        strip_anchor(runs)
    }

    /// The index of the `End` that closes the `Start` at `start`, or the last index.
    fn end_of(&self, start: usize) -> usize {
        let mut depth = 0usize;
        for (j, (event, _)) in self.events.iter().enumerate().skip(start) {
            match event {
                Event::Start(_) => depth += 1,
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return j;
                    }
                }
                _ => {}
            }
        }
        self.events.len().saturating_sub(1)
    }

    /// The runs of the inline events `range` (event indices), in normal form.
    fn inline(&mut self, range: Range<usize>) -> Vec<Value> {
        let end = range.end.min(self.events.len());
        let start = range.start.min(end);
        inline::read_inline(
            &self.events[start..end],
            self.source,
            &self.line_starts,
            self.resolver,
            &mut self.warnings,
            &mut self.tags,
        )
    }

    /// Reads the blocks of the events `start..end` as siblings at `depth`, applying the
    /// lone-anchor rule: a `^<id>` paragraph is the anchor of the preceding sibling when that
    /// sibling's kind carries its anchor on its own line and it has none yet, else an empty
    /// paragraph with that id. A non-empty sequence at `MAX_BLOCK_DEPTH` is refused.
    fn sequence(
        &mut self,
        start: usize,
        end: usize,
        depth: usize,
    ) -> Result<Vec<FileBlock>, ParseError> {
        if start < end && depth >= MAX_BLOCK_DEPTH {
            return Err(self.error(
                self.events[start].1.start,
                format!("blocks nested deeper than {MAX_BLOCK_DEPTH} levels"),
            ));
        }
        let mut blocks: Vec<FileBlock> = Vec::new();
        let mut last_verbatim = false;
        let mut i = start;
        while i < end {
            let (next, item) = self.block(i, end, depth)?;
            i = next.max(i + 1);
            let verbatim = matches!(item, Item::Verbatim(_));
            match item {
                Item::None => {}
                Item::One(block) | Item::Verbatim(block) => blocks.push(block),
                Item::Many(items) => blocks.extend(items),
                Item::Anchor(id) => match blocks.last_mut() {
                    Some(prev)
                        if prev.id.is_none()
                            && (last_verbatim || anchor_on_own_line(&prev.kind)) =>
                    {
                        prev.id = Some(id);
                    }
                    _ => blocks.push(empty_paragraph(id)),
                },
            }
            last_verbatim = verbatim;
        }
        Ok(blocks)
    }

    /// Reads the block starting at event `i` (bounded by `end`); gives the index after it.
    fn block(&mut self, i: usize, end: usize, depth: usize) -> Result<(usize, Item), ParseError> {
        let (event, range) = &self.events[i];
        let range = range.clone();
        match event {
            Event::Start(Tag::Paragraph) => {
                let e = self.end_of(i).min(end);
                Ok((e + 1, self.paragraph(i + 1, e)))
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let e = self.end_of(i).min(end);
                let level = *level as usize;
                let mut runs = self.inline(i + 1..e);
                heading_one_line(&mut runs);
                let content = json!({ "level": level, "inline": runs });
                Ok((e + 1, Item::One(plain("heading", content))))
            }
            Event::Start(Tag::List(first)) => {
                let e = self.end_of(i).min(end);
                let items = self.list(i + 1, e, first.is_some(), depth)?;
                Ok((e + 1, Item::Many(items)))
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let e = self.end_of(i).min(end);
                let runs = self.quote(i + 1, e);
                Ok((e + 1, Item::One(plain("quote", json!({ "inline": runs })))))
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let e = self.end_of(i).min(end);
                let kind = kind.clone();
                let block = self.code(i + 1, e, &kind, depth, range.start)?;
                Ok((e + 1, Item::One(block)))
            }
            Event::Rule => Ok((i + 1, Item::One(plain("divider", json!({}))))),
            // Tables, HTML blocks, footnote definitions, metadata blocks, definition lists: the
            // source slice as one text run, nothing lost.
            Event::Start(_) => {
                let e = self.end_of(i).min(end);
                Ok((e + 1, Item::Verbatim(self.verbatim(range))))
            }
            Event::End(_) | Event::TaskListMarker(_) => Ok((i + 1, Item::None)),
            // Inline events outside a block (the parser does not produce them, but a slice of a
            // tight item does): a paragraph of what follows.
            _ => {
                let mut j = i;
                while j < end && is_inline_event(&self.events[j].0) {
                    j += 1;
                }
                let mut runs = self.inline(i..j);
                let id = self.anchor_of(i, j, &mut runs);
                let mut block = plain("paragraph", json!({ "inline": runs }));
                block.id = id;
                Ok((j, Item::One(block)))
            }
        }
    }

    /// A paragraph's inner events: an image block, a lone anchor or a paragraph.
    fn paragraph(&mut self, start: usize, end: usize) -> Item {
        if let Some(item) = self.image(start, end) {
            return item;
        }
        let mut runs = self.inline(start..end);
        if let Some(id) = self.raw_lone_anchor(start, end)
            && lone_anchor_runs(&runs) == Some(id)
        {
            return Item::Anchor(id);
        }
        let id = self.anchor_of(start, end, &mut runs);
        let mut block = plain("paragraph", json!({ "inline": runs }));
        block.id = id;
        Item::One(block)
    }

    /// The source slice of a block as a paragraph with one text run.
    fn verbatim(&self, range: Range<usize>) -> FileBlock {
        let text = self.source[range].trim_end_matches(['\n', '\r']);
        let mut runs = vec![text_run(text)];
        inline::normalize_values(&mut runs);
        plain("paragraph", json!({ "inline": runs }))
    }
}

impl Reader<'_, '_> {
    /// An image block: a paragraph that is exactly one `![[attachments/<hex><ext>]]` (with an
    /// optional `|alt` and an optional trailing anchor).
    fn image(&mut self, start: usize, end: usize) -> Option<Item> {
        let (
            Event::Start(Tag::Image {
                link_type: LinkType::WikiLink { has_pothole },
                dest_url,
                ..
            }),
            _,
        ) = self.events.get(start)?
        else {
            return None;
        };
        let blob = attachment_blob(dest_url)?;
        let image_end = self.end_of(start);
        if image_end >= end {
            return None;
        }
        // What follows the embed on the line: nothing, or ` ^<id>` in the source (an escaped
        // `\^` is text, and makes this a paragraph).
        let tail = self.raw(image_end + 1, end);
        let id = if tail.is_empty() {
            None
        } else if tail.starts_with([' ', '\t']) && !tail.contains('\n') {
            Some(lone_anchor(tail.trim_start())?)
        } else {
            return None;
        };
        let mut content = Map::new();
        content.insert("blob".to_owned(), Value::String(blob));
        if *has_pothole {
            // The alt is the raw text after `|`, unescaped, as the writer escaped it: the inline
            // events would read `*a*` as emphasis and lose the stars.
            let raw = &self.source[self.events[start].1.clone()];
            let inner = raw.trim_start_matches('!').trim_start_matches("[[");
            let inner = inner.strip_suffix("]]").unwrap_or(inner);
            let alt = inner
                .split_once('|')
                .map(|(_, shown)| inline::unescape(shown))
                .unwrap_or_default();
            if !alt.is_empty() {
                content.insert("alt".to_owned(), Value::String(alt));
            }
        }
        let mut block = plain("image", Value::Object(content));
        block.id = id;
        Some(Item::One(block))
    }

    /// The items of a list whose inner events are `start..end`.
    fn list(
        &mut self,
        start: usize,
        end: usize,
        numbered: bool,
        depth: usize,
    ) -> Result<Vec<FileBlock>, ParseError> {
        let mut items = Vec::new();
        let mut j = start;
        while j < end {
            if matches!(self.events[j].0, Event::Start(Tag::Item)) {
                let item_end = self.end_of(j).min(end);
                items.push(self.item(j + 1, item_end, numbered, depth)?);
                j = item_end + 1;
            } else {
                j += 1;
            }
        }
        Ok(items)
    }

    /// The `TaskListMarker` at event `j`, if there is one before `end`.
    fn task_marker(&self, j: usize, end: usize) -> Option<bool> {
        match self.events.get(j) {
            Some((Event::TaskListMarker(checked), _)) if j < end => Some(*checked),
            _ => None,
        }
    }

    /// One list item: its marker decides the kind, its first paragraph (or bare inline events)
    /// is the inline, everything after is children at `depth + 1`.
    fn item(
        &mut self,
        start: usize,
        end: usize,
        numbered: bool,
        depth: usize,
    ) -> Result<FileBlock, ParseError> {
        let mut j = start;
        let mut checked = self.task_marker(j, end);
        if checked.is_some() {
            j += 1;
        }
        let (mut runs, text_range) =
            if j < end && matches!(self.events[j].0, Event::Start(Tag::Paragraph)) {
                let p_end = self.end_of(j).min(end);
                let mut k = j + 1;
                if checked.is_none() {
                    checked = self.task_marker(k, p_end);
                    if checked.is_some() {
                        k += 1;
                    }
                }
                let runs = self.inline(k..p_end);
                j = p_end + 1;
                (runs, k..p_end)
            } else {
                let k = j;
                while j < end && is_inline_event(&self.events[j].0) {
                    j += 1;
                }
                (self.inline(k..j), k..j)
            };
        let id = self.anchor_of(text_range.start, text_range.end, &mut runs);
        let (kind, content) = match checked {
            Some(checked) => ("todo", json!({ "inline": runs, "checked": checked })),
            None if numbered => ("numbered", json!({ "inline": runs })),
            None => ("bullet", json!({ "inline": runs })),
        };
        let children = self.sequence(j, end, depth + 1)?;
        Ok(FileBlock {
            id,
            kind: kind.to_owned(),
            content,
            children,
        })
    }

    /// A quote's inner events flattened to one run list: the paragraphs' runs joined with `\n`,
    /// nested quotes and lists transparent, other blocks as their source slice.
    fn quote(&mut self, start: usize, end: usize) -> Vec<Value> {
        let mut runs: Vec<Value> = Vec::new();
        let mut segments = 0usize;
        // How many `>` containers enclose the current event: the quote being read, plus nested
        // quotes, each of which prefixes every source line of a block inside it.
        let mut depth = 1usize;
        let mut j = start;
        while j < end {
            let (event, range) = &self.events[j];
            let range = range.clone();
            let segment = match event {
                Event::Start(Tag::Paragraph | Tag::Heading { .. }) => {
                    let p_end = self.end_of(j).min(end);
                    let runs = self.inline(j + 1..p_end);
                    j = p_end + 1;
                    runs
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    depth += 1;
                    j += 1;
                    continue;
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    depth = depth.saturating_sub(1).max(1);
                    j += 1;
                    continue;
                }
                Event::Start(Tag::List(_) | Tag::Item)
                | Event::End(_)
                | Event::TaskListMarker(_) => {
                    j += 1;
                    continue;
                }
                Event::Rule => {
                    j += 1;
                    vec![text_run(&strip_quote_prefix(&self.source[range], depth))]
                }
                Event::Start(_) => {
                    j = self.end_of(j).min(end) + 1;
                    vec![text_run(&strip_quote_prefix(&self.source[range], depth))]
                }
                _ => {
                    let k = j;
                    while j < end && is_inline_event(&self.events[j].0) {
                        j += 1;
                    }
                    self.inline(k..j)
                }
            };
            if segments > 0 {
                runs.push(text_run("\n"));
            }
            segments += 1;
            runs.extend(segment);
        }
        inline::normalize_values(&mut runs);
        runs
    }
}

impl Reader<'_, '_> {
    /// A code block: the `Text` events joined, CRLF to LF, exactly one trailing newline removed.
    /// A fence whose info word is `dbme` holds an unknown-kind block as JSON; when that JSON is
    /// malformed the block stays a code block with language `dbme`, so nothing is lost. `offset`
    /// is where the fence starts, for an error.
    fn code(
        &mut self,
        start: usize,
        end: usize,
        kind: &CodeBlockKind<'_>,
        depth: usize,
        offset: usize,
    ) -> Result<FileBlock, ParseError> {
        let mut code: String = self.events[start..end.min(self.events.len())]
            .iter()
            .filter_map(|(event, _)| match event {
                Event::Text(text) => Some(&**text),
                _ => None,
            })
            .collect();
        if code.contains('\r') {
            // CommonMark: a lone CR is a line ending too.
            code = code.replace("\r\n", "\n").replace('\r', "\n");
        }
        if code.ends_with('\n') {
            code.pop();
        }
        let language = match kind {
            CodeBlockKind::Fenced(info) => info.split_whitespace().next(),
            CodeBlockKind::Indented => None,
        };
        if language == Some("dbme")
            && let Some(block) = self.dbme(&code, depth, offset)?
        {
            return Ok(block);
        }
        let mut content = Map::new();
        content.insert("code".to_owned(), Value::String(code));
        if let Some(language) = language {
            content.insert("language".to_owned(), Value::String(language.to_owned()));
        }
        Ok(plain("code", Value::Object(content)))
    }

    /// The unknown-kind block a `dbme` fence holds: `{"kind", "content", "children"?, "id"?}`,
    /// children as `{"id"?, "kind", "content", "children"?}`. `None` when malformed; an error
    /// when the children nest past the cap.
    fn dbme(
        &self,
        json: &str,
        depth: usize,
        offset: usize,
    ) -> Result<Option<FileBlock>, ParseError> {
        let Ok(value) = serde_json::from_str::<Value>(json) else {
            return Ok(None);
        };
        // The fence holds the block verbatim; normal form is the reader's promise, so a
        // hand-written fence normalises here like every other block does on its way in.
        let Some(block) = self.block_from_json(&value, depth, offset)? else {
            return Ok(None);
        };
        let mut normal = vec![block];
        normalize(&mut normal);
        Ok(normal.pop())
    }

    /// One block of a `dbme` fence's JSON, recursively (bounded by the depth cap).
    fn block_from_json(
        &self,
        value: &Value,
        depth: usize,
        offset: usize,
    ) -> Result<Option<FileBlock>, ParseError> {
        let Some(map) = value.as_object() else {
            return Ok(None);
        };
        let Some(kind) = map.get("kind").and_then(Value::as_str) else {
            return Ok(None);
        };
        let Some(content) = map.get("content").filter(|c| c.is_object()).cloned() else {
            return Ok(None);
        };
        let id = match map.get("id") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => match Id::parse(text) {
                Ok(id) => Some(id),
                Err(_) => return Ok(None),
            },
            Some(_) => return Ok(None),
        };
        let mut children = Vec::new();
        match map.get("children") {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                if !items.is_empty() && depth + 1 >= MAX_BLOCK_DEPTH {
                    return Err(self.error(
                        offset,
                        format!("blocks nested deeper than {MAX_BLOCK_DEPTH} levels"),
                    ));
                }
                for item in items {
                    match self.block_from_json(item, depth + 1, offset)? {
                        Some(child) => children.push(child),
                        None => return Ok(None),
                    }
                }
            }
            Some(_) => return Ok(None),
        }
        Ok(Some(FileBlock {
            id,
            kind: kind.to_owned(),
            content,
            children,
        }))
    }
}

/// The source slice of a block nested `depth` quotes deep (a code block, a table, an HTML block
/// inside `> `), with the quote markers taken off every line after the first: pulldown's range
/// starts after the first line's markers but spans the raw lines below it, which still carry
/// them. Each marker is up to three spaces or tabs, one `>` and one optional space or tab; a
/// line with fewer markers than `depth` (a lazy continuation) keeps what it has, so a `>` that
/// is content stays.
fn strip_quote_prefix(slice: &str, depth: usize) -> String {
    let slice = slice.trim_end_matches(['\n', '\r']);
    let mut out = String::with_capacity(slice.len());
    for (i, raw_line) in slice.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if i == 0 {
            out.push_str(line);
            continue;
        }
        out.push('\n');
        let mut rest = line;
        for _ in 0..depth {
            let bytes = rest.as_bytes();
            let mut k = 0;
            while k < 3 && k < bytes.len() && matches!(bytes[k], b' ' | b'\t') {
                k += 1;
            }
            if bytes.get(k) != Some(&b'>') {
                break;
            }
            k += 1;
            if matches!(bytes.get(k), Some(b' ' | b'\t')) {
                k += 1;
            }
            rest = &rest[k..];
        }
        out.push_str(rest);
    }
    out
}

/// A heading is one line: every line break in its runs becomes a space.
fn heading_one_line(runs: &mut Vec<Value>) {
    let mut changed = false;
    for run in runs.iter_mut() {
        if let Some(Value::String(text)) = run.get_mut("text")
            && text.contains(['\n', '\r'])
        {
            *text = text.replace("\r\n", " ").replace(['\n', '\r'], " ");
            changed = true;
        }
    }
    if changed {
        inline::normalize_values(runs);
    }
}

/// Reads a single block's text (as `write::render_block_text` wrote it) back into content of
/// `kind`: the text is parsed as a body and must yield exactly one block. A list kind accepts a
/// paragraph or another list kind (the marker is wrapped on); `code` accepts any text as its
/// code when no fence parses. `None` otherwise.
pub(super) fn parse_block_text(
    kind: &str,
    text: &str,
    resolver: &dyn LinkResolver,
) -> Option<Value> {
    let (mut blocks, _, _) = read_body(text, 0, resolver).ok()?;
    if blocks.len() != 1 {
        return (kind == "code").then(|| json!({ "code": text }));
    }
    let block = blocks.pop()?;
    if block.kind == kind {
        return Some(block.content);
    }
    let is_list = |k: &str| matches!(k, "bullet" | "numbered" | "todo");
    if is_list(kind) && (block.kind == "paragraph" || is_list(&block.kind)) {
        let inline = block
            .content
            .get("inline")
            .cloned()
            .unwrap_or_else(|| json!([]));
        return Some(if kind == "todo" {
            let checked = block
                .content
                .get("checked")
                .cloned()
                .unwrap_or(Value::Bool(false));
            json!({ "inline": inline, "checked": checked })
        } else {
            json!({ "inline": inline })
        });
    }
    (kind == "code").then(|| json!({ "code": text }))
}

/// Puts blocks into normal form, recursively (with a worklist, so a deep tree costs no stack):
/// inline runs through `inline::normalize_inline`, headings on one line, `code` content without
/// trailing newlines, LF only, and with an absent empty or unusable `language`.
#[allow(
    clippy::ptr_arg,
    reason = "the contract signature `files/mod.rs` delegates to; a slice there would trip the same lint on the frozen file"
)]
pub(super) fn normalize(blocks: &mut Vec<FileBlock>) {
    let mut stack: Vec<&mut FileBlock> = blocks.iter_mut().collect();
    while let Some(block) = stack.pop() {
        normalize_block(block);
        stack.extend(block.children.iter_mut());
    }
}

fn normalize_block(block: &mut FileBlock) {
    match block.kind.as_str() {
        "code" => normalize_code(&mut block.content),
        "image" => {
            if let Some(paragraph) = demote_image(&block.content) {
                // The writer could only spell the blob as the embed's name, outside every
                // escape, and the reader reads a non-hash embed as text: so it is text here.
                block.kind = "paragraph".to_owned();
                block.content = paragraph;
                inline::normalize_inline(&mut block.content);
            } else {
                normalize_image(&mut block.content);
            }
        }
        kind => {
            if kind == "heading"
                && let Some(Value::Array(runs)) = block.content.get_mut("inline")
            {
                heading_one_line(runs);
            }
            inline::normalize_inline(&mut block.content);
        }
    }
}

/// The paragraph an `image` block becomes when its `blob` is no content hash: one text run of
/// the embed as the reader would read it, `![[attachments/<blob>|<alt>]]`, with the alt cleaned
/// as [`normalize_image`] cleans it. `None` when the blob is a content hash.
pub(super) fn demote_image(content: &Value) -> Option<Value> {
    let blob = content.get("blob").and_then(Value::as_str).unwrap_or("");
    if ContentHash::parse(blob).is_ok() {
        return None;
    }
    let mut text = format!("![[attachments/{blob}");
    let alt: String = content
        .get("alt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .filter(|c| !matches!(c, ']' | '|' | '\n' | '\r'))
        .collect();
    if !alt.is_empty() {
        text.push('|');
        text.push_str(&alt);
    }
    text.push_str("]]");
    Some(json!({ "inline": [text_run(&text)] }))
}

/// An image's `alt` without the characters that would end the embed (`]`, `|`, line breaks),
/// absent when nothing is left or it is no string.
fn normalize_image(content: &mut Value) {
    let Some(map) = content.as_object_mut() else {
        return;
    };
    let alt = match map.get("alt") {
        None => return,
        Some(Value::String(alt)) => alt
            .chars()
            .filter(|c| !matches!(c, ']' | '|' | '\n' | '\r'))
            .collect::<String>(),
        Some(_) => String::new(),
    };
    if alt.is_empty() {
        map.remove("alt");
    } else {
        map.insert("alt".to_owned(), Value::String(alt));
    }
}

fn normalize_code(content: &mut Value) {
    let Some(map) = content.as_object_mut() else {
        return;
    };
    if let Some(Value::String(code)) = map.get_mut("code") {
        if code.contains('\r') {
            *code = code.replace("\r\n", "\n").replace('\r', "\n");
        }
        while code.ends_with('\n') {
            code.pop();
        }
    }
    let language = match map.get("language") {
        None => return,
        Some(Value::String(language)) => language
            .split_whitespace()
            .next()
            .filter(|word| !word.contains('`'))
            .map(str::to_owned),
        Some(_) => None,
    };
    match language {
        Some(word) => {
            map.insert("language".to_owned(), Value::String(word));
        }
        None => {
            map.remove("language");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests: a failure is the finding"
    )]

    use serde_json::json;

    use super::*;
    use crate::files::NoResolver;

    const ID: &str = "01JB3R7Q8Y5K3M2N4P6Q8R0S2T";
    const ID2: &str = "01JB3R9C0D4E5F6G7H8J9K0M1N";
    const HEX: &str = "0123456789abcdef0123456789ABCDEF0123456789abcdef0123456789abcdef";

    fn id(text: &str) -> Id {
        Id::parse(text).unwrap()
    }

    fn parse(md: &str) -> ParsedBody {
        read_body(md, 0, &NoResolver).unwrap_or_else(|e| panic!("{md:?}: {e}"))
    }

    fn blocks(md: &str) -> Vec<FileBlock> {
        parse(md).0
    }

    fn runs(text: &str) -> Value {
        json!([{ "text": text }])
    }

    fn para(text: &str) -> FileBlock {
        plain("paragraph", json!({ "inline": runs(text) }))
    }

    fn item(kind: &str, text: &str) -> FileBlock {
        plain(kind, json!({ "inline": runs(text) }))
    }

    fn todo(text: &str, checked: bool) -> FileBlock {
        plain("todo", json!({ "inline": runs(text), "checked": checked }))
    }

    fn code(text: &str, language: Option<&str>) -> FileBlock {
        let mut content = json!({ "code": text });
        if let Some(language) = language {
            content["language"] = Value::String(language.to_owned());
        }
        plain("code", content)
    }

    fn anchored(mut block: FileBlock, id_text: &str) -> FileBlock {
        block.id = Some(id(id_text));
        block
    }

    fn with(mut block: FileBlock, children: Vec<FileBlock>) -> FileBlock {
        block.children = children;
        block
    }

    #[test]
    fn paragraphs_headings_and_dividers_with_their_anchors() {
        let md = format!("Hello **world** ^{ID}\n\n# Title\n\n^{ID2}\n\n***\n\nplain\n");
        let title = plain("heading", json!({ "level": 1, "inline": runs("Title") }));
        assert_eq!(
            blocks(&md),
            vec![
                anchored(
                    plain(
                        "paragraph",
                        json!({ "inline": [{ "text": "Hello " }, { "text": "world", "bold": true }] })
                    ),
                    ID
                ),
                anchored(title, ID2),
                plain("divider", json!({})),
                para("plain"),
            ]
        );
        // Every heading level; a heading's anchor is never read off its own line, and a setext
        // heading's two lines become one.
        for level in 1..=6 {
            let md = format!("{} h ^{ID}\n", "#".repeat(level));
            let content = json!({ "level": level, "inline": runs(&format!("h ^{ID}")) });
            assert_eq!(blocks(&md), vec![plain("heading", content)]);
        }
        assert_eq!(
            blocks("one\ntwo\n===\n"),
            vec![plain(
                "heading",
                json!({ "level": 1, "inline": runs("one two") })
            )]
        );
        // A lower-case anchor and one after inline code are read too.
        let lower = ID.to_ascii_lowercase();
        assert_eq!(
            blocks(&format!("x `y` ^{lower}\n")),
            vec![anchored(
                plain(
                    "paragraph",
                    json!({ "inline": [{ "text": "x " }, { "text": "y", "code": true }] })
                ),
                ID
            )]
        );
        // Inside a code span or a link the caret is text.
        assert_eq!(
            blocks(&format!("`x ^{ID}`\n")),
            vec![plain(
                "paragraph",
                json!({ "inline": [{ "text": format!("x ^{ID}"), "code": true }] })
            )]
        );
    }

    #[test]
    fn lone_anchor_rule() {
        let empty = |id_text: &str| empty_paragraph(id(id_text));
        assert_eq!(blocks(&format!("^{ID}\n")), vec![empty(ID)]);
        assert_eq!(
            blocks(&format!("Hello ^{ID}\n\n^{ID2}\n")),
            vec![anchored(para("Hello"), ID), empty(ID2)]
        );
        assert_eq!(
            blocks(&format!("# T\n\n^{ID}\n\n^{ID2}\n")),
            vec![
                anchored(
                    plain("heading", json!({ "level": 1, "inline": runs("T") })),
                    ID
                ),
                empty(ID2),
            ]
        );
        assert_eq!(
            blocks(&format!("- a\n\n^{ID}\n")),
            vec![item("bullet", "a"), empty(ID)]
        );
        assert_eq!(
            blocks(&format!("```\nx\n```\n\n^{ID}\n")),
            vec![anchored(code("x", None), ID)]
        );
        assert_eq!(
            blocks(&format!("> q\n\n^{ID}\n")),
            vec![anchored(plain("quote", json!({ "inline": runs("q") })), ID)]
        );
        assert_eq!(
            blocks(&format!("***\n\n^{ID}\n")),
            vec![anchored(plain("divider", json!({})), ID)]
        );
        // A table's anchor sits on its own line too (Obsidian), after a blank line.
        let table = "| a | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(
            blocks(&format!("{table}\n\n^{ID}\n")),
            vec![anchored(para(table), ID)]
        );
        // Nested: the same rule among an item's children.
        assert_eq!(
            blocks(&format!("- a\n\n  > q\n\n  ^{ID}\n\n  ^{ID2}\n")),
            vec![with(
                item("bullet", "a"),
                vec![
                    anchored(plain("quote", json!({ "inline": runs("q") })), ID),
                    empty(ID2),
                ]
            )]
        );
    }

    #[test]
    fn lists_by_kind_marker_and_width() {
        assert_eq!(
            blocks("- a\n- [ ] b\n- [x] c\n\n1. one\n2. two\n"),
            vec![
                item("bullet", "a"),
                todo("b", false),
                todo("c", true),
                item("numbered", "one"),
                item("numbered", "two"),
            ]
        );
        assert_eq!(
            blocks(&format!("- a ^{ID}\n- ^{ID2}\n")),
            vec![
                anchored(item("bullet", "a"), ID),
                anchored(plain("bullet", json!({ "inline": [] })), ID2),
            ]
        );
        let nested = with(
            item("bullet", "a"),
            vec![with(item("bullet", "b"), vec![item("bullet", "c")])],
        );
        assert_eq!(blocks("- a\n  - b\n    - c\n"), vec![nested]);
        assert_eq!(
            blocks("1. a\n   - b\n"),
            vec![with(item("numbered", "a"), vec![item("bullet", "b")])]
        );
        assert_eq!(
            blocks("10. a\n    - b\n"),
            vec![with(item("numbered", "a"), vec![item("bullet", "b")])]
        );
        // Three spaces under `10. ` do not reach the content column: a new list (ADR-0022 rule 4).
        assert_eq!(
            blocks("10. a\n   - b\n"),
            vec![item("numbered", "a"), item("bullet", "b")]
        );
        // Loose and tight read the same; a loose todo keeps its box.
        assert_eq!(blocks("- a\n\n- b\n"), blocks("- a\n- b\n"));
        assert_eq!(
            blocks("- [ ] a\n\n- [x] b\n"),
            vec![todo("a", false), todo("b", true)]
        );
        // Other blocks inside an item are children of their kind.
        assert_eq!(
            blocks("- a\n\n  para\n\n  ```\n  code\n  ```\n"),
            vec![with(
                item("bullet", "a"),
                vec![para("para"), code("code", None)]
            )]
        );
    }

    #[test]
    fn nesting_past_the_cap_is_an_error_at_that_line() {
        let nested = |levels: usize| -> String {
            (0..levels)
                .map(|d| format!("{}- x\n", "  ".repeat(d)))
                .collect()
        };
        let ok = blocks(&nested(MAX_BLOCK_DEPTH));
        let mut depth = 0;
        let mut cursor = &ok[0];
        while let Some(child) = cursor.children.first() {
            depth += 1;
            cursor = child;
        }
        assert_eq!(depth + 1, MAX_BLOCK_DEPTH);
        let err = read_body(&nested(MAX_BLOCK_DEPTH + 1), 2, &NoResolver).unwrap_err();
        assert_eq!(
            (err.line, err.col),
            (MAX_BLOCK_DEPTH + 1 + 2, 2 * MAX_BLOCK_DEPTH)
        );
        assert!(err.message.contains("deeper"), "{err}");
    }

    #[test]
    fn quotes_join_their_paragraphs_and_flatten_nesting() {
        let quote = |inline: Value| plain("quote", json!({ "inline": inline }));
        assert_eq!(
            blocks("> p1\n>\n> p2 **b**\n"),
            vec![quote(
                json!([{ "text": "p1\np2 " }, { "text": "b", "bold": true }])
            )]
        );
        assert_eq!(blocks("> a\n> > b\n"), vec![quote(runs("a\nb"))]);
        assert_eq!(blocks("> - a\n> - b\n"), vec![quote(runs("a\nb"))]);
        // 10,000 nested quotes are one quote, no recursion.
        let deep = format!("{}x\n", "> ".repeat(10_000));
        assert_eq!(blocks(&deep), vec![quote(runs("x"))]);
    }

    #[test]
    fn code_blocks_fenced_indented_and_dbme() {
        assert_eq!(
            blocks("```rust extra\nfn x() {}\n```\n"),
            vec![code("fn x() {}", Some("rust"))]
        );
        assert_eq!(
            blocks("````\n```\n---\n````\n"),
            vec![code("```\n---", None)]
        );
        assert_eq!(blocks("    x\n    y\n"), vec![code("x\ny", None)]);
        assert_eq!(blocks("```\n```\n"), vec![code("", None)]);
        assert_eq!(blocks("```\na\n\n```\n"), vec![code("a\n", None)]);
        assert_eq!(blocks("```\r\na\r\nb\r\n```\r\n"), vec![code("a\nb", None)]);
        let json = format!(
            "{{\"kind\":\"chart\",\"content\":{{\"x\":1}},\"children\":[{{\"id\":\"{ID}\",\
             \"kind\":\"paragraph\",\"content\":{{\"inline\":[]}}}}]}}"
        );
        let md = format!("```dbme\n{json}\n```\n\n^{ID2}\n");
        assert_eq!(
            blocks(&md),
            vec![anchored(
                with(
                    plain("chart", json!({ "x": 1 })),
                    vec![anchored(plain("paragraph", json!({ "inline": [] })), ID)]
                ),
                ID2
            )]
        );
        for bad in [
            "{not json",
            "{\"content\":{}}",
            "{\"kind\":\"c\",\"content\":1}",
            "[]",
        ] {
            assert_eq!(
                blocks(&format!("```dbme\n{bad}\n```\n")),
                vec![code(bad, Some("dbme"))],
                "{bad}"
            );
        }
        // Children nested past the cap: counted together with the list depth the fence sits at.
        // (serde_json's own limit of 128 nesting levels refuses 65 levels of `{"children":[…]}`
        // before the cap is reached, and that is the code fallback, nothing lost.)
        let nest = |levels: usize| {
            let mut deep = String::from("{\"kind\":\"k\",\"content\":{}}");
            for _ in 0..levels {
                deep = format!("{{\"kind\":\"k\",\"content\":{{}},\"children\":[{deep}]}}");
            }
            deep
        };
        let too_deep = nest(MAX_BLOCK_DEPTH);
        assert_eq!(
            blocks(&format!("```dbme\n{too_deep}\n```\n")),
            vec![code(&too_deep, Some("dbme"))]
        );
        let list: String = (0..40)
            .map(|d| format!("{}- x\n", "  ".repeat(d)))
            .collect();
        let indent = "  ".repeat(40);
        let json = nest(30);
        let md = format!("{list}\n{indent}```dbme\n{indent}{json}\n{indent}```\n");
        let err = read_body(&md, 0, &NoResolver).unwrap_err();
        assert_eq!((err.line, err.col), (42, 80), "{err}");
        assert!(read_body(&md.replace(&json, &nest(20)), 0, &NoResolver).is_ok());
    }

    #[test]
    fn tables_and_html_are_kept_verbatim() {
        let table = "| a | b |\n|---|---|\n| **1** | 2 |";
        assert_eq!(
            blocks(&format!("{table}\n\nafter\n")),
            vec![para(table), para("after")]
        );
        assert_eq!(blocks("<div>\nx\n</div>\n"), vec![para("<div>\nx\n</div>")]);
    }

    #[test]
    fn image_blocks_from_attachment_embeds() {
        let hex = HEX.to_ascii_lowercase();
        let image = |alt: Option<&str>| {
            let mut content = json!({ "blob": format!("blake3:{hex}") });
            if let Some(alt) = alt {
                content["alt"] = Value::String(alt.to_owned());
            }
            plain("image", content)
        };
        assert_eq!(
            blocks(&format!("![[attachments/{HEX}.png]]\n")),
            vec![image(None)]
        );
        assert_eq!(
            blocks(&format!("![[attachments/{HEX}]]\n")),
            vec![image(None)]
        );
        assert_eq!(
            blocks(&format!("![[attachments/{HEX}.png|A cat]] ^{ID}\n")),
            vec![anchored(image(Some("A cat")), ID)]
        );
        // Not an image block: another embed, a short hash, or more text around it.
        let (parsed, _, warnings) = parse("![[Title]]\n");
        assert_eq!(parsed, vec![para("![[Title]]")]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            blocks("![[attachments/abc.png]]\n"),
            vec![para("![[attachments/abc.png]]")]
        );
        assert_eq!(
            blocks(&format!("![[attachments/{HEX}.png]] more\n")),
            vec![para(&format!("![[attachments/{HEX}.png]] more"))]
        );
    }

    #[test]
    fn crlf_input_reads_like_lf() {
        let crlf = format!("Hello ^{ID}\r\n\r\n- a\r\n  - b\r\n\r\n# T\r\n\r\n^{ID2}\r\n");
        assert_eq!(blocks(&crlf), blocks(&crlf.replace("\r\n", "\n")));
        assert_eq!(blocks(&crlf)[0], anchored(para("Hello"), ID));
    }

    #[test]
    fn tags_and_warning_lines_carry_the_body_offset() {
        let (_, tags, warnings) = read_body(
            "Hello #Tag and #tag/sub\n\n- #Other #1\n\n[[Nope]]\n",
            10,
            &NoResolver,
        )
        .unwrap();
        assert_eq!(tags, ["tag", "tag/sub", "other"]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].line, 15);
        assert!(read_body("", 0, &NoResolver).unwrap().0.is_empty());
    }

    #[test]
    fn single_block_text_reads_back_by_kind() {
        let read = |kind: &str, text: &str| parse_block_text(kind, text, &NoResolver);
        assert_eq!(
            read("paragraph", "hello"),
            Some(json!({ "inline": runs("hello") }))
        );
        assert_eq!(
            read("heading", "## T"),
            Some(json!({ "level": 2, "inline": runs("T") }))
        );
        assert_eq!(read("bullet", "- a"), Some(json!({ "inline": runs("a") })));
        assert_eq!(read("bullet", "a"), Some(json!({ "inline": runs("a") })));
        assert_eq!(
            read("numbered", "- a"),
            Some(json!({ "inline": runs("a") }))
        );
        assert_eq!(
            read("todo", "- a"),
            Some(json!({ "inline": runs("a"), "checked": false }))
        );
        assert_eq!(
            read("todo", "- [x] a"),
            Some(json!({ "inline": runs("a"), "checked": true }))
        );
        assert_eq!(read("quote", "> q"), Some(json!({ "inline": runs("q") })));
        assert_eq!(read("code", "```\nx\n```"), Some(json!({ "code": "x" })));
        assert_eq!(
            read("code", "raw\n\ntext"),
            Some(json!({ "code": "raw\n\ntext" }))
        );
        assert_eq!(read("paragraph", "a\n\nb"), None);
        assert_eq!(read("heading", "para"), None);
        assert_eq!(read("paragraph", "- a"), None);
        assert_eq!(read("divider", "***"), Some(json!({})));
    }

    #[test]
    fn normalize_puts_every_kind_into_normal_form() {
        let mut given = vec![
            plain(
                "heading",
                json!({ "level": 2, "inline": [{ "text": " a\nb " }] }),
            ),
            with(
                code("x\r\ny\n\n", Some("")),
                vec![code("z", Some("rust extra")), code("w\n", Some("a`b"))],
            ),
            plain(
                "paragraph",
                json!({ "inline": [{ "text": "a" }, { "text": "b" }] }),
            ),
            plain("divider", json!({})),
        ];
        let expected = vec![
            plain("heading", json!({ "level": 2, "inline": runs("a b") })),
            with(
                code("x\ny", None),
                vec![code("z", Some("rust")), code("w", None)],
            ),
            para("ab"),
            plain("divider", json!({})),
        ];
        normalize(&mut given);
        assert_eq!(given, expected);
        normalize(&mut given);
        assert_eq!(given, expected, "idempotent");
    }
}
