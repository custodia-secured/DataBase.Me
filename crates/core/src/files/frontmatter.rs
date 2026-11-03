// SPDX-License-Identifier: AGPL-3.0-or-later
//! Frontmatter ⇄ YAML (saphyr, ADR-0023): fence detection, the pre-scan caps, kind-driven decoding
//! of user fields and the emitter.
//!
//! Reading happens in three steps. [`split`] finds the fences with a line scanner. [`prescan`]
//! refuses, before the YAML crate sees a byte, a block over [`MAX_FRONTMATTER_BYTES`], nested
//! deeper than [`MAX_YAML_DEPTH`] (indent levels plus `- ` runs plus unclosed flow brackets) or
//! holding a second document, because the crate has no size cap and block nesting past a few
//! hundred levels overflows the stack. Then `MarkedYaml` loads the text and the mapping is read
//! into a [`Frontmatter`]: every node carries a span, so a scalar whose Core Schema value lost its
//! spelling (`0755` is `Integer(755)`, `1e3` is `Real`) is decoded from the source slice when its
//! field kind wants the text. Nothing is lost: a key that does not fit its slot goes to `extra`
//! with a [`Warning`].
//!
//! Writing builds a `Yaml` tree and lets `YamlEmitter` quote what a reader could misread; this
//! module never quotes a value itself. The emitter's leading `---` line is stripped and the fences
//! are written here. Timestamps are formatted and parsed with civil-date arithmetic, no clock and
//! no chrono.

use std::borrow::Cow;

use saphyr::{
    LoadableYamlNode, Mapping, MarkedYaml, Scalar, ScalarStyle, ScanError, Yaml, YamlData,
    YamlEmitter,
};
use serde_json::{Map, Number, Value};

use super::{
    Frontmatter, LinkEntry, MAX_FRONTMATTER_BYTES, MAX_YAML_DEPTH, ParseError, RESERVED_KEYS,
    RelationRef, Schema, Warning, WikiLink,
};
use crate::model::{FieldKind, Props, Sensitivity, is_identifier};
use crate::{Id, SortKey};

/// The `rel_type` a `links` entry gets when its `type` is missing.
const DEFAULT_REL_TYPE: &str = "related";

/// Splits a file into the YAML between the fences (if any), the body after the closing fence,
/// and the 0-based line index where the body starts. The file has frontmatter only when it starts
/// with `---\n` or `---\r\n` at offset 0 and a later line is exactly `---` or `...`; `---`
/// followed by a blank line (or by nothing) is a body. CRLF is accepted. An unterminated fence is
/// not a fence: the whole text is body, as pulldown reads it (ADR-0022 rule 1: a leading `---`
/// with no closer is a `Rule`, so the body opens with a divider).
pub(crate) fn split(text: &str) -> (Option<&str>, &str, usize) {
    let Some(after_open) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (None, text, 0);
    };
    let first_line = after_open.split('\n').next().unwrap_or("");
    if first_line.trim().is_empty() {
        // `---` then a blank line (or the end of the file) is a thematic break, not a fence.
        return (None, text, 0);
    }
    let yaml_start = text.len() - after_open.len();
    let mut pos = yaml_start;
    // The opening fence is line index 0; the first YAML line is index 1.
    let mut line_index = 1;
    while pos < text.len() {
        let rest = &text[pos..];
        let (line, next) = match rest.find('\n') {
            Some(i) => (&rest[..i], pos + i + 1),
            None => (rest, text.len()),
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line == "---" || line == "..." {
            return (Some(&text[yaml_start..pos]), &text[next..], line_index + 1);
        }
        pos = next;
        line_index += 1;
    }
    (None, text, 0)
}

/// Reads the YAML between the fences into an envelope: the pre-scan caps (size, depth, one
/// document), then `MarkedYaml`, then the mapping to [`Frontmatter`] with user fields decoded by
/// kind through `schema`. An empty block is the empty envelope.
///
/// Positions are file lines: the YAML starts on line 2, after the opening fence.
pub(crate) fn parse(
    yaml: &str,
    schema: &Schema,
) -> Result<(Frontmatter, Vec<Warning>), ParseError> {
    if yaml.len() > MAX_FRONTMATTER_BYTES {
        return Err(ParseError {
            line: 1,
            col: 0,
            message: format!(
                "the frontmatter is {} bytes, more than the {MAX_FRONTMATTER_BYTES} allowed",
                yaml.len()
            ),
        });
    }
    let text: Cow<'_, str> = if yaml.contains('\r') {
        let text = yaml.replace("\r\n", "\n");
        if let Some(at) = text.find('\r') {
            // The YAML crate breaks a line at a lone CR too, so the pre-scan (which counts `\n`
            // like the fences and the body do) would measure a different text than it loads.
            let line = 2 + text[..at].matches('\n').count();
            return Err(ParseError {
                line,
                col: 0,
                message: "a bare carriage return in the frontmatter; use LF or CRLF line endings"
                    .to_owned(),
            });
        }
        Cow::Owned(text)
    } else {
        Cow::Borrowed(yaml)
    };
    prescan(&text)?;
    let source = Source::new(&text);
    let docs = MarkedYaml::load_from_str(&text).map_err(|e| scan_error(e, &source))?;
    if let Some(second) = docs.get(1) {
        let (line, col) = source.position(second.span.start.index());
        return Err(ParseError {
            line,
            col,
            message: "a second YAML document; the frontmatter holds exactly one".to_owned(),
        });
    }
    let Some(root) = docs.first() else {
        return Ok((Frontmatter::default(), Vec::new()));
    };
    let root = untag(root);
    let mapping = match &root.data {
        YamlData::Mapping(mapping) => mapping,
        YamlData::Value(Scalar::Null) => return Ok((Frontmatter::default(), Vec::new())),
        _ => {
            let (line, col) = source.position(root.span.start.index());
            return Err(ParseError {
                line,
                col,
                message: "the frontmatter must be a mapping of keys to values".to_owned(),
            });
        }
    };
    let mut reader = Reader {
        source,
        schema,
        types: Vec::new(),
        warnings: Vec::new(),
    };
    let front = reader.envelope(root, mapping);
    Ok((front, reader.warnings))
}

fn scan_error(error: ScanError, source: &Source<'_>) -> ParseError {
    let (line, col) = source.position(error.marker().index());
    ParseError {
        line,
        col,
        message: error.info().to_owned(),
    }
}

/// Follows `Tagged` wrappers (`!!str 5`) down to the node they decorate.
fn untag<'n, 'a>(mut node: &'n MarkedYaml<'a>) -> &'n MarkedYaml<'a> {
    while let YamlData::Tagged(_, inner) = &node.data {
        node = inner.as_ref();
    }
    node
}

// ---------------------------------------------------------------------------------------------
// Pre-scan
// ---------------------------------------------------------------------------------------------

/// What one open block collection is to the pre-scan: a mapping or a sequence at a column.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Mapping,
    Sequence,
}

