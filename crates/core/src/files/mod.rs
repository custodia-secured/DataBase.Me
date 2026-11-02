// SPDX-License-Identifier: AGPL-3.0-or-later
//! The file format of the files folder (FORMAT.md §6, ADR-0006, ADR-0022, ADR-0023): one Markdown
//! file per object, a YAML frontmatter around a body of blocks, readable by Obsidian and by any
//! text editor. This module is pure: no I/O, no clock, no randomness. [`parse`] turns a file's text
//! into a [`FileDoc`], [`serialize`] writes one back, [`from_rows`] builds one from the vault's
//! rows, and [`merge`] reconciles three of them.
//!
//! # Frontmatter
//!
//! A file starts with `---` at offset 0 and the frontmatter ends at the first following line that
//! is exactly `---` or `...`. Between the fences is exactly one YAML document whose root is a
//! mapping; a stray `---` or `...` line inside, a duplicate key, a root that is not a mapping, a
//! block larger than [`MAX_FRONTMATTER_BYTES`] or nested deeper than [`MAX_YAML_DEPTH`] (indent
//! levels, `- ` runs and unclosed `[`/`{` together) is a [`ParseError`] with a position. An empty
//! block is the empty envelope. A file that does not start with `---` has no frontmatter and is
//! all body; `---` followed by a blank line is a body too (a rule, not a fence).
//!
//! The envelope keys, in the order they are written:
//!
//! ```yaml
//! ---
//! id: 01JB3R7Q8Y5K3M2N4P6Q8R0S2T       # always when known
//! types: [Task]                         # always, type names in order
//! tags: [travel, admin]                 # only when non-empty; body `#tags` are not listed here
//! created: 2026-10-25T08:21:11.000Z    # ISO 8601 UTC with milliseconds; read with or without
//! updated: 2026-10-25T09:02:40.000Z
//! summary: one line                     # only when set; likewise captured_at, event_start,
//! captured_at: 2026-10-25T08:00:00.000Z # event_end
//! geo_lat: 48.85                        # only when set, always together with geo_lon
//! geo_lon: 2.35
//! sensitivity: sensitive                # only when sensitive; absent means normal
//! sort: a0V                             # the object's sort key, always
//! due: 2026-11-30                       # user fields, top level, in schema order
//! status: Waiting
//! fields:                               # only user fields whose key is reserved (rare)
//!   tags: [x]
//! relations:                            # only when non-empty; by field key, id then wikilink
//!   assigned_to:
//!     - id: 01JB3R9C0D4E5F6G7H8J9K0M1N
//!       link: "[[People/Alex Doe]]"
//! links:                                # only when non-empty; relations with no field
//!   - id: 01JB3RA1B2C3D4E5F6G7H8J9K0
//!     link: "[[Archive/Alpha]]"
//!     type: related
//!     label: the earlier run            # only when set
//!     props: {weight: 2}                # only when non-empty
//!     block: 01JB3RB0C1D2E3F4G5H6J7K8M9 # only when set: the source block
//! aliases: [Alpha]                      # extras, verbatim, after everything else
//! ---
//! ```
//!
//! [`RESERVED_KEYS`] lists the keys the envelope owns, plus Obsidian's `aliases` and `cssclasses`.
//! On read, a top-level key that is not reserved and is a lower-case identifier is a user field;
//! `fields:` is a map of user fields too (so the older shape loads); every other top-level key is
//! kept verbatim in [`Frontmatter::extra`], in file order. User field values are written as YAML
//! and decoded by the field's kind from the [`Schema`] (FORMAT.md §3.3); a scalar whose raw
//! spelling matters (`0755`, `1e3`, `08:21`, `no`) is decoded from the source text, never from
//! what YAML's Core Schema made of it. A key with no field decodes generically. A timestamp that
//! does not parse goes to `extra` with a [`Warning`]; nothing is lost on read.
//!
//! # Body
//!
//! One blank line separates the frontmatter from the body, and blocks from each other, except
//! consecutive list items of one list, which are adjacent lines. The body ends with one `\n`; an
//! object with no blocks has no body. Only LF is written; CRLF is accepted on read.
//!
//! | Kind | Markdown | Anchor ` ^<ID>` |
//! | --- | --- | --- |
//! | paragraph | the inline text | end of the last line |
//! | heading | `#{level} ` text on one line | its own line, blank lines around it |
//! | bullet, numbered, todo | `- `, `N. `, `- [ ] ` or `- [x] ` and the text | on the item's line |
//! | quote | each line prefixed `> ` | its own line |
//! | code | a fence of at least three backticks plus the language, the code, the fence | its own line |
//! | divider | `***` (never `---`, which would read as a fence or a setext heading) | its own line |
//! | image | `![[attachments/<hash><ext>]]`, `\|alt` before the `]]` when set | on the line |
//! | unknown kind | a `dbme` fence holding one JSON line `{"kind", "content", "children"}` | its own line |
//!
//! The anchor is the block's ULID, 26 upper-case characters; lower case is accepted. An empty
//! paragraph is its anchor alone on a line. A lone `^id` line attaches to the block before it only
//! when that block's anchor belongs on its own line and it has none yet; otherwise it is an empty
//! paragraph with that id. A block that has no id (one a person typed) is written without an
//! anchor and is given one by [`assign_ids`].
//!
//! Children belong to list items and to unknown kinds only. A child of a list item is indented by
//! the parent's marker width (two under `- `, three under `1. `, four under `10. `); a child that
//! is not itself a list item is a continuation paragraph inside the item, after a blank line.
//! Children of an unknown kind travel inside its JSON. Nesting deeper than [`MAX_BLOCK_DEPTH`] is a
//! [`ParseError`].
//!
//! Inline marks are `**bold**`, `*italic*`, `<u>underline</u>`, `~~strike~~` and `` `code` ``,
//! nested in that order from the outside in; a link is `[text](url)` with `(`, `)` and space
//! percent-encoded in the URL; a mention is `[[path]]`, `[[path|shown]]`, `[[path#^block]]` or
//! `![[path]]` for an embed, the path from the [`LinkResolver`], or the ULID itself when the object
//! has no file. A wikilink that resolves to nothing stays text, so `[` and `]` are never escaped.
//! Escaped on write: `\`, `*`, `_`, `` ` ``, `~`, `&`, `<` before a letter, `#` at the start of a
//! line, and at the start of a line `-`, `+`, `>`, `|`, `=`, `^`, a digit run before `.` or `)`,
//! and a ` ^` before 26 base32 characters at the end of a line. `#tag` in text (not in code or
//! links) is collected into [`Parsed::body_tags`], lower-cased, and left in the text.
//!
//! # Normal form
//!
//! `parse(serialize(d)) == d` holds for every `d` in normal form, and `serialize` of that is
//! byte-identical: that is the fixpoint and the rebuild guarantee. [`normalize`] puts any doc into
//! normal form, idempotently: inline runs have no empty text, adjacent text runs with the same
//! marks and link are merged, no line of a run starts or ends with a space or tab, no `\n\n`, no
//! leading or trailing `\n` in a block, no `\n` inside a `code` run; a heading has no `\n` at all;
//! `code` content has no trailing `\n` and its `language` no whitespace or backtick (an empty one is
//! absent); an `image` whose `blob` is no content hash is a paragraph of its embed text; no block
//! sits deeper than [`MAX_BLOCK_DEPTH`] (a subtree that would is moved to the top level);
//! `sensitivity: normal` is absent; `types` is non-empty and every relation and link has an id.
//! In the frontmatter, a `null` user field is absent, a user field's key is an identifier and an
//! extra key is not one or is reserved, user fields with a reserved key come after all others
//! (they are written under `fields:`), and an extra key that names an envelope slot the envelope
//! has set is dropped (the slot wins). Tags keep their case; the store case-folds when matching.
//!
//! # Caps
//!
//! [`MAX_FILE_BYTES`] on the whole file, checked before anything else; [`MAX_FRONTMATTER_BYTES`]
//! and [`MAX_YAML_DEPTH`] on the frontmatter, checked before the YAML crate sees it;
//! [`MAX_BLOCK_DEPTH`] on list nesting. Each is a [`ParseError`] at the offending line, and the
//! input is never changed by a failed parse.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{BlockRow, ObjectRow, Props, RelationRow, Sensitivity, TypeRow};
use crate::{ContentHash, Id, SortKey};

