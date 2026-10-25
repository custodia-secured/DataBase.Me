// SPDX-License-Identifier: AGPL-3.0-or-later
//! Ops: the only way anything in a vault changes (ADR-0002, ADR-0012).
//!
//! An op changes exactly one row. Its kind is `<entity>.<action>`, for example `object.trash`, and
//! its payload is one of three shapes:
//!
//! - **create** carries the whole new row;
//! - **delete** carries the whole row as it was just before it was removed;
//! - **update** (and its named variants `move`, `trash`, `restore`) carries, for each changed
//!   column, the value before and after (`{"from": …, "to": …}`), and for an object's field values,
//!   the same per key, where a missing `from` or `to` means the key was or becomes absent.
//!
//! Because every op records what it expects to find, an op applies only if the row still looks
//! that way ([`Op::apply`]), and an undo can tell whether anything has changed since
//! ([`Op::check_still_current`]) before it reverses the op ([`Op::inverse`]). An undo never
//! overwrites a later edit.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::model::{
    AttachmentRole, AttachmentRow, BlockRow, FieldRow, ObjectRow, ObjectTypeRow, RelationRow,
    TypeRow,
};
use crate::{ContentHash, CoreError, Id};

/// The tables an op can change.
#[allow(missing_docs, reason = "one variant per table, named after it")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Entity {
    Type,
    Field,
    Object,
    ObjectType,
    Block,
    Relation,
    Attachment,
}

impl Entity {
    /// Every entity.
    pub const ALL: [Entity; 7] = [
        Self::Type,
        Self::Field,
        Self::Object,
        Self::ObjectType,
        Self::Block,
        Self::Relation,
        Self::Attachment,
    ];

    /// The name used in op kinds.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Field => "field",
            Self::Object => "object",
            Self::ObjectType => "object_type",
            Self::Block => "block",
            Self::Relation => "relation",
            Self::Attachment => "attachment",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == text)
    }

    /// Whether the table has an `updated_at` column, which every update sets to the op's time.
    pub fn has_updated_at(self) -> bool {
        matches!(self, Self::Type | Self::Object | Self::Block)
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What an op does to its row.
#[allow(missing_docs, reason = "described in FORMAT.md §5")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Create,
    Update,
    Move,
    Trash,
    Restore,
    Delete,
}

impl Action {
    const ALL: [Action; 6] = [
        Self::Create,
        Self::Update,
        Self::Move,
        Self::Trash,
        Self::Restore,
        Self::Delete,
    ];

    /// The name used in op kinds.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Move => "move",
            Self::Trash => "trash",
            Self::Restore => "restore",
            Self::Delete => "delete",
        }
    }

    /// `true` for the actions whose payload is an [`Update`].
    pub fn is_update(self) -> bool {
        matches!(
            self,
            Self::Update | Self::Move | Self::Trash | Self::Restore
        )
    }
}

/// An op kind such as `object.move`. Only meaningful combinations exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpKind {
    entity: Entity,
    action: Action,
}

impl OpKind {
    /// The kind for `action` on `entity`, if that combination exists.
    pub fn new(entity: Entity, action: Action) -> Result<Self, CoreError> {
        let kind = Self { entity, action };
        let exists = match action {
            Action::Create | Action::Delete => true,
            Action::Update => !matches!(entity, Entity::ObjectType | Entity::Attachment),
            Action::Move => matches!(entity, Entity::Object | Entity::Block),
            Action::Trash | Action::Restore => matches!(entity, Entity::Object | Entity::Relation),
        };
        if exists {
            Ok(kind)
        } else {
            Err(CoreError::UnknownOpKind(kind.to_string()))
        }
    }

    /// Parses `<entity>.<action>`.
    pub fn parse(text: &str) -> Result<Self, CoreError> {
        let unknown = || CoreError::UnknownOpKind(text.to_owned());
        let (entity, action) = text.split_once('.').ok_or_else(unknown)?;
        let entity = Entity::parse(entity).ok_or_else(unknown)?;
        let action = Action::ALL
            .into_iter()
            .find(|a| a.as_str() == action)
            .ok_or_else(unknown)?;
        Self::new(entity, action)
    }

    /// The table the op changes.
    pub fn entity(self) -> Entity {
        self.entity
    }

    /// What the op does.
    pub fn action(self) -> Action {
        self.action
    }

    /// The kind of the op that reverses this one.
    pub fn inverse(self) -> Self {
        let action = match self.action {
            Action::Create => Action::Delete,
            Action::Delete => Action::Create,
            Action::Trash => Action::Restore,
            Action::Restore => Action::Trash,
            other => other,
        };
        Self {
            entity: self.entity,
            action,
        }
    }

