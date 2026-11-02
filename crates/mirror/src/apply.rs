// SPDX-License-Identifier: AGPL-3.0-or-later
//! Putting a document into an object: the ops that make the vault's rows for `object_id` render
//! to `doc` again. Everything is checked before the first service call, because a service that
//! fails marks the transaction so it cannot commit (`Txn::guard` is sticky): a bad value is
//! reported and skipped, never allowed to fail the whole file.

use std::collections::{BTreeMap, HashMap, HashSet};

use dbme_core::Id;
use dbme_core::block::{self, MENTIONS};
use dbme_core::files::{FileBlock, FileDoc, Schema, TAGGED};
use dbme_core::model::{FieldRow, Props, Sensitivity};
use dbme_core::value::validate_field_value;
use dbme_store::{NewBlock, NewObject, NewRelation, Position, Txn};
use serde_json::Value;

/// The store's result: everything here runs inside a store transaction.
use dbme_store::Result;

/// The key under which `props_json` keeps the frontmatter's unknown keys.
const FRONTMATTER_PROP: &str = "_frontmatter";

/// What the application of a document reported.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Applied {
    /// Values the file held that do not fit their field, as `(key, why)`; the object keeps its own.
    pub invalid: Vec<(String, String)>,
    /// Type names the file used that the vault does not have; they are not added.
    pub unknown_types: Vec<String>,
    /// Relation targets the file names that are not live objects; those relations are not made.
    pub missing_targets: Vec<Id>,
}

/// The object's new title and place, when the file's path says so.
#[derive(Clone, Debug, Default)]
pub(crate) struct Placement {
    /// A new title, from the file's name.
    pub title: Option<String>,
    /// A new parent (`Some(None)` is the top level), from the file's folder.
    pub parent: Option<Option<Id>>,
}

/// Makes a new object for `doc` and fills it. `id` is the file's `id:` to adopt, or `None`.
pub(crate) fn create_from_doc(
    txn: &mut Txn<'_>,
    id: Option<Id>,
    title: &str,
    parent: Option<Id>,
    doc: &FileDoc,
    tags: &[String],
    schema: &Schema,
) -> Result<(Id, Applied)> {
    let object_id = txn.create_object(NewObject {
        id,
        created_at: doc.front.created,
        title: title.to_owned(),
        parent_id: parent,
        sensitivity: doc.front.sensitivity.unwrap_or_default(),
        ..NewObject::default()
    })?;
    let applied = apply_doc(txn, object_id, &Placement::default(), doc, tags, schema)?;
    Ok((object_id, applied))
}

