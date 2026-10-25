// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one engine every change goes through: read the row an op targets, let the op check it and
//! compute the new row ([`Op::apply`]), and write the result. Statements are prepared once per
//! connection and cached.

use rusqlite::{Connection, OptionalExtension, params};

use dbme_core::model::{
    AttachmentRole, AttachmentRow, BlockRow, FieldKind, FieldRow, ObjectRow, ObjectTypeRow,
    RelationRow, Sensitivity, TypeRow,
};
use dbme_core::op::{Entity, Op, Row, RowKey};
use dbme_core::{CoreError, Id};

use crate::sql::{self, optional, to_json};
use crate::{Result, StoreError};

// Column lists, in the order the row mappers below read them.
macro_rules! type_columns {
    () => {
        "id, name, plural, icon, color, extends, description, is_system, created_at, updated_at"
    };
}
macro_rules! field_columns {
    () => {
        "id, type_id, key, label, kind, options_json, required, sort_order"
    };
}
macro_rules! object_columns {
    () => {
        "id, title, summary, parent_id, sort_key, props_json, created_at, updated_at, captured_at, event_start, \
         event_end, geo_lat, geo_lon, deleted_at, sensitivity"
    };
}
macro_rules! block_columns {
    () => {
        "id, object_id, parent_block_id, sort_key, kind, content_json, created_at, updated_at"
    };
}
macro_rules! relation_columns {
    () => {
        "id, src_id, dst_id, rel_type, field_id, src_block_id, label, props_json, created_at, deleted_at"
    };
}
macro_rules! attachment_select {
    () => {
        "SELECT a.object_id, a.blob_hash, a.role, a.filename, a.mime, b.size \
         FROM attachments a JOIN blobs b ON b.hash = a.blob_hash"
    };
}
pub(crate) use {
    attachment_select, block_columns, field_columns, object_columns, relation_columns, type_columns,
};

