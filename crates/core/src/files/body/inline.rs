// SPDX-License-Identifier: AGPL-3.0-or-later
//! Inline runs ⇄ Markdown text (spec §4 and §7). [`write_inline`] renders a block's runs with
//! the escaping that keeps the parse unchanged; [`read_inline`] builds runs from a slice of
//! pulldown events and collects body tags; [`normalize_inline`] puts runs into the normal form the
//! round trip preserves.
//!
//! Marks are `**bold**`, `*italic*`, `<u>underline</u>`, `~~strike~~` and `` `code` ``, nested
//! in that order from the outside in. A delimiter that CommonMark would not read as an opener or a
//! closer where it stands (a `**` after a letter and before punctuation, two runs of the same
//! delimiter back to back) is written as `<b>`, `<i>` or `<s>` instead, and those tags are read
//! as marks like `<u>`; so every run renders, and the fixpoint holds for any content.
//!
//! The normal form adds one rule to spec §7 that the Markdown grammar forces: a marked run never
//! starts or ends with whitespace (CommonMark's flanking rules would not close `**a **`), so
//! [`normalize_inline`] moves such whitespace into a neighbouring unmarked run.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Tag, TagEnd};
use serde_json::{Map, Value};

use super::super::{LinkResolver, Warning};
use super::read::TagSink;
use crate::Id;

#[cfg(test)]
mod tests;

/// The marks of a text run, in nesting order from the outside in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Marks {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
}

impl Marks {
    /// Whether any delimiter- or tag-based mark (everything but `code`) is set.
    fn any_wrapping(self) -> bool {
        self.bold || self.italic || self.underline || self.strike
    }
}

/// A text run: `{"text", marks…, "link"?}`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TextRun {
    text: String,
    marks: Marks,
    link: Option<String>,
}

/// A mention run: `{"mention", "shown"?, "block"?, "embed"?}`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mention {
    id: Id,
    shown: Option<String>,
    block: Option<Id>,
    embed: bool,
}

/// One inline run, typed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Run {
    Text(TextRun),
    Mention(Mention),
}

impl Run {
    fn text(text: impl Into<String>, marks: Marks, link: Option<&str>) -> Self {
        Self::Text(TextRun {
            text: text.into(),
            marks,
            link: link.map(str::to_owned),
        })
    }