/// Refuses, line by line and before the YAML crate sees the text, a nesting deeper than
/// [`MAX_YAML_DEPTH`] and a `---` or `...` document marker inside the block.
///
/// The nesting of a line is the number of block collections open at its deepest node plus the
/// deepest run of unclosed `[` and `{` reached on it. Block collections are tracked as a stack of
/// (column, kind): a line indented deeper than the top opens one, a line indented less closes
/// down to its column, a `- ` at a mapping's own column opens the compact sequence form, and each
/// further `- ` run and a `key:` after a run on the same line open one more (`- - k:` is three).
/// Flow brackets count when they start a value or an item, after any `!tag`, `&anchor` or
/// `*alias`, or when they start a line of their own (a value on the line after its key). Lines of
/// a block scalar (`|`, `>`) are content, not structure, and are skipped; quoted scalars, comments
/// and brackets inside a plain scalar do not count.
fn prescan(yaml: &str) -> Result<(), ParseError> {
    let mut open_flow = 0usize;
    // The indentation of the line that opened a block scalar, while inside one.
    let mut block_scalar: Option<usize> = None;
    let mut stack: Vec<(usize, Block)> = Vec::new();
    for (i, raw_line) in yaml.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let file_line = i + 2;
        let indent = line.len() - line.trim_start_matches(' ').len();
        let content = &line[indent..];
        if let Some(owner) = block_scalar {
            if content.trim().is_empty() || indent > owner {
                continue;
            }
            block_scalar = None;
        }
        if content.trim().is_empty() || content.starts_with('#') {
            continue;
        }
        if indent == 0 && open_flow == 0 && is_document_marker(content) {
            return Err(ParseError {
                line: file_line,
                col: 0,
                message: "a second YAML document begins here; the frontmatter holds exactly one"
                    .to_owned(),
            });
        }
        if open_flow > 0 {
            // A flow collection continued from an earlier line: the whole line is flow text.
            let (after, peak) = flow_depth_after(content, open_flow);
            let depth = stack.len() + peak;
            if depth > MAX_YAML_DEPTH {
                return Err(too_deep(file_line, indent, depth));
            }
            open_flow = after;
            continue;
        }
        // `- ` runs: each opens a sequence at its own column.
        let mut col = indent;
        let mut rest = content;
        let mut dash_cols: Vec<usize> = Vec::new();
        loop {
            match rest.strip_prefix('-') {
                Some("") => {
                    dash_cols.push(col);
                    rest = "";
                    break;
                }
                Some(after) if after.starts_with([' ', '\t']) => {
                    dash_cols.push(col);
                    let trimmed = after.trim_start_matches([' ', '\t']);
                    col += 1 + (after.len() - trimmed.len());
                    rest = trimmed;
                }
                _ => break,
            }
        }
        let dashes = dash_cols.len();
        // A `? ` complex key opens nothing more; what follows is the key, read like a value.
        let (complex_key, head) = match rest.strip_prefix('?') {
            Some(after) if after.is_empty() || after.starts_with([' ', '\t']) => {
                (true, after.trim_start_matches([' ', '\t']))
            }
            _ => (false, rest),
        };
        // A head that opens a flow collection is a flow node (counted by its brackets below, a
        // `{a: 1}` item included), not a block mapping entry.
        let found = if complex_key || strip_node_properties(head).starts_with(['[', '{']) {
            None
        } else {
            key_value(head)
        };
        let is_key = complex_key || found.is_some();
        let value = found.unwrap_or(head);
        let line_kind = if dashes > 0 {
            Block::Sequence
        } else if is_key {
            Block::Mapping
        } else {
            // A scalar continuation or a flow value on its own line: no block collection.
            while stack.last().is_some_and(|(c, _)| *c > indent) {
                stack.pop();
            }
            let flow = strip_node_properties(head);
            let depth = stack.len() + flow_peak(flow);
            if depth > MAX_YAML_DEPTH {
                return Err(too_deep(file_line, indent, depth));
            }
            if flow.starts_with(['[', '{']) {
                open_flow = flow_depth_after(flow, 0).0;
            }
            continue;
        };
        // Close what this line's indentation leaves behind, then open or continue at `indent`.
        while stack.last().is_some_and(|(c, _)| *c > indent) {
            stack.pop();
        }
        match stack.last().copied() {
            Some((c, kind)) if c == indent && kind == line_kind => {}
            Some((c, Block::Sequence)) if c == indent && line_kind == Block::Mapping => {
                // The compact sequence under a key at the same column has ended.
                stack.pop();
                if stack.last() != Some(&(indent, Block::Mapping)) {
                    stack.push((indent, Block::Mapping));
                }
            }
            _ => stack.push((indent, line_kind)),
        }
        // Further runs on the line, and the mapping a `key:` after a run opens.
        for dash_col in dash_cols.iter().skip(1) {
            stack.push((*dash_col, Block::Sequence));
        }
        if dashes > 0 && is_key {
            stack.push((col, Block::Mapping));
        }
        let value = strip_node_properties(value);
        if value.starts_with(['|', '>']) {
            block_scalar = Some(indent);
            if stack.len() > MAX_YAML_DEPTH {
                return Err(too_deep(file_line, indent, stack.len()));
            }
            continue;
        }
        // Brackets that open the key (`[a]: v`) and the value both nest; the peak within the
        // line counts, not what is still open at its end: a `[[[…x…]]]` closed on its own line
        // nests just as deep for the YAML crate.
        let key_text = strip_node_properties(head);
        let flow = if key_text.starts_with(['[', '{']) {
            key_text
        } else if value.starts_with(['[', '{']) {
            value
        } else {
            ""
        };
        let (after, peak) = flow_depth_after(flow, 0);
        let depth = stack.len() + peak;
        if depth > MAX_YAML_DEPTH {
            return Err(too_deep(file_line, indent, depth));
        }
        open_flow = after;
    }
    Ok(())
}

/// Drops the `!tag`, `&anchor` and `*alias` tokens in front of a value.
fn strip_node_properties(mut text: &str) -> &str {
    while text.starts_with(['!', '&', '*']) {
        let end = text.find([' ', '\t']).unwrap_or(text.len());
        text = text[end..].trim_start_matches([' ', '\t']);
    }
    text
}

/// The deepest run of unclosed brackets within flow text, or 0 for anything else.
fn flow_peak(text: &str) -> usize {
    if text.starts_with(['[', '{']) {
        flow_depth_after(text, 0).1
    } else {
        0
    }
}

fn too_deep(line: usize, col: usize, depth: usize) -> ParseError {
    ParseError {
        line,
        col,
        message: format!("nested {depth} levels deep, more than the {MAX_YAML_DEPTH} allowed"),
    }
}

/// `---` or `...` followed by whitespace or the end of the line, at column 0.
fn is_document_marker(content: &str) -> bool {
    (content.starts_with("---") || content.starts_with("..."))
        && content[3..].chars().next().is_none_or(char::is_whitespace)
}

/// The value part of a `key: value` line: what follows the first `: ` (or `:` before a tab or
/// the end) outside a quoted key, trimmed. `None` when the text holds no key.
fn key_value(rest: &str) -> Option<&str> {
    let bytes = rest.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' && q == b'"' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' if i == 0 => quote = Some(b),
                b'#' if i == 0 || bytes[i - 1] == b' ' => return None,
                b':' if i + 1 == bytes.len() || matches!(bytes[i + 1], b' ' | b'\t') => {
                    return Some(rest[i + 1..].trim_start());
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// The count of unclosed `[` and `{` after `text`, starting from `open`, and the deepest count
/// reached within it. Quoted scalars and comments are skipped; closers never take the count
/// below zero.
fn flow_depth_after(text: &str, open: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut depth = open;
    let mut peak = open;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' && q == b'"' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'#' if i == 0 || bytes[i - 1] == b' ' => break,
                b'[' | b'{' => {
                    depth += 1;
                    peak = peak.max(depth);
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            },
        }
        i += 1;
    }
    (depth, peak)
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

/// The YAML text with a map from the parser's character indices to byte offsets, for recovering
/// the raw spelling of a node from its span, and the character index of every line start, for
/// positions.
struct Source<'a> {
    text: &'a str,
    /// Byte offset of every character index, plus the end; `None` when the text is ASCII and the
    /// two coincide.
    bytes: Option<Vec<usize>>,
    /// The character index of the first character of every line; `line_starts[0] == 0`.
    line_starts: Vec<usize>,
}

