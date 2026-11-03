// SPDX-License-Identifier: AGPL-3.0-or-later
//! The three-way merge on the model (ADR-0025, M5 Decision 17): frontmatter site by site, blocks
//! by id, `diffy` inside one block both sides changed.
//!
//! `ours` is the app's version and wins wherever a site differs three ways; `theirs` is the file.
//! Each side's value at a conflicting site is recorded in a [`Conflict`], so nothing either side
//! wrote is lost: it is in the document or in `conflicts`.
//!
//! Frontmatter: every envelope key, every user field key, every relation key (its list as a
//! whole), `links` as a whole (compared as sets by `(id, type, block)`) and every `extra` key is a
//! site; `types` and `tags` merge as sets and never conflict. Blocks: every id is a site with
//! three fields, parent, kind and content; a block one side deleted and the other edited is kept
//! (an edit/delete conflict); content both sides changed is merged line by line through `diffy`
//! when the kind has text (`inline` or `code`), and conflicts otherwise. The children of each
//! parent are ordered by merging the three id lists as lines; when that conflicts, ours' order
//! stands and theirs' extra blocks follow their nearest surviving predecessor from theirs.
//!
//! All three documents are normalised first, so formatting never reaches the merge, and the
//! result is in normal form. A block without an id (one a person typed; see `assign_ids`) exists
//! on its side alone and is kept as an addition of that side.

use std::collections::{BTreeSet, HashMap, HashSet};

use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions};
use serde::Serialize;
use serde_json::{Value, json};

use super::{
    FileBlock, FileDoc, Frontmatter, LinkEntry, LinkResolver, MAX_BLOCK_DEPTH, Schema, body,
    normalize,
};
use crate::Id;
use crate::block::BlockKind;

/// The result of a merge: the document, in normal form, and what could not be reconciled.
/// `conflicts` is empty exactly when the merge was clean.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    /// The merged document; `ours` wins wherever a conflict is recorded.
    pub doc: FileDoc,
    /// Every site that differed three ways.
    pub conflicts: Vec<Conflict>,
}

/// Where a conflict sits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ConflictSite {
    /// A user field, or an `extra` key, by key (the two never share a key: a field key is an
    /// identifier, an extra key is not or is reserved).
    Field(String),
    /// An envelope key (`summary`, `sort`, `geo`, …).
    Envelope(&'static str),
    /// A relation field's whole list, by key.
    Relations(String),
    /// The `links` list as a whole.
    Links,
    /// A block's kind, parent or content, or a block one side deleted and the other edited.
    Block(Id),
    /// The order of a parent's children; `None` for the top level.
    Children(Option<Id>),
}

/// One site that differed three ways. Every value a side wrote is here or in the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    /// Where.
    pub site: ConflictSite,
    /// The base's value at the site; `Null` when the base had none.
    pub base: Value,
    /// The app's value, which the document keeps; `Null` when ours deleted the block.
    pub ours: Value,
    /// The file's value; `Null` when theirs deleted the block.
    pub theirs: Value,
    /// For a text block both sides changed, or a children order that could not be merged:
    /// `diffy`'s diff3-style text.
    pub diff3: Option<String>,
}

/// Merges `theirs` (the file) into `ours` (the app's version) against their common `base`. Every
/// block of all three documents should have an id (see `assign_ids`); a block without one is kept
/// as an addition of its side. `ours` wins on conflict. The result is in normal form.
pub fn merge(
    base: &FileDoc,
    ours: &FileDoc,
    theirs: &FileDoc,
    _schema: &Schema,
    resolver: &dyn LinkResolver,
) -> Merged {
    let (mut base, mut ours, mut theirs) = (base.clone(), ours.clone(), theirs.clone());
    normalize(&mut base);
    normalize(&mut ours);
    normalize(&mut theirs);
    let mut conflicts = Vec::new();
    let front = merge_front(&base.front, &ours.front, &theirs.front, &mut conflicts);
    let body = merge_blocks(
        &base.body,
        &ours.body,
        &theirs.body,
        resolver,
        &mut conflicts,
    );
    let mut doc = FileDoc { front, body };
    normalize(&mut doc);
    Merged { doc, conflicts }
}