    /// Reads one JSON run leniently: marks that are not `true` are unset, a link or shown that is
    /// not a non-empty string is absent, a run with neither a string `text` nor a valid `mention`
    /// is `None`.
    fn from_value(value: &Value) -> Option<Self> {
        let map = value.as_object()?;
        let flag = |key: &str| map.get(key) == Some(&Value::Bool(true));
        let string = |key: &str| {
            map.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        if let Some(text) = map.get("text").and_then(Value::as_str) {
            return Some(Self::Text(TextRun {
                text: text.to_owned(),
                marks: Marks {
                    bold: flag("bold"),
                    italic: flag("italic"),
                    underline: flag("underline"),
                    strike: flag("strike"),
                    code: flag("code"),
                },
                link: string("link"),
            }));
        }
        let id = Id::parse(map.get("mention")?.as_str()?).ok()?;
        Some(Self::Mention(Mention {
            id,
            shown: string("shown"),
            block: map
                .get("block")
                .and_then(Value::as_str)
                .and_then(|b| Id::parse(b).ok()),
            embed: flag("embed"),
        }))
    }

    fn to_value(&self) -> Value {
        let mut map = Map::new();
        match self {
            Self::Text(run) => {
                map.insert("text".into(), Value::String(run.text.clone()));
                let m = run.marks;
                for (key, on) in [
                    ("bold", m.bold),
                    ("italic", m.italic),
                    ("underline", m.underline),
                    ("strike", m.strike),
                    ("code", m.code),
                ] {
                    if on {
                        map.insert(key.into(), Value::Bool(true));
                    }
                }
                if let Some(url) = &run.link {
                    map.insert("link".into(), Value::String(url.clone()));
                }
            }
            Self::Mention(mention) => {
                map.insert("mention".into(), Value::String(mention.id.to_string()));
                if let Some(shown) = &mention.shown {
                    map.insert("shown".into(), Value::String(shown.clone()));
                }
                if let Some(block) = mention.block {
                    map.insert("block".into(), Value::String(block.to_string()));
                }
                if mention.embed {
                    map.insert("embed".into(), Value::Bool(true));
                }
            }
        }
        Value::Object(map)
    }
}

/// Reads a JSON run list, dropping what is not a run.
fn runs_from_values(values: &[Value]) -> Vec<Run> {
    values.iter().filter_map(Run::from_value).collect()
}

fn runs_to_values(runs: &[Run]) -> Vec<Value> {
    runs.iter().map(Run::to_value).collect()
}

/// A space or tab: what the normal form strips from line edges.
fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Undoes the backslash escapes of a raw Markdown slice: `\` before ASCII punctuation is removed.
pub(super) fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(escaped) = chars.next_if(char::is_ascii_punctuation)
        {
            out.push(escaped);
            continue;
        }
        out.push(c);
    }
    out
}

/// The percent sequences the writer uses in a URL, decoded on read; anything else stays as it is.
fn decode_url(url: &str) -> String {
    let mut out = url.to_owned();
    for (code, plain) in [
        ("%20", " "),
        ("%28", "("),
        ("%29", ")"),
        ("%3C", "<"),
        ("%3E", ">"),
    ] {
        if out.contains(code) {
            out = out.replace(code, plain);
        }
    }
    out
}

/// Cleans a URL for the normal form: the writer's percent sequences decoded, control characters
/// dropped; `None` when nothing is left.
fn normal_url(url: &str) -> Option<String> {
    let decoded: String = decode_url(url)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    (!decoded.is_empty()).then_some(decoded)
}

/// Cleans a mention's `shown` for the normal form: no `]`, `|` or line break, not empty.
fn normal_shown(shown: &str) -> Option<String> {
    let cleaned: String = shown
        .chars()
        .filter(|c| !matches!(c, ']' | '|' | '\n' | '\r'))
        .collect();
    (!cleaned.is_empty()).then_some(cleaned)
}

// Normal form.

/// Puts a block's `inline` runs into normal form (see the module doc and spec §7). A content
/// without an `inline` array is left alone.
pub(super) fn normalize_inline(content: &mut Value) {
    if let Some(Value::Array(runs)) = content.get_mut("inline") {
        normalize_values(runs);
    }
}

/// [`normalize_inline`] on a bare run list.
pub(super) fn normalize_values(values: &mut Vec<Value>) {
    let mut runs = runs_from_values(values);
    normalize_runs(&mut runs);
    *values = runs_to_values(&runs);
}

/// The normal form on typed runs. Idempotent.
fn normalize_runs(runs: &mut Vec<Run>) {
    // Empty runs first: an empty code run must not count as content between two line breaks.
    runs.retain(|run| !matches!(run, Run::Text(t) if t.text.is_empty()));
    for run in runs.iter_mut() {
        match run {
            Run::Text(t) => {
                t.link = t.link.as_deref().and_then(normal_url);
                if t.marks.code && t.text.contains(['\n', '\r']) {
                    t.text = t.text.replace("\r\n", " ").replace(['\n', '\r'], " ");
                } else if t.text.contains('\r') {
                    // The serializer writes LF only, and CommonMark reads a lone CR as a line end.
                    t.text = t.text.replace("\r\n", "\n").replace('\r', "\n");
                }
            }
            Run::Mention(m) => m.shown = m.shown.as_deref().and_then(normal_shown),
        }
    }
    trim_line_starts(runs);
    trim_line_ends(runs);
    collapse_newlines(runs);
    runs.retain(|run| !matches!(run, Run::Text(t) if t.text.is_empty()));
    // Merge before moving whitespace out of marks: the edge of a run is not the edge of the mark
    // when the next run carries the same marks.
    merge_adjacent(runs);
    move_whitespace_out_of_marks(runs);
    merge_adjacent(runs);
}

/// Whether a run takes part in line trimming: plain and marked text, not code, not a mention.
fn is_soft(run: &Run) -> bool {
    matches!(run, Run::Text(t) if !t.marks.code)
}

/// Drops spaces and tabs at the start of every line, across runs.
fn trim_line_starts(runs: &mut [Run]) {
    let mut at_start = true;
    for run in runs.iter_mut() {
        if !is_soft(run) {
            at_start = false;
            continue;
        }
        let Run::Text(t) = run else { continue };
        let mut text = String::with_capacity(t.text.len());
        for c in t.text.chars() {
            if at_start && is_blank(c) {
                continue;
            }
            text.push(c);
            at_start = c == '\n';
        }
        t.text = text;
    }
}

/// Drops spaces and tabs at the end of every line, across runs.
fn trim_line_ends(runs: &mut [Run]) {
    let mut at_end = true;
    for run in runs.iter_mut().rev() {
        if !is_soft(run) {
            at_end = false;
            continue;
        }
        let Run::Text(t) = run else { continue };
        let mut kept: Vec<char> = Vec::with_capacity(t.text.len());
        for c in t.text.chars().rev() {
            if at_end && is_blank(c) {
                continue;
            }
            kept.push(c);
            at_end = c == '\n';
        }
        t.text = kept.into_iter().rev().collect();
    }
}

/// Collapses runs of `\n` to one and drops the block's leading and trailing line breaks.
fn collapse_newlines(runs: &mut [Run]) {
    let mut after_break = true;
    for run in runs.iter_mut() {
        if !is_soft(run) {
            after_break = false;
            continue;
        }
        let Run::Text(t) = run else { continue };
        let mut text = String::with_capacity(t.text.len());
        for c in t.text.chars() {
            if c == '\n' {
                if after_break {
                    continue;
                }
                after_break = true;
            } else {
                after_break = false;
            }
            text.push(c);
        }
        t.text = text;
    }
    let mut at_end = true;
    for run in runs.iter_mut().rev() {
        if !at_end {
            break;
        }
        if !is_soft(run) {
            at_end = false;
            continue;
        }
        let Run::Text(t) = run else { continue };
        let trimmed = t.text.trim_end_matches('\n');
        at_end = trimmed.is_empty();
        if trimmed.len() != t.text.len() {
            t.text = trimmed.to_owned();
        }
    }
}

/// Moves leading and trailing whitespace out of marked runs into unmarked neighbours that keep
/// only the link, so no delimiter touches whitespace.
fn move_whitespace_out_of_marks(runs: &mut Vec<Run>) {
    let mut out = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        let Run::Text(t) = &run else {
            out.push(run);
            continue;
        };
        if !t.marks.any_wrapping() || t.marks.code {
            out.push(run);
            continue;
        }
        let core = t.text.trim();
        if core.len() == t.text.len() {
            out.push(run);
            continue;
        }
        let link = t.link.as_deref();
        if core.is_empty() {
            // Only whitespace: the marks go, the text stays.
            out.push(Run::text(&t.text, Marks::default(), link));
            continue;
        }
        let lead = &t.text[..t.text.len() - t.text.trim_start().len()];
        let tail = &t.text[t.text.trim_end().len()..];
        if !lead.is_empty() {
            out.push(Run::text(lead, Marks::default(), link));
        }
        out.push(Run::text(core, t.marks, link));
        if !tail.is_empty() {
            out.push(Run::text(tail, Marks::default(), link));
        }
    }
    *runs = out;
}

/// Merges adjacent text runs with identical marks and link.
fn merge_adjacent(runs: &mut Vec<Run>) {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        if let (Some(Run::Text(last)), Run::Text(next)) = (out.last_mut(), &run)
            && last.marks == next.marks
            && last.link == next.link
        {
            last.text.push_str(&next.text);
            continue;
        }
        out.push(run);
    }
    *runs = out;
}

// Writing.

/// What a character is to CommonMark's flanking rules. The end of the text counts as whitespace;
/// anything that is neither whitespace nor alphanumeric counts as punctuation (a superset of
/// CommonMark's set, which only ever makes the writer fall back to a tag it did not need).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Whitespace,
    Punctuation,
    Alphanumeric,
}

fn class(c: Option<char>) -> Class {
    match c {
        None => Class::Whitespace,
        Some(c) if c.is_whitespace() => Class::Whitespace,
        Some(c) if c.is_alphanumeric() => Class::Alphanumeric,
        Some(_) => Class::Punctuation,
    }
}

/// A delimiter run between `before` and `after` can open emphasis.
fn left_flanking(before: Class, after: Class) -> bool {
    after != Class::Whitespace && (after != Class::Punctuation || before != Class::Alphanumeric)
}

/// A delimiter run between `before` and `after` can close emphasis.
fn right_flanking(before: Class, after: Class) -> bool {
    before != Class::Whitespace && (before != Class::Punctuation || after != Class::Alphanumeric)
}

/// Whether `rest` (the text after a `^`) is an anchor id up to the end of its line: exactly 26
/// ASCII characters, then a line break or the end. Constant time: `get(..26)` is `None` when
/// the text is shorter or byte 26 is no character boundary, and `anchor_id` wants all 26 ASCII,
/// so byte 26 being `\n` or the end is the whole "rest of the line" condition.
fn anchor_follows(rest: &str) -> bool {
    let Some(candidate) = rest.get(..26) else {
        return false;
    };
    matches!(rest.as_bytes().get(26), None | Some(b'\n')) && super::anchor_id(candidate).is_some()
}

/// Characters a backslash escapes wherever they stand in text.
fn always_escaped(c: char) -> bool {
    matches!(c, '\\' | '*' | '_' | '`' | '~' | '&')
}

