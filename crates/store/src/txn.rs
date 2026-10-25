// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transactions and the services that change a vault.
//!
//! Every change happens inside a [`Txn`], through the services below. Each service checks the
//! vault's rules, then expresses the change as ops. Each op is applied, and appended to the
//! hash-chained log, in the same SQLite transaction. A committed transaction is one undo step.

use std::collections::BTreeSet;

use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::chain::OpEntry;
use dbme_core::model::{
    AttachmentRole, AttachmentRow, BlockRow, FieldKind, FieldRow, ObjectRow, ObjectTypeRow, Props,
    RelationRow, Sensitivity, TypeRow,
};
use dbme_core::op::{Action, Entity, Op, OpKind, Row, RowKey};
use dbme_core::value::validate_field_value;
use dbme_core::{ContentHash, Id, IdGenerator, SortKey};

use crate::query::{Reader, Siblings};
use crate::vault::{Committed, Context};
use crate::{Result, StoreError, log, rows};

/// Where to put an item among its siblings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Position {
    /// Before every sibling.
    First,
    /// After every sibling.
    #[default]
    Last,
    /// Just before this sibling.
    Before(Id),
    /// Just after this sibling.
    After(Id),
}

/// A new type.
#[allow(missing_docs, reason = "the columns of `types`")]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewType {
    pub name: String,
    pub plural: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub extends: Option<Id>,
    pub description: Option<String>,
}

/// A new field of a type.
#[allow(missing_docs, reason = "the columns of `fields`")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewField {
    pub type_id: Id,
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default = "empty_object")]
    pub options: Value,
    #[serde(default)]
    pub required: bool,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// A new object.
#[allow(
    missing_docs,
    reason = "the columns of `objects`, plus its types and position"
)]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewObject {
    pub title: String,
    pub parent_id: Option<Id>,
    pub position: Position,
    pub type_ids: Vec<Id>,
    pub props: Props,
    pub summary: Option<String>,
    pub captured_at: Option<i64>,
    pub event_start: Option<i64>,
    pub event_end: Option<i64>,
    pub geo_lat: Option<f64>,
    pub geo_lon: Option<f64>,
    pub sensitivity: Sensitivity,
}

/// A new block.
#[allow(missing_docs, reason = "the columns of `blocks`, plus its position")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewBlock {
    pub object_id: Id,
    #[serde(default)]
    pub parent_block_id: Option<Id>,
    #[serde(default)]
    pub position: Position,
    pub kind: String,
    #[serde(default = "empty_object")]
    pub content: Value,
}

/// A new relation.
#[allow(missing_docs, reason = "the columns of `relations`")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewRelation {
    pub src_id: Id,
    pub dst_id: Id,
    pub rel_type: String,
    #[serde(default)]
    pub field_id: Option<Id>,
    #[serde(default)]
    pub src_block_id: Option<Id>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub props: Props,
}

/// A blob, already in the blob store, to attach to an object.
#[allow(missing_docs, reason = "the columns of `attachments`")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewAttachment {
    pub object_id: Id,
    pub blob_hash: ContentHash,
    pub role: AttachmentRole,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub mime: Option<String>,
}

fn kind(entity: Entity, action: Action) -> Result<OpKind> {
    Ok(OpKind::new(entity, action)?)
}

fn rule(message: impl Into<String>) -> StoreError {
    StoreError::Rule(message.into())
}

/// One transaction: one undo step. Changes are visible to [`Txn::read`] at once, and to everyone
/// else when the transaction commits.
///
/// If a service fails, the transaction will not commit, even if the caller carries on: a change is
/// saved whole or not at all.
pub struct Txn<'v> {
    tx: rusqlite::Transaction<'v>,
    ctx: &'v Context,
    ts: i64,
    group_id: Id,
    undo_of: Option<Id>,
    head: Option<ContentHash>,
    first_seq: Option<i64>,
    last_seq: i64,
    changed: BTreeSet<Id>,
    failed: bool,
}