impl<'a> Source<'a> {
    fn new(text: &'a str) -> Self {
        let bytes = if text.is_ascii() {
            None
        } else {
            let mut offsets: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
            offsets.push(text.len());
            Some(offsets)
        };
        let mut line_starts = vec![0];
        line_starts.extend(
            text.chars()
                .enumerate()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i + 1),
        );
        Self {
            text,
            bytes,
            line_starts,
        }
    }

    /// The file line (1-based; the YAML starts on line 2) and the 0-based column, in characters,
    /// of the YAML character index `index`. Counted by `\n` alone, like the fences and the body;
    /// a lone `\r` never reaches here (see [`parse`]). Binary search, so a warning per line costs
    /// `O(log lines)` and not a scan from the start.
    fn position(&self, index: usize) -> (usize, usize) {
        let line = self
            .line_starts
            .partition_point(|&start| start <= index)
            .saturating_sub(1);
        let start = self.line_starts.get(line).copied().unwrap_or(0);
        (line + 2, index.saturating_sub(start))
    }

    /// The source slice under `node`, trimmed.
    fn raw(&self, node: &MarkedYaml<'_>) -> &'a str {
        let to_byte = |index: usize| match &self.bytes {
            None => index.min(self.text.len()),
            Some(offsets) => offsets.get(index).copied().unwrap_or(self.text.len()),
        };
        let start = to_byte(node.span.start.index());
        let end = to_byte(node.span.end.index());
        if start > end {
            return "";
        }
        self.text.get(start..end).unwrap_or("").trim()
    }
}

/// Reads the loaded mapping into an envelope, collecting warnings.
struct Reader<'a, 's> {
    source: Source<'a>,
    schema: &'s Schema,
    /// The object's type names, read first so user fields can be decoded by kind.
    types: Vec<String>,
    warnings: Vec<Warning>,
}