/// Whether `<` at this point could start HTML or an autolink: before anything but whitespace or
/// the end (a tag or a URI scheme needs a letter, a comment `!`, a closing tag `/`, a processing
/// instruction `?`, but an email autolink `<1@a>` may start with any of its local part's
/// characters, so every non-blank follower counts).
fn lt_needs_escape(next: Option<char>) -> bool {
    next.is_some_and(|c| !c.is_whitespace())
}

/// Characters that open a block when they start a line (and `#`, `^` and digits, handled apart).
fn line_start_escaped(c: char) -> bool {
    matches!(c, '-' | '+' | '>' | '|' | '=' | ':')
}

/// Writes `text` escaped so that it reads back unchanged, and returns whether the output ends at
/// a line start. `in_link` also escapes brackets, which would end the link text.
fn escape_text(text: &str, at_line_start: bool, in_link: bool, out: &mut String) -> bool {
    let mut line_start = at_line_start;
    let mut escape_colon_at: Option<usize> = None;
    // The last `]]` of the run decides whether any `[[` before it is closed within the run: one
    // search, not one per bracket.
    let last_close = text.rfind("]]");
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        let next = iter.peek().map(|(_, c)| *c);
        if c == '\n' {
            out.push('\n');
            line_start = true;
            continue;
        }
        if escape_colon_at == Some(i) {
            out.push_str("\\:");
            escape_colon_at = None;
            line_start = false;
            continue;
        }
        // A `[[` whose `]]` is not in this run would swallow the runs after it into a wikilink;
        // one the run closes itself stays a wikilink for Obsidian (M5 Decision 10).
        let open_wikilink =
            c == '[' && next == Some('[') && !last_close.is_some_and(|close| close >= i + 2);
        let escape = always_escaped(c)
            || (c == '<' && lt_needs_escape(next))
            || (line_start && (c == '#' || line_start_escaped(c)))
            || (c == '^' && (line_start || anchor_follows(&text[i + 1..])))
            || (in_link && matches!(c, '[' | ']'))
            || open_wikilink;
        if escape {
            out.push('\\');
            out.push(c);
        } else if line_start && c.is_ascii_digit() {
            // `1.` or `1)` would open a numbered list: escape the punctuation after the digits.
            out.push(c);
            while let Some((_, d)) = iter.next_if(|(_, d)| d.is_ascii_digit()) {
                out.push(d);
            }
            if let Some((_, p)) = iter.next_if(|(_, p)| matches!(p, '.' | ')')) {
                out.push('\\');
                out.push(p);
            }
        } else {
            if line_start && c == '[' && !in_link {
                escape_colon_at = reference_definition_colon(text, i);
            }
            out.push(c);
        }
        line_start = false;
    }
    line_start
}