impl<'v> Txn<'v> {
    pub(crate) fn begin(
        conn: &'v mut Connection,
        ctx: &'v Context,
        undo_of: Option<Id>,
    ) -> Result<Self> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ts = ctx.clock.now_ms();
        let group_id = ctx.ids.next(ts)?;
        let head = log::head(&tx)?;
        Ok(Self {
            tx,
            ctx,
            ts,
            group_id,
            undo_of,
            head,
            first_seq: None,
            last_seq: 0,
            changed: BTreeSet::new(),
            failed: false,
        })
    }

    /// Commits, unless a service failed. Returns `None` if nothing changed.
    pub(crate) fn finish(self) -> Result<Option<Committed>> {
        if self.failed {
            return Err(rule(
                "a change in this transaction failed, so none of it was saved",
            ));
        }
        self.tx.commit()?;
        Ok(self.first_seq.map(|first_seq| Committed {
            group_id: self.group_id,
            undo_of: self.undo_of,
            first_seq,
            last_seq: self.last_seq,
            changed: self.changed.into_iter().collect(),
        }))
    }

    /// Reads the vault as this transaction sees it.
    pub fn read(&self) -> Reader<'_> {
        Reader { conn: &self.tx }
    }

    /// The time every op of this transaction records.
    pub fn now(&self) -> i64 {
        self.ts
    }

    /// The undo step this transaction will be.
    pub fn group_id(&self) -> Id {
        self.group_id
    }

    /// A fresh ID.
    pub fn new_id(&self) -> Result<Id> {
        Ok(self.ctx.ids.next(self.ts)?)
    }

    pub(crate) fn ids(&self) -> &IdGenerator {
        &self.ctx.ids
    }

    /// Applies one op and appends it to the log.
    pub(crate) fn apply(&mut self, op: Op) -> Result<()> {
        rows::apply(&self.tx, &op, self.ts)?;
        self.log(op)
    }

    /// [`Txn::apply`], given `current`: the row `op` targets as this transaction reads it now.
    pub(crate) fn apply_to(&mut self, op: Op, current: Option<&Row>) -> Result<()> {
        rows::apply_to(&self.tx, &op, current, self.ts)?;
        self.log(op)
    }

    /// Appends an applied op to the log.
    fn log(&mut self, op: Op) -> Result<()> {
        let target_id = op.key().target_id();
        let entry = OpEntry {
            op_id: self.ctx.ids.next(self.ts)?,
            group_id: self.group_id,
            undo_of: self.undo_of,
            actor_id: self.ctx.identity.actor_id,
            device_id: self.ctx.identity.device_id,
            ts: self.ts,
            kind: op.kind().to_string(),
            target_id,
            payload_json: op.payload_json()?,
        };
        let (seq, hash) = log::append(&self.tx, &entry, self.head.as_ref())?;
        self.head = Some(hash);
        self.first_seq.get_or_insert(seq);
        self.last_seq = seq;
        self.changed.insert(target_id);
        Ok(())
    }

    /// Runs one service; if it fails, marks the transaction so it cannot commit.
    fn guard<T>(&mut self, service: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let result = service(self);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// Applies the update-shaped op that turns `before` into `after`, if they differ.
    fn update(&mut self, kind: OpKind, before: Row, after: Row) -> Result<bool> {
        match Op::between(kind, &before, &after)? {
            Some(op) => {
                self.apply(op)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn live_object(&self, id: Id) -> Result<ObjectRow> {
        let object = self.read().object(id)?;
        if object.deleted_at.is_some() {
            return Err(rule(format!("object {id} is in the Trash")));
        }
        Ok(object)
    }

    /// Checks the values in `props` that differ from `before` against the fields of `type_ids`.
    fn check_props(&self, type_ids: &[Id], props: &Props, before: Option<&Props>) -> Result<()> {
        let fields = self.read().fields_of_types(type_ids)?;
        for (key, value) in props {
            if before.and_then(|b| b.get(key)) == Some(value) {
                continue;
            }
            for field in fields.iter().filter(|f| &f.key == key) {
                validate_field_value(field, value)?;
            }
        }
        Ok(())
    }

    // Types and fields.

    /// Creates a type.
    pub fn create_type(&mut self, new: NewType) -> Result<Id> {
        self.guard(|t| {
            if let Some(parent) = new.extends {
                t.read().type_row(parent)?;
            }
            let id = t.new_id()?;
            t.apply(Op::create(Row::Type(TypeRow {
                id,
                name: new.name,
                plural: new.plural,
                icon: new.icon,
                color: new.color,
                extends: new.extends,
                description: new.description,
                is_system: false,
                created_at: t.ts,
                updated_at: t.ts,
            })))?;
            Ok(id)
        })
    }

    /// Edits a type's name, plural, icon, colour, parent or description. Returns `false` if
    /// nothing changed.
    pub fn update_type(&mut self, id: Id, edit: impl FnOnce(&mut TypeRow)) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().type_row(id)?;
            let mut after = before.clone();
            edit(&mut after);
            let mut ancestor = after.extends;
            while let Some(parent) = ancestor {
                if parent == id {
                    return Err(rule(
                        "a type cannot extend itself or a type that extends it",
                    ));
                }
                ancestor = t.read().type_row(parent)?.extends;
            }
            t.update(
                kind(Entity::Type, Action::Update)?,
                Row::Type(before),
                Row::Type(after),
            )
        })
    }

    /// Deletes a type and its fields. System types, types any object has, and types another type
    /// extends cannot be deleted.
    pub fn delete_type(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            let row = t.read().type_row(id)?;
            if row.is_system {
                return Err(rule(format!(
                    "{} is a built-in type and cannot be deleted",
                    row.name
                )));
            }
            let used = t.read().count_of_type(id)?;
            if used > 0 {
                return Err(rule(format!("{} is the type of {used} objects", row.name)));
            }
            if t.read()
                .types()?
                .iter()
                .any(|other| other.extends == Some(id))
            {
                return Err(rule(format!("another type extends {}", row.name)));
            }
            for field in t.read().fields(id)? {
                t.remove_field(field)?;
            }
            t.apply(Op::delete(Row::Type(row)))
        })
    }

    /// Adds a field to a type, after its existing fields.
    pub fn create_field(&mut self, new: NewField) -> Result<Id> {
        self.guard(|t| {
            let fields = t.read().fields(new.type_id)?;
            t.read().type_row(new.type_id)?;
            if fields.iter().any(|f| f.key == new.key) {
                return Err(rule(format!(
                    "the type already has a field with key {:?}",
                    new.key
                )));
            }
            let id = t.new_id()?;
            let sort_order = fields.iter().map(|f| f.sort_order + 1).max().unwrap_or(0);
            t.apply(Op::create(Row::Field(FieldRow {
                id,
                type_id: new.type_id,
                key: new.key,
                label: new.label,
                kind: new.kind,
                options_json: new.options,
                required: new.required,
                sort_order,
            })))?;
            Ok(id)
        })
    }

    /// Edits a field's label, options, `required` flag or position. Its key and kind are fixed.
    pub fn update_field(&mut self, id: Id, edit: impl FnOnce(&mut FieldRow)) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().field(id)?;
            let mut after = before.clone();
            edit(&mut after);
            t.update(
                kind(Entity::Field, Action::Update)?,
                Row::Field(before),
                Row::Field(after),
            )
        })
    }

    /// Deletes a field. Values objects hold for it stay in their properties.
    pub fn delete_field(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            let field = t.read().field(id)?;
            t.remove_field(field)
        })
    }

    fn remove_field(&mut self, field: FieldRow) -> Result<()> {
        if !self
            .read()
            .relations_using(Some(field.id), None)?
            .is_empty()
        {
            return Err(rule(format!("relations still use field {:?}", field.key)));
        }
        self.apply(Op::delete(Row::Field(field)))
    }

    // Objects.

    /// Creates an object with its types. Its property values are checked against the fields of
    /// those types.
    pub fn create_object(&mut self, new: NewObject) -> Result<Id> {
        self.guard(|t| {
            if let Some(parent) = new.parent_id {
                t.live_object(parent)?;
            }
            let mut type_ids: Vec<Id> = Vec::with_capacity(new.type_ids.len());
            for type_id in new.type_ids {
                if !type_ids.contains(&type_id) {
                    t.read().type_row(type_id)?;
                    type_ids.push(type_id);
                }
            }
            t.check_props(&type_ids, &new.props, None)?;
            let id = t.new_id()?;
            let sort_key =
                t.read()
                    .sort_key_at(Siblings::Objects(new.parent_id), new.position, None)?;
            t.apply(Op::create(Row::Object(ObjectRow {
                id,
                title: new.title,
                summary: new.summary,
                parent_id: new.parent_id,
                sort_key,
                props_json: new.props,
                created_at: t.ts,
                updated_at: t.ts,
                captured_at: new.captured_at,
                event_start: new.event_start,
                event_end: new.event_end,
                geo_lat: new.geo_lat,
                geo_lon: new.geo_lon,
                deleted_at: None,
                sensitivity: new.sensitivity,
            })))?;
            for type_id in type_ids {
                t.apply(Op::create(Row::ObjectType(ObjectTypeRow {
                    object_id: id,
                    type_id,
                })))?;
            }
            Ok(id)
        })
    }

    /// Edits an object's title, summary, dates, location, sensitivity or property values. Use
    /// [`Txn::move_object`], [`Txn::trash_object`] and [`Txn::restore_object`] for the rest.
    /// Returns `false` if nothing changed.
    pub fn update_object(&mut self, id: Id, edit: impl FnOnce(&mut ObjectRow)) -> Result<bool> {
        self.guard(|t| {
            let before = t.live_object(id)?;
            let mut after = before.clone();
            edit(&mut after);
            if after.props_json != before.props_json {
                let type_ids = t.read().object_type_ids(id)?;
                t.check_props(&type_ids, &after.props_json, Some(&before.props_json))?;
            }
            t.update(
                kind(Entity::Object, Action::Update)?,
                Row::Object(before),
                Row::Object(after),
            )
        })
    }

    /// Moves an object under `parent` (`None`: the top level), at `position` among its new
    /// siblings.
    pub fn move_object(&mut self, id: Id, parent: Option<Id>, position: Position) -> Result<bool> {
        self.guard(|t| {
            let before = t.live_object(id)?;
            let mut ancestor = parent;
            while let Some(above) = ancestor {
                if above == id {
                    return Err(rule(
                        "an object cannot be moved into itself or below itself",
                    ));
                }
                ancestor = t.live_object(above)?.parent_id;
            }
            let mut after = before.clone();
            after.parent_id = parent;
            after.sort_key = t
                .read()
                .sort_key_at(Siblings::Objects(parent), position, Some(id))?;
            t.update(
                kind(Entity::Object, Action::Move)?,
                Row::Object(before),
                Row::Object(after),
            )
        })
    }

    /// Gives the live children of `parent` (`None`: the top level) short, evenly spread sort keys,
    /// keeping their order. Repeated inserts at one place make keys grow; this shrinks them again
    /// (ADR-0013). Trashed children keep their keys. Returns how many objects got a new key.
    pub fn rebalance_objects(&mut self, parent: Option<Id>) -> Result<usize> {
        self.guard(|t| {
            let children = t.read().children(parent)?;
            let keys = SortKey::n_between(None, None, children.len())?;
            let mut moved = 0;
            for (before, sort_key) in children.into_iter().zip(keys) {
                let after = ObjectRow {
                    sort_key,
                    ..before.clone()
                };
                let move_kind = kind(Entity::Object, Action::Move)?;
                if t.update(move_kind, Row::Object(before), Row::Object(after))? {
                    moved += 1;
                }
            }
            Ok(moved)
        })
    }

    /// Moves an object, and everything below it, to the Trash.
    pub fn trash_object(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            let root = t.live_object(id)?;
            let trash = kind(Entity::Object, Action::Trash)?;
            for before in t.read().subtree_of(root)? {
                if before.deleted_at.is_none() {
                    let mut after = before.clone();
                    after.deleted_at = Some(t.ts);
                    t.update(trash, Row::Object(before), Row::Object(after))?;
                }
            }
            Ok(())
        })
    }

    /// Takes an object out of the Trash, with everything that was trashed together with it. Its
    /// parent must not be in the Trash.
    pub fn restore_object(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            let object = t.read().object(id)?;
            let Some(trashed_at) = object.deleted_at else {
                return Err(rule(format!("object {id} is not in the Trash")));
            };
            if let Some(parent) = object.parent_id
                && t.read().object(parent)?.deleted_at.is_some()
            {
                return Err(rule("its parent is in the Trash: restore that first"));
            }
            let restore = kind(Entity::Object, Action::Restore)?;
            for before in t.read().subtree_of(object)? {
                if before.deleted_at == Some(trashed_at) {
                    let mut after = before.clone();
                    after.deleted_at = None;
                    t.update(restore, Row::Object(before), Row::Object(after))?;
                }
            }
            Ok(())
        })
    }

    /// Deletes an object from the Trash for good, with everything below it and everything that
    /// refers to it: its blocks, relations from and to it, attachments and types. Blobs stay in
    /// the blob store. Like every change, this can be undone in the same session.
    pub fn delete_object(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            let root = t.read().object(id)?;
            if root.deleted_at.is_none() {
                return Err(rule(format!(
                    "object {id} must be in the Trash before it is deleted"
                )));
            }
            for object in t.read().subtree_of(root)?.into_iter().rev() {
                for relation in t.read().relations_touching(object.id)? {
                    t.apply(Op::delete(Row::Relation(relation)))?;
                }
                for attachment in t.read().attachments(object.id)? {
                    t.apply(Op::delete(Row::Attachment(attachment)))?;
                }
                for block in t.read().blocks(object.id)?.into_iter().rev() {
                    t.apply(Op::delete(Row::Block(block)))?;
                }
                for type_id in t.read().object_type_ids(object.id)? {
                    t.apply(Op::delete(Row::ObjectType(ObjectTypeRow {
                        object_id: object.id,
                        type_id,
                    })))?;
                }
                t.apply(Op::delete(Row::Object(object)))?;
            }
            Ok(())
        })
    }

    /// Gives an object another type. Returns `false` if it already had it.
    pub fn add_type(&mut self, object_id: Id, type_id: Id) -> Result<bool> {
        self.guard(|t| {
            t.live_object(object_id)?;
            t.read().type_row(type_id)?;
            let row = Row::ObjectType(ObjectTypeRow { object_id, type_id });
            if rows::load(&t.tx, &row.key())?.is_some() {
                return Ok(false);
            }
            t.apply(Op::create(row))?;
            Ok(true)
        })
    }

    /// Takes a type away from an object. Its values for that type's fields stay in its
    /// properties. Returns `false` if it did not have the type.
    pub fn remove_type(&mut self, object_id: Id, type_id: Id) -> Result<bool> {
        self.guard(|t| {
            let key = RowKey::ObjectType { object_id, type_id };
            match rows::load(&t.tx, &key)? {
                Some(row) => {
                    t.apply(Op::delete(row))?;
                    Ok(true)
                }
                None => Ok(false),
            }
        })
    }

    // Blocks.

    /// Adds a block to an object's content.
    pub fn create_block(&mut self, new: NewBlock) -> Result<Id> {
        self.guard(|t| {
            t.live_object(new.object_id)?;
            if let Some(parent) = new.parent_block_id
                && t.read().block(parent)?.object_id != new.object_id
            {
                return Err(rule("a block's parent must belong to the same object"));
            }
            let id = t.new_id()?;
            let sort_key = t.read().sort_key_at(
                Siblings::Blocks(new.object_id, new.parent_block_id),
                new.position,
                None,
            )?;
            t.apply(Op::create(Row::Block(BlockRow {
                id,
                object_id: new.object_id,
                parent_block_id: new.parent_block_id,
                sort_key,
                kind: new.kind,
                content_json: new.content,
                created_at: t.ts,
                updated_at: t.ts,
            })))?;
            Ok(id)
        })
    }

    /// Edits a block's kind or content. Returns `false` if nothing changed.
    pub fn update_block(&mut self, id: Id, edit: impl FnOnce(&mut BlockRow)) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().block(id)?;
            let mut after = before.clone();
            edit(&mut after);
            t.update(
                kind(Entity::Block, Action::Update)?,
                Row::Block(before),
                Row::Block(after),
            )
        })
    }

    /// Moves a block under another block of the same object (`None`: the top level).
    pub fn move_block(&mut self, id: Id, parent: Option<Id>, position: Position) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().block(id)?;
            let mut ancestor = parent;
            while let Some(above) = ancestor {
                let block = t.read().block(above)?;
                if above == id {
                    return Err(rule("a block cannot be moved into itself or below itself"));
                }
                if block.object_id != before.object_id {
                    return Err(rule("a block can only move within its object"));
                }
                ancestor = block.parent_block_id;
            }
            let mut after = before.clone();
            after.parent_block_id = parent;
            after.sort_key = t.read().sort_key_at(
                Siblings::Blocks(before.object_id, parent),
                position,
                Some(id),
            )?;
            t.update(
                kind(Entity::Block, Action::Move)?,
                Row::Block(before),
                Row::Block(after),
            )
        })
    }

    /// Gives the blocks directly under `parent` (`None`: the top level) of an object short, evenly
    /// spread sort keys, keeping their order, like [`Txn::rebalance_objects`]. Returns how many
    /// blocks got a new key.
    pub fn rebalance_blocks(&mut self, object_id: Id, parent: Option<Id>) -> Result<usize> {
        self.guard(|t| {
            t.read().object(object_id)?;
            // Tree order lists each block's children in order, so filtering keeps sibling order.
            let siblings: Vec<BlockRow> = t
                .read()
                .blocks(object_id)?
                .into_iter()
                .filter(|block| block.parent_block_id == parent)
                .collect();
            let keys = SortKey::n_between(None, None, siblings.len())?;
            let mut moved = 0;
            for (before, sort_key) in siblings.into_iter().zip(keys) {
                let after = BlockRow {
                    sort_key,
                    ..before.clone()
                };
                let move_kind = kind(Entity::Block, Action::Move)?;
                if t.update(move_kind, Row::Block(before), Row::Block(after))? {
                    moved += 1;
                }
            }
            Ok(moved)
        })
    }

    /// Deletes a block, the blocks below it, and the relations they are the source of.
    pub fn delete_block(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            for block in t.read().block_subtree(id)?.into_iter().rev() {
                for relation in t.read().relations_using(None, Some(block.id))? {
                    t.apply(Op::delete(Row::Relation(relation)))?;
                }
                t.apply(Op::delete(Row::Block(block)))?;
            }
            Ok(())
        })
    }

    // Relations.

    /// Links two objects.
    pub fn create_relation(&mut self, new: NewRelation) -> Result<Id> {
        self.guard(|t| {
            t.live_object(new.src_id)?;
            t.live_object(new.dst_id)?;
            if let Some(field_id) = new.field_id
                && t.read().field(field_id)?.kind != FieldKind::Relation
            {
                return Err(rule("only relation fields can hold relations"));
            }
            if let Some(block_id) = new.src_block_id
                && t.read().block(block_id)?.object_id != new.src_id
            {
                return Err(rule(
                    "a relation's source block must belong to its source object",
                ));
            }
            let id = t.new_id()?;
            t.apply(Op::create(Row::Relation(RelationRow {
                id,
                src_id: new.src_id,
                dst_id: new.dst_id,
                rel_type: new.rel_type,
                field_id: new.field_id,
                src_block_id: new.src_block_id,
                label: new.label,
                props_json: new.props,
                created_at: t.ts,
                deleted_at: None,
            })))?;
            Ok(id)
        })
    }

    /// Edits a relation's label or properties. Returns `false` if nothing changed.
    pub fn update_relation(&mut self, id: Id, edit: impl FnOnce(&mut RelationRow)) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().relation(id)?;
            let mut after = before.clone();
            edit(&mut after);
            t.update(
                kind(Entity::Relation, Action::Update)?,
                Row::Relation(before),
                Row::Relation(after),
            )
        })
    }

    /// Moves a relation to the Trash. Returns `false` if it was already there.
    pub fn trash_relation(&mut self, id: Id) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().relation(id)?;
            let mut after = before.clone();
            after.deleted_at = before.deleted_at.or(Some(t.ts));
            t.update(
                kind(Entity::Relation, Action::Trash)?,
                Row::Relation(before),
                Row::Relation(after),
            )
        })
    }

    /// Takes a relation out of the Trash. Returns `false` if it was not there.
    pub fn restore_relation(&mut self, id: Id) -> Result<bool> {
        self.guard(|t| {
            let before = t.read().relation(id)?;
            let mut after = before.clone();
            after.deleted_at = None;
            t.update(
                kind(Entity::Relation, Action::Restore)?,
                Row::Relation(before),
                Row::Relation(after),
            )
        })
    }

    /// Deletes a relation for good.
    pub fn delete_relation(&mut self, id: Id) -> Result<()> {
        self.guard(|t| {
            let row = t.read().relation(id)?;
            t.apply(Op::delete(Row::Relation(row)))
        })
    }

    // Attachments.

    /// Attaches a blob that is already in the blob store to an object.
    pub fn attach(&mut self, new: NewAttachment) -> Result<()> {
        self.guard(|t| {
            t.live_object(new.object_id)?;
            let size = t
                .ctx
                .blobs
                .size(&new.blob_hash)?
                .ok_or_else(|| StoreError::MissingBlob(new.blob_hash.clone()))?;
            t.apply(Op::create(Row::Attachment(AttachmentRow {
                object_id: new.object_id,
                blob_hash: new.blob_hash,
                role: new.role,
                filename: new.filename,
                mime: new.mime,
                blob_size: i64::try_from(size).map_err(|_| rule("the blob is too large"))?,
            })))
        })
    }

    /// Removes an attachment. The blob stays in the blob store. Returns `false` if there was no
    /// such attachment.
    pub fn detach(
        &mut self,
        object_id: Id,
        blob_hash: ContentHash,
        role: AttachmentRole,
    ) -> Result<bool> {
        self.guard(|t| {
            match rows::load(
                &t.tx,
                &RowKey::Attachment {
                    object_id,
                    blob_hash,
                    role,
                },
            )? {
                Some(row) => {
                    t.apply(Op::delete(row))?;
                    Ok(true)
                }
                None => Ok(false),
            }
        })
    }
}