mod body;
mod frontmatter;
mod merge;
pub mod paths;
pub mod schema;

pub use merge::{Conflict, ConflictSite, Merged, merge};
pub use schema::{Schema, SchemaType};

/// The largest file the parser reads, in bytes. Beyond it, [`parse`] fails at line 1.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// The largest frontmatter block (the YAML between the fences), in bytes.
pub const MAX_FRONTMATTER_BYTES: usize = 256 * 1024;
/// The deepest YAML nesting accepted: indent levels plus `- ` runs on a line plus unclosed flow
/// brackets. The YAML crate has no block-nesting limit of its own (ADR-0023).
pub const MAX_YAML_DEPTH: usize = 64;
/// The deepest list nesting accepted in the body.
pub const MAX_BLOCK_DEPTH: usize = 64;

/// The top-level frontmatter keys the envelope owns, plus Obsidian's own. A user field with one
/// of these keys is written under `fields:`.
pub const RESERVED_KEYS: &[&str] = &[
    "id",
    "types",
    "tags",
    "created",
    "updated",
    "summary",
    "captured_at",
    "event_start",
    "event_end",
    "geo_lat",
    "geo_lon",
    "sensitivity",
    "sort",
    "fields",
    "relations",
    "links",
    "aliases",
    "cssclasses",
];