impl<'a> Reader<'a, '_> {
    /// The file line a node starts on.
    fn line(&self, node: &MarkedYaml<'a>) -> usize {
        self.source.position(node.span.start.index()).0
    }

    fn warn(&mut self, node: &MarkedYaml<'a>, message: String) {
        let line = self.line(node);
        self.warnings.push(Warning { line, message });
    }

    /// Keeps a value that does not fit its envelope slot as an extra key, with a warning.
    fn keep(&mut self, front: &mut Frontmatter, key: String, node: &MarkedYaml<'a>, why: &str) {
        self.warn(node, format!("`{key}` {why}; kept as an extra key"));
        front.extra.push((key, self.to_value(node)));
    }

    fn raw(&self, node: &MarkedYaml<'a>) -> &'a str {
        self.source.raw(node)
    }

    /// A mapping key as text: the string, or the raw spelling of anything else.
    fn key_text(&self, key: &MarkedYaml<'a>) -> String {
        let key = untag(key);
        match &key.data {
            YamlData::Value(Scalar::String(s)) => s.as_ref().to_owned(),
            _ => self.raw(key).to_owned(),
        }
    }

    /// A scalar as text: a string as it is, any other scalar by its raw spelling, `null` as
    /// nothing. Collections are nothing.
    fn scalar_str(&self, node: &MarkedYaml<'a>) -> Option<String> {
        let node = untag(node);
        match &node.data {
            YamlData::Value(Scalar::String(s)) => Some(s.as_ref().to_owned()),
            YamlData::Value(Scalar::Null) => None,
            YamlData::Value(_) | YamlData::Representation(..) => Some(self.raw(node).to_owned()),
            _ => None,
        }
    }

    /// [`Self::scalar_str`], plus an unquoted `[[Title]]`, which YAML reads as a sequence holding
    /// a sequence holding one scalar, unwrapped to the string `[[Title]]` (gate 1c, probe 10).
    fn scalar_or_wikilink(&self, node: &MarkedYaml<'a>) -> Option<String> {
        if let Some(text) = self.scalar_str(node) {
            return Some(text);
        }
        let outer = untag(node).data.as_sequence()?;
        if let [inner] = outer.as_slice()
            && let Some([leaf]) = untag(inner).data.as_sequence().map(Vec::as_slice)
        {
            return self.scalar_str(leaf).map(|s| format!("[[{s}]]"));
        }
        None
    }

    /// A sequence of scalars (nulls skipped), or one scalar, as strings; `null` is the empty
    /// list. `None` when an item is a collection.
    fn string_list(&self, node: &MarkedYaml<'a>) -> Option<Vec<String>> {
        let node = untag(node);
        match &node.data {
            YamlData::Sequence(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let item = untag(item);
                    if item.data.is_null() {
                        continue;
                    }
                    out.push(self.scalar_str(item)?);
                }
                Some(out)
            }
            YamlData::Value(Scalar::Null) => Some(Vec::new()),
            _ => self.scalar_str(node).map(|s| vec![s]),
        }
    }

    /// Generic decoding of any node: string → string, integer → number, real → number, bool →
    /// bool, null → null, sequence → array, mapping → object; anything else by its raw spelling.
    fn to_value(&self, node: &MarkedYaml<'a>) -> Value {
        let node = untag(node);
        match &node.data {
            YamlData::Value(Scalar::Null) => Value::Null,
            YamlData::Value(Scalar::Boolean(b)) => Value::Bool(*b),
            YamlData::Value(Scalar::Integer(i)) => Value::from(*i),
            YamlData::Value(Scalar::FloatingPoint(f)) => Number::from_f64(**f)
                .map_or_else(|| Value::String(self.raw(node).to_owned()), Value::Number),
            YamlData::Value(Scalar::String(s)) => Value::String(s.as_ref().to_owned()),
            YamlData::Sequence(items) => {
                Value::Array(items.iter().map(|item| self.to_value(item)).collect())
            }
            YamlData::Mapping(mapping) => Value::Object(
                mapping
                    .iter()
                    .map(|(k, v)| (self.key_text(k), self.to_value(v)))
                    .collect::<Map<String, Value>>(),
            ),
            YamlData::Tagged(_, inner) => self.to_value(inner),
            YamlData::Representation(..) | YamlData::Alias(_) | YamlData::BadValue => {
                Value::String(self.raw(node).to_owned())
            }
        }
    }

    /// Generic decoding of a field value: `null` means the field is absent.
    fn generic(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        if node.data.is_null() {
            None
        } else {
            Some(self.to_value(node))
        }
    }

    /// A `number` field: the raw spelling when JSON accepts it (`1e3`, `12`, `-2.5`), else the
    /// Core Schema value (`0755` is 755), else the text as it was.
    fn number(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        let YamlData::Value(scalar) = &node.data else {
            return self.generic(node);
        };
        let raw: &str = match scalar {
            Scalar::Null => return None,
            Scalar::String(s) => s.as_ref(),
            _ => self.raw(node),
        };
        if let Ok(n) = raw.parse::<Number>() {
            return Some(Value::Number(n));
        }
        Some(match scalar {
            Scalar::Integer(i) => Value::from(*i),
            Scalar::FloatingPoint(f) => {
                Number::from_f64(**f).map_or_else(|| Value::String(raw.to_owned()), Value::Number)
            }
            _ => Value::String(raw.to_owned()),
        })
    }

    /// A `duration` or `rating` field: an integer, also from an integral real or a string.
    fn integer(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        match &node.data {
            YamlData::Value(Scalar::Null) => None,
            YamlData::Value(Scalar::Integer(i)) => Some(Value::from(*i)),
            YamlData::Value(Scalar::FloatingPoint(f))
                if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 =>
            {
                Some(Value::from(**f as i64))
            }
            YamlData::Value(Scalar::String(s)) => Some(
                s.trim()
                    .parse::<i64>()
                    .map_or_else(|_| Value::String(s.as_ref().to_owned()), Value::from),
            ),
            _ => self.generic(node),
        }
    }

    /// A `boolean` field: a boolean, or the strings `true` and `false`.
    fn boolean(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        match &node.data {
            YamlData::Value(Scalar::Boolean(b)) => Some(Value::Bool(*b)),
            YamlData::Value(Scalar::String(s)) if s == "true" => Some(Value::Bool(true)),
            YamlData::Value(Scalar::String(s)) if s == "false" => Some(Value::Bool(false)),
            _ => self.generic(node),
        }
    }

    /// A number as `f64`, for `geo_lat` and `geo_lon`.
    fn f64_of(&self, node: &MarkedYaml<'a>) -> Option<f64> {
        let node = untag(node);
        match &node.data {
            YamlData::Value(Scalar::Integer(i)) => Some(*i as f64),
            YamlData::Value(Scalar::FloatingPoint(f)) => Some(**f),
            YamlData::Value(Scalar::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// A `currency` field: `{amount, currency}` with the amount kept as text (`12.50` stays
    /// `"12.50"`).
    fn currency(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        let Some(mapping) = node.data.as_mapping() else {
            return self.generic(node);
        };
        let mut out = Map::new();
        for (k, v) in mapping {
            let key = self.key_text(k);
            let value = if key == "amount" {
                self.scalar_str(v)
                    .map_or_else(|| self.to_value(v), Value::String)
            } else {
                self.to_value(v)
            };
            out.insert(key, value);
        }
        Some(Value::Object(out))
    }

    /// A `geo` field: `{lat, lon}` with numbers.
    fn geo(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        let Some(mapping) = node.data.as_mapping() else {
            return self.generic(node);
        };
        let mut out = Map::new();
        for (k, v) in mapping {
            if let Some(value) = self.number(v) {
                out.insert(self.key_text(k), value);
            }
        }
        Some(Value::Object(out))
    }

    /// A `multi_select` or `file` field: an array of strings; one scalar is a one-item array.
    fn string_array(&self, node: &MarkedYaml<'a>) -> Option<Value> {
        let node = untag(node);
        match &node.data {
            YamlData::Sequence(items) => Some(Value::Array(
                items
                    .iter()
                    .map(|item| {
                        self.scalar_str(item)
                            .map_or_else(|| self.to_value(item), Value::String)
                    })
                    .collect(),
            )),
            YamlData::Value(Scalar::Null) => None,
            YamlData::Value(_) => self
                .scalar_str(node)
                .map(|s| Value::Array(vec![Value::String(s)])),
            _ => self.generic(node),
        }
    }

    /// Decodes a user field's value by its kind (FORMAT.md §3.3), generically when the schema
    /// has no field for the key. `None` means the field is absent.
    fn field_value(&self, node: &MarkedYaml<'a>, kind: Option<FieldKind>) -> Option<Value> {
        match kind {
            Some(
                FieldKind::Text
                | FieldKind::Url
                | FieldKind::Email
                | FieldKind::Phone
                | FieldKind::Select
                | FieldKind::Date
                | FieldKind::Datetime,
            ) => self
                .scalar_or_wikilink(node)
                .map(Value::String)
                .or_else(|| self.generic(node)),
            Some(FieldKind::Number) => self.number(node),
            Some(FieldKind::Boolean) => self.boolean(node),
            Some(FieldKind::Duration | FieldKind::Rating) => self.integer(node),
            Some(FieldKind::Currency) => self.currency(node),
            Some(FieldKind::Geo) => self.geo(node),
            Some(FieldKind::MultiSelect | FieldKind::File) => self.string_array(node),
            Some(
                FieldKind::RichText
                | FieldKind::Json
                | FieldKind::Relation
                | FieldKind::Formula
                | FieldKind::Rollup,
            )
            | None => self.generic(node),
        }
    }

    /// A ULID in a scalar, with a warning when the text is not one.
    fn id_of(&mut self, node: &MarkedYaml<'a>) -> Option<Id> {
        let text = self.scalar_str(node)?;
        match Id::parse(&text) {
            Ok(id) => Some(id),
            Err(_) => {
                self.warn(node, format!("`{text}` is not a ULID; ignored"));
                None
            }
        }
    }

    /// Takes a `[[target#^block|shown]]` apart. A `#heading` part is dropped with a warning.
    fn wikilink(&mut self, text: &str, node: &MarkedYaml<'a>) -> WikiLink {
        let inner = text
            .strip_prefix("[[")
            .and_then(|t| t.strip_suffix("]]"))
            .unwrap_or(text);
        let (head, shown) = match inner.split_once('|') {
            Some((head, shown)) => (head, Some(shown)),
            None => (inner, None),
        };
        let (target, block) = match head.split_once('#') {
            Some((target, fragment)) => {
                let block = fragment.strip_prefix('^').and_then(|b| Id::parse(b).ok());
                if block.is_none() {
                    self.warn(
                        node,
                        format!("the `#{fragment}` part of `{text}` names no block; dropped"),
                    );
                }
                (target, block)
            }
            None => (head, None),
        };
        WikiLink {
            target: target.to_owned(),
            block,
            shown: shown.filter(|s| !s.is_empty()).map(str::to_owned),
        }
    }

    /// One `relations` entry: `{id, link}`, or a bare ULID, or a bare wikilink.
    fn relation_ref(&mut self, node: &MarkedYaml<'a>) -> Option<RelationRef> {
        let node = untag(node);
        if let Some(mapping) = node.data.as_mapping() {
            let mut entry = RelationRef::default();
            for (k, v) in mapping {
                let key = self.key_text(k);
                match key.as_str() {
                    "id" => entry.id = self.id_of(v),
                    "link" => {
                        entry.link = self
                            .scalar_or_wikilink(v)
                            .map(|link_text| self.wikilink(&link_text, v));
                    }
                    _ => self.warn(k, format!("`{key}` is not part of a relation; ignored")),
                }
            }
            return Some(entry);
        }
        let Some(text) = self.scalar_or_wikilink(node) else {
            self.warn(
                node,
                "a relation must be a mapping of `id` and `link`; ignored".into(),
            );
            return None;
        };
        Some(match Id::parse(&text) {
            Ok(id) => RelationRef {
                id: Some(id),
                link: None,
            },
            Err(_) => RelationRef {
                id: None,
                link: Some(self.wikilink(&text, node)),
            },
        })
    }

    /// The entries under one relation key: a sequence, one entry, or nothing.
    fn relation_refs(&mut self, node: &MarkedYaml<'a>) -> Vec<RelationRef> {
        let node = untag(node);
        match &node.data {
            YamlData::Sequence(items) => items
                .iter()
                .filter_map(|item| self.relation_ref(item))
                .collect(),
            YamlData::Value(Scalar::Null) => Vec::new(),
            _ => self.relation_ref(node).into_iter().collect(),
        }
    }

    /// One `links` entry.
    fn link_entry(&mut self, node: &MarkedYaml<'a>) -> Option<LinkEntry> {
        let node = untag(node);
        let Some(mapping) = node.data.as_mapping() else {
            self.warn(node, "a `links` entry must be a mapping; ignored".into());
            return None;
        };
        let mut entry = LinkEntry::default();
        let mut has_type = false;
        for (k, v) in mapping {
            let key = self.key_text(k);
            match key.as_str() {
                "id" => entry.id = self.id_of(v),
                "link" => {
                    entry.link = self
                        .scalar_or_wikilink(v)
                        .map(|link_text| self.wikilink(&link_text, v));
                }
                "type" => match self.scalar_str(v) {
                    Some(rel_type) => {
                        entry.rel_type = rel_type;
                        has_type = true;
                    }
                    None => self.warn(v, "the link `type` must be a name; ignored".into()),
                },
                "label" => entry.label = self.scalar_str(v),
                "props" => match untag(v).data.as_mapping() {
                    Some(props) => {
                        entry.props = props
                            .iter()
                            .map(|(pk, pv)| (self.key_text(pk), self.to_value(pv)))
                            .collect::<Props>();
                    }
                    None => self.warn(v, "the link `props` must be a mapping; ignored".into()),
                },
                "block" => entry.block = self.id_of(v),
                _ => self.warn(k, format!("`{key}` is not part of a link; ignored")),
            }
        }
        if !has_type {
            self.warn(
                node,
                format!("a link without a `type`; read as `{DEFAULT_REL_TYPE}`"),
            );
            entry.rel_type = DEFAULT_REL_TYPE.to_owned();
        }
        Some(entry)
    }

    /// Reads the root mapping into an envelope.
    fn envelope(
        &mut self,
        root: &MarkedYaml<'a>,
        mapping: &saphyr::AnnotatedMapping<'a, MarkedYaml<'a>>,
    ) -> Frontmatter {
        let mut front = Frontmatter::default();
        // Types first: user fields are decoded by the kind their types give the key.
        self.types = root
            .data
            .as_mapping_get("types")
            .and_then(|v| self.string_list(v))
            .unwrap_or_default();
        let mut lat: Option<(f64, Value, usize)> = None;
        let mut lon: Option<(f64, Value, usize)> = None;
        for (k, v) in mapping {
            let key = self.key_text(k);
            match key.as_str() {
                "id" => match self.scalar_str(v).and_then(|s| Id::parse(&s).ok()) {
                    Some(id) => front.id = Some(id),
                    None => self.keep(&mut front, key, v, "is not a ULID"),
                },
                "types" => match self.string_list(v) {
                    Some(types) => front.types = types,
                    None => self.keep(&mut front, key, v, "is not a list of type names"),
                },
                "tags" => match self.string_list(v) {
                    Some(tags) => front.tags = tags,
                    None => self.keep(&mut front, key, v, "is not a list of tags"),
                },
                "created" | "updated" | "captured_at" | "event_start" | "event_end" => {
                    match self.scalar_str(v).and_then(|s| parse_ms(&s)) {
                        Some(ms) => match key.as_str() {
                            "created" => front.created = Some(ms),
                            "updated" => front.updated = Some(ms),
                            "captured_at" => front.captured_at = Some(ms),
                            "event_start" => front.event_start = Some(ms),
                            _ => front.event_end = Some(ms),
                        },
                        None => self.keep(
                            &mut front,
                            key,
                            v,
                            "is not an ISO 8601 UTC timestamp (`2026-10-25T08:21:11.000Z`)",
                        ),
                    }
                }
                "summary" => match self.scalar_str(v) {
                    Some(summary) => front.summary = Some(summary),
                    None => self.keep(&mut front, key, v, "is not text"),
                },
                "geo_lat" => match self.f64_of(v) {
                    Some(f) => lat = Some((f, self.to_value(v), self.line(v))),
                    None => self.keep(&mut front, key, v, "is not a number"),
                },
                "geo_lon" => match self.f64_of(v) {
                    Some(f) => lon = Some((f, self.to_value(v), self.line(v))),
                    None => self.keep(&mut front, key, v, "is not a number"),
                },
                "sensitivity" => match self.scalar_str(v).and_then(|s| Sensitivity::parse(&s)) {
                    Some(Sensitivity::Sensitive) => {
                        front.sensitivity = Some(Sensitivity::Sensitive)
                    }
                    Some(Sensitivity::Normal) => front.sensitivity = None,
                    None => self.keep(&mut front, key, v, "is neither `normal` nor `sensitive`"),
                },
                "sort" => match self.scalar_str(v).and_then(|s| SortKey::parse(&s).ok()) {
                    Some(sort) => front.sort = Some(sort),
                    None => self.keep(&mut front, key, v, "is not a sort key"),
                },
                "fields" => match untag(v).data.as_mapping() {
                    Some(fields) => {
                        for (fk, fv) in fields {
                            let field_key = self.key_text(fk);
                            if is_identifier(&field_key, 64) {
                                let kind =
                                    self.schema.field(&self.types, &field_key).map(|f| f.kind);
                                if let Some(value) = self.field_value(fv, kind) {
                                    front.fields.push((field_key, value));
                                }
                            } else {
                                self.keep(
                                    &mut front,
                                    field_key,
                                    fv,
                                    "under `fields` is not a field key",
                                );
                            }
                        }
                    }
                    None => self.keep(&mut front, key, v, "is not a mapping of fields"),
                },
                "relations" => match untag(v).data.as_mapping() {
                    Some(relations) => {
                        for (rk, rv) in relations {
                            let field_key = self.key_text(rk);
                            let refs = self.relation_refs(rv);
                            front.relations.push((field_key, refs));
                        }
                    }
                    None => self.keep(&mut front, key, v, "is not a mapping of relation lists"),
                },
                "links" => match untag(v).data.as_sequence() {
                    Some(items) => {
                        for item in items {
                            if let Some(entry) = self.link_entry(item) {
                                front.links.push(entry);
                            }
                        }
                    }
                    None => self.keep(&mut front, key, v, "is not a list of links"),
                },
                _ if is_identifier(&key, 64) && !RESERVED_KEYS.contains(&key.as_str()) => {
                    let kind = self.schema.field(&self.types, &key).map(|f| f.kind);
                    if let Some(value) = self.field_value(v, kind) {
                        front.fields.push((key, value));
                    }
                }
                _ => front.extra.push((key, self.to_value(v))),
            }
        }
        match (lat, lon) {
            (Some((lat, _, _)), Some((lon, _, _))) => front.geo = Some((lat, lon)),
            (Some((_, value, line)), None) => {
                self.warnings.push(Warning {
                    line,
                    message: "`geo_lat` without `geo_lon`; kept as an extra key".to_owned(),
                });
                front.extra.push(("geo_lat".to_owned(), value));
            }
            (None, Some((_, value, line))) => {
                self.warnings.push(Warning {
                    line,
                    message: "`geo_lon` without `geo_lat`; kept as an extra key".to_owned(),
                });
                front.extra.push(("geo_lon".to_owned(), value));
            }
            (None, None) => {}
        }
        front
    }
}

// ---------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------

/// Puts an envelope into the normal form the reader produces from what [`write`] writes (see the
/// module doc of `files/mod.rs`): a `null` user field is absent; a user field whose key is not an
/// identifier is an extra key, and an extra key that is an unreserved identifier is a user field
/// (the reader sorts a top-level key by its shape, not by where the writer took it from); user
/// fields with a reserved key come after all others (they are written under `fields:`, which the
/// reader visits after the top-level keys); an extra key is never written twice, and one that
/// names an envelope slot the envelope has set is dropped (the slot wins on write; `types` is
/// always set unless empty); a numeric `geo_lat`/`geo_lon` extra sits last (the reader files it
/// after the loop), and both together are the `geo` slot. Idempotent.
pub(crate) fn normalize(front: &mut Frontmatter) {
    let mut fields: Vec<(String, Value)> = Vec::new();
    let mut reserved: Vec<(String, Value)> = Vec::new();
    let mut extra: Vec<(String, Value)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut moved_to_extra: Vec<(String, Value)> = Vec::new();
    for (key, value) in front.fields.drain(..) {
        if !is_identifier(&key, 64) {
            moved_to_extra.push((key, value));
        } else if value.is_null() || !seen.insert(key.clone()) {
            // Written as `~`, read as absent; a second spelling of one key never reaches the file.
        } else if RESERVED_KEYS.contains(&key.as_str()) {
            reserved.push((key, value));
        } else {
            fields.push((key, value));
        }
    }
    for (key, value) in front.extra.drain(..).chain(moved_to_extra) {
        if is_identifier(&key, 64) && !RESERVED_KEYS.contains(&key.as_str()) {
            if !value.is_null() && seen.insert(key.clone()) {
                fields.push((key, value));
            }
            continue;
        }
        if seen.insert(key.clone()) {
            extra.push((key, value));
        }
    }
    fields.extend(reserved);
    front.fields = fields;

    let has_reserved_field = front
        .fields
        .iter()
        .any(|(k, _)| RESERVED_KEYS.contains(&k.as_str()));
    let slot_is_set = |key: &str| match key {
        "id" => front.id.is_some(),
        "types" => !front.types.is_empty(),
        "tags" => !front.tags.is_empty(),
        "created" => front.created.is_some(),
        "updated" => front.updated.is_some(),
        "summary" => front.summary.is_some(),
        "captured_at" => front.captured_at.is_some(),
        "event_start" => front.event_start.is_some(),
        "event_end" => front.event_end.is_some(),
        "geo_lat" | "geo_lon" => front.geo.is_some(),
        "sensitivity" => front.sensitivity == Some(Sensitivity::Sensitive),
        "sort" => front.sort.is_some(),
        "fields" => has_reserved_field,
        "relations" => !front.relations.is_empty(),
        "links" => !front.links.is_empty(),
        _ => false,
    };
    extra.retain(|(key, _)| !slot_is_set(key));
    // A numeric `geo_lat` or `geo_lon` alone is filed after the loop by the reader; both together
    // are the slot.
    let as_f64 = |value: &Value| match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    let numeric_geo = |(key, value): &(String, Value)| {
        (key == "geo_lat" || key == "geo_lon") && as_f64(value).is_some()
    };
    if front.geo.is_none() {
        let lat = extra
            .iter()
            .find(|e| e.0 == "geo_lat")
            .and_then(|e| as_f64(&e.1));
        let lon = extra
            .iter()
            .find(|e| e.0 == "geo_lon")
            .and_then(|e| as_f64(&e.1));
        if let (Some(lat), Some(lon)) = (lat, lon) {
            front.geo = Some((lat, lon));
            extra.retain(|(k, _)| k != "geo_lat" && k != "geo_lon");
        }
    }
    let (geo, rest): (Vec<_>, Vec<_>) = extra.into_iter().partition(numeric_geo);
    front.extra = rest;
    front.extra.extend(geo);
    if front.sensitivity == Some(Sensitivity::Normal) {
        front.sensitivity = None;
    }
}

/// Writes the envelope: an empty string when `front == Frontmatter::default()`, otherwise
/// `---\n`, the YAML, `\n---\n`. The envelope is written in normal form (see [`normalize`]), so
/// `serialize(d) == serialize(normalize(d))`. The emitter decides every quote; `types` is always
/// written (unless it is empty and an extra key spells it), the other envelope keys only when
/// set, `tags`/`relations`/`links` only when non-empty, user fields with a reserved key under
/// `fields:`, and `extra` last, in the order given. The schema is not needed: a JSON value writes
/// the same way whatever its kind.
pub(crate) fn write(front: &Frontmatter, _schema: &Schema) -> String {
    let mut normal = front.clone();
    normalize(&mut normal);
    let front = &normal;
    if *front == Frontmatter::default() {
        return String::new();
    }
    let mut map = Mapping::new();
    if let Some(id) = front.id {
        map.insert(text("id"), text(id.to_string()));
    }
    if !(front.types.is_empty() && front.extra.iter().any(|(k, _)| k == "types")) {
        map.insert(text("types"), strings(&front.types));
    }
    if !front.tags.is_empty() {
        map.insert(text("tags"), strings(&front.tags));
    }
    for (name, ms) in [("created", front.created), ("updated", front.updated)] {
        if let Some(ms) = ms {
            map.insert(text(name), text(format_ms(ms)));
        }
    }
    if let Some(summary) = &front.summary {
        map.insert(text("summary"), text(summary.clone()));
    }
    for (name, ms) in [
        ("captured_at", front.captured_at),
        ("event_start", front.event_start),
        ("event_end", front.event_end),
    ] {
        if let Some(ms) = ms {
            map.insert(text(name), text(format_ms(ms)));
        }
    }
    if let Some((lat, lon)) = front.geo {
        map.insert(
            text("geo_lat"),
            Yaml::Value(Scalar::FloatingPoint(lat.into())),
        );
        map.insert(
            text("geo_lon"),
            Yaml::Value(Scalar::FloatingPoint(lon.into())),
        );
    }
    if front.sensitivity == Some(Sensitivity::Sensitive) {
        map.insert(text("sensitivity"), text(Sensitivity::Sensitive.as_str()));
    }
    if let Some(sort) = &front.sort {
        map.insert(text("sort"), text(sort.as_str()));
    }
    let mut collisions = Mapping::new();
    for (key, value) in &front.fields {
        let target = if RESERVED_KEYS.contains(&key.as_str()) {
            &mut collisions
        } else {
            &mut map
        };
        target.insert(text(key.clone()), from_json(value));
    }
    if !collisions.is_empty() {
        map.insert(text("fields"), Yaml::Mapping(collisions));
    }
    if !front.relations.is_empty() {
        let mut relations = Mapping::new();
        for (key, refs) in &front.relations {
            relations.insert(
                text(key.clone()),
                Yaml::Sequence(refs.iter().map(relation_ref_yaml).collect()),
            );
        }
        map.insert(text("relations"), Yaml::Mapping(relations));
    }
    if !front.links.is_empty() {
        map.insert(
            text("links"),
            Yaml::Sequence(front.links.iter().map(link_yaml).collect()),
        );
    }
    for (key, value) in &front.extra {
        let key = text(key.clone());
        // An extra never replaces a key the envelope wrote (`normalize` drops such extras; this
        // is the guard for the slot `types`, which is written even when the envelope is bare).
        if !map.contains_key(&key) {
            map.insert(key, from_json(value));
        }
    }
    emit(&Yaml::Mapping(map))
}

/// A string scalar node. A string the YAML crate would read back as something else although its
/// emitter writes it plain is forced into double quotes (see [`misread_when_plain`]).
fn text(s: impl Into<String>) -> Yaml<'static> {
    let s = s.into();
    if misread_when_plain(&s) {
        return Yaml::Representation(Cow::Owned(s), ScalarStyle::DoubleQuoted, None);
    }
    Yaml::Value(Scalar::String(Cow::Owned(s)))
}

/// Whether the crate's loader resolves the plain spelling of `s` to a non-string while its
/// emitter would not quote it: `+-1` is `Integer(-1)` to the loader (a `+` sign followed by
/// anything `i64` parses) but no number to the emitter. Strings with a quote, a backslash or a
/// control character are left to the emitter, which quotes and escapes them itself; the forced
/// representation is written verbatim between the quotes, so it must need no escaping.
fn misread_when_plain(s: &str) -> bool {
    !s.is_empty()
        && !s.contains(['"', '\\'])
        && !s.chars().any(char::is_control)
        && !matches!(Scalar::parse_from_cow(Cow::Borrowed(s)), Scalar::String(_))
}

/// A sequence of string scalars.
fn strings(items: &[String]) -> Yaml<'static> {
    Yaml::Sequence(items.iter().map(|s| text(s.clone())).collect())
}