/// Whether `[` + `rendered` would be read as a link label followed by `:`: the first unescaped
/// `]` comes before any unescaped `[` and a `:` follows it.
fn reads_as_reference_label(rendered: &str) -> bool {
    let mut chars = rendered.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => return false,
            ']' => return chars.next() == Some(':'),
            _ => {}
        }
    }
    false
}

/// For a `[` at byte `start` of a line: the byte of the `:` of a `[label]:` that would be read
/// as a link reference definition, if the line has one.
fn reference_definition_colon(text: &str, start: usize) -> Option<usize> {
    let rest = &text[start + 1..];
    let close = rest.find([']', '[', '\n'])?;
    if rest.as_bytes()[close] != b']' || rest.as_bytes().get(close + 1) != Some(&b':') {
        return None;
    }
    Some(start + 1 + close + 1)
}

/// Writes a code span: a backtick fence longer than any run inside, padded with a space when the
/// text starts or ends with a backtick or would otherwise lose a space to CommonMark's stripping.
fn write_code_span(text: &str, out: &mut String) {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let bytes = text.as_bytes();
    let edge_tick = bytes.first() == Some(&b'`') || bytes.last() == Some(&b'`');
    let edge_space = bytes.first() == Some(&b' ')
        && bytes.last() == Some(&b' ')
        && !bytes.iter().all(|b| *b == b' ');
    let pad = if edge_tick || edge_space { " " } else { "" };
    out.push_str(&fence);
    out.push_str(pad);
    out.push_str(text);
    out.push_str(pad);
    out.push_str(&fence);
}

