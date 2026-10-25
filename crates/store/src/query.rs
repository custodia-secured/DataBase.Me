// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading a vault. [`Reader`] works the same on an open vault and inside a transaction, where it
//! sees the transaction's own changes.

use rusqlite::{Connection, OptionalExtension, params};

use dbme_core::chain::OpRecord;
use dbme_core::model::{AttachmentRow, BlockRow, FieldRow, ObjectRow, RelationRow, TypeRow};
use dbme_core::op::{Entity, Row, RowKey};
use dbme_core::{Id, SortKey};

use crate::log::op_columns;
use crate::rows::{
    self, attachment_row, attachment_select, block_columns, block_row, field_columns, field_row,
    object_columns, object_row, relation_columns, relation_row, type_columns, type_row,
};
use crate::sql::{self, to_json};
use crate::{Position, Result, StoreError};

/// Read access to a vault.
#[derive(Clone, Copy, Debug)]
pub struct Reader<'c> {
    pub(crate) conn: &'c Connection,
}

macro_rules! by_id {
    ($name:ident, $variant:ident, $row:ty, $doc:literal) => {
        #[doc = $doc]
        pub fn $name(&self, id: Id) -> Result<$row> {
            match rows::load(self.conn, &RowKey::Id(Entity::$variant, id))? {
                Some(Row::$variant(row)) => Ok(row),
                _ => Err(StoreError::not_found(Entity::$variant.as_str(), id)),
            }
        }
    };
}

fn collect<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    map: fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut statement = conn.prepare_cached(sql)?;
    let rows = statement.query_map(params, map)?;
    Ok(rows.collect::<rusqlite::Result<Vec<T>>>()?)
}

/// Runs a `count(*)` query.
pub(crate) fn count(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<u64> {
    let count: i64 = conn.prepare_cached(sql)?.query_row(params, |r| r.get(0))?;
    Ok(u64::try_from(count).unwrap_or_default())
}

/// The siblings a new or moved item goes among.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Siblings {
    /// Objects with this parent.
    Objects(Option<Id>),
    /// Blocks of this object with this parent block.
    Blocks(Id, Option<Id>),
}