/// `[[target#^block|shown]]`.
fn wikilink_text(link: &WikiLink) -> String {
    let mut out = String::with_capacity(link.target.len() + 4);
    out.push_str("[[");
    out.push_str(&link.target);
    if let Some(block) = link.block {
        out.push_str("#^");
        out.push_str(&block.to_string());
    }
    if let Some(shown) = &link.shown {
        out.push('|');
        out.push_str(shown);
    }
    out.push_str("]]");
    out
}

fn relation_ref_yaml(entry: &RelationRef) -> Yaml<'static> {
    let mut map = Mapping::new();
    if let Some(id) = entry.id {
        map.insert(text("id"), text(id.to_string()));
    }
    if let Some(link) = &entry.link {
        map.insert(text("link"), text(wikilink_text(link)));
    }
    Yaml::Mapping(map)
}

fn link_yaml(entry: &LinkEntry) -> Yaml<'static> {
    let mut map = Mapping::new();
    if let Some(id) = entry.id {
        map.insert(text("id"), text(id.to_string()));
    }
    if let Some(link) = &entry.link {
        map.insert(text("link"), text(wikilink_text(link)));
    }
    map.insert(text("type"), text(entry.rel_type.clone()));
    if let Some(label) = &entry.label {
        map.insert(text("label"), text(label.clone()));
    }
    if !entry.props.is_empty() {
        let mut props = Mapping::new();
        for (key, value) in &entry.props {
            props.insert(text(key.clone()), from_json(value));
        }
        map.insert(text("props"), Yaml::Mapping(props));
    }
    if let Some(block) = entry.block {
        map.insert(text("block"), text(block.to_string()));
    }
    Yaml::Mapping(map)
}