/// A value as a conflict records it.
fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The three-way rule on one site: a side that equals the base yields to the other, agreement
/// stands, and anything else is a conflict that ours wins. The flag says whether it conflicted.
fn three_way<T: PartialEq + Clone>(base: &T, ours: &T, theirs: &T) -> (T, bool) {
    if ours == base {
        (theirs.clone(), false)
    } else if theirs == base || ours == theirs {
        (ours.clone(), false)
    } else {
        (ours.clone(), true)
    }
}

/// [`three_way`] where the base may have no value at the site (a block both sides added).
fn three_way_opt<T: PartialEq + Clone>(base: Option<&T>, ours: &T, theirs: &T) -> (T, bool) {
    match base {
        Some(base) => three_way(base, ours, theirs),
        None => (ours.clone(), ours != theirs),
    }
}

fn conflict<T: Serialize>(site: ConflictSite, base: &T, ours: &T, theirs: &T) -> Conflict {
    Conflict {
        site,
        base: json(base),
        ours: json(ours),
        theirs: json(theirs),
        diff3: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Frontmatter
// ---------------------------------------------------------------------------------------------

fn merge_front(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    conflicts: &mut Vec<Conflict>,
) -> Frontmatter {
    /// One envelope key, by field.
    macro_rules! envelope {
        ($name:literal, $field:ident) => {
            envelope_site($name, &base.$field, &ours.$field, &theirs.$field, conflicts)
        };
    }
    let id = envelope!("id", id);
    let created = envelope!("created", created);
    let updated = envelope!("updated", updated);
    let summary = envelope!("summary", summary);
    let captured_at = envelope!("captured_at", captured_at);
    let event_start = envelope!("event_start", event_start);
    let event_end = envelope!("event_end", event_end);
    let geo = envelope!("geo", geo);
    let sensitivity = envelope!("sensitivity", sensitivity);
    let sort = envelope!("sort", sort);
    Frontmatter {
        id,
        types: merge_set(&base.types, &ours.types, &theirs.types),
        tags: merge_set(&base.tags, &ours.tags, &theirs.tags),
        created,
        updated,
        summary,
        captured_at,
        event_start,
        event_end,
        geo,
        sensitivity,
        sort,
        fields: merge_keyed(
            &base.fields,
            &ours.fields,
            &theirs.fields,
            |key| ConflictSite::Field(key.to_owned()),
            conflicts,
        ),
        relations: merge_keyed(
            &base.relations,
            &ours.relations,
            &theirs.relations,
            |key| ConflictSite::Relations(key.to_owned()),
            conflicts,
        ),
        links: merge_links(&base.links, &ours.links, &theirs.links, conflicts),
        extra: merge_keyed(
            &base.extra,
            &ours.extra,
            &theirs.extra,
            |key| ConflictSite::Field(key.to_owned()),
            conflicts,
        ),
    }
}

/// One envelope key.
fn envelope_site<T: PartialEq + Clone + Serialize>(
    name: &'static str,
    base: &T,
    ours: &T,
    theirs: &T,
    conflicts: &mut Vec<Conflict>,
) -> T {
    let (value, conflicted) = three_way(base, ours, theirs);
    if conflicted {
        conflicts.push(conflict(ConflictSite::Envelope(name), base, ours, theirs));
    }
    value
}

/// `types` and `tags`: `base ∪ (ours − base) ∪ (theirs − base) − (base − ours) − (base − theirs)`,
/// in the order of the side that changed the list (ours when both did), then what the other side
/// added. Never a conflict.
fn merge_set(base: &[String], ours: &[String], theirs: &[String]) -> Vec<String> {
    let (first, second) = if ours == base {
        (theirs, ours)
    } else {
        (ours, theirs)
    };
    let mut out: Vec<String> = Vec::new();
    for item in first {
        let deleted_by_second = base.contains(item) && !second.contains(item);
        if !deleted_by_second && !out.contains(item) {
            out.push(item.clone());
        }
    }
    for item in second {
        if !base.contains(item) && !out.contains(item) {
            out.push(item.clone());
        }
    }
    out
}

/// Keyed lists (user fields, relations by key, extras): every key is a site, absent counts as a
/// value. Keys come in the order of the side that changed the list (ours when both did), then the
/// other side's new keys.
fn merge_keyed<V: PartialEq + Clone + Serialize>(
    base: &[(String, V)],
    ours: &[(String, V)],
    theirs: &[(String, V)],
    site: impl Fn(&str) -> ConflictSite,
    conflicts: &mut Vec<Conflict>,
) -> Vec<(String, V)> {
    let lookup = |list: &[(String, V)], key: &str| -> Option<V> {
        list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let (first, second) = if ours == base {
        (theirs, ours)
    } else {
        (ours, theirs)
    };
    let keys = first
        .iter()
        .map(|(k, _)| k)
        .chain(second.iter().map(|(k, _)| k))
        .fold(Vec::<&String>::new(), |mut keys, key| {
            if !keys.contains(&key) {
                keys.push(key);
            }
            keys
        });
    let mut out = Vec::new();
    for key in keys {
        let (b, o, t) = (lookup(base, key), lookup(ours, key), lookup(theirs, key));
        let (value, conflicted) = three_way(&b, &o, &t);
        if conflicted {
            conflicts.push(conflict(site(key), &b, &o, &t));
        }
        if let Some(value) = value {
            out.push((key.clone(), value));
        }
    }
    out
}

/// `links` as a whole, compared as sets by `(id, type, block)`.
fn merge_links(
    base: &[LinkEntry],
    ours: &[LinkEntry],
    theirs: &[LinkEntry],
    conflicts: &mut Vec<Conflict>,
) -> Vec<LinkEntry> {
    let keys = |list: &[LinkEntry]| -> BTreeSet<(Option<Id>, String, Option<Id>)> {
        list.iter()
            .map(|l| (l.id, l.rel_type.clone(), l.block))
            .collect()
    };
    // Equal lists first, so a reorder or a relabel on one side alone is kept as written.
    if ours == base {
        return theirs.to_vec();
    }
    if theirs == base || ours == theirs {
        return ours.to_vec();
    }
    let (b, o, t) = (keys(base), keys(ours), keys(theirs));
    if o == b {
        theirs.to_vec()
    } else if t == b || o == t {
        ours.to_vec()
    } else {
        conflicts.push(conflict(ConflictSite::Links, &base, &ours, &theirs));
        ours.to_vec()
    }
}

// ---------------------------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------------------------

/// Which document a block without an id came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Side {
    Base,
    Ours,
    Theirs,
}

/// How a block is identified across the three documents: by its id, or, for a block without one,
/// by its side and its position in that side's depth-first order (so it never matches a block of
/// another document).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key {
    Id(Id),
    New(Side, usize),
}

impl Key {
    fn id(self) -> Option<Id> {
        match self {
            Self::Id(id) => Some(id),
            Self::New(..) => None,
        }
    }

    /// The line this key is to `diffy` when child lists are merged.
    fn line(self) -> String {
        match self {
            Self::Id(id) => id.to_string(),
            Self::New(side, n) => format!("new:{side:?}:{n}"),
        }
    }
}

/// One block's three merge fields.
#[derive(Clone, Debug, PartialEq)]
struct Node {
    parent: Option<Key>,
    kind: String,
    content: Value,
}

impl Node {
    /// The whole state, as an edit/delete conflict records it.
    fn json(&self) -> Value {
        json!({
            "parent": self.parent.and_then(Key::id),
            "kind": self.kind,
            "content": self.content,
        })
    }
}

/// One document's blocks, flat: every block by key, each parent's children in order, and the
/// keys in depth-first document order.
#[derive(Default)]
struct Index {
    nodes: HashMap<Key, Node>,
    children: HashMap<Option<Key>, Vec<Key>>,
    order: Vec<Key>,
}

impl Index {
    fn of(blocks: &[FileBlock], side: Side) -> Self {
        let mut index = Self::default();
        let mut fresh = 0;
        index.walk(blocks, None, side, &mut fresh);
        index
    }

    fn walk(&mut self, blocks: &[FileBlock], parent: Option<Key>, side: Side, fresh: &mut usize) {
        for block in blocks {
            let key = match block.id {
                Some(id) => Key::Id(id),
                None => {
                    *fresh += 1;
                    Key::New(side, *fresh)
                }
            };
            self.nodes.insert(
                key,
                Node {
                    parent,
                    kind: block.kind.clone(),
                    content: block.content.clone(),
                },
            );
            self.children.entry(parent).or_default().push(key);
            self.order.push(key);
            self.walk(&block.children, Some(key), side, fresh);
        }
    }

    fn list(&self, parent: Option<Key>) -> &[Key] {
        self.children.get(&parent).map_or(&[], Vec::as_slice)
    }
}

/// Whether both-changed content of this kind is merged as text (ADR-0025) rather than conflicting
/// whole: the kinds with `inline`, and `code`.
fn is_textual(kind: &str) -> bool {
    BlockKind::parse(kind).is_some_and(|k| k.has_inline() || k == BlockKind::Code)
}

/// What the text merge of one block decided.
enum TextMerge {
    Clean(Value),
    Conflict(Option<String>),
}

/// Merges content both sides changed as block text: each content rendered without anchor or
/// children, LF, no trailing newline; `diffy` with `Diff3` markers and `Git` incomplete hunks; a
/// clean result parsed back as one block of `kind`. A result that does not parse back is a
/// conflict without diff3 text.
fn merge_text(
    kind: &str,
    base: Option<&Value>,
    ours: &Value,
    theirs: &Value,
    resolver: &dyn LinkResolver,
) -> TextMerge {
    let render = |content: &Value| -> String {
        let text = body::render_block_text(kind, content, resolver);
        debug_assert!(
            !text.contains('\r') && !text.ends_with('\n'),
            "render_block_text must give LF text without a trailing newline"
        );
        text.replace("\r\n", "\n")
            .replace('\r', "\n")
            .trim_end_matches('\n')
            .to_owned()
    };
    let base_text = base.map(render).unwrap_or_default();
    let mut options = MergeOptions::new();
    options.set_conflict_style(ConflictStyle::Diff3);
    options.set_incomplete_hunk_style(IncompleteHunkStyle::Git);
    match options.merge(&base_text, &render(ours), &render(theirs)) {
        Ok(text) => match body::parse_block_text(kind, text.trim_end_matches('\n'), resolver) {
            Some(content) => TextMerge::Clean(content),
            None => TextMerge::Conflict(None),
        },
        Err(diff3) => TextMerge::Conflict(Some(diff3)),
    }
}

/// Kind and content of one block present on both sides. The kind is three-way on its own. The
/// content yields to the side that left it as the base had it and stands when both agree; both
/// changed is a text merge when both sides hold the same textual kind (the base's text counts
/// only when its kind is the same), and a conflict otherwise. A content conflict keeps ours' kind
/// too, so the block stays coherent; a kind that reverts this way is recorded as well.
fn merge_kind_and_content(
    key: Key,
    base: Option<&Node>,
    ours: &Node,
    theirs: &Node,
    resolver: &dyn LinkResolver,
    conflicts: &mut Vec<Conflict>,
) -> (String, Value) {
    let (kind, kind_conflict) = three_way_opt(base.map(|n| &n.kind), &ours.kind, &theirs.kind);
    let kinds = |conflicts: &mut Vec<Conflict>| {
        if let Some(id) = key.id() {
            conflicts.push(Conflict {
                site: ConflictSite::Block(id),
                base: base.map_or(Value::Null, |n| json(&n.kind)),
                ours: json(&ours.kind),
                theirs: json(&theirs.kind),
                diff3: None,
            });
        }
    };
    if kind_conflict {
        kinds(conflicts);
    }
    let base_content = base.map(|n| &n.content);
    if base_content == Some(&ours.content) {
        return (kind, theirs.content.clone());
    }
    if base_content == Some(&theirs.content) || ours.content == theirs.content {
        return (kind, ours.content.clone());
    }
    let decision = if ours.kind == theirs.kind && is_textual(&kind) {
        let base_text = base.filter(|n| n.kind == kind).map(|n| &n.content);
        merge_text(&kind, base_text, &ours.content, &theirs.content, resolver)
    } else {
        TextMerge::Conflict(None)
    };
    match decision {
        TextMerge::Clean(content) => (kind, content),
        TextMerge::Conflict(diff3) => {
            if let Some(id) = key.id() {
                conflicts.push(Conflict {
                    site: ConflictSite::Block(id),
                    base: base_content.cloned().unwrap_or(Value::Null),
                    ours: ours.content.clone(),
                    theirs: theirs.content.clone(),
                    diff3,
                });
            }
            if kind != ours.kind {
                kinds(conflicts);
            }
            (ours.kind.clone(), ours.content.clone())
        }
    }
}

/// Merges the three block trees by id.
fn merge_blocks(
    base: &[FileBlock],
    ours: &[FileBlock],
    theirs: &[FileBlock],
    resolver: &dyn LinkResolver,
    conflicts: &mut Vec<Conflict>,
) -> Vec<FileBlock> {
    let b = Index::of(base, Side::Base);
    let o = Index::of(ours, Side::Ours);
    let t = Index::of(theirs, Side::Theirs);

    // Every key once, in ours' order, then theirs' new ones, then what only the base had.
    let mut union: Vec<Key> = Vec::new();
    let mut seen = HashSet::new();
    for key in o.order.iter().chain(&t.order).chain(&b.order) {
        if seen.insert(*key) {
            union.push(*key);
        }
    }

    // Which blocks survive, with what parent, kind and content.
    let mut result: HashMap<Key, Node> = HashMap::new();
    let mut survivors: Vec<Key> = Vec::new();
    for key in &union {
        let node = match (b.nodes.get(key), o.nodes.get(key), t.nodes.get(key)) {
            (_, None, None) => None,
            (None, Some(node), None) | (None, None, Some(node)) => Some(node.clone()),
            (Some(bn), Some(on), None) => edit_or_delete(*key, bn, on, Side::Ours, conflicts),
            (Some(bn), None, Some(tn)) => edit_or_delete(*key, bn, tn, Side::Theirs, conflicts),
            (bn, Some(on), Some(tn)) => Some(merge_node(*key, bn, on, tn, resolver, conflicts)),
        };
        if let Some(node) = node {
            result.insert(*key, node);
            survivors.push(*key);
        }
    }

    // The children of each surviving parent, and of the top level, in merged order.
    let mut wanted: HashMap<Option<Key>, Vec<Key>> = HashMap::new();
    for key in &survivors {
        if let Some(node) = result.get(key) {
            wanted.entry(node.parent).or_default().push(*key);
        }
    }
    let parents = std::iter::once(None).chain(survivors.iter().map(|k| Some(*k)));
    let mut orders: HashMap<Option<Key>, Vec<Key>> = HashMap::new();
    for parent in parents {
        if let Some(members) = wanted.get(&parent) {
            let order = merge_order(parent, members, &b, &o, &t, conflicts);
            orders.insert(parent, order);
        }
    }

    // The tree, from the top; a block whose parent is gone, sits in a cycle or would sit past the
    // depth cap joins the top level.
    let mut placed = HashSet::new();
    let mut top = build(None, &orders, &result, &mut placed, 0);
    for key in &survivors {
        if !placed.contains(key) {
            top.push(build_block(*key, &orders, &result, &mut placed, 0));
        }
    }
    top
}

/// A block one side deleted (`survivor` is the other): gone when the survivor left it as the
/// base had it, kept with an edit/delete conflict otherwise.
fn edit_or_delete(
    key: Key,
    base: &Node,
    kept: &Node,
    survivor: Side,
    conflicts: &mut Vec<Conflict>,
) -> Option<Node> {
    if kept == base {
        return None;
    }
    if let Some(id) = key.id() {
        let (ours, theirs) = match survivor {
            Side::Ours => (kept.json(), Value::Null),
            _ => (Value::Null, kept.json()),
        };
        conflicts.push(Conflict {
            site: ConflictSite::Block(id),
            base: base.json(),
            ours,
            theirs,
            diff3: None,
        });
    }
    Some(kept.clone())
}

/// A block present on both sides: parent, kind and content each three-way.
fn merge_node(
    key: Key,
    base: Option<&Node>,
    ours: &Node,
    theirs: &Node,
    resolver: &dyn LinkResolver,
    conflicts: &mut Vec<Conflict>,
) -> Node {
    let (parent, parent_conflict) =
        three_way_opt(base.map(|n| &n.parent), &ours.parent, &theirs.parent);
    if parent_conflict && let Some(id) = key.id() {
        let parent_id = |p: Option<Key>| json(&p.and_then(Key::id));
        conflicts.push(Conflict {
            site: ConflictSite::Block(id),
            base: base.map_or(Value::Null, |n| parent_id(n.parent)),
            ours: parent_id(ours.parent),
            theirs: parent_id(theirs.parent),
            diff3: None,
        });
    }
    let (kind, content) = merge_kind_and_content(key, base, ours, theirs, resolver, conflicts);
    Node {
        parent,
        kind,
        content,
    }
}

/// The order of one parent's surviving children (`members`, in union order): each side's child
/// list, kept to the members, as lines through `diffy`. A clean merge is the order; a conflict
/// takes ours' order, places theirs' extra blocks right after their nearest surviving predecessor
/// in theirs (or at the end) and records a `Children` conflict. A member no list placed follows
/// its predecessor in ours or theirs the same way.
fn merge_order(
    parent: Option<Key>,
    members: &[Key],
    base: &Index,
    ours: &Index,
    theirs: &Index,
    conflicts: &mut Vec<Conflict>,
) -> Vec<Key> {
    let member_set: HashSet<Key> = members.iter().copied().collect();
    let kept = |index: &Index| -> Vec<Key> {
        index
            .list(parent)
            .iter()
            .copied()
            .filter(|k| member_set.contains(k))
            .collect()
    };
    let (b, o, t) = (kept(base), kept(ours), kept(theirs));
    let lines = |keys: &[Key]| -> String { keys.iter().map(|k| k.line() + "\n").collect() };
    let by_line: HashMap<String, Key> = b
        .iter()
        .chain(&o)
        .chain(&t)
        .map(|k| (k.line(), *k))
        .collect();

    let mut options = MergeOptions::new();
    options.set_conflict_style(ConflictStyle::Diff3);
    options.set_incomplete_hunk_style(IncompleteHunkStyle::Git);
    // `placed` mirrors `order` as a set, so one parent's many children cost no quadratic scan.
    let mut order: Vec<Key> = Vec::new();
    let mut placed: HashSet<Key> = HashSet::new();
    let positions = |keys: &[Key]| -> HashMap<Key, usize> {
        keys.iter().enumerate().map(|(i, k)| (*k, i)).collect()
    };
    let (o_at, t_at) = (positions(&o), positions(&t));
    match options.merge(&lines(&b), &lines(&o), &lines(&t)) {
        Ok(text) => {
            for key in text.lines().filter_map(|line| by_line.get(line)) {
                if placed.insert(*key) {
                    order.push(*key);
                }
            }
        }
        Err(diff3) => {
            order.clone_from(&o);
            placed.extend(o.iter().copied());
            for key in &t {
                if !placed.contains(key) {
                    place(&mut order, &mut placed, *key, &t, &t_at);
                }
            }
            let names = |keys: &[Key]| -> Vec<String> { keys.iter().map(|k| k.line()).collect() };
            conflicts.push(Conflict {
                site: ConflictSite::Children(parent.and_then(Key::id)),
                base: json(&names(&b)),
                ours: json(&names(&o)),
                theirs: json(&names(&t)),
                diff3: Some(diff3),
            });
        }
    }
    for key in members {
        if !placed.contains(key) {
            if o_at.contains_key(key) {
                place(&mut order, &mut placed, *key, &o, &o_at);
            } else {
                place(&mut order, &mut placed, *key, &t, &t_at);
            }
        }
    }
    order
}

/// Inserts `key` into `order` right after its nearest predecessor in `side` that `order` already
/// holds, or at the end when none does. `side_at` is each key's index in `side`; `placed` is
/// kept in step with `order`.
fn place(
    order: &mut Vec<Key>,
    placed: &mut HashSet<Key>,
    key: Key,
    side: &[Key],
    side_at: &HashMap<Key, usize>,
) {
    let at = side_at.get(&key).and_then(|&i| {
        side[..i]
            .iter()
            .rev()
            .find(|p| placed.contains(p))
            .and_then(|p| order.iter().position(|k| k == p))
    });
    placed.insert(key);
    match at {
        Some(i) => order.insert(i + 1, key),
        None => order.push(key),
    }
}

/// The children of `parent` at `depth`, built in merged order; none past [`MAX_BLOCK_DEPTH`]
/// (the caller places them at the top level, like a block whose parent is gone), so the
/// recursion is bounded by the cap even when the sides' parent links chain deeper together than
/// either side alone. `placed` guards against a parent cycle.
fn build(
    parent: Option<Key>,
    orders: &HashMap<Option<Key>, Vec<Key>>,
    result: &HashMap<Key, Node>,
    placed: &mut HashSet<Key>,
    depth: usize,
) -> Vec<FileBlock> {
    let mut out = Vec::new();
    if depth >= MAX_BLOCK_DEPTH {
        return out;
    }
    for key in orders.get(&parent).into_iter().flatten() {
        if !placed.contains(key) && result.contains_key(key) {
            out.push(build_block(*key, orders, result, placed, depth));
        }
    }
    out
}

fn build_block(
    key: Key,
    orders: &HashMap<Option<Key>, Vec<Key>>,
    result: &HashMap<Key, Node>,
    placed: &mut HashSet<Key>,
    depth: usize,
) -> FileBlock {
    placed.insert(key);
    let node = result.get(&key);
    FileBlock {
        id: key.id(),
        kind: node.map(|n| n.kind.clone()).unwrap_or_default(),
        content: node.map(|n| n.content.clone()).unwrap_or(Value::Null),
        children: build(Some(key), orders, result, placed, depth + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u128) -> Key {
        Key::Id(Id::from_parts(1, n))
    }

    #[test]
    fn three_way_rules() {
        assert_eq!(three_way(&1, &1, &2), (2, false));
        assert_eq!(three_way(&1, &2, &1), (2, false));
        assert_eq!(three_way(&1, &2, &2), (2, false));
        assert_eq!(three_way(&1, &2, &3), (2, true));
        assert_eq!(three_way_opt(None, &2, &2), (2, false));
        assert_eq!(three_way_opt(None, &2, &3), (2, true));
        assert_eq!(three_way_opt(Some(&1), &1, &3), (3, false));
    }

    #[test]
    fn sets_merge_in_ours_order_then_theirs_additions() {
        let s = |items: &[&str]| -> Vec<String> { items.iter().map(|i| (*i).to_owned()).collect() };
        let merged = merge_set(
            &s(&["a", "b", "c"]),
            &s(&["c", "a", "x"]),
            &s(&["b", "c", "y"]),
        );
        assert_eq!(merged, s(&["c", "x", "y"]));
        assert_eq!(merge_set(&[], &s(&["a", "a"]), &s(&["a"])), s(&["a"]));
    }

    #[test]
    fn place_follows_the_nearest_surviving_predecessor() {
        let at = |side: &[Key]| -> HashMap<Key, usize> {
            side.iter().enumerate().map(|(i, k)| (*k, i)).collect()
        };
        let mut order = vec![key(1), key(3)];
        let mut placed: HashSet<Key> = order.iter().copied().collect();
        let side = [key(1), key(2), key(4), key(3)];
        place(&mut order, &mut placed, key(4), &side, &at(&side));
        assert_eq!(order, [key(1), key(4), key(3)]);
        let side = [key(5), key(1)];
        place(&mut order, &mut placed, key(5), &side, &at(&side));
        assert_eq!(order, [key(1), key(4), key(3), key(5)]);
        place(&mut order, &mut placed, key(6), &[], &HashMap::new());
        assert_eq!(order.last(), Some(&key(6)));
        assert_eq!(placed.len(), order.len());
    }

    #[test]
    fn keys_without_ids_never_collide() {
        assert_ne!(Key::New(Side::Ours, 1), Key::New(Side::Theirs, 1));
        assert_eq!(Key::New(Side::Base, 2).id(), None);
        assert_eq!(Key::New(Side::Ours, 2).line(), "new:Ours:2");
        assert_eq!(key(7).line().len(), 26);
        assert!(is_textual("paragraph") && is_textual("code") && is_textual("todo"));
        assert!(!is_textual("divider") && !is_textual("image") && !is_textual("chart"));
    }
}