/// The `rel_type` of the relations that `tags:` stands for; they are never written to `links`.
pub const TAGGED: &str = "tagged";

/// What one file in the files folder says.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileDoc {
    /// The frontmatter.
    pub front: Frontmatter,
    /// The top-level blocks, in order.
    pub body: Vec<FileBlock>,
}

/// One block in a file, with its children (list items and unknown kinds only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileBlock {
    /// `None` only for a parsed block that had no anchor: a new block.
    pub id: Option<Id>,
    /// `blocks.kind`.
    pub kind: String,
    /// `blocks.content_json`, with mentions already resolved to ids.
    pub content: Value,
    /// Child blocks, in order.
    pub children: Vec<FileBlock>,
}

/// The envelope of a file: everything in the frontmatter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    /// The object's id.
    pub id: Option<Id>,
    /// Type names, in order.
    pub types: Vec<String>,
    /// From `tags:` only; tags found in the body are returned separately by [`parse`].
    pub tags: Vec<String>,
    /// `created_at`, in milliseconds.
    pub created: Option<i64>,
    /// `updated_at`, in milliseconds.
    pub updated: Option<i64>,
    /// The object's summary.
    pub summary: Option<String>,
    /// `captured_at`, in milliseconds.
    pub captured_at: Option<i64>,
    /// `event_start`, in milliseconds.
    pub event_start: Option<i64>,
    /// `event_end`, in milliseconds.
    pub event_end: Option<i64>,
    /// `geo_lat` and `geo_lon`.
    pub geo: Option<(f64, f64)>,
    /// Absent means normal; the normal form never carries `Some(Normal)`.
    pub sensitivity: Option<Sensitivity>,
    /// The object's sort key.
    pub sort: Option<SortKey>,
    /// User fields, decoded by kind, in file order.
    pub fields: Vec<(String, Value)>,
    /// Relations by field key, in file order.
    pub relations: Vec<(String, Vec<RelationRef>)>,
    /// Relations that belong to no field.
    pub links: Vec<LinkEntry>,
    /// Unknown top-level keys, verbatim, in file order.
    pub extra: Vec<(String, Value)>,
}

/// One target of a relation field: the id (authoritative) and the wikilink shown to people.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RelationRef {
    /// The target's id.
    pub id: Option<Id>,
    /// The wikilink written next to it, when the target has a file.
    pub link: Option<WikiLink>,
}

/// One entry of `links:`: a relation with no field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkEntry {
    /// The target's id.
    pub id: Option<Id>,
    /// The wikilink written next to it, when the target has a file.
    pub link: Option<WikiLink>,
    /// `relations.rel_type`.
    pub rel_type: String,
    /// `relations.label`.
    pub label: Option<String>,
    /// `relations.props_json`.
    pub props: Props,
    /// `relations.src_block_id`.
    pub block: Option<Id>,
}

/// `[[target#^block|shown]]` taken apart; `target` is the raw path or title, never resolved here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiLink {
    /// The text between `[[` and `#` or `|` or `]]`.
    pub target: String,
    /// The block after `#^`, when given.
    pub block: Option<Id>,
    /// The text after `|`, when given.
    pub shown: Option<String>,
}