impl Reader<'_> {
    by_id!(type_row, Type, TypeRow, "A type.");
    by_id!(field, Field, FieldRow, "A field definition.");
    by_id!(
        object,
        Object,
        ObjectRow,
        "An object, whether or not it is in the Trash."
    );
    by_id!(block, Block, BlockRow, "A block.");
    by_id!(
        relation,
        Relation,
        RelationRow,
        "A relation, whether or not it is in the Trash."
    );

    /// Every type, by name.
    pub fn types(&self) -> Result<Vec<TypeRow>> {
        collect(
            self.conn,
            concat!("SELECT ", type_columns!(), " FROM types ORDER BY name, id"),
            [],
            type_row,
        )
    }

    /// The fields of one type, in display order. Inherited fields are not included.
    pub fn fields(&self, type_id: Id) -> Result<Vec<FieldRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                field_columns!(),
                " FROM fields WHERE type_id = ?1 ORDER BY sort_order, key"
            ),
            [type_id.to_string()],
            field_row,
        )
    }

    /// The fields of the given types and every type they extend.
    pub fn fields_of_types(&self, type_ids: &[Id]) -> Result<Vec<FieldRow>> {
        if type_ids.is_empty() {
            return Ok(Vec::new());
        }
        collect(
            self.conn,
            concat!(
                "WITH RECURSIVE t(id) AS (SELECT value FROM json_each(?1) ",
                "UNION SELECT types.extends FROM types JOIN t ON types.id = t.id WHERE types.extends IS NOT NULL) ",
                "SELECT ",
                field_columns!(),
                " FROM fields WHERE type_id IN t ORDER BY sort_order, key"
            ),
            [to_json(&type_ids)?],
            field_row,
        )
    }

    /// The live (not trashed) children of `parent`, or the top-level objects, in order.
    pub fn children(&self, parent: Option<Id>) -> Result<Vec<ObjectRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                object_columns!(),
                " FROM objects WHERE parent_id IS ?1 AND deleted_at IS NULL ORDER BY sort_key, id"
            ),
            [sql::optional(parent)],
            object_row,
        )
    }

    /// What the Trash shows: trashed objects whose parent is not trashed with them, most recent
    /// first.
    pub fn trash(&self) -> Result<Vec<ObjectRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                object_columns!(),
                " FROM objects o WHERE deleted_at IS NOT NULL AND NOT EXISTS ",
                "(SELECT 1 FROM objects p WHERE p.id = o.parent_id AND p.deleted_at IS NOT NULL) ",
                "ORDER BY deleted_at DESC, id"
            ),
            [],
            object_row,
        )
    }

    /// `id` and everything below it in the hierarchy, parents before children.
    pub fn subtree(&self, id: Id) -> Result<Vec<ObjectRow>> {
        collect(
            self.conn,
            concat!(
                "WITH RECURSIVE tree(node, depth) AS (SELECT ?1, 0 ",
                "UNION ALL SELECT o.id, tree.depth + 1 FROM objects o JOIN tree ON o.parent_id = tree.node) ",
                "SELECT ",
                object_columns!(),
                " FROM tree CROSS JOIN objects ON objects.id = tree.node ORDER BY tree.depth, sort_key, id"
            ),
            [id.to_string()],
            object_row,
        )
    }

    /// The IDs of an object's types.
    pub fn object_type_ids(&self, object_id: Id) -> Result<Vec<Id>> {
        collect(
            self.conn,
            "SELECT type_id FROM object_types WHERE object_id = ?1 ORDER BY type_id",
            [object_id.to_string()],
            |r| sql::id(r, 0),
        )
    }

    /// How many objects have type `type_id`.
    pub fn count_of_type(&self, type_id: Id) -> Result<u64> {
        count(
            self.conn,
            "SELECT count(*) FROM object_types WHERE type_id = ?1",
            [type_id.to_string()],
        )
    }

    /// All blocks of an object, in tree order: each block is followed by its children.
    pub fn blocks(&self, object_id: Id) -> Result<Vec<BlockRow>> {
        collect(
            self.conn,
            concat!(
                "WITH RECURSIVE tree(node, path) AS (",
                "SELECT id, sort_key || ' ' || id FROM blocks WHERE object_id = ?1 AND parent_block_id IS NULL ",
                "UNION ALL SELECT b.id, tree.path || '/' || b.sort_key || ' ' || b.id ",
                "FROM blocks b JOIN tree ON b.parent_block_id = tree.node) ",
                "SELECT ",
                block_columns!(),
                " FROM tree CROSS JOIN blocks ON blocks.id = tree.node ORDER BY tree.path"
            ),
            [object_id.to_string()],
            block_row,
        )
    }

    /// Block `id` and the blocks below it, parents before children.
    pub fn block_subtree(&self, id: Id) -> Result<Vec<BlockRow>> {
        collect(
            self.conn,
            concat!(
                "WITH RECURSIVE tree(node, depth) AS (SELECT ?1, 0 ",
                "UNION ALL SELECT b.id, tree.depth + 1 FROM blocks b JOIN tree ON b.parent_block_id = tree.node) ",
                "SELECT ",
                block_columns!(),
                " FROM tree CROSS JOIN blocks ON blocks.id = tree.node ORDER BY tree.depth, sort_key, id"
            ),
            [id.to_string()],
            block_row,
        )
    }

    /// Live relations from an object.
    pub fn relations_from(&self, id: Id) -> Result<Vec<RelationRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                relation_columns!(),
                " FROM relations WHERE src_id = ?1 AND deleted_at IS NULL ORDER BY rel_type, created_at, id"
            ),
            [id.to_string()],
            relation_row,
        )
    }

    /// Live relations to an object: its backlinks.
    pub fn relations_to(&self, id: Id) -> Result<Vec<RelationRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                relation_columns!(),
                " FROM relations WHERE dst_id = ?1 AND deleted_at IS NULL ORDER BY rel_type, created_at, id"
            ),
            [id.to_string()],
            relation_row,
        )
    }

    /// Every relation from or to an object, live or trashed.
    pub(crate) fn relations_touching(&self, id: Id) -> Result<Vec<RelationRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                relation_columns!(),
                " FROM relations WHERE src_id = ?1 OR dst_id = ?1 ORDER BY id"
            ),
            [id.to_string()],
            relation_row,
        )
    }

    /// Every relation that a field or a block is the source of, live or trashed.
    pub(crate) fn relations_using(
        &self,
        field_id: Option<Id>,
        block_id: Option<Id>,
    ) -> Result<Vec<RelationRow>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                relation_columns!(),
                " FROM relations WHERE field_id = ?1 OR src_block_id = ?2 ORDER BY id"
            ),
            [sql::optional(field_id), sql::optional(block_id)],
            relation_row,
        )
    }

    /// An object's attachments.
    pub fn attachments(&self, object_id: Id) -> Result<Vec<AttachmentRow>> {
        collect(
            self.conn,
            concat!(
                attachment_select!(),
                " WHERE a.object_id = ?1 ORDER BY a.role, a.blob_hash"
            ),
            [object_id.to_string()],
            attachment_row,
        )
    }

    /// The most recent ops, newest first.
    pub fn history(&self, limit: u32) -> Result<Vec<OpRecord>> {
        collect(
            self.conn,
            concat!(
                "SELECT ",
                op_columns!(),
                " FROM ops ORDER BY seq DESC LIMIT ?1"
            ),
            [limit],
            crate::log::record,
        )
    }

    /// How many objects the vault holds, including those in the Trash.
    pub fn object_count(&self) -> Result<u64> {
        count(self.conn, "SELECT count(*) FROM objects", [])
    }

    /// A sort key that puts an item at `position` among `siblings`. `moving` is the item being
    /// moved, if any, which does not count as its own sibling.
    pub(crate) fn sort_key_at(
        &self,
        siblings: Siblings,
        position: Position,
        moving: Option<Id>,
    ) -> Result<SortKey> {
        let (table, scope, scope_params) = match siblings {
            Siblings::Objects(parent) => {
                ("objects", "parent_id IS ?1", [sql::optional(parent), None])
            }
            Siblings::Blocks(object_id, parent) => (
                "blocks",
                "object_id = ?1 AND parent_block_id IS ?2",
                [Some(object_id.to_string()), sql::optional(parent)],
            ),
        };
        let moving = sql::optional(moving);
        let edge = |function: &str,
                    bound: &str,
                    key: Option<&SortKey>|
         -> Result<Option<SortKey>> {
            let sql = format!(
                "SELECT {function}(sort_key) FROM {table} WHERE {scope} AND id IS NOT ?3 {bound}"
            );
            let key = key.map(SortKey::as_str);
            Ok(self
                .conn
                .prepare_cached(&sql)?
                .query_row(
                    params![scope_params[0], scope_params[1], moving, key],
                    |r| {
                        r.get_ref(0)?
                            .as_str_or_null()
                            .map(|text| text.map(SortKey::parse))
                            .map_err(Into::into)
                    },
                )?
                .transpose()?)
        };
        let anchor = |id: Id| -> Result<SortKey> {
            if Some(id.to_string()) == moving {
                return Err(StoreError::Rule(
                    "an item cannot be placed next to itself".to_owned(),
                ));
            }
            let in_scope =
                format!("SELECT sort_key FROM {table} WHERE {scope} AND id = ?3 AND ?4 IS NULL");
            self.conn
                .prepare_cached(&in_scope)?
                .query_row(
                    params![
                        scope_params[0],
                        scope_params[1],
                        id.to_string(),
                        None::<String>
                    ],
                    |r| sql::sort_key(r, 0),
                )
                .optional()?
                .ok_or_else(|| {
                    StoreError::Rule(format!(
                        "{id} is not among the siblings to place this next to"
                    ))
                })
        };
        Ok(match position {
            Position::First => {
                SortKey::between(None, edge("min", "AND ?4 IS NULL", None)?.as_ref())?
            }
            Position::Last => {
                SortKey::between(edge("max", "AND ?4 IS NULL", None)?.as_ref(), None)?
            }
            Position::Before(id) => {
                let next = anchor(id)?;
                let previous = edge("max", "AND sort_key < ?4", Some(&next))?;
                SortKey::between(previous.as_ref(), Some(&next))?
            }
            Position::After(id) => {
                let previous = anchor(id)?;
                let next = edge("min", "AND sort_key > ?4", Some(&previous))?;
                SortKey::between(Some(&previous), next.as_ref())?
            }
        })
    }
}