/// A JSON value as a YAML tree: the emitter quotes the strings that need it.
fn from_json(value: &Value) -> Yaml<'static> {
    match value {
        Value::Null => Yaml::Value(Scalar::Null),
        Value::Bool(b) => Yaml::Value(Scalar::Boolean(*b)),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Yaml::Value(Scalar::Integer(i)),
            (None, Some(f)) => Yaml::Value(Scalar::FloatingPoint(f.into())),
            (None, None) => text(n.to_string()),
        },
        Value::String(s) => text(s.clone()),
        Value::Array(items) => Yaml::Sequence(items.iter().map(from_json).collect()),
        Value::Object(object) => {
            let mut map = Mapping::new();
            for (key, value) in object {
                map.insert(text(key.clone()), from_json(value));
            }
            Yaml::Mapping(map)
        }
    }
}

/// Dumps the tree with `multiline(true)`, strips the emitter's `---` line and its `...` end
/// marker (written after a block scalar that keeps its trailing newlines), and writes the fences.
fn emit(doc: &Yaml<'_>) -> String {
    let mut out = String::new();
    {
        let mut emitter = YamlEmitter::new(&mut out);
        emitter.multiline(true);
        if emitter.dump(doc).is_err() {
            // Writing to a String cannot fail; this branch is unreachable in practice.
            return String::new();
        }
    }
    let mut yaml = out.strip_prefix("---\n").unwrap_or(&out);
    if yaml.ends_with("\n...") {
        yaml = &yaml[..yaml.len() - 3];
    }
    let mut result = String::with_capacity(yaml.len() + 9);
    result.push_str("---\n");
    result.push_str(yaml);
    if !yaml.ends_with('\n') {
        result.push('\n');
    }
    result.push_str("---\n");
    result
}