/// Writes a URL inside `(…)`: space and parentheses percent-encoded, `<` and `>` too (a
/// destination may not start with `<`), backslash and `&` backslash-escaped.
fn write_url(url: &str, out: &mut String) {
    for c in url.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' | '&' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
}

/// Escapes the text after `|` in a wikilink (a mention's `shown`, an image's `alt`): the
/// always-escaped set, `<` before a letter and `[`. The reader undoes it with [`unescape`].
pub(super) fn write_shown(shown: &str, out: &mut String) {
    let mut iter = shown.chars().peekable();
    while let Some(c) = iter.next() {
        if always_escaped(c) || c == '[' || (c == '<' && lt_needs_escape(iter.peek().copied())) {
            out.push('\\');
        }
        out.push(c);
    }
}

/// Writes a mention: `[[path]]`, `[[path|shown]]`, `[[path#^block]]`, `![[path]]` for an embed;
/// the ULID itself as the path when the object has no file.
fn write_mention(mention: &Mention, resolver: &dyn LinkResolver, out: &mut String) {
    if mention.embed {
        out.push('!');
    }
    out.push_str("[[");
    match resolver.path_of(mention.id) {
        Some(path) => out.push_str(&path),
        None => out.push_str(&mention.id.to_string()),
    }
    if let Some(block) = mention.block {
        out.push_str("#^");
        out.push_str(&block.to_string());
    }
    if let Some(shown) = &mention.shown {
        out.push('|');
        write_shown(shown, out);
    }
    out.push_str("]]");
}

/// The character a run's rendering starts with, as the flanking check of the run before it needs
/// it: `[` for a link (unless both are inside the same link) or mention, `!` for an embed, the
/// delimiter of the outermost mark (assumed, a tag fallback starts with `<`, punctuation as
/// well), the backtick of a code span, or the first text character (`\` when it is escaped).
fn first_rendered(run: &Run, in_link: bool) -> Option<char> {
    match run {
        Run::Mention(m) => Some(if m.embed { '!' } else { '[' }),
        Run::Text(t) => {
            if t.link.is_some() && !in_link {
                return Some('[');
            }
            let m = t.marks;
            if m.bold || m.italic {
                Some('*')
            } else if m.underline {
                Some('<')
            } else if m.strike {
                Some('~')
            } else if m.code {
                Some('`')
            } else {
                let c = t.text.chars().next()?;
                Some(if always_escaped(c) { '\\' } else { c })
            }
        }
    }
}

/// A wrapping mark: its delimiter (empty for underline, which is always a tag), its tag fallback
/// and the character the delimiter is made of.
const WRAPS: [(&str, &str, &str, char); 4] = [
    ("**", "<b>", "</b>", '*'),
    ("*", "<i>", "</i>", '*'),
    ("", "<u>", "</u>", '\0'),
    ("~~", "<s>", "</s>", '~'),
];