/// What the parser needs from the vault and what the serializer needs to write links.
pub trait LinkResolver {
    /// The object a wikilink target names, if exactly one does. `target` is the raw text between
    /// `[[` and `]]` without `#^…` and `|…`. Resolution rule (ours, ADR-0022): an exact `rel_path`
    /// without `.md` first, then a unique basename (case-insensitive, NFC) among all objects'
    /// paths, else `None`.
    fn resolve(&self, target: &str) -> Option<Id>;
    /// The path of an object, without `.md`, as it is written inside `[[…]]`
    /// (`Projects/Alpha`). `None` for an object with no file (trashed, excluded).
    fn path_of(&self, id: Id) -> Option<String>;
    /// The file name under `attachments/` for a blob: `<hex><ext>`, the extension from the blob's
    /// media type (`.png`, `.jpg`, `.pdf`, …) or empty when unknown.
    fn attachment_name(&self, blob: &ContentHash) -> String;
}

/// A resolver that resolves nothing and writes ids as paths: for tests and schema-only rebuilds.
/// Attachments are named by their hex alone.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoResolver;

impl LinkResolver for NoResolver {
    fn resolve(&self, _target: &str) -> Option<Id> {
        None
    }

    fn path_of(&self, _id: Id) -> Option<String> {
        None
    }

    fn attachment_name(&self, blob: &ContentHash) -> String {
        blob.hex().to_owned()
    }
}

/// What [`parse`] returns.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    /// The document.
    pub doc: FileDoc,
    /// `#tag`s found in body text, lower-cased, deduplicated, in order of first appearance.
    pub body_tags: Vec<String>,
    /// Non-fatal findings: an unresolved wikilink kept as text, an unknown key, a timestamp that
    /// did not parse…
    pub warnings: Vec<Warning>,
}

/// Why a file could not be read. Nothing is lost: the input is untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line.
    pub line: usize,
    /// 0-based column.
    pub col: usize,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {} col {}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A non-fatal finding while reading a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// 1-based line.
    pub line: usize,
    /// What was found.
    pub message: String,
}

/// Reads a file. On `Err` the caller's text is untouched by construction.
pub fn parse(
    text: &str,
    schema: &Schema,
    resolver: &dyn LinkResolver,
) -> Result<Parsed, ParseError> {
    if text.len() > MAX_FILE_BYTES {
        return Err(ParseError {
            line: 1,
            col: 0,
            message: format!(
                "the file is {} bytes, more than the {MAX_FILE_BYTES} allowed",
                text.len()
            ),
        });
    }
    let (yaml, body, line_offset) = frontmatter::split(text);
    let (front, mut warnings) = match yaml {
        Some(yaml) => frontmatter::parse(yaml, schema)?,
        None => (Frontmatter::default(), Vec::new()),
    };
    let (blocks, body_tags, body_warnings) = body::parse(body, line_offset, schema, resolver)?;
    warnings.extend(body_warnings);
    Ok(Parsed {
        doc: FileDoc {
            front,
            body: blocks,
        },
        body_tags,
        warnings,
    })
}

/// Writes a file. Total: any document renders. LF only, ending with one `\n` when non-empty. A
/// tree nested past [`MAX_BLOCK_DEPTH`] is written as [`normalize`] flattens it, so the writer's
/// recursion, like the parser's, is bounded by the cap.
pub fn serialize(doc: &FileDoc, schema: &Schema, resolver: &dyn LinkResolver) -> String {
    let head = frontmatter::write(&doc.front, schema);
    let body = if body::depth_of(&doc.body) > MAX_BLOCK_DEPTH {
        let mut shallow = doc.body.clone();
        body::flatten_deep(&mut shallow);
        body::write(&shallow, resolver)
    } else {
        body::write(&doc.body, resolver)
    };
    if head.is_empty() {
        body
    } else if body.is_empty() {
        head
    } else {
        let mut out = String::with_capacity(head.len() + 1 + body.len());
        out.push_str(&head);
        out.push('\n');
        out.push_str(&body);
        out
    }
}

/// The rows of one object, as [`from_rows`] needs them.
#[derive(Clone, Copy, Debug)]
pub struct ObjectRows<'a> {
    /// The object.
    pub object: &'a ObjectRow,
    /// The object's types, in `object_types` order.
    pub types: &'a [&'a TypeRow],
    /// Every block of the object, in any order.
    pub blocks: &'a [BlockRow],
    /// Live relations with `src_id == object.id`, excluding `mentions`.
    pub relations: &'a [RelationRow],
    /// Titles of the Tag objects this object has a `tagged` relation to.
    pub tags: &'a [String],
}