/// Changes `object_id` so that it renders to `doc`: its types, envelope, fields, tags, relations,
/// links and blocks. `tags` are the file's tags, frontmatter and body together. Every block of
/// `doc` must have an id (see `assign_ids`).
pub(crate) fn apply_doc(
    txn: &mut Txn<'_>,
    object_id: Id,
    placement: &Placement,
    doc: &FileDoc,
    tags: &[String],
    schema: &Schema,
) -> Result<Applied> {
    let mut applied = Applied::default();
    let row = txn.read().object(object_id)?;

    // Types.
    let current_types = txn.read().object_type_ids(object_id)?;
    let mut wanted_types = Vec::new();
    for name in &doc.front.types {
        match schema.type_by_name(name) {
            Some(t) => {
                if !wanted_types.contains(&t.id) {
                    wanted_types.push(t.id);
                }
            }
            None => applied.unknown_types.push(name.clone()),
        }
    }
    let type_names: Vec<String> = wanted_types
        .iter()
        .filter_map(|id| schema.type_by_id(*id).map(|t| t.name.clone()))
        .collect();
    for type_id in &wanted_types {
        if !current_types.contains(type_id) {
            txn.add_type(object_id, *type_id)?;
        }
    }
    for type_id in &current_types {
        // A type the schema does not know stays: the file could not have named it.
        if !wanted_types.contains(type_id) && schema.type_by_id(*type_id).is_some() {
            txn.remove_type(object_id, *type_id)?;
        }
    }

    // Envelope and fields.
    let mut props: Props = Props::new();
    for (key, value) in &doc.front.fields {
        if value.is_null() {
            continue;
        }
        match schema.field(&type_names, key) {
            Some(field) => match validate_field_value(field, value) {
                Ok(()) => {
                    props.insert(key.clone(), value.clone());
                }
                Err(e) => {
                    applied.invalid.push((key.clone(), e.to_string()));
                    if let Some(kept) = row.props_json.get(key) {
                        props.insert(key.clone(), kept.clone());
                    }
                }
            },
            None => {
                props.insert(key.clone(), value.clone());
            }
        }
    }
    if !doc.front.extra.is_empty() {
        let extra: serde_json::Map<String, Value> = doc
            .front
            .extra
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        props.insert(FRONTMATTER_PROP.to_owned(), Value::Object(extra));
    }
    let front = &doc.front;
    let title = placement.title.clone();
    txn.update_object(object_id, |o| {
        if let Some(title) = title {
            o.title = title;
        }
        o.summary = front.summary.clone();
        o.captured_at = front.captured_at;
        o.event_start = front.event_start;
        o.event_end = front.event_end;
        o.geo_lat = front.geo.map(|(lat, _)| lat);
        o.geo_lon = front.geo.map(|(_, lon)| lon);
        o.sensitivity = front.sensitivity.unwrap_or(Sensitivity::Normal);
        o.props_json = props;
    })?;

    // Place.
    if let Some(parent) = placement.parent
        && parent != row.parent_id
        && parent_is_safe(txn, object_id, parent)?
    {
        txn.move_object(object_id, parent, Position::Last)?;
    }

    // Tags: `tagged` relations to Tag objects by title, matched case-insensitively.
    let tag_type = schema.type_by_name("Tag").map(|t| t.id);
    let current_relations: Vec<_> = txn
        .read()
        .relations_from(object_id)?
        .into_iter()
        .filter(|r| r.rel_type != MENTIONS)
        .collect();
    let mut wanted_tags: Vec<&str> = Vec::new();
    for tag in tags {
        if !wanted_tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            wanted_tags.push(tag);
        }
    }
    let mut kept_tags: HashSet<String> = HashSet::new();
    for relation in current_relations.iter().filter(|r| r.rel_type == TAGGED) {
        let title = txn.read().object(relation.dst_id).map(|o| o.title)?;
        if wanted_tags.iter().any(|t| t.eq_ignore_ascii_case(&title)) {
            kept_tags.insert(title.to_lowercase());
        } else {
            txn.trash_relation(relation.id)?;
        }
    }
    for tag in wanted_tags {
        if kept_tags.contains(&tag.to_lowercase()) {
            continue;
        }
        let dst_id = tag_object(txn, tag, tag_type)?;
        if dst_id == object_id {
            continue;
        }
        txn.create_relation(NewRelation {
            src_id: object_id,
            dst_id,
            rel_type: TAGGED.to_owned(),
            field_id: None,
            src_block_id: None,
            label: None,
            props: Props::new(),
        })?;
    }

    // Relations with a field, and links without one.
    type RelKey = (Id, Option<Id>, String, Option<Id>);
    let mut wanted: BTreeMap<RelKey, (Option<String>, Props)> = BTreeMap::new();
    for (key, refs) in &doc.front.relations {
        let Some(field) = schema.field(&type_names, key) else {
            applied
                .invalid
                .push((key.clone(), "no relation field with this key".to_owned()));
            continue;
        };
        for r in refs {
            if let Some(dst) = r.id {
                wanted.insert(
                    (dst, Some(field.id), "related".to_owned(), None),
                    (None, Props::new()),
                );
            }
        }
    }
    for link in &doc.front.links {
        if let Some(dst) = link.id {
            wanted.insert(
                (dst, None, link.rel_type.clone(), link.block),
                (link.label.clone(), link.props.clone()),
            );
        }
    }
    let rel_type_of = |field: Option<&FieldRow>, given: &str| -> String {
        field.map_or(given.to_owned(), |_| "related".to_owned())
    };
    let mut present: HashSet<RelKey> = HashSet::new();
    for relation in current_relations.iter().filter(|r| r.rel_type != TAGGED) {
        let key: RelKey = (
            relation.dst_id,
            relation.field_id,
            relation.rel_type.clone(),
            relation.src_block_id,
        );
        if wanted.contains_key(&key) {
            present.insert(key.clone());
            let (label, props) = &wanted[&key];
            if relation.field_id.is_none()
                && (&relation.label != label || &relation.props_json != props)
            {
                let (label, props) = (label.clone(), props.clone());
                txn.update_relation(relation.id, |r| {
                    r.label = label;
                    r.props_json = props;
                })?;
            }
        } else {
            txn.trash_relation(relation.id)?;
        }
    }
    // Blocks first: a link may name its source block.
    apply_blocks(txn, object_id, &doc.body, &mut applied)?;
    let block_ids: HashSet<Id> = txn.read().blocks(object_id)?.iter().map(|b| b.id).collect();
    for (key, (label, props)) in wanted {
        if present.contains(&key) {
            continue;
        }
        let (dst_id, field_id, rel_type, src_block_id) = key;
        let live = txn
            .read()
            .object(dst_id)
            .map(|o| o.deleted_at.is_none())
            .unwrap_or(false);
        if !live {
            applied.missing_targets.push(dst_id);
            continue;
        }
        let src_block_id = src_block_id.filter(|b| block_ids.contains(b));
        let field = field_id.and_then(|id| schema.field_by_id(id).map(|(_, f)| f));
        txn.create_relation(NewRelation {
            src_id: object_id,
            dst_id,
            rel_type: rel_type_of(field, &rel_type),
            field_id,
            src_block_id,
            label,
            props,
        })?;
    }

    Ok(applied)
}