/// Writes one text run between `before` (the character already in `out`) and `after` (what the
/// next run starts with). Returns whether the output ends at a line start.
fn write_text_run(
    run: &TextRun,
    before: Option<char>,
    after: Option<char>,
    at_line_start: bool,
    in_link: bool,
    out: &mut String,
) -> bool {
    let m = run.marks;
    let mut content = String::with_capacity(run.text.len() + 8);
    let ends_at_line_start = if m.code {
        write_code_span(&run.text, &mut content);
        false
    } else {
        let content_line_start = at_line_start && !m.any_wrapping() && !in_link;
        escape_text(&run.text, content_line_start, in_link, &mut content)
    };
    let first = class(content.chars().next());
    let last = class(content.chars().next_back());
    let active: Vec<usize> = [m.bold, m.italic, m.underline, m.strike]
        .into_iter()
        .enumerate()
        .filter_map(|(k, on)| on.then_some(k))
        .collect();
    // A `!` right before the `[` of a link or mention would turn it into an image or an embed.
    if after == Some('[') && active.is_empty() && !m.code && content.ends_with('!') {
        content.pop();
        content.push_str("\\!");
    }
    let mut opens = Vec::with_capacity(active.len());
    let mut closes = Vec::with_capacity(active.len());
    for (k, wrap) in active.iter().enumerate() {
        let (delim, open_tag, close_tag, ch) = WRAPS[*wrap];
        let outermost = k == 0;
        let innermost = k + 1 == active.len() && !m.code;
        let (outer_before, outer_after) = if outermost {
            (class(before), class(after))
        } else {
            (Class::Punctuation, Class::Punctuation)
        };
        let (inner_open, inner_close) = if innermost {
            (first, last)
        } else {
            (Class::Punctuation, Class::Punctuation)
        };
        let use_delimiter = !delim.is_empty()
            && left_flanking(outer_before, inner_open)
            && right_flanking(inner_close, outer_after)
            && !(outermost && (before == Some(ch) || after == Some(ch)));
        if use_delimiter {
            opens.push(delim);
            closes.push(delim);
        } else {
            opens.push(open_tag);
            closes.push(close_tag);
        }
    }
    for open in &opens {
        out.push_str(open);
    }
    out.push_str(&content);
    for close in closes.iter().rev() {
        out.push_str(close);
    }
    ends_at_line_start && active.is_empty()
}

/// Writes a block's inline runs as Markdown text, after putting them into normal form. The last
/// character already in `out` is the flanking context of the first run (a marker such as `- ` or
/// `> ` ends in a space, which is right); `at_line_start` says whether the text starts a line, so
/// that `#`, `-`, `1.`, `>` and the like get escaped there.
pub(super) fn write_inline(
    runs: &[Value],
    resolver: &dyn LinkResolver,
    out: &mut String,
    at_line_start: bool,
) {
    let mut runs = runs_from_values(runs);
    normalize_runs(&mut runs);
    let mut line_start = at_line_start;
    let mut i = 0;
    while i < runs.len() {
        match &runs[i] {
            Run::Mention(mention) => {
                write_mention(mention, resolver, out);
                line_start = false;
                i += 1;
            }
            Run::Text(first) if first.link.is_some() => {
                let url = first.link.clone();
                let end = runs[i..]
                    .iter()
                    .position(|r| !matches!(r, Run::Text(t) if t.link == url))
                    .map_or(runs.len(), |p| i + p);
                let mut text = String::new();
                for k in i..end {
                    let Run::Text(run) = &runs[k] else { continue };
                    let after = if k + 1 < end {
                        first_rendered(&runs[k + 1], true)
                    } else {
                        Some(']')
                    };
                    let before = text.chars().next_back().or(Some('['));
                    write_text_run(run, before, after, false, true, &mut text);
                }
                // `[label]: dest` at the start of a line is a link reference definition, and a
                // code span inside the link text can hold the `]:` that makes one (`[` and `]`
                // outside code are escaped). A numeric space entity keeps the `[` off column 0;
                // the reader trims the space it decodes to.
                if line_start && reads_as_reference_label(&text) {
                    out.push_str("&#32;");
                }
                out.push('[');
                out.push_str(&text);
                out.push_str("](");
                write_url(url.as_deref().unwrap_or_default(), out);
                out.push(')');
                line_start = false;
                i = end;
            }
            Run::Text(run) => {
                let after = runs.get(i + 1).and_then(|r| first_rendered(r, false));
                let before = out.chars().next_back();
                line_start = write_text_run(run, before, after, line_start, false, out);
                i += 1;
            }
        }
    }
}