    /// The columns an update-shaped op of this kind may change.
    pub fn columns(self) -> &'static [&'static str] {
        match (self.entity, self.action) {
            (Entity::Type, Action::Update) => {
                &["name", "plural", "icon", "color", "extends", "description"]
            }
            (Entity::Field, Action::Update) => &["label", "options_json", "required", "sort_order"],
            (Entity::Object, Action::Update) => &[
                "title",
                "summary",
                "captured_at",
                "event_start",
                "event_end",
                "geo_lat",
                "geo_lon",
                "sensitivity",
            ],
            (Entity::Object, Action::Move) => &["parent_id", "sort_key"],
            (Entity::Block, Action::Move) => &["parent_block_id", "sort_key"],
            (Entity::Block, Action::Update) => &["kind", "content_json"],
            (Entity::Relation, Action::Update) => &["label", "props_json"],
            (_, Action::Trash | Action::Restore) => &["deleted_at"],
            _ => &[],
        }
    }

    /// Whether an op of this kind may change individual field values of an object.
    pub fn allows_props(self) -> bool {
        self.entity == Entity::Object && self.action == Action::Update
    }
}

impl fmt::Display for OpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.entity.as_str(), self.action.as_str())
    }
}

/// The identity of a row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RowKey {
    /// A row identified by its `id` column.
    Id(Entity, Id),
    /// A row of `object_types`.
    ObjectType {
        /// The object.
        object_id: Id,
        /// One of its types.
        type_id: Id,
    },
    /// A row of `attachments`.
    Attachment {
        /// The object.
        object_id: Id,
        /// The blob.
        blob_hash: ContentHash,
        /// What the blob is to the object.
        role: AttachmentRole,
    },
}

impl RowKey {
    /// The table.
    pub fn entity(&self) -> Entity {
        match self {
            Self::Id(entity, _) => *entity,
            Self::ObjectType { .. } => Entity::ObjectType,
            Self::Attachment { .. } => Entity::Attachment,
        }
    }

    /// The ID recorded as the op's `target_id`: the row's own ID, or its object's.
    pub fn target_id(&self) -> Id {
        match self {
            Self::Id(_, id) => *id,
            Self::ObjectType { object_id, .. } | Self::Attachment { object_id, .. } => *object_id,
        }
    }
}

impl fmt::Display for RowKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(_, id) => write!(f, "{id}"),
            Self::ObjectType { object_id, type_id } => write!(f, "{object_id}/{type_id}"),
            Self::Attachment {
                object_id,
                blob_hash,
                role,
            } => write!(f, "{object_id}/{blob_hash}/{}", role.as_str()),
        }
    }
}

/// A whole row of one of the tables ops change.
#[allow(missing_docs, reason = "one variant per table")]
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Type(TypeRow),
    Field(FieldRow),
    Object(ObjectRow),
    ObjectType(ObjectTypeRow),
    Block(BlockRow),
    Relation(RelationRow),
    Attachment(AttachmentRow),
}

fn to_map<T: Serialize>(row: &T) -> Result<Map<String, Value>, CoreError> {
    match serde_json::to_value(row)? {
        Value::Object(map) => Ok(map),
        _ => Err(CoreError::invalid(
            "row",
            "does not serialise to a JSON object",
        )),
    }
}

impl Row {
    /// The row's table.
    pub fn entity(&self) -> Entity {
        match self {
            Self::Type(_) => Entity::Type,
            Self::Field(_) => Entity::Field,
            Self::Object(_) => Entity::Object,
            Self::ObjectType(_) => Entity::ObjectType,
            Self::Block(_) => Entity::Block,
            Self::Relation(_) => Entity::Relation,
            Self::Attachment(_) => Entity::Attachment,
        }
    }

    /// The row's identity.
    pub fn key(&self) -> RowKey {
        match self {
            Self::Type(r) => RowKey::Id(Entity::Type, r.id),
            Self::Field(r) => RowKey::Id(Entity::Field, r.id),
            Self::Object(r) => RowKey::Id(Entity::Object, r.id),
            Self::Block(r) => RowKey::Id(Entity::Block, r.id),
            Self::Relation(r) => RowKey::Id(Entity::Relation, r.id),
            Self::ObjectType(r) => RowKey::ObjectType {
                object_id: r.object_id,
                type_id: r.type_id,
            },
            Self::Attachment(r) => RowKey::Attachment {
                object_id: r.object_id,
                blob_hash: r.blob_hash.clone(),
                role: r.role,
            },
        }
    }

    /// Checks the row against the data model.
    pub fn validate(&self) -> Result<(), CoreError> {
        match self {
            Self::Type(r) => r.validate(),
            Self::Field(r) => r.validate(),
            Self::Object(r) => r.validate(),
            Self::ObjectType(_) => Ok(()),
            Self::Block(r) => r.validate(),
            Self::Relation(r) => r.validate(),
            Self::Attachment(r) => r.validate(),
        }
    }