/// The key under which `props_json` keeps the frontmatter's extra keys.
const FRONTMATTER_PROP: &str = "_frontmatter";

/// Builds the document of an object from its rows, in normal form.
///
/// Blocks are arranged into the tree by `parent_block_id`, siblings ordered by `sort_key` then
/// `id`; a block whose parent is not among the rows, or that would sit deeper than
/// [`MAX_BLOCK_DEPTH`], is placed at the top level, so nothing is lost and the file parses back.
/// User fields come in the schema's order for the object's types (own fields before inherited,
/// by `sort_order` then key), then unknown keys alphabetically. `props_json["_frontmatter"]`
/// becomes `extra` in alphabetical key order. A relation with a `field_id` the schema knows goes
/// under `relations` by the field's key, in schema order; every other relation goes to `links`
/// with its `rel_type`, except `tagged` (that is `tags:`) and `mentions` (those are in the body).
/// Relations keep the order they were given in.
pub fn from_rows(rows: &ObjectRows<'_>, schema: &Schema, resolver: &dyn LinkResolver) -> FileDoc {
    let object = rows.object;
    let type_names: Vec<String> = rows.types.iter().map(|t| t.name.clone()).collect();
    let field_order: Vec<&str> = schema
        .fields_of(&type_names)
        .into_iter()
        .map(|f| f.key.as_str())
        .collect();

    let mut fields = Vec::new();
    for key in &field_order {
        if let Some(value) = object.props_json.get(*key) {
            fields.push(((*key).to_owned(), value.clone()));
        }
    }
    for (key, value) in &object.props_json {
        if key != FRONTMATTER_PROP && !field_order.contains(&key.as_str()) {
            fields.push((key.clone(), value.clone()));
        }
    }
    let extra: Vec<(String, Value)> = match object.props_json.get(FRONTMATTER_PROP) {
        Some(Value::Object(map)) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            sorted
                .into_iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        }
        _ => Vec::new(),
    };

    let wikilink = |id: Id| {
        resolver.path_of(id).map(|target| WikiLink {
            target,
            block: None,
            shown: None,
        })
    };
    let mut by_key: Vec<(String, Vec<RelationRef>)> = Vec::new();
    let mut links = Vec::new();
    for relation in rows.relations {
        if relation.deleted_at.is_some()
            || relation.rel_type == TAGGED
            || relation.rel_type == crate::block::MENTIONS
        {
            continue;
        }
        let field = relation.field_id.and_then(|id| schema.field_by_id(id));
        match field {
            Some((_, field)) => {
                let entry = RelationRef {
                    id: Some(relation.dst_id),
                    link: wikilink(relation.dst_id),
                };
                match by_key.iter_mut().find(|(k, _)| *k == field.key) {
                    Some((_, list)) => list.push(entry),
                    None => by_key.push((field.key.clone(), vec![entry])),
                }
            }
            None => links.push(LinkEntry {
                id: Some(relation.dst_id),
                link: wikilink(relation.dst_id),
                rel_type: relation.rel_type.clone(),
                label: relation.label.clone(),
                props: relation.props_json.clone(),
                block: relation.src_block_id,
            }),
        }
    }
    let position = |key: &str| field_order.iter().position(|k| *k == key);
    by_key.sort_by(|(a, _), (b, _)| match (position(a), position(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });

    let front = Frontmatter {
        id: Some(object.id),
        types: type_names,
        tags: rows.tags.to_vec(),
        created: Some(object.created_at),
        updated: Some(object.updated_at),
        summary: object.summary.clone(),
        captured_at: object.captured_at,
        event_start: object.event_start,
        event_end: object.event_end,
        geo: match (object.geo_lat, object.geo_lon) {
            (Some(lat), Some(lon)) => Some((lat, lon)),
            _ => None,
        },
        sensitivity: match object.sensitivity {
            Sensitivity::Sensitive => Some(Sensitivity::Sensitive),
            Sensitivity::Normal => None,
        },
        sort: Some(object.sort_key.clone()),
        fields,
        relations: by_key,
        links,
        extra,
    };

    let mut doc = FileDoc {
        front,
        body: block_tree(rows.blocks),
    };
    normalize(&mut doc);
    doc
}