// Reading.

/// The 1-based line of byte `offset` in a text whose line starts are `line_starts` (the offset
/// of the first byte of every line, `line_starts[0] == 0`): a binary search, so a warning per
/// line costs `O(log lines)` and not a scan from the start.
pub(super) fn line_of(line_starts: &[usize], offset: usize) -> usize {
    line_starts.partition_point(|&start| start <= offset).max(1)
}

/// The state of one block's inline read.
struct Reader<'a> {
    source: &'a str,
    line_starts: &'a [usize],
    resolver: &'a dyn LinkResolver,
    warnings: &'a mut Vec<Warning>,
    tags: &'a mut TagSink,
    runs: Vec<Run>,
    marks: Marks,
    link: Option<String>,
}

impl Reader<'_> {
    fn warn(&mut self, offset: usize, message: String) {
        let line = line_of(self.line_starts, offset.min(self.source.len()));
        self.warnings.push(Warning { line, message });
    }

    /// Adds a text run with the current marks and link. Tags are collected once the runs are
    /// merged ([`read_inline`]): a backslash escape splits one text into several events, and
    /// `#beta\_1` is the one tag `beta_1`.
    fn text(&mut self, text: &str) {
        self.runs
            .push(Run::text(text, self.marks, self.link.as_deref()));
    }

    fn code(&mut self, text: &str) {
        let marks = Marks {
            code: true,
            ..self.marks
        };
        self.runs.push(Run::text(text, marks, self.link.as_deref()));
    }

    /// `<u>`, `<b>`, `<i>` and `<s>` (and their HTML synonyms) toggle marks; other HTML is text.
    fn html(&mut self, html: &str) {
        let toggle = match html {
            "<u>" => Some((true, 2)),
            "</u>" => Some((false, 2)),
            "<b>" | "<strong>" => Some((true, 0)),
            "</b>" | "</strong>" => Some((false, 0)),
            "<i>" | "<em>" => Some((true, 1)),
            "</i>" | "</em>" => Some((false, 1)),
            "<s>" | "<del>" => Some((true, 3)),
            "</s>" | "</del>" => Some((false, 3)),
            _ => None,
        };
        match toggle {
            Some((on, 0)) => self.marks.bold = on,
            Some((on, 1)) => self.marks.italic = on,
            Some((on, 2)) => self.marks.underline = on,
            Some((on, _)) => self.marks.strike = on,
            None => self.text(html),
        }
    }

    /// A `[[…]]` or `![[…]]`: a mention when the target names an object (a ULID directly, else
    /// through the resolver), otherwise its raw text, unescaped.
    fn wikilink(&mut self, embed: bool, has_pothole: bool, range: &Range<usize>) {
        let raw = &self.source[range.clone()];
        let inner = raw.strip_prefix('!').unwrap_or(raw);
        let inner = inner
            .strip_prefix("[[")
            .and_then(|s| s.strip_suffix("]]"))
            .unwrap_or(inner);
        let (target_raw, shown_raw) = if has_pothole {
            inner.split_once('|').unwrap_or((inner, ""))
        } else {
            (inner, "")
        };
        let target_full = unescape(target_raw);
        let (target, fragment) = match target_full.split_once('#') {
            Some((t, f)) => (t, Some(f)),
            None => (target_full.as_str(), None),
        };
        let mut block = None;
        if let Some(fragment) = fragment {
            match fragment.strip_prefix('^').and_then(super::anchor_id) {
                Some(id) => block = Some(id),
                None => self.warn(
                    range.start,
                    format!("[[{target}#{fragment}]]: the heading part is not kept"),
                ),
            }
        }
        let id = super::anchor_id(target).or_else(|| self.resolver.resolve(target));
        match id {
            Some(id) => {
                let shown = has_pothole
                    .then(|| normal_shown(&unescape(shown_raw)))
                    .flatten();
                self.runs.push(Run::Mention(Mention {
                    id,
                    shown,
                    block,
                    embed,
                }));
            }
            None => {
                self.warn(
                    range.start,
                    format!("{raw}: no object has this name; kept as text"),
                );
                let text = unescape(raw);
                self.text(&text);
            }
        }
    }
}

