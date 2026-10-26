// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixtures for the end-to-end tests: a small tree of typed objects with blocks and covers, or a
//! bulk set of objects for the 100k-row table budget. Seeding goes through the same store services
//! as everything else, in one transaction.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use dbme_api::Reply;
use dbme_core::Id;
use dbme_core::model::{AttachmentRole, FieldKind, Props};
use dbme_store::{
    NewAttachment, NewBlock, NewField, NewObject, NewType, Position, StoreError, Txn,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// What to create.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Seed {
    /// Types with fields, then objects with their children, values, blocks and covers.
    Tree {
        #[serde(default)]
        types: Vec<SeedType>,
        objects: Vec<SeedObject>,
    },
    /// `count` objects of a new type with four fields, under a new top-level parent.
    Bulk {
        count: u32,
        type_name: String,
        parent_title: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeedType {
    name: String,
    #[serde(default)]
    fields: Vec<SeedField>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeedField {
    key: String,
    label: String,
    kind: FieldKind,
    #[serde(default)]
    options: Option<Value>,
}

/// An object. Titles must be unique across the seed: the reply maps them to IDs, and a mention
/// written `{"mention": "title:<title>"}` is resolved to that object.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeedObject {
    title: String,
    #[serde(default)]
    types: Vec<String>,
    #[serde(default)]
    props: Props,
    #[serde(default)]
    blocks: Vec<SeedBlock>,
    #[serde(default)]
    children: Vec<SeedObject>,
    /// Bytes of a PNG to attach as the cover, base64.
    #[serde(default)]
    cover_png: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SeedBlock {
    kind: String,
    content: Value,
    #[serde(default)]
    children: Vec<SeedBlock>,
}

fn rule(message: impl Into<String>) -> StoreError {
    StoreError::Rule(message.into())
}

/// Creates `seed` in one transaction and returns the IDs it made.
pub(crate) fn run(session: &dbme_api::Session, seed: Seed) -> Reply<Value> {
    session.with(|vault| {
        let covers = match &seed {
            Seed::Tree { objects, .. } => {
                let mut covers = BTreeMap::new();
                collect_covers(vault, objects, &mut covers)?;
                covers
            }
            Seed::Bulk { .. } => BTreeMap::new(),
        };
        let (ids, _) = vault.transact(|txn| match seed {
            Seed::Tree { types, objects } => tree(txn, types, objects, &covers),
            Seed::Bulk {
                count,
                type_name,
                parent_title,
            } => bulk(txn, count, &type_name, &parent_title),
        })?;
        Ok(ids)
    })
}

/// Puts each cover in the blob store first: the transaction only attaches them.
fn collect_covers(
    vault: &dbme_store::Vault,
    objects: &[SeedObject],
    covers: &mut BTreeMap<String, dbme_core::ContentHash>,
) -> Result<(), StoreError> {
    for object in objects {
        if let Some(data) = &object.cover_png {
            let bytes = BASE64
                .decode(data)
                .map_err(|e| rule(format!("cover_png is not base64: {e}")))?;
            covers.insert(object.title.clone(), vault.blobs().put(&bytes)?.hash);
        }
        collect_covers(vault, &object.children, covers)?;
    }
    Ok(())
}

fn tree(
    txn: &mut Txn<'_>,
    types: Vec<SeedType>,
    objects: Vec<SeedObject>,
    covers: &BTreeMap<String, dbme_core::ContentHash>,
) -> Result<Value, StoreError> {
    let mut type_ids: BTreeMap<String, Id> = txn
        .read()
        .types()?
        .into_iter()
        .map(|t| (t.name, t.id))
        .collect();
    for seed_type in types {
        let id = txn.create_type(NewType {
            name: seed_type.name.clone(),
            ..Default::default()
        })?;
        for field in seed_type.fields {
            txn.create_field(NewField {
                type_id: id,
                key: field.key,
                label: field.label,
                kind: field.kind,
                options: field.options.unwrap_or_else(|| json!({})),
                required: false,
            })?;
        }
        type_ids.insert(seed_type.name, id);
    }
    let mut ids = BTreeMap::new();
    let mut pending_blocks = Vec::new();
    create_objects(txn, objects, None, &type_ids, &mut ids, &mut pending_blocks)?;
    for (object_id, blocks) in pending_blocks {
        create_blocks(txn, object_id, None, blocks, &ids)?;
    }
    for (title, hash) in covers {
        let object_id = *ids
            .get(title)
            .ok_or_else(|| rule(format!("no object {title:?}")))?;
        txn.attach(NewAttachment {
            object_id,
            blob_hash: hash.clone(),
            role: AttachmentRole::Original,
            filename: Some(format!("{title}.png")),
            mime: Some("image/png".into()),
        })?;
    }
    Ok(json!({ "ids": ids, "types": type_ids }))
}

fn create_objects(
    txn: &mut Txn<'_>,
    objects: Vec<SeedObject>,
    parent: Option<Id>,
    type_ids: &BTreeMap<String, Id>,
    ids: &mut BTreeMap<String, Id>,
    pending_blocks: &mut Vec<(Id, Vec<SeedBlock>)>,
) -> Result<(), StoreError> {
    for object in objects {
        let types = object
            .types
            .iter()
            .map(|name| {
                type_ids
                    .get(name)
                    .copied()
                    .ok_or_else(|| rule(format!("no type {name:?}")))
            })
            .collect::<Result<Vec<Id>, StoreError>>()?;
        let id = txn.create_object(NewObject {
            title: object.title.clone(),
            parent_id: parent,
            position: Position::Last,
            type_ids: types,
            props: object.props,
            ..Default::default()
        })?;
        if ids.insert(object.title.clone(), id).is_some() {
            return Err(rule(format!(
                "two seed objects are titled {:?}",
                object.title
            )));
        }
        pending_blocks.push((id, object.blocks));
        create_objects(
            txn,
            object.children,
            Some(id),
            type_ids,
            ids,
            pending_blocks,
        )?;
    }
    Ok(())
}

/// Replaces `{"mention": "title:<title>"}` with the object's ID.
fn resolve_mentions(content: &mut Value, ids: &BTreeMap<String, Id>) -> Result<(), StoreError> {
    let runs = content.get_mut("inline").and_then(Value::as_array_mut);
    for run in runs.into_iter().flatten() {
        if let Some(title) = run
            .get("mention")
            .and_then(Value::as_str)
            .and_then(|m| m.strip_prefix("title:"))
        {
            let id = ids
                .get(title)
                .ok_or_else(|| rule(format!("no object {title:?} to mention")))?;
            run["mention"] = Value::String(id.to_string());
        }
    }
    Ok(())
}

fn create_blocks(
    txn: &mut Txn<'_>,
    object_id: Id,
    parent: Option<Id>,
    blocks: Vec<SeedBlock>,
    ids: &BTreeMap<String, Id>,
) -> Result<(), StoreError> {
    for mut block in blocks {
        resolve_mentions(&mut block.content, ids)?;
        let id = txn.create_block(NewBlock {
            id: None,
            object_id,
            parent_block_id: parent,
            position: Position::Last,
            kind: block.kind,
            content: block.content,
        })?;
        create_blocks(txn, object_id, Some(id), block.children, ids)?;
    }
    Ok(())
}

const STATUSES: [&str; 3] = ["todo", "doing", "done"];

fn bulk(
    txn: &mut Txn<'_>,
    count: u32,
    type_name: &str,
    parent_title: &str,
) -> Result<Value, StoreError> {
    let type_id = txn.create_type(NewType {
        name: type_name.to_owned(),
        ..Default::default()
    })?;
    let options = json!({"options": STATUSES.map(|s| json!({"id": s, "label": s}))});
    let fields = [
        ("status", "Status", FieldKind::Select, options),
        ("due", "Due", FieldKind::Date, json!({})),
        ("estimate", "Estimate", FieldKind::Number, json!({})),
        ("done", "Done", FieldKind::Boolean, json!({})),
    ];
    for (key, label, kind, options) in fields {
        txn.create_field(NewField {
            type_id,
            key: key.into(),
            label: label.into(),
            kind,
            options,
            required: false,
        })?;
    }
    let parent_id = txn.create_object(NewObject {
        title: parent_title.to_owned(),
        ..Default::default()
    })?;
    // Varied but repeatable values: the same seed gives the same table.
    for n in 0..count {
        let status = STATUSES[(n % 3) as usize];
        let mut props = Props::new();
        props.insert("status".into(), json!(status));
        props.insert(
            "due".into(),
            json!(format!("2026-{:02}-{:02}", n % 12 + 1, n % 28 + 1)),
        );
        props.insert("estimate".into(), json!(n % 13));
        props.insert("done".into(), json!(status == "done"));
        txn.create_object(NewObject {
            title: format!("Task {:06}", n + 1),
            parent_id: Some(parent_id),
            position: Position::Last,
            type_ids: vec![type_id],
            props,
            ..Default::default()
        })?;
    }
    Ok(json!({ "type_id": type_id, "parent_id": parent_id }))
}