pub(crate) fn type_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TypeRow> {
    Ok(TypeRow {
        id: sql::id(r, 0)?,
        name: r.get(1)?,
        plural: r.get(2)?,
        icon: r.get(3)?,
        color: r.get(4)?,
        extends: sql::optional_id(r, 5)?,
        description: r.get(6)?,
        is_system: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub(crate) fn field_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FieldRow> {
    Ok(FieldRow {
        id: sql::id(r, 0)?,
        type_id: sql::id(r, 1)?,
        key: r.get(2)?,
        label: r.get(3)?,
        kind: sql::name(r, 4, FieldKind::parse)?,
        options_json: sql::json(r, 5)?,
        required: r.get(6)?,
        sort_order: r.get(7)?,
    })
}

pub(crate) fn object_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ObjectRow> {
    Ok(ObjectRow {
        id: sql::id(r, 0)?,
        title: r.get(1)?,
        summary: r.get(2)?,
        parent_id: sql::optional_id(r, 3)?,
        sort_key: sql::sort_key(r, 4)?,
        props_json: sql::json(r, 5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        captured_at: r.get(8)?,
        event_start: r.get(9)?,
        event_end: r.get(10)?,
        geo_lat: r.get(11)?,
        geo_lon: r.get(12)?,
        deleted_at: r.get(13)?,
        sensitivity: sql::name(r, 14, Sensitivity::parse)?,
    })
}

pub(crate) fn block_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BlockRow> {
    Ok(BlockRow {
        id: sql::id(r, 0)?,
        object_id: sql::id(r, 1)?,
        parent_block_id: sql::optional_id(r, 2)?,
        sort_key: sql::sort_key(r, 3)?,
        kind: r.get(4)?,
        content_json: sql::json(r, 5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

pub(crate) fn relation_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RelationRow> {
    Ok(RelationRow {
        id: sql::id(r, 0)?,
        src_id: sql::id(r, 1)?,
        dst_id: sql::id(r, 2)?,
        rel_type: r.get(3)?,
        field_id: sql::optional_id(r, 4)?,
        src_block_id: sql::optional_id(r, 5)?,
        label: r.get(6)?,
        props_json: sql::json(r, 7)?,
        created_at: r.get(8)?,
        deleted_at: r.get(9)?,
    })
}

pub(crate) fn attachment_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRow> {
    Ok(AttachmentRow {
        object_id: sql::id(r, 0)?,
        blob_hash: sql::hash(r, 1)?,
        role: sql::name(r, 2, AttachmentRole::parse)?,
        filename: r.get(3)?,
        mime: r.get(4)?,
        blob_size: r.get(5)?,
    })
}

fn by_id<T>(
    conn: &Connection,
    sql: &str,
    id: Id,
    map: fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Option<T>> {
    Ok(conn
        .prepare_cached(sql)?
        .query_row([id.to_string()], map)
        .optional()?)
}

fn not_keyed_by_id(entity: Entity) -> StoreError {
    CoreError::invalid("row key", format!("{entity} rows have no id column")).into()
}

/// The row with `key` as it is now, if it exists.
pub(crate) fn load(conn: &Connection, key: &RowKey) -> Result<Option<Row>> {
    Ok(match key {
        RowKey::Id(Entity::Type, id) => by_id(
            conn,
            concat!("SELECT ", type_columns!(), " FROM types WHERE id = ?1"),
            *id,
            type_row,
        )?
        .map(Row::Type),
        RowKey::Id(Entity::Field, id) => by_id(
            conn,
            concat!("SELECT ", field_columns!(), " FROM fields WHERE id = ?1"),
            *id,
            field_row,
        )?
        .map(Row::Field),
        RowKey::Id(Entity::Object, id) => by_id(
            conn,
            concat!("SELECT ", object_columns!(), " FROM objects WHERE id = ?1"),
            *id,
            object_row,
        )?
        .map(Row::Object),
        RowKey::Id(Entity::Block, id) => by_id(
            conn,
            concat!("SELECT ", block_columns!(), " FROM blocks WHERE id = ?1"),
            *id,
            block_row,
        )?
        .map(Row::Block),
        RowKey::Id(Entity::Relation, id) => by_id(
            conn,
            concat!(
                "SELECT ",
                relation_columns!(),
                " FROM relations WHERE id = ?1"
            ),
            *id,
            relation_row,
        )?
        .map(Row::Relation),
        RowKey::Id(entity @ (Entity::ObjectType | Entity::Attachment), _) => {
            return Err(not_keyed_by_id(*entity));
        }
        RowKey::ObjectType { object_id, type_id } => conn
            .prepare_cached("SELECT 1 FROM object_types WHERE object_id = ?1 AND type_id = ?2")?
            .query_row([object_id.to_string(), type_id.to_string()], |_| Ok(()))
            .optional()?
            .map(|()| {
                Row::ObjectType(ObjectTypeRow {
                    object_id: *object_id,
                    type_id: *type_id,
                })
            }),
        RowKey::Attachment {
            object_id,
            blob_hash,
            role,
        } => conn
            .prepare_cached(concat!(
                attachment_select!(),
                " WHERE a.object_id = ?1 AND a.blob_hash = ?2 AND a.role = ?3"
            ))?
            .query_row(
                params![object_id.to_string(), blob_hash.as_str(), role.as_str()],
                attachment_row,
            )
            .optional()?
            .map(Row::Attachment),
    })
}

/// Writes `row` with an `INSERT` (`new`) or an `UPDATE` of every column. Both statements number
/// their parameters the same way, so one parameter list serves both.
fn write(conn: &Connection, row: &Row, new: bool) -> Result<()> {
    let changed = match row {
        Row::Type(r) => conn
            .prepare_cached(if new {
                concat!("INSERT INTO types (", type_columns!(), ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)")
            } else {
                "UPDATE types SET name = ?2, plural = ?3, icon = ?4, color = ?5, extends = ?6, description = ?7, \
                 is_system = ?8, created_at = ?9, updated_at = ?10 WHERE id = ?1"
            })?
            .execute(params![
                r.id.to_string(),
                r.name,
                r.plural,
                r.icon,
                r.color,
                optional(r.extends),
                r.description,
                r.is_system,
                r.created_at,
                r.updated_at
            ])?,
        Row::Field(r) => conn
            .prepare_cached(if new {
                concat!("INSERT INTO fields (", field_columns!(), ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")
            } else {
                "UPDATE fields SET type_id = ?2, key = ?3, label = ?4, kind = ?5, options_json = ?6, required = ?7, \
                 sort_order = ?8 WHERE id = ?1"
            })?
            .execute(params![
                r.id.to_string(),
                r.type_id.to_string(),
                r.key,
                r.label,
                r.kind.as_str(),
                to_json(&r.options_json)?,
                r.required,
                r.sort_order
            ])?,
        Row::Object(r) => conn
            .prepare_cached(if new {
                concat!(
                    "INSERT INTO objects (",
                    object_columns!(),
                    ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"
                )
            } else {
                "UPDATE objects SET title = ?2, summary = ?3, parent_id = ?4, sort_key = ?5, props_json = ?6, \
                 created_at = ?7, updated_at = ?8, captured_at = ?9, event_start = ?10, event_end = ?11, \
                 geo_lat = ?12, geo_lon = ?13, deleted_at = ?14, sensitivity = ?15 WHERE id = ?1"
            })?
            .execute(params![
                r.id.to_string(),
                r.title,
                r.summary,
                optional(r.parent_id),
                r.sort_key.as_str(),
                to_json(&r.props_json)?,
                r.created_at,
                r.updated_at,
                r.captured_at,
                r.event_start,
                r.event_end,
                r.geo_lat,
                r.geo_lon,
                r.deleted_at,
                r.sensitivity.as_str()
            ])?,
        Row::Block(r) => conn
            .prepare_cached(if new {
                concat!("INSERT INTO blocks (", block_columns!(), ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")
            } else {
                "UPDATE blocks SET object_id = ?2, parent_block_id = ?3, sort_key = ?4, kind = ?5, content_json = ?6, \
                 created_at = ?7, updated_at = ?8 WHERE id = ?1"
            })?
            .execute(params![
                r.id.to_string(),
                r.object_id.to_string(),
                optional(r.parent_block_id),
                r.sort_key.as_str(),
                r.kind,
                to_json(&r.content_json)?,
                r.created_at,
                r.updated_at
            ])?,
        Row::Relation(r) => conn
            .prepare_cached(if new {
                concat!("INSERT INTO relations (", relation_columns!(), ") VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)")
            } else {
                "UPDATE relations SET src_id = ?2, dst_id = ?3, rel_type = ?4, field_id = ?5, src_block_id = ?6, \
                 label = ?7, props_json = ?8, created_at = ?9, deleted_at = ?10 WHERE id = ?1"
            })?
            .execute(params![
                r.id.to_string(),
                r.src_id.to_string(),
                r.dst_id.to_string(),
                r.rel_type,
                optional(r.field_id),
                optional(r.src_block_id),
                r.label,
                to_json(&r.props_json)?,
                r.created_at,
                r.deleted_at
            ])?,
        Row::ObjectType(r) if new => conn
            .prepare_cached("INSERT INTO object_types (object_id, type_id) VALUES (?1, ?2)")?
            .execute([r.object_id.to_string(), r.type_id.to_string()])?,
        Row::Attachment(r) if new => conn
            .prepare_cached("INSERT INTO attachments (object_id, blob_hash, role, filename, mime) VALUES (?1, ?2, ?3, ?4, ?5)")?
            .execute(params![r.object_id.to_string(), r.blob_hash.as_str(), r.role.as_str(), r.filename, r.mime])?,
        Row::ObjectType(_) | Row::Attachment(_) => return Err(not_keyed_by_id(row.entity())),
    };
    if changed == 1 {
        Ok(())
    } else {
        Err(CoreError::invalid(
            "write",
            format!("{} row {} changed {changed} rows", row.entity(), row.key()),
        )
        .into())
    }
}

fn delete(conn: &Connection, key: &RowKey) -> Result<()> {
    let changed = match key {
        RowKey::Id(entity, id) => {
            let sql = match entity {
                Entity::Type => "DELETE FROM types WHERE id = ?1",
                Entity::Field => "DELETE FROM fields WHERE id = ?1",
                Entity::Object => "DELETE FROM objects WHERE id = ?1",
                Entity::Block => "DELETE FROM blocks WHERE id = ?1",
                Entity::Relation => "DELETE FROM relations WHERE id = ?1",
                Entity::ObjectType | Entity::Attachment => return Err(not_keyed_by_id(*entity)),
            };
            conn.prepare_cached(sql)?.execute([id.to_string()])?
        }
        RowKey::ObjectType { object_id, type_id } => conn
            .prepare_cached("DELETE FROM object_types WHERE object_id = ?1 AND type_id = ?2")?
            .execute([object_id.to_string(), type_id.to_string()])?,
        RowKey::Attachment {
            object_id,
            blob_hash,
            role,
        } => conn
            .prepare_cached(
                "DELETE FROM attachments WHERE object_id = ?1 AND blob_hash = ?2 AND role = ?3",
            )?
            .execute(params![
                object_id.to_string(),
                blob_hash.as_str(),
                role.as_str()
            ])?,
    };
    if changed == 1 {
        Ok(())
    } else {
        Err(CoreError::invalid("delete", format!("row {key} changed {changed} rows")).into())
    }
}

/// Records the blob of a new attachment in `blobs`, unless it is there already. A blob's size is
/// fixed by its content, so an attachment that disagrees with the recorded size is refused.
fn ensure_blob(conn: &Connection, attachment: &AttachmentRow, ts: i64) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO blobs (hash, size, mime, encrypted, created_at) VALUES (?1, ?2, ?3, 0, ?4) \
         ON CONFLICT (hash) DO NOTHING",
    )?
    .execute(params![
        attachment.blob_hash.as_str(),
        attachment.blob_size,
        attachment.mime,
        ts
    ])?;
    let size: i64 = conn
        .prepare_cached("SELECT size FROM blobs WHERE hash = ?1")?
        .query_row([attachment.blob_hash.as_str()], |r| r.get(0))?;
    if size == attachment.blob_size {
        Ok(())
    } else {
        Err(CoreError::invalid(
            "attachment",
            format!(
                "blob {} has size {size}, not {}",
                attachment.blob_hash, attachment.blob_size
            ),
        )
        .into())
    }
}

/// Applies `op` at time `ts`: checks the row it targets against what the op expects, then writes
/// the result. Nothing is written if the check fails.
pub(crate) fn apply(conn: &Connection, op: &Op, ts: i64) -> Result<()> {
    let key = op.key();
    let current = load(conn, &key)?;
    let after = op.apply(current.as_ref(), ts)?;
    match (current.is_some(), after) {
        (false, Some(row)) => {
            if let Row::Attachment(attachment) = &row {
                ensure_blob(conn, attachment, ts)?;
            }
            write(conn, &row, true)
        }
        (true, Some(row)) => write(conn, &row, false),
        (true, None) => delete(conn, &key),
        (false, None) => Ok(()),
    }
}