/// Collects `#tag`s: a `#` at the start of the text or after whitespace or `(`, followed by
/// `[A-Za-z0-9_/-]+` with at least one non-digit, lower-cased, each once.
fn collect_tags(text: &str, tags: &mut TagSink) {
    let bytes = text.as_bytes();
    let is_tag_byte = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'/' | b'-');
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }
        let before_ok = i == 0
            || text[..i]
                .chars()
                .next_back()
                .is_some_and(|p| p.is_whitespace() || p == '(');
        let mut end = i + 1;
        while end < bytes.len() && is_tag_byte(bytes[end]) {
            end += 1;
        }
        let name = &text[i + 1..end];
        if before_ok && !name.is_empty() && name.bytes().any(|b| !b.is_ascii_digit()) {
            tags.insert(name.to_ascii_lowercase());
        }
        i = end.max(i + 1);
    }
}

/// The index of the `End` that closes the `Start` at `start`, or the last index.
fn matching_end(events: &[(Event<'_>, Range<usize>)], start: usize) -> usize {
    let mut depth = 0usize;
    for (j, (event, _)) in events.iter().enumerate().skip(start) {
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
    events.len().saturating_sub(1)
}

/// Builds the runs of one block from its inline events (those between the block's `Start` and
/// `End`), in normal form. `source` is the text the ranges index and `line_starts` its line
/// table (see [`line_of`]); a warning's `line` is the 1-based line within it, and the caller adds
/// the body's offset in the file. Body tags found in plain text are appended to `tags`,
/// lower-cased, each once.
pub(super) fn read_inline(
    events: &[(Event<'_>, Range<usize>)],
    source: &str,
    line_starts: &[usize],
    resolver: &dyn LinkResolver,
    warnings: &mut Vec<Warning>,
    tags: &mut TagSink,
) -> Vec<Value> {
    let mut reader = Reader {
        source,
        line_starts,
        resolver,
        warnings,
        tags,
        runs: Vec::new(),
        marks: Marks::default(),
        link: None,
    };
    let mut i = 0;
    while i < events.len() {
        let (event, range) = &events[i];
        match event {
            Event::Text(text) => reader.text(text),
            Event::Code(text) => reader.code(text),
            Event::SoftBreak | Event::HardBreak => reader.text("\n"),
            Event::Start(Tag::Strong) => reader.marks.bold = true,
            Event::End(TagEnd::Strong) => reader.marks.bold = false,
            Event::Start(Tag::Emphasis) => reader.marks.italic = true,
            Event::End(TagEnd::Emphasis) => reader.marks.italic = false,
            Event::Start(Tag::Strikethrough) => reader.marks.strike = true,
            Event::End(TagEnd::Strikethrough) => reader.marks.strike = false,
            Event::InlineHtml(html) => reader.html(html),
            Event::Html(html) => reader.text(html),
            Event::Start(
                Tag::Link {
                    link_type: LinkType::WikiLink { has_pothole },
                    ..
                }
                | Tag::Image {
                    link_type: LinkType::WikiLink { has_pothole },
                    ..
                },
            ) => {
                let embed = matches!(event, Event::Start(Tag::Image { .. }));
                reader.wikilink(embed, *has_pothole, range);
                i = matching_end(events, i);
            }
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                reader.link = Some(decode_url(dest_url));
            }
            Event::End(TagEnd::Link | TagEnd::Image) => reader.link = None,
            Event::FootnoteReference(_)
            | Event::TaskListMarker(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {
                let raw = &source[range.clone()];
                reader.text(raw);
            }
            Event::Start(_) | Event::End(_) | Event::Rule => {}
        }
        i += 1;
    }
    normalize_runs(&mut reader.runs);
    for run in &reader.runs {
        if let Run::Text(t) = run
            && !t.marks.code
            && t.link.is_none()
        {
            collect_tags(&t.text, reader.tags);
        }
    }
    runs_to_values(&reader.runs)
}