/// Arranges block rows into a tree by `parent_block_id`, siblings by `sort_key` then `id`. A
/// block whose parent is not among the rows, or that sits in a parent cycle, is placed at the top
/// level.
fn block_tree(rows: &[BlockRow]) -> Vec<FileBlock> {
    let ids: HashSet<Id> = rows.iter().map(|b| b.id).collect();
    let mut children: HashMap<Option<Id>, Vec<&BlockRow>> = HashMap::new();
    for row in rows {
        let parent = row.parent_block_id.filter(|p| ids.contains(p));
        children.entry(parent).or_default().push(row);
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| (&a.sort_key, a.id).cmp(&(&b.sort_key, b.id)));
    }
    let mut placed = HashSet::new();
    let mut top = build_children(None, &children, &mut placed, 0);
    // Orphans, cycles and blocks past the depth cap: each at the top level with its own subtree.
    if placed.len() < rows.len() {
        let mut rest: Vec<&BlockRow> = rows.iter().filter(|b| !placed.contains(&b.id)).collect();
        rest.sort_by(|a, b| (&a.sort_key, a.id).cmp(&(&b.sort_key, b.id)));
        for row in rest {
            if !placed.contains(&row.id) {
                top.push(build_block(row, &children, &mut placed, 0));
            }
        }
    }
    top
}

/// The children of `parent` at `depth`; none when they would sit past [`MAX_BLOCK_DEPTH`] (the
/// caller places them at the top level), so the recursion is bounded by the cap, not the rows.
fn build_children(
    parent: Option<Id>,
    children: &HashMap<Option<Id>, Vec<&BlockRow>>,
    placed: &mut HashSet<Id>,
    depth: usize,
) -> Vec<FileBlock> {
    let mut out = Vec::new();
    if depth >= MAX_BLOCK_DEPTH {
        return out;
    }
    for row in children.get(&parent).into_iter().flatten() {
        if !placed.contains(&row.id) {
            out.push(build_block(row, children, placed, depth));
        }
    }
    out
}

fn build_block(
    row: &BlockRow,
    children: &HashMap<Option<Id>, Vec<&BlockRow>>,
    placed: &mut HashSet<Id>,
    depth: usize,
) -> FileBlock {
    placed.insert(row.id);
    FileBlock {
        id: Some(row.id),
        kind: row.kind.clone(),
        content: row.content_json.clone(),
        children: build_children(Some(row.id), children, placed, depth + 1),
    }
}

/// Puts a document into normal form (see the module doc). Idempotent.
pub fn normalize(doc: &mut FileDoc) {
    body::flatten_deep(&mut doc.body);
    body::normalize(&mut doc.body);
    frontmatter::normalize(&mut doc.front);
}

/// Puts one block's inline runs into normal form: trims line edges, collapses blank lines, drops
/// empty text runs and merges adjacent text runs with identical marks and link.
pub fn normalize_inline(content: &mut Value) {
    body::normalize_inline(content);
}