// ---------------------------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------------------------

const MS_PER_DAY: i64 = 86_400_000;

/// Milliseconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SS.sssZ`. Proleptic Gregorian, UTC;
/// years before 0 or after 9999 are not formatted to four digits and do not read back.
pub(crate) fn format_ms(ms: i64) -> String {
    let days = ms.div_euclid(MS_PER_DAY);
    let rem = ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3_600_000;
    let minute = rem / 60_000 % 60;
    let second = rem / 1_000 % 60;
    let milli = rem % 1_000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milli:03}Z")
}

/// Reads `YYYY-MM-DDTHH:MM:SSZ` or `YYYY-MM-DDTHH:MM:SS.sssZ` (a real calendar date, UTC) to
/// milliseconds since the Unix epoch.
pub(crate) fn parse_ms(text: &str) -> Option<i64> {
    if !crate::value::is_datetime(text) {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    let millis = if text.len() == 24 { number(20..23)? } else { 0 };
    Some(
        days_from_civil(year, month, day) * MS_PER_DAY
            + ((hour * 60 + minute) * 60 + second) * 1_000
            + millis,
    )
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { y + 1 } else { y }, month, day)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn timestamps_format_known_values() {
        assert_eq!(format_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_ms(1_000_000_000_000), "2001-09-09T01:46:40.000Z");
        assert_eq!(format_ms(946_684_800_000), "2000-01-01T00:00:00.000Z");
        // 2000 is a leap year (divisible by 400); 2100 is not.
        assert_eq!(format_ms(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(format_ms(4_102_444_800_000), "2100-01-01T00:00:00.000Z");
        assert_eq!(format_ms(4_107_542_400_000), "2100-03-01T00:00:00.000Z");
        assert_eq!(format_ms(1_709_208_000_000), "2024-02-29T12:00:00.000Z");
        assert_eq!(format_ms(1_761_380_471_123), "2025-10-25T08:21:11.123Z");
        // Before the epoch the day arithmetic floors, so the time of day stays positive.
        assert_eq!(format_ms(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(format_ms(-86_400_000), "1969-12-31T00:00:00.000Z");
    }

    #[test]
    fn timestamps_parse_with_and_without_millis() {
        assert_eq!(parse_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_ms("2024-02-29T12:00:00Z"), Some(1_709_208_000_000));
        assert_eq!(
            parse_ms("2025-10-25T08:21:11.123Z"),
            Some(1_761_380_471_123)
        );
        for bad in [
            "2023-02-29T00:00:00Z",
            "2100-02-29T00:00:00Z",
            "2026-10-25T08:21:11",
            "2026-10-25 08:21:11Z",
            "2026-10-25T08:21:11.0Z",
            "2026-10-25",
            "yesterday",
            "",
        ] {
            assert_eq!(parse_ms(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn timestamps_round_trip_across_years() {
        // Every 7 days plus an odd offset, from 1900 to 2300: the formatter and the parser agree.
        let mut ms = -2_208_988_800_000_i64 + 12_345;
        while ms < 10_413_792_000_000 {
            let text = format_ms(ms);
            assert_eq!(parse_ms(&text), Some(ms), "{text}");
            ms += 7 * MS_PER_DAY;
        }
        for (days, date) in [
            (0, (1970, 1, 1)),
            (-1, (1969, 12, 31)),
            (19_782, (2024, 2, 29)),
        ] {
            assert_eq!(civil_from_days(days), date);
            assert_eq!(days_from_civil(date.0, date.1, date.2), days);
        }
    }

    #[test]
    fn split_finds_the_fences() {
        assert_eq!(split("hello\n"), (None, "hello\n", 0));
        assert_eq!(split(""), (None, "", 0));
        assert_eq!(split("---\n"), (None, "---\n", 0));
        assert_eq!(split("---\n\nid: x\n"), (None, "---\n\nid: x\n", 0));
        // The opening fence is line 0, so a one-line envelope puts the body at line 3.
        assert_eq!(split("---\nid: x\n---\n"), (Some("id: x\n"), "", 3));
        assert_eq!(
            split("---\nid: x\n---\n\nbody\n"),
            (Some("id: x\n"), "\nbody\n", 3)
        );
        assert_eq!(
            split("---\nid: x\nb: 2\n...\nbody"),
            (Some("id: x\nb: 2\n"), "body", 4)
        );
        assert_eq!(split("---\n---\nbody\n"), (Some(""), "body\n", 2));
        assert_eq!(split("---\nid: x\n---"), (Some("id: x\n"), "", 3));
        assert_eq!(
            split("---\r\nid: x\r\n---\r\nbody\r\n"),
            (Some("id: x\r\n"), "body\r\n", 3)
        );
        // Not a fence: indented, trailing text, or four dashes. An opening fence that is never
        // closed is no fence either: the whole text is body, as pulldown reads it.
        let unterminated = "---\nid: x\n ---\n----\n--- \n";
        assert_eq!(split(unterminated), (None, unterminated, 0));
        assert_eq!(split("---\nfoo\n"), (None, "---\nfoo\n", 0));
        assert!(split("--- \nid: x\n---\n").0.is_none());
    }

    #[test]
    fn prescan_measures_depth_and_documents() {
        assert!(prescan("a: 1\n").is_ok());
        assert!(prescan(&format!("{}x\n", "- ".repeat(MAX_YAML_DEPTH))).is_ok());
        let deep = prescan(&format!("{}x\n", "- ".repeat(MAX_YAML_DEPTH + 1))).unwrap_err();
        assert_eq!((deep.line, deep.col), (2, 0));
        // Nesting is counted in indentation levels, whatever their width: a chain of one-space
        // indents nests one level per line, and a lone deeply indented line nests once.
        let chain = |levels: usize, width: usize| -> String {
            (0..levels)
                .map(|l| {
                    format!(
                        "{}k{l}:{}\n",
                        " ".repeat(width * l),
                        if l + 1 == levels { " v" } else { "" }
                    )
                })
                .collect()
        };
        assert!(prescan(&chain(MAX_YAML_DEPTH, 1)).is_ok());
        let one_space = prescan(&chain(MAX_YAML_DEPTH + 1, 1)).unwrap_err();
        assert_eq!(
            (one_space.line, one_space.col),
            (2 + MAX_YAML_DEPTH, MAX_YAML_DEPTH)
        );
        assert!(prescan(&chain(MAX_YAML_DEPTH, 2)).is_ok());
        assert!(prescan(&chain(MAX_YAML_DEPTH + 1, 4)).is_err());
        assert!(prescan(&format!("{}k: v\n", " ".repeat(2 * MAX_YAML_DEPTH + 2))).is_ok());
        // The root mapping is a level, so 63 flow brackets under a key reach the cap.
        assert!(prescan(&format!("k: {}1{}\n", "[".repeat(63), "]".repeat(63))).is_ok());
        assert!(prescan(&format!("k: {}1{}\n", "[".repeat(64), "]".repeat(64))).is_err());
        assert!(prescan(&format!("k: {}\n", "[".repeat(65))).is_err());
        // A tag, an anchor, a tab or a line break between the key and its flow value hides
        // nothing.
        for prefix in [
            "k: !!seq ",
            "k: &a ",
            "k: !!seq &a ",
            "k:\t",
            "k:\n  ",
            "? ",
            "-\t",
        ] {
            let text = format!("{prefix}{}x{}\n", "[".repeat(64), "]".repeat(64));
            assert!(prescan(&text).is_err(), "{prefix:?}");
        }
        // Brackets across lines accumulate; brackets in quotes, comments, plain text and block
        // scalars do not.
        assert!(prescan("k: [\n  [\n  [\n  ]]]\n").is_ok());
        assert!(prescan(&format!("k: \"{}\"\n", "[".repeat(200))).is_ok());
        assert!(prescan(&format!("k: 1 # {}\n", "[".repeat(200))).is_ok());
        assert!(prescan(&format!("k: see {}\n", "[".repeat(200))).is_ok());
        assert!(prescan(&format!("k: |\n  {}\n  - - - x\nj: 1\n", "[".repeat(200))).is_ok());
        assert!(prescan(&format!("- |\n  {}\n", "{".repeat(200))).is_ok());
        // The compact sequence form and `- k:` each open a level.
        assert!(prescan("k:\n- a: 1\n- b: 2\nj: 3\n").is_ok());
        for marker in ["--- ", "---\tx", "... ", "---"] {
            let err = prescan(&format!("a: 1\n{marker}\nb: 2\n")).unwrap_err();
            assert_eq!((err.line, err.col), (3, 0), "{marker:?}");
        }
        assert!(prescan("a: 1\n  ---\n").is_ok());
        assert!(prescan("a: ---\n").is_ok());
        assert_eq!(key_value("k: v"), Some("v"));
        assert_eq!(key_value("\"a: b\": [1]"), Some("[1]"));
        assert_eq!(key_value("'it''s': x"), Some("x"));
        assert_eq!(key_value("k:"), Some(""));
        assert_eq!(key_value("k:\t[1]"), Some("[1]"));
        assert_eq!(key_value("[a, b]"), None);
        assert_eq!(key_value("plain"), None);
        assert_eq!(key_value("# c"), None);
        assert_eq!(strip_node_properties("!!seq &a  [1]"), "[1]");
        assert_eq!(strip_node_properties("plain !x"), "plain !x");
        // The `]` inside the quotes does not close anything; the `}` closes the mapping.
        assert_eq!(flow_depth_after("[{a: \"]\"}", 0), (1, 2));
        assert_eq!(flow_depth_after("[{a: \"]\"", 0), (2, 2));
        assert_eq!(flow_depth_after("]]]", 1), (0, 1));
        // Closed on the line, the peak still counts.
        assert_eq!(flow_depth_after("[[[x]]]", 0), (0, 3));
        let closed = format!("k: {}x{}\n", "[".repeat(64), "]".repeat(64));
        assert!(prescan(&closed).is_err());
        let fine = format!("k: {}x{}\n", "[".repeat(63), "]".repeat(63));
        assert!(prescan(&fine).is_ok());
    }

    #[test]
    fn positions_come_from_the_line_table() {
        let source = Source::new("a: 1\nbé: 2\n\nc: 3");
        assert_eq!(source.position(0), (2, 0));
        assert_eq!(source.position(3), (2, 3));
        assert_eq!(source.position(5), (3, 0));
        assert_eq!(source.position(9), (3, 4));
        assert_eq!(source.position(11), (4, 0));
        assert_eq!(source.position(12), (5, 0));
        assert_eq!(source.position(15), (5, 3));
        assert_eq!(source.position(999), (5, 987));
        assert_eq!(Source::new("").position(0), (2, 0));
    }

    #[test]
    fn a_bare_carriage_return_is_refused_with_its_line() {
        let error = parse("a: 1\nb:\rc: 2\n", &Schema::default()).unwrap_err();
        assert_eq!((error.line, error.col), (3, 0));
        assert!(error.message.contains("carriage return"), "{error}");
        assert!(parse("a: 1\r\nb: 2\r\n", &Schema::default()).is_ok());
    }

    #[test]
    fn raw_spelling_is_sliced_by_character_index() {
        let text = "é: 0755\nx: 1e3\n";
        let docs = MarkedYaml::load_from_str(text).unwrap();
        let source = Source::new(text);
        let root = &docs[0];
        assert_eq!(source.raw(root.data.as_mapping_get("é").unwrap()), "0755");
        assert_eq!(source.raw(root.data.as_mapping_get("x").unwrap()), "1e3");
        let ascii = Source::new("a: 12 # c\n");
        let docs = MarkedYaml::load_from_str(ascii.text).unwrap();
        assert_eq!(ascii.raw(docs[0].data.as_mapping_get("a").unwrap()), "12");
    }

    #[test]
    fn emit_handles_block_scalars_that_keep_their_newlines() {
        let mut map = Mapping::new();
        map.insert(text("a"), text("one\n\n"));
        map.insert(text("b"), text("two\n"));
        let out = emit(&Yaml::Mapping(map));
        assert!(
            out.starts_with("---\n") && out.ends_with("\n---\n"),
            "{out}"
        );
        assert!(!out.contains("..."), "{out}");
        let (front, warnings) = parse(&out[4..out.len() - 4], &Schema::default()).unwrap();
        assert!(warnings.is_empty());
        // `a` and `b` are identifiers, so they read back as (generically decoded) user fields.
        assert_eq!(
            front.fields,
            [
                ("a".to_owned(), json!("one\n\n")),
                ("b".to_owned(), json!("two\n"))
            ]
        );
    }
}