/// Whether `parent` can take `object_id` without a loop.
fn parent_is_safe(txn: &Txn<'_>, object_id: Id, parent: Option<Id>) -> Result<bool> {
    let mut ancestor = parent;
    while let Some(above) = ancestor {
        if above == object_id {
            return Ok(false);
        }
        match txn.read().object(above) {
            Ok(row) if row.deleted_at.is_none() => ancestor = row.parent_id,
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// The Tag object with this title, made if there is none.
fn tag_object(txn: &mut Txn<'_>, title: &str, tag_type: Option<Id>) -> Result<Id> {
    for candidate in txn.read().objects_titled(None, title)? {
        let types = txn.read().object_type_ids(candidate.id)?;
        if tag_type.is_none_or(|t| types.contains(&t)) {
            return Ok(candidate.id);
        }
    }
    // Titles are matched case-insensitively, as tags are.
    let id = txn.create_object(NewObject {
        title: title.to_owned(),
        type_ids: tag_type.into_iter().collect(),
        ..NewObject::default()
    })?;
    // A Tag object is an object like any other: it gets a file. The transaction is the folder's
    // own, which marks nothing by itself.
    txn.mirror_dirty_mark(&[id])?;
    Ok(id)
}

/// Makes the object's blocks match `wanted`: creates, updates, moves, then deletes what is left.
fn apply_blocks(
    txn: &mut Txn<'_>,
    object_id: Id,
    wanted: &[FileBlock],
    applied: &mut Applied,
) -> Result<()> {
    let current = txn.read().blocks(object_id)?;
    let mut by_id: HashMap<Id, (Option<Id>, String, Value)> = current
        .iter()
        .map(|b| {
            (
                b.id,
                (b.parent_block_id, b.kind.clone(), b.content_json.clone()),
            )
        })
        .collect();
    let mut seen: HashSet<Id> = HashSet::new();
    let mut stack: Vec<(Option<Id>, &[FileBlock])> = vec![(None, wanted)];
    while let Some((parent, siblings)) = stack.pop() {
        let mut prev: Option<Id> = None;
        for block in siblings {
            let Some(id) = block.id else {
                continue;
            };
            if seen.contains(&id) {
                // A duplicate anchor in the file: the first wins.
                continue;
            }
            seen.insert(id);
            let has_children = block
                .children
                .iter()
                .any(|c| c.id.is_some_and(|cid| !seen.contains(&cid)));
            let kind_ok = block::validate_block(&block.kind, &block.content).is_ok()
                && (block::may_have_children(&block.kind) || !has_children);
            let position = match prev {
                Some(p) => Position::After(p),
                None => Position::First,
            };
            match by_id.get(&id).cloned() {
                None => {
                    if !kind_ok {
                        applied.invalid.push((
                            format!("block {id}"),
                            "the block does not fit its kind".into(),
                        ));
                        continue;
                    }
                    txn.create_block(NewBlock {
                        id: Some(id),
                        object_id,
                        parent_block_id: parent,
                        position,
                        kind: block.kind.clone(),
                        content: block.content.clone(),
                    })?;
                    by_id.insert(id, (parent, block.kind.clone(), block.content.clone()));
                }
                Some((current_parent, kind, content)) => {
                    if (&kind, &content) != (&block.kind, &block.content) {
                        if kind_ok {
                            let (k, c) = (block.kind.clone(), block.content.clone());
                            txn.update_block(id, |b| {
                                b.kind = k;
                                b.content_json = c;
                            })?;
                        } else {
                            applied.invalid.push((
                                format!("block {id}"),
                                "the block does not fit its kind".into(),
                            ));
                        }
                    }
                    // Always re-place: the order of siblings is only right if every one is
                    // moved after its predecessor in turn. A move to where a block already is
                    // writes no op.
                    let _ = current_parent;
                    if parent.is_none_or(|p| by_id.contains_key(&p)) {
                        txn.move_block(id, parent, position)?;
                    }
                }
            }
            prev = Some(id);
            if !block.children.is_empty() && block::may_have_children(&block.kind) {
                stack.push((Some(id), &block.children));
            }
        }
    }
    // Leftovers: the top-most first, since deleting a block deletes its subtree.
    let leftovers: Vec<Id> = current
        .iter()
        .filter(|b| !seen.contains(&b.id))
        .map(|b| b.id)
        .collect();
    for id in leftovers {
        if txn.read().block(id).is_ok() {
            txn.delete_block(id)?;
        }
    }
    Ok(())
}