/// Gives every block without an id one from `next`, depth-first in document order. The merge
/// needs every block identified. A worklist, so a deep tree costs no stack.
pub fn assign_ids(doc: &mut FileDoc, next: &mut dyn FnMut() -> Id) {
    let mut stack: Vec<&mut FileBlock> = doc.body.iter_mut().rev().collect();
    while let Some(block) = stack.pop() {
        if block.id.is_none() {
            block.id = Some(next());
        }
        stack.extend(block.children.iter_mut().rev());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::tests::{id, object, type_row};
    use crate::model::{FieldKind, FieldRow};

    fn block(n: u128, parent: Option<u128>, key: &str) -> BlockRow {
        BlockRow {
            id: id(n),
            object_id: id(1),
            parent_block_id: parent.map(id),
            sort_key: SortKey::parse(key).unwrap(),
            kind: "paragraph".into(),
            content_json: json!({"inline": [{"text": format!("b{n}")}]}),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn relation(n: u128, dst: u128, rel_type: &str, field_id: Option<u128>) -> RelationRow {
        RelationRow {
            id: id(n),
            src_id: id(1),
            dst_id: id(dst),
            rel_type: rel_type.into(),
            field_id: field_id.map(id),
            src_block_id: None,
            label: None,
            props_json: Props::new(),
            created_at: 1,
            deleted_at: None,
        }
    }

    fn field(n: u128, type_id: u128, key: &str, sort_order: i64, kind: FieldKind) -> FieldRow {
        FieldRow {
            id: id(n),
            type_id: id(type_id),
            key: key.into(),
            label: key.into(),
            kind,
            options_json: json!({}),
            required: false,
            sort_order,
        }
    }

    #[test]
    fn parse_error_displays_its_position() {
        let error = ParseError {
            line: 3,
            col: 7,
            message: "duplicated key".into(),
        };
        assert_eq!(error.to_string(), "line 3 col 7: duplicated key");
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(boxed.to_string().starts_with("line 3"));
    }

    #[test]
    fn oversized_files_are_refused_at_line_one() {
        let text = "x".repeat(MAX_FILE_BYTES + 1);
        let error = parse(&text, &Schema::default(), &NoResolver).unwrap_err();
        assert_eq!((error.line, error.col), (1, 0));
        assert!(error.message.contains("bytes"));
    }

    #[test]
    fn no_resolver_resolves_nothing() {
        assert_eq!(NoResolver.resolve("Anything"), None);
        assert_eq!(NoResolver.path_of(id(1)), None);
        let hash = ContentHash::of(b"x");
        assert_eq!(NoResolver.attachment_name(&hash), hash.hex());
    }

    #[test]
    fn from_rows_arranges_blocks_fields_and_relations() {
        let mut row = object(1);
        row.props_json.insert("zeta".into(), json!("z"));
        row.props_json.insert("due".into(), json!("2026-11-30"));
        row.props_json.insert("status".into(), json!("open"));
        row.props_json
            .insert(FRONTMATTER_PROP.into(), json!({"b": 1, "aliases": ["A"]}));
        row.geo_lat = Some(1.0);
        row.geo_lon = Some(2.0);
        row.sensitivity = Sensitivity::Sensitive;
        let base = type_row(10);
        let mut task = type_row(11);
        task.extends = Some(base.id);
        let schema = Schema::new(
            vec![base.clone(), task.clone()],
            vec![
                field(20, 10, "status", 0, FieldKind::Text),
                field(21, 11, "due", 1, FieldKind::Date),
                field(22, 11, "assigned_to", 0, FieldKind::Relation),
            ],
        );
        let blocks = vec![
            block(100, None, "a1"),
            block(101, None, "a0"),
            block(102, Some(100), "a0"),
            block(103, Some(999), "a0"),
        ];
        let relations = vec![
            relation(200, 300, "related", None),
            relation(201, 301, TAGGED, None),
            relation(202, 302, "assigned_to", Some(22)),
            relation(203, 303, "whatever", Some(555)),
        ];
        let types = [&task];
        let tags = ["travel".to_owned()];
        let rows = ObjectRows {
            object: &row,
            types: &types,
            blocks: &blocks,
            relations: &relations,
            tags: &tags,
        };
        let doc = from_rows(&rows, &schema, &NoResolver);
        let front = &doc.front;
        assert_eq!(front.id, Some(id(1)));
        assert_eq!(front.types, ["Type 11"]);
        assert_eq!(front.tags, ["travel"]);
        assert_eq!(front.geo, Some((1.0, 2.0)));
        assert_eq!(front.sensitivity, Some(Sensitivity::Sensitive));
        assert_eq!(front.sort, Some(SortKey::first()));
        let keys: Vec<&str> = front.fields.iter().map(|(k, _)| k.as_str()).collect();
        // Own fields before inherited ones, by sort_order, then unknown keys alphabetically; the
        // `_frontmatter` key `b` is an unreserved identifier, so the normal form reads it as a
        // user field (the parser would), after the others.
        assert_eq!(keys, ["due", "status", "zeta", "b"]);
        assert_eq!(front.extra, [("aliases".to_owned(), json!(["A"]))]);
        assert_eq!(front.relations.len(), 1);
        assert_eq!(front.relations[0].0, "assigned_to");
        assert_eq!(front.relations[0].1[0].id, Some(id(302)));
        let link_types: Vec<&str> = front.links.iter().map(|l| l.rel_type.as_str()).collect();
        assert_eq!(link_types, ["related", "whatever"]);
        // Siblings by sort key then id; the orphan (parent 999) joins the top level.
        let ids: Vec<Option<Id>> = doc.body.iter().map(|b| b.id).collect();
        assert_eq!(ids, [Some(id(101)), Some(id(103)), Some(id(100))]);
        assert_eq!(doc.body[2].children[0].id, Some(id(102)));
        assert!(doc.body[0].children.is_empty());
    }

    #[test]
    fn from_rows_output_is_a_fixpoint_with_a_reserved_field_key_first() {
        // The schema orders a field named `tags` before `due`: the writer puts it under
        // `fields:`, which the reader visits last, so the normal form has it last too.
        let mut row = object(1);
        row.props_json.insert("tags".into(), json!("x"));
        row.props_json.insert("due".into(), json!("2026-11-30"));
        row.props_json.insert("gone".into(), Value::Null);
        row.props_json.insert(
            FRONTMATTER_PROP.into(),
            json!({"created": "2026-10-25", "types": 5}),
        );
        let ty = type_row(10);
        let schema = Schema::new(
            vec![ty.clone()],
            vec![
                field(20, 10, "tags", 0, FieldKind::Text),
                field(21, 10, "due", 1, FieldKind::Date),
            ],
        );
        let types = [&ty];
        let rows = ObjectRows {
            object: &row,
            types: &types,
            blocks: &[],
            relations: &[],
            tags: &[],
        };
        let doc = from_rows(&rows, &schema, &NoResolver);
        let keys: Vec<&str> = doc.front.fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["due", "tags"]);
        // `created` and `types` are set on the object, so the misfit extras are dropped.
        assert!(doc.front.extra.is_empty(), "{:?}", doc.front.extra);
        let text = serialize(&doc, &schema, &NoResolver);
        let parsed = parse(&text, &schema, &NoResolver).unwrap();
        assert_eq!(parsed.doc, doc, "{text}");
        assert_eq!(serialize(&parsed.doc, &schema, &NoResolver), text);
    }

    #[test]
    fn deep_parent_chains_are_flattened_and_round_trip() {
        // A chain of `MAX_BLOCK_DEPTH + 1` rows and one of 10,000, each row the child of the
        // one before: `from_rows` places what would sit past the cap at the top level, on a
        // 1 MiB stack, and the file parses back to the same document.
        for len in [MAX_BLOCK_DEPTH as u128 + 1, 10_000] {
            let rows: Vec<BlockRow> = (1..=len)
                .map(|i| {
                    let mut b = block(i, (i > 1).then(|| i - 1), "a0");
                    b.kind = "bullet".into();
                    b
                })
                .collect();
            let outcome = std::thread::Builder::new()
                .stack_size(1 << 20)
                .spawn(move || {
                    let object = object(1);
                    let rows = ObjectRows {
                        object: &object,
                        types: &[],
                        blocks: &rows,
                        relations: &[],
                        tags: &[],
                    };
                    let schema = Schema::default();
                    let doc = from_rows(&rows, &schema, &NoResolver);
                    let text = serialize(&doc, &schema, &NoResolver);
                    let parsed = parse(&text, &schema, &NoResolver).map(|p| p.doc);
                    let mut count = 0;
                    let mut deepest = 0;
                    let mut stack: Vec<(&FileBlock, usize)> =
                        doc.body.iter().map(|b| (b, 1)).collect();
                    while let Some((b, depth)) = stack.pop() {
                        count += 1;
                        deepest = deepest.max(depth);
                        stack.extend(b.children.iter().map(|c| (c, depth + 1)));
                    }
                    (parsed == Ok(doc), count, deepest)
                })
                .unwrap()
                .join()
                .unwrap();
            assert_eq!(outcome, (true, len as usize, MAX_BLOCK_DEPTH), "{len}");
        }
    }

    #[test]
    fn normalize_drops_normal_sensitivity() {
        let mut doc = FileDoc::default();
        doc.front.sensitivity = Some(Sensitivity::Normal);
        normalize(&mut doc);
        assert_eq!(doc.front.sensitivity, None);
        normalize(&mut doc);
        assert_eq!(doc, FileDoc::default());
    }

    #[test]
    fn assign_ids_walks_depth_first() {
        let leaf = |n: Option<u128>| FileBlock {
            id: n.map(id),
            kind: "paragraph".into(),
            content: json!({"inline": []}),
            children: Vec::new(),
        };
        let mut doc = FileDoc {
            front: Frontmatter::default(),
            body: vec![
                FileBlock {
                    children: vec![leaf(None), leaf(Some(5))],
                    ..leaf(None)
                },
                leaf(None),
            ],
        };
        let mut n = 0;
        assign_ids(&mut doc, &mut || {
            n += 1;
            id(n)
        });
        assert_eq!(doc.body[0].id, Some(id(1)));
        assert_eq!(doc.body[0].children[0].id, Some(id(2)));
        assert_eq!(doc.body[0].children[1].id, Some(id(5)));
        assert_eq!(doc.body[1].id, Some(id(3)));
    }
}