    /// The row as payload JSON.
    pub fn to_json(&self) -> Result<String, CoreError> {
        Ok(match self {
            Self::Type(r) => serde_json::to_string(r),
            Self::Field(r) => serde_json::to_string(r),
            Self::Object(r) => serde_json::to_string(r),
            Self::ObjectType(r) => serde_json::to_string(r),
            Self::Block(r) => serde_json::to_string(r),
            Self::Relation(r) => serde_json::to_string(r),
            Self::Attachment(r) => serde_json::to_string(r),
        }?)
    }

    /// Reads a row of `entity` from payload JSON.
    pub fn from_json(entity: Entity, json: &str) -> Result<Self, CoreError> {
        Self::from_value(entity, serde_json::from_str(json)?)
    }

    fn from_value(entity: Entity, value: Value) -> Result<Self, CoreError> {
        Ok(match entity {
            Entity::Type => Self::Type(serde_json::from_value(value)?),
            Entity::Field => Self::Field(serde_json::from_value(value)?),
            Entity::Object => Self::Object(serde_json::from_value(value)?),
            Entity::ObjectType => Self::ObjectType(serde_json::from_value(value)?),
            Entity::Block => Self::Block(serde_json::from_value(value)?),
            Entity::Relation => Self::Relation(serde_json::from_value(value)?),
            Entity::Attachment => Self::Attachment(serde_json::from_value(value)?),
        })
    }

    fn to_map(&self) -> Result<Map<String, Value>, CoreError> {
        match self {
            Self::Type(r) => to_map(r),
            Self::Field(r) => to_map(r),
            Self::Object(r) => to_map(r),
            Self::ObjectType(r) => to_map(r),
            Self::Block(r) => to_map(r),
            Self::Relation(r) => to_map(r),
            Self::Attachment(r) => to_map(r),
        }
    }

    /// Equality that ignores `updated_at`, which an undo moves forward rather than back.
    fn same_ignoring_updated_at(&self, other: &Row) -> bool {
        let strip = |row: &Row| {
            let mut row = row.clone();
            match &mut row {
                Self::Type(r) => r.updated_at = 0,
                Self::Object(r) => r.updated_at = 0,
                Self::Block(r) => r.updated_at = 0,
                _ => {}
            }
            row
        };
        strip(self) == strip(other)
    }
}

/// The value of one column before and after an update.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnChange {
    /// The value the op expects to find.
    pub from: Value,
    /// The value the op leaves.
    pub to: Value,
}

/// One field value of an object before and after an update. `None` means the key is absent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropChange {
    /// The value the op expects to find.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub from: Option<Value>,
    /// The value the op leaves.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub to: Option<Value>,
}

/// Reads a present key as `Some`, even when its value is `null`.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// The payload of an update-shaped op.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    /// The row's ID.
    pub id: Id,
    /// Changed columns.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub set: BTreeMap<String, ColumnChange>,
    /// Changed field values (objects only).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub props: BTreeMap<String, PropChange>,
}

impl Update {
    /// An update of row `id` that changes nothing yet.
    pub fn new(id: Id) -> Self {
        Self {
            id,
            set: BTreeMap::new(),
            props: BTreeMap::new(),
        }
    }

    /// Records a column change, unless `from` and `to` are equal.
    pub fn column(&mut self, column: &str, from: Value, to: Value) -> &mut Self {
        if from != to {
            self.set
                .insert(column.to_owned(), ColumnChange { from, to });
        }
        self
    }

    /// Records a field value change, unless `from` and `to` are equal.
    pub fn prop(&mut self, key: &str, from: Option<Value>, to: Option<Value>) -> &mut Self {
        if from != to {
            self.props.insert(key.to_owned(), PropChange { from, to });
        }
        self
    }

    /// `true` if the update changes nothing.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.props.is_empty()
    }

    fn swapped(&self) -> Self {
        Self {
            id: self.id,
            set: self
                .set
                .iter()
                .map(|(k, c)| {
                    (
                        k.clone(),
                        ColumnChange {
                            from: c.to.clone(),
                            to: c.from.clone(),
                        },
                    )
                })
                .collect(),
            props: self
                .props
                .iter()
                .map(|(k, c)| {
                    (
                        k.clone(),
                        PropChange {
                            from: c.to.clone(),
                            to: c.from.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// What an op carries.
#[allow(missing_docs, reason = "described in the module documentation")]
#[derive(Clone, Debug, PartialEq)]
pub enum OpBody {
    Create(Row),
    Update(Update),
    Delete(Row),
}

/// One change to one row.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    kind: OpKind,
    body: OpBody,
}

fn conflict(key: &RowKey, reason: impl Into<String>) -> CoreError {
    CoreError::Conflict {
        entity: key.entity().as_str(),
        key: key.to_string(),
        reason: reason.into(),
    }
}

impl Op {
    /// Creates `row`.
    pub fn create(row: Row) -> Self {
        Self {
            kind: OpKind {
                entity: row.entity(),
                action: Action::Create,
            },
            body: OpBody::Create(row),
        }
    }

    /// Deletes `row`, which must be the row exactly as it is now.
    pub fn delete(row: Row) -> Self {
        Self {
            kind: OpKind {
                entity: row.entity(),
                action: Action::Delete,
            },
            body: OpBody::Delete(row),
        }
    }

    /// An update-shaped op. Checks that the kind takes an update, that the update changes
    /// something, and that it touches only columns that kind may change.
    pub fn update(kind: OpKind, update: Update) -> Result<Self, CoreError> {
        let what = format!("{kind} op");
        if !kind.action.is_update() {
            return Err(CoreError::invalid(
                what,
                "this kind does not take an update",
            ));
        }
        if update.is_empty() {
            return Err(CoreError::invalid(what, "changes nothing"));
        }
        if let Some(column) = update
            .set
            .keys()
            .find(|c| !kind.columns().contains(&c.as_str()))
        {
            return Err(CoreError::invalid(
                what,
                format!("cannot change column {column:?}"),
            ));
        }
        if !update.props.is_empty() && !kind.allows_props() {
            return Err(CoreError::invalid(what, "cannot change field values"));
        }
        Ok(Self {
            kind,
            body: OpBody::Update(update),
        })
    }

    /// The update-shaped op of `kind` that turns `before` into `after`, or `None` if they differ
    /// only in `updated_at`. Fails if they are different rows, or if they differ in a column that
    /// `kind` may not change.
    pub fn between(kind: OpKind, before: &Row, after: &Row) -> Result<Option<Op>, CoreError> {
        let key = before.key();
        let RowKey::Id(entity, id) = key else {
            return Err(CoreError::invalid(
                "op",
                format!("{} rows are never updated", key.entity()),
            ));
        };
        if after.key() != key || entity != kind.entity {
            return Err(CoreError::invalid(
                "op",
                format!(
                    "cannot make a {kind} op from row {key} to row {}",
                    after.key()
                ),
            ));
        }
        let old = before.to_map()?;
        let mut new = after.to_map()?;
        let mut update = Update::new(id);
        for (column, from) in old {
            let to = new.remove(&column).unwrap_or(Value::Null);
            if column == "updated_at" {
                continue;
            }
            match (column == "props_json" && kind.allows_props(), from, to) {
                (true, Value::Object(from), Value::Object(to)) => {
                    let keys: std::collections::BTreeSet<&String> =
                        from.keys().chain(to.keys()).collect();
                    for prop in keys {
                        update.prop(prop, from.get(prop).cloned(), to.get(prop).cloned());
                    }
                }
                (_, from, to) => {
                    update.column(&column, from, to);
                }
            }
        }
        if update.is_empty() {
            Ok(None)
        } else {
            Self::update(kind, update).map(Some)
        }
    }

    /// Reads an op from its stored kind and payload.
    pub fn decode(kind: &str, payload_json: &str) -> Result<Self, CoreError> {
        let kind = OpKind::parse(kind)?;
        match kind.action {
            Action::Create => Ok(Self::create(Row::from_json(kind.entity, payload_json)?)),
            Action::Delete => Ok(Self::delete(Row::from_json(kind.entity, payload_json)?)),
            _ => Self::update(kind, serde_json::from_str(payload_json)?),
        }
    }

    /// The op's kind.
    pub fn kind(&self) -> OpKind {
        self.kind
    }

    /// What the op carries.
    pub fn body(&self) -> &OpBody {
        &self.body
    }

    /// The row the op changes.
    pub fn key(&self) -> RowKey {
        match &self.body {
            OpBody::Create(row) | OpBody::Delete(row) => row.key(),
            OpBody::Update(update) => RowKey::Id(self.kind.entity, update.id),
        }
    }

    /// The payload as stored in `ops.payload_json`.
    pub fn payload_json(&self) -> Result<String, CoreError> {
        match &self.body {
            OpBody::Create(row) | OpBody::Delete(row) => row.to_json(),
            OpBody::Update(update) => Ok(serde_json::to_string(update)?),
        }
    }

    /// Applies the op to `current`, the row as it is now (`None`: absent), at time `ts`. Returns
    /// the row as it must be afterwards (`None`: removed). Fails with [`CoreError::Conflict`] if
    /// `current` is not what the op expects.
    pub fn apply(&self, current: Option<&Row>, ts: i64) -> Result<Option<Row>, CoreError> {
        let key = self.key();
        if let Some(row) = current
            && row.key() != key
        {
            return Err(CoreError::invalid(
                "op",
                format!("was given row {} instead of {key}", row.key()),
            ));
        }
        match &self.body {
            OpBody::Create(row) => {
                if current.is_some() {
                    return Err(conflict(&key, "it already exists"));
                }
                row.validate()?;
                Ok(Some(row.clone()))
            }
            OpBody::Delete(row) => match current {
                Some(now) if now == row => Ok(None),
                Some(_) => Err(conflict(&key, "it has changed since the op was made")),
                None => Err(conflict(&key, "it does not exist")),
            },
            OpBody::Update(update) => {
                let now = current.ok_or_else(|| conflict(&key, "it does not exist"))?;
                let row = self.apply_update(&key, now, update, ts)?;
                row.validate()?;
                Ok(Some(row))
            }
        }
    }

    fn apply_update(
        &self,
        key: &RowKey,
        now: &Row,
        update: &Update,
        ts: i64,
    ) -> Result<Row, CoreError> {
        let mut map = now.to_map()?;
        for (column, change) in &update.set {
            let value = map.get(column).unwrap_or(&Value::Null);
            if *value != change.from {
                return Err(conflict(
                    key,
                    format!("{column} is {value}, not {}", change.from),
                ));
            }
            map.insert(column.clone(), change.to.clone());
        }
        if !update.props.is_empty() {
            let Some(Value::Object(props)) = map.get_mut("props_json") else {
                return Err(CoreError::invalid("op", "row has no field values"));
            };
            for (prop, change) in &update.props {
                if props.get(prop) != change.from.as_ref() {
                    return Err(conflict(key, format!("field {prop:?} has changed")));
                }
                match &change.to {
                    Some(value) => props.insert(prop.clone(), value.clone()),
                    None => props.remove(prop),
                };
            }
        }
        if self.kind.entity.has_updated_at() {
            map.insert("updated_at".to_owned(), Value::from(ts));
        }
        Row::from_value(self.kind.entity, Value::Object(map))
    }

    /// Checks that `current` still shows this op's effect, ignoring `updated_at`. Undo calls this
    /// first: if anything has changed since, undoing would overwrite that change, so it refuses.
    pub fn check_still_current(&self, current: Option<&Row>) -> Result<(), CoreError> {
        let key = self.key();
        match (&self.body, current) {
            (OpBody::Create(row), Some(now)) if now.same_ignoring_updated_at(row) => Ok(()),
            (OpBody::Create(_) | OpBody::Update(_), None) => {
                Err(conflict(&key, "it no longer exists"))
            }
            (OpBody::Create(_), Some(_)) => Err(conflict(&key, "it has changed since")),
            (OpBody::Delete(_), None) => Ok(()),
            (OpBody::Delete(_), Some(_)) => Err(conflict(&key, "it exists again")),
            (OpBody::Update(update), Some(now)) => {
                let map = now.to_map()?;
                for (column, change) in &update.set {
                    if map.get(column).unwrap_or(&Value::Null) != &change.to {
                        return Err(conflict(&key, format!("{column} has changed since")));
                    }
                }
                let props = map.get("props_json").and_then(Value::as_object);
                for (prop, change) in &update.props {
                    if props.and_then(|p| p.get(prop)) != change.to.as_ref() {
                        return Err(conflict(&key, format!("field {prop:?} has changed since")));
                    }
                }
                Ok(())
            }
        }
    }

    /// The op that reverses this one, given the row as it is now. Fails like
    /// [`Op::check_still_current`] if something has changed since.
    pub fn inverse(&self, current: Option<&Row>) -> Result<Op, CoreError> {
        self.check_still_current(current)?;
        Ok(match (&self.body, current) {
            (OpBody::Create(_), Some(now)) => Op::delete(now.clone()),
            (OpBody::Create(_), None) => return Err(conflict(&self.key(), "it no longer exists")),
            (OpBody::Delete(row), _) => Op::create(row.clone()),
            (OpBody::Update(update), _) => Op {
                kind: self.kind.inverse(),
                body: OpBody::Update(update.swapped()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SortKey;
    use crate::model::tests::{field, id, object, type_row};
    use crate::model::{FieldKind, Sensitivity};
    use serde_json::json;

    fn kind(text: &str) -> OpKind {
        OpKind::parse(text).unwrap()
    }

    fn obj(n: u128) -> Row {
        Row::Object(object(n))
    }

    #[test]
    fn kinds_parse_print_and_invert() {
        let all = [
            "type.create",
            "type.update",
            "type.delete",
            "field.create",
            "field.update",
            "field.delete",
            "object.create",
            "object.update",
            "object.move",
            "object.trash",
            "object.restore",
            "object.delete",
            "object_type.create",
            "object_type.delete",
            "block.create",
            "block.update",
            "block.move",
            "block.delete",
            "relation.create",
            "relation.update",
            "relation.trash",
            "relation.restore",
            "relation.delete",
            "attachment.create",
            "attachment.delete",
        ];
        for text in all {
            let k = kind(text);
            assert_eq!(k.to_string(), text);
            assert_eq!(k.inverse().inverse(), k);
            assert_eq!(k.entity().to_string(), text.split('.').next().unwrap());
        }
        assert_eq!(kind("object.trash").inverse(), kind("object.restore"));
        assert_eq!(kind("block.create").inverse(), kind("block.delete"));
        assert_eq!(kind("object.move").inverse(), kind("object.move"));
        assert_eq!(kind("object.move").action(), Action::Move);
        for bad in [
            "object",
            "object.fly",
            "planet.create",
            "object_type.update",
            "attachment.move",
            "type.trash",
            "field.move",
            ".create",
        ] {
            assert!(
                matches!(OpKind::parse(bad), Err(CoreError::UnknownOpKind(_))),
                "{bad:?}"
            );
        }
        assert!(Entity::Object.has_updated_at() && !Entity::Relation.has_updated_at());
        assert!(kind("object.update").allows_props() && !kind("object.move").allows_props());
        assert!(kind("object_type.create").columns().is_empty());
    }

    #[test]
    fn create_then_delete() {
        let row = obj(1);
        let create = Op::create(row.clone());
        assert_eq!(create.apply(None, 5).unwrap(), Some(row.clone()));
        assert!(matches!(
            create.apply(Some(&row), 5),
            Err(CoreError::Conflict { .. })
        ));

        let delete = Op::delete(row.clone());
        assert_eq!(delete.apply(Some(&row), 5).unwrap(), None);
        assert!(matches!(
            delete.apply(None, 5),
            Err(CoreError::Conflict { .. })
        ));
        let mut changed = object(1);
        changed.title = "changed".into();
        assert!(matches!(
            delete.apply(Some(&Row::Object(changed)), 5),
            Err(CoreError::Conflict { .. })
        ));

        let mut invalid = object(2);
        invalid.parent_id = Some(invalid.id);
        assert!(matches!(
            Op::create(Row::Object(invalid)).apply(None, 5),
            Err(CoreError::Invalid { .. })
        ));
        assert!(
            Op::create(obj(1)).apply(Some(&obj(2)), 5).is_err(),
            "rows of another key are refused"
        );
    }

    #[test]
    fn update_checks_from_values_and_stamps_updated_at() {
        let row = obj(1);
        let mut update = Update::new(id(1));
        update.column("title", json!("Object 1"), json!("Renamed"));
        update.column("summary", Value::Null, Value::Null); // unchanged: not recorded
        update.prop("due", None, Some(json!("2026-11-30")));
        let op = Op::update(kind("object.update"), update).unwrap();
        let Some(Row::Object(after)) = op.apply(Some(&row), 99).unwrap() else {
            panic!()
        };
        assert_eq!(after.title, "Renamed");
        assert_eq!(after.props_json.get("due"), Some(&json!("2026-11-30")));
        assert_eq!(after.updated_at, 99);

        // Applying it again fails: the title is no longer "Object 1".
        assert!(matches!(
            op.apply(Some(&Row::Object(after.clone())), 100),
            Err(CoreError::Conflict { .. })
        ));
        // ...and a missing row is a conflict too.
        assert!(matches!(
            op.apply(None, 100),
            Err(CoreError::Conflict { .. })
        ));

        // A prop whose current value differs is a conflict.
        let mut other = object(1);
        other.props_json.insert("due".into(), json!("2027-01-01"));
        let mut update = Update::new(id(1));
        update.prop("due", None, Some(json!("x")));
        let op = Op::update(kind("object.update"), update).unwrap();
        assert!(matches!(
            op.apply(Some(&Row::Object(other)), 1),
            Err(CoreError::Conflict { .. })
        ));

        // Removing a prop.
        let mut with_due = object(1);
        with_due.props_json.insert("due".into(), json!("x"));
        let mut update = Update::new(id(1));
        update.prop("due", Some(json!("x")), None);
        let op = Op::update(kind("object.update"), update).unwrap();
        let Some(Row::Object(after)) = op.apply(Some(&Row::Object(with_due)), 1).unwrap() else {
            panic!()
        };
        assert!(after.props_json.is_empty());

        // A type-incorrect value fails when the row is rebuilt.
        let mut update = Update::new(id(1));
        update.column("title", json!("Object 1"), json!(42));
        let op = Op::update(kind("object.update"), update).unwrap();
        assert!(matches!(op.apply(Some(&row), 1), Err(CoreError::Json(_))));

        // A rule-breaking value fails validation.
        let mut update = Update::new(id(1));
        update.column("geo_lat", Value::Null, json!(1.0));
        let op = Op::update(kind("object.update"), update).unwrap();
        assert!(matches!(
            op.apply(Some(&row), 1),
            Err(CoreError::Invalid { .. })
        ));
    }

    #[test]
    fn updates_are_limited_to_their_kinds_columns() {
        let mut update = Update::new(id(1));
        update.column("deleted_at", Value::Null, json!(5));
        assert!(Op::update(kind("object.update"), update.clone()).is_err());
        assert!(Op::update(kind("object.trash"), update.clone()).is_ok());
        assert!(Op::update(kind("object.create"), update).is_err());
        assert!(Op::update(kind("object.update"), Update::new(id(1))).is_err());
        let mut props = Update::new(id(1));
        props.prop("x", None, Some(json!(1)));
        assert!(Op::update(kind("object.move"), props.clone()).is_err());
        assert!(Op::update(kind("relation.update"), props).is_err());
    }

    #[test]
    fn props_on_a_row_without_props_are_refused() {
        // Built by hand: Op::update would already refuse this.
        let mut update = Update::new(id(1));
        update.prop("x", None, Some(json!(1)));
        let op = Op {
            kind: kind("type.update"),
            body: OpBody::Update(update),
        };
        assert!(op.apply(Some(&Row::Type(type_row(1))), 1).is_err());
    }

    #[test]
    fn undo_checks_and_inverses() {
        // Create: undoable while unchanged apart from updated_at.
        let created = obj(1);
        let create = Op::create(created.clone());
        let mut touched = object(1);
        touched.updated_at = 50;
        let inverse = create.inverse(Some(&Row::Object(touched.clone()))).unwrap();
        assert_eq!(inverse, Op::delete(Row::Object(touched)));
        let mut edited = object(1);
        edited.title = "edited".into();
        assert!(
            create
                .check_still_current(Some(&Row::Object(edited)))
                .is_err()
        );
        assert!(create.check_still_current(None).is_err());
        assert!(create.inverse(None).is_err());

        // Delete: undoable while the row is still absent.
        let delete = Op::delete(created.clone());
        assert_eq!(delete.inverse(None).unwrap(), Op::create(created.clone()));
        assert!(delete.check_still_current(Some(&created)).is_err());

        // Update: undoable while the changed columns and props still hold the new values.
        let mut update = Update::new(id(1));
        update.column("title", json!("Object 1"), json!("New"));
        update.prop("p", None, Some(json!(true)));
        let op = Op::update(kind("object.update"), update).unwrap();
        let after = op.apply(Some(&created), 7).unwrap().unwrap();
        let undo = op.inverse(Some(&after)).unwrap();
        assert_eq!(undo.kind(), kind("object.update"));
        let restored = undo.apply(Some(&after), 8).unwrap().unwrap();
        let Row::Object(restored) = restored else {
            panic!()
        };
        assert_eq!(restored.title, "Object 1");
        assert!(restored.props_json.is_empty());
        assert_eq!(restored.updated_at, 8);
        assert!(
            op.check_still_current(Some(&created)).is_err(),
            "title changed back"
        );
        let mut prop_changed = after.clone();
        if let Row::Object(r) = &mut prop_changed {
            r.props_json.insert("p".into(), json!(false));
        }
        assert!(op.check_still_current(Some(&prop_changed)).is_err());
        assert!(op.check_still_current(None).is_err());

        // Trash inverts to restore.
        let mut trash = Update::new(id(1));
        trash.column("deleted_at", Value::Null, json!(9));
        let trash = Op::update(kind("object.trash"), trash).unwrap();
        let trashed = trash.apply(Some(&created), 9).unwrap().unwrap();
        assert_eq!(
            trash.inverse(Some(&trashed)).unwrap().kind(),
            kind("object.restore")
        );
    }

    #[test]
    fn payloads_round_trip_through_decode() {
        let mut update = Update::new(id(1));
        update.column("sort_key", json!("a0"), json!("a1"));
        update.column("parent_id", Value::Null, json!(id(2)));
        let ops = vec![
            Op::create(obj(1)),
            Op::delete(Row::Type(type_row(3))),
            Op::create(Row::Field(field(
                4,
                FieldKind::Select,
                json!({"options": [{"id": "a"}]}),
            ))),
            Op::create(Row::ObjectType(ObjectTypeRow {
                object_id: id(1),
                type_id: id(3),
            })),
            Op::create(Row::Block(BlockRow {
                id: id(5),
                object_id: id(1),
                parent_block_id: None,
                sort_key: SortKey::first(),
                kind: "paragraph".into(),
                content_json: json!({"text": "hi"}),
                created_at: 1,
                updated_at: 1,
            })),
            Op::create(Row::Relation(RelationRow {
                id: id(6),
                src_id: id(1),
                dst_id: id(2),
                rel_type: "links_to".into(),
                field_id: None,
                src_block_id: None,
                label: None,
                props_json: Default::default(),
                created_at: 1,
                deleted_at: None,
            })),
            Op::create(Row::Attachment(AttachmentRow {
                object_id: id(1),
                blob_hash: ContentHash::of(b"x"),
                role: AttachmentRole::Original,
                filename: None,
                blob_size: 1,
                mime: None,
            })),
            Op::update(kind("object.move"), update).unwrap(),
        ];
        for op in ops {
            let payload = op.payload_json().unwrap();
            let decoded = Op::decode(&op.kind().to_string(), &payload).unwrap();
            assert_eq!(decoded, op, "{payload}");
            assert_eq!(decoded.key().target_id(), op.key().target_id());
            assert!(!op.key().to_string().is_empty());
            assert_eq!(op.body(), decoded.body());
        }
        assert!(Op::decode("object.create", "{}").is_err());
        assert!(Op::decode("object.create", "not json").is_err());
        assert!(Op::decode("object.fly", "{}").is_err());
        assert!(
            Op::decode(
                "object.update",
                r#"{"id":"01JB3R7Q8Y5K3M2N4P6Q8R0S2T","set":{"title":{"from":"a"}}}"#
            )
            .is_err(),
            "missing to"
        );
    }

    #[test]
    fn between_diffs_two_rows() {
        let before = object(1);
        let mut after = object(1);
        after.title = "New".into();
        after.updated_at = 99;
        after.props_json.insert("due".into(), json!("2026-11-30"));
        let op = Op::between(
            kind("object.update"),
            &Row::Object(before.clone()),
            &Row::Object(after.clone()),
        )
        .unwrap()
        .unwrap();
        let OpBody::Update(update) = op.body() else {
            panic!()
        };
        assert_eq!(update.set.keys().collect::<Vec<_>>(), ["title"]);
        assert_eq!(
            update.props["due"],
            PropChange {
                from: None,
                to: Some(json!("2026-11-30"))
            }
        );
        let Some(Row::Object(applied)) = op.apply(Some(&Row::Object(before.clone())), 99).unwrap()
        else {
            panic!()
        };
        assert_eq!(applied, after);

        // Only updated_at differs: nothing to do.
        let mut touched = object(1);
        touched.updated_at = 5;
        assert_eq!(
            Op::between(
                kind("object.update"),
                &Row::Object(before.clone()),
                &Row::Object(touched)
            )
            .unwrap(),
            None
        );

        // A column the kind may not change.
        let mut moved = object(1);
        moved.sort_key = SortKey::parse("a1").unwrap();
        assert!(
            Op::between(
                kind("object.update"),
                &Row::Object(before.clone()),
                &Row::Object(moved.clone())
            )
            .is_err()
        );
        assert!(
            Op::between(
                kind("object.move"),
                &Row::Object(before.clone()),
                &Row::Object(moved)
            )
            .unwrap()
            .is_some()
        );

        // Relation props are one column, not per-key changes.
        let relation = RelationRow {
            id: id(6),
            src_id: id(1),
            dst_id: id(2),
            rel_type: "links_to".into(),
            field_id: None,
            src_block_id: None,
            label: None,
            props_json: Default::default(),
            created_at: 1,
            deleted_at: None,
        };
        let mut changed = relation.clone();
        changed.props_json.insert("weight".into(), json!(2));
        let op = Op::between(
            kind("relation.update"),
            &Row::Relation(relation.clone()),
            &Row::Relation(changed),
        )
        .unwrap()
        .unwrap();
        let OpBody::Update(update) = op.body() else {
            panic!()
        };
        assert!(update.set.contains_key("props_json") && update.props.is_empty());

        // Different rows, the wrong kind, and rows without an ID are refused.
        assert!(Op::between(kind("object.update"), &Row::Object(before.clone()), &obj(2)).is_err());
        assert!(Op::between(kind("relation.update"), &Row::Object(before), &obj(1)).is_err());
        let link = Row::ObjectType(ObjectTypeRow {
            object_id: id(1),
            type_id: id(2),
        });
        assert!(Op::between(kind("object.update"), &link, &link).is_err());
    }

    #[test]
    fn prop_changes_tell_null_from_absent() {
        let explicit_null: PropChange = serde_json::from_str(r#"{"from":null}"#).unwrap();
        assert_eq!(explicit_null.from, Some(Value::Null));
        assert_eq!(explicit_null.to, None);
        assert_eq!(
            serde_json::to_string(&explicit_null).unwrap(),
            r#"{"from":null}"#
        );
    }

    #[test]
    fn row_keys_identify_rows() {
        let attachment = AttachmentRow {
            object_id: id(1),
            blob_hash: ContentHash::of(b"x"),
            role: AttachmentRole::Thumbnail,
            filename: None,
            blob_size: 1,
            mime: None,
        };
        let key = Row::Attachment(attachment).key();
        assert_eq!(key.entity(), Entity::Attachment);
        assert_eq!(key.target_id(), id(1));
        assert!(key.to_string().ends_with("/thumbnail"));
        let key = Row::ObjectType(ObjectTypeRow {
            object_id: id(1),
            type_id: id(2),
        })
        .key();
        assert_eq!(key.entity(), Entity::ObjectType);
        assert_eq!(key.to_string(), format!("{}/{}", id(1), id(2)));
        assert_eq!(Row::Object(object(1)).entity(), Entity::Object);
        for row in [
            Row::Type(type_row(1)),
            Row::ObjectType(ObjectTypeRow {
                object_id: id(1),
                type_id: id(2),
            }),
        ] {
            row.validate().unwrap();
            let json = row.to_json().unwrap();
            assert_eq!(Row::from_json(row.entity(), &json).unwrap(), row);
        }
        let mut sensitive = object(1);
        sensitive.sensitivity = Sensitivity::Sensitive;
        assert!(!Row::Object(sensitive).same_ignoring_updated_at(&obj(1)));
    }
}
