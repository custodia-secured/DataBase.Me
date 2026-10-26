// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads for the views (ADR-0017): pages of objects from a source in a chosen order, counts,
//! summaries, a title search, backlinks and ancestors.
//!
//! A page is read in two steps: first the IDs, in order, then a summary for each. Computing the
//! summary columns inside the ordered query would compute them for every row the sort sees, not
//! just the page.

use serde::{Deserialize, Serialize};

use dbme_core::model::{BlockRow, FieldRow, ObjectRow, RelationRow, is_identifier};
use dbme_core::{ContentHash, CoreError, Id};

use crate::query::{Reader, count};
use crate::rows::{object_columns, object_row};
use crate::sql::{self, to_json};
use crate::{Result, StoreError};

/// The most rows one page may hold.
pub const MAX_PAGE: u32 = 1000;

/// Which objects a list shows. Objects in the Trash are never listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Source {
    /// The children of `parent`, or the top-level objects.
    Children {
        /// The parent, or `None` for the top level.
        parent: Option<Id>,
    },
    /// The objects of a type, or of any type that extends it.
    Type {
        /// The type.
        type_id: Id,
    },
}

/// What a list is ordered by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SortBy {
    /// The order people gave the children (sort keys), or creation order for a type.
    #[default]
    Manual,
    /// The title, ignoring case.
    Title,
    /// When the object was created.
    Created,
    /// When the object last changed.
    Updated,
    /// The value of a field. Objects without a value come last.
    Field,
}

/// The order of a list. Ties are broken by ID, in the same direction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Sort {
    /// What to order by.
    pub by: SortBy,
    /// The field's key, when `by` is [`SortBy::Field`].
    #[serde(default)]
    pub field: Option<String>,
    /// Largest first.
    #[serde(default)]
    pub descending: bool,
}

/// An object with what a list shows beside it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ObjectSummary {
    /// The object.
    pub object: ObjectRow,
    /// Its types, by ID.
    pub type_ids: Vec<Id>,
    /// How many live children it has.
    pub child_count: u32,
    /// Its cover: the first original image attached to it, by hash.
    pub cover: Option<ContentHash>,
}

/// A relation to an object, with where it comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Backlink {
    /// The relation.
    pub relation: RelationRow,
    /// The object it comes from.
    pub source: ObjectRow,
    /// The block it comes from, for a mention.
    pub block: Option<BlockRow>,
}

fn first_id(row: &rusqlite::Row<'_>) -> rusqlite::Result<Id> {
    sql::id(row, 0)
}

/// The `ORDER BY` terms of `sort` over objects `o`. A field key is bound as `?4`.
fn order_by(source: Source, sort: &Sort) -> Result<String> {
    let direction = if sort.descending { " DESC" } else { "" };
    let terms = match (sort.by, source) {
        (SortBy::Manual, Source::Children { .. }) => vec!["o.sort_key"],
        (SortBy::Manual, Source::Type { .. }) => vec![],
        (SortBy::Title, _) => vec!["o.title COLLATE NOCASE"],
        (SortBy::Created, _) => vec!["o.created_at"],
        (SortBy::Updated, _) => vec!["o.updated_at"],
        (SortBy::Field, _) => {
            if !sort
                .field
                .as_deref()
                .is_some_and(|key| is_identifier(key, 64))
            {
                return Err(CoreError::invalid("sort", "sorting by a field needs its key").into());
            }
            vec!["o.props_json ->> ?4"]
        }
    };
    let mut clause = String::new();
    if sort.by == SortBy::Field {
        // Objects without a value come last in either direction.
        clause.push_str("(o.props_json ->> ?4) IS NULL, ");
    }
    for term in terms.into_iter().chain(["o.id"]) {
        clause.push_str(term);
        clause.push_str(direction);
        clause.push_str(", ");
    }
    clause.truncate(clause.len() - 2);
    Ok(clause)
}

impl Reader<'_> {
    /// `type_id` and every type that extends it, directly or not.
    fn type_and_subtypes(&self, type_id: Id) -> Result<Vec<Id>> {
        self.type_row(type_id)?;
        let mut statement = self.conn.prepare_cached(
            "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION SELECT t.id FROM types t JOIN sub ON t.extends = sub.id) \
             SELECT id FROM sub",
        )?;
        let ids = statement.query_map([type_id.to_string()], |r| sql::id(r, 0))?;
        Ok(ids.collect::<rusqlite::Result<Vec<Id>>>()?)
    }

    /// The `FROM ... WHERE` part of a query over `source`, with `?1` bound to the returned value.
    fn source_scope(&self, source: Source) -> Result<(&'static str, Option<String>)> {
        Ok(match source {
            Source::Children { parent } => (
                "FROM objects o WHERE o.parent_id IS ?1 AND o.deleted_at IS NULL",
                sql::optional(parent),
            ),
            Source::Type { type_id } => {
                let types = self.type_and_subtypes(type_id)?;
                if let [only] = types.as_slice() {
                    (
                        "FROM object_types ot JOIN objects o ON o.id = ot.object_id \
                         WHERE ot.type_id = ?1 AND o.deleted_at IS NULL",
                        Some(only.to_string()),
                    )
                } else {
                    (
                        "FROM objects o WHERE o.deleted_at IS NULL AND o.id IN \
                         (SELECT object_id FROM object_types WHERE type_id IN (SELECT value FROM json_each(?1)))",
                        Some(to_json(&types)?),
                    )
                }
            }
        })
    }

    /// How many objects `source` lists.
    pub fn count_in(&self, source: Source) -> Result<u64> {
        let (scope, param) = self.source_scope(source)?;
        count(self.conn, &format!("SELECT count(*) {scope}"), [param])
    }

    /// Up to `limit` (at most [`MAX_PAGE`]) objects of `source` in `sort` order, starting at
    /// `offset`.
    pub fn page(
        &self,
        source: Source,
        sort: &Sort,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<ObjectSummary>> {
        if limit > MAX_PAGE {
            return Err(StoreError::Rule(format!(
                "a page holds at most {MAX_PAGE} rows"
            )));
        }
        let (scope, param) = self.source_scope(source)?;
        let sql = format!(
            "SELECT o.id {scope} ORDER BY {} LIMIT ?2 OFFSET ?3",
            order_by(source, sort)?
        );
        let mut statement = self.conn.prepare_cached(&sql)?;
        let ids = match sort.field.as_deref().filter(|_| sort.by == SortBy::Field) {
            Some(key) => statement
                .query_map(rusqlite::params![param, limit, offset, key], first_id)?
                .collect::<rusqlite::Result<Vec<Id>>>()?,
            None => statement
                .query_map(rusqlite::params![param, limit, offset], first_id)?
                .collect::<rusqlite::Result<Vec<Id>>>()?,
        };
        self.summaries(&ids)
    }

    /// The summaries of `ids`, in the same order. IDs of objects that do not exist are skipped.
    pub fn summaries(&self, ids: &[Id]) -> Result<Vec<ObjectSummary>> {
        let mut statement = self.conn.prepare_cached(concat!(
            "SELECT ",
            object_columns!(),
            ", (SELECT group_concat(type_id, ' ') FROM object_types WHERE object_id = o.id), \
             (SELECT count(*) FROM objects c WHERE c.parent_id = o.id AND c.deleted_at IS NULL), \
             (SELECT min(a.blob_hash) FROM attachments a WHERE a.object_id = o.id \
              AND a.role = 'original' AND a.mime LIKE 'image/%') \
             FROM objects o WHERE o.id = ?1"
        ))?;
        let mut summaries = Vec::with_capacity(ids.len());
        for id in ids {
            let mut rows = statement.query([id.to_string()])?;
            let Some(row) = rows.next()? else {
                continue;
            };
            let mut type_ids = row
                .get::<_, Option<String>>(15)?
                .unwrap_or_default()
                .split_whitespace()
                .map(Id::parse)
                .collect::<std::result::Result<Vec<Id>, CoreError>>()?;
            type_ids.sort_unstable();
            let children: i64 = row.get(16)?;
            summaries.push(ObjectSummary {
                object: object_row(row)?,
                type_ids,
                child_count: u32::try_from(children).unwrap_or(u32::MAX),
                cover: sql::optional_hash(row, 17)?,
            });
        }
        Ok(summaries)
    }

    /// The fields the objects of `source` can show: those of their types and the types those
    /// extend, one per key (the first by display order).
    pub fn fields_in(&self, source: Source) -> Result<Vec<FieldRow>> {
        let type_ids = match source {
            Source::Type { type_id } => vec![type_id],
            Source::Children { parent } => {
                let mut statement = self.conn.prepare_cached(
                    "SELECT DISTINCT ot.type_id FROM objects o JOIN object_types ot ON ot.object_id = o.id \
                     WHERE o.parent_id IS ?1 AND o.deleted_at IS NULL",
                )?;
                let ids = statement.query_map([sql::optional(parent)], |r| sql::id(r, 0))?;
                ids.collect::<rusqlite::Result<Vec<Id>>>()?
            }
        };
        let mut fields = self.fields_of_types(&type_ids)?;
        let mut seen = Vec::with_capacity(fields.len());
        fields.retain(|field| {
            let new = !seen.contains(&field.key);
            if new {
                seen.push(field.key.clone());
            }
            new
        });
        Ok(fields)
    }

    /// Live objects whose title contains `text`, ignoring case: titles that start with it first,
    /// then shorter titles. With no text, the most recently changed objects.
    pub fn search_titles(&self, text: &str, limit: u32) -> Result<Vec<ObjectSummary>> {
        let limit = limit.min(MAX_PAGE);
        let text = text.trim();
        let ids = if text.is_empty() {
            let mut statement = self.conn.prepare_cached(
                "SELECT id FROM objects WHERE deleted_at IS NULL ORDER BY updated_at DESC, id DESC LIMIT ?1",
            )?;
            statement
                .query_map([limit], |r| sql::id(r, 0))?
                .collect::<rusqlite::Result<Vec<Id>>>()?
        } else {
            let escaped = text
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let mut statement = self.conn.prepare_cached(
                "SELECT id FROM objects WHERE deleted_at IS NULL AND title LIKE ?1 ESCAPE '\\' \
                 ORDER BY title NOT LIKE ?2 ESCAPE '\\', length(title), title COLLATE NOCASE, id LIMIT ?3",
            )?;
            statement
                .query_map(
                    rusqlite::params![format!("%{escaped}%"), format!("{escaped}%"), limit],
                    |r| sql::id(r, 0),
                )?
                .collect::<rusqlite::Result<Vec<Id>>>()?
        };
        self.summaries(&ids)
    }

    /// The live relations to an object, each with its source object and, for a mention, its
    /// block.
    pub fn backlinks(&self, id: Id) -> Result<Vec<Backlink>> {
        self.relations_to(id)?
            .into_iter()
            .map(|relation| {
                Ok(Backlink {
                    source: self.object(relation.src_id)?,
                    block: relation
                        .src_block_id
                        .map(|block| self.block(block))
                        .transpose()?,
                    relation,
                })
            })
            .collect()
    }

    /// The objects above `id`, from the top level down to its parent.
    pub fn ancestors(&self, id: Id) -> Result<Vec<ObjectRow>> {
        let mut ancestors = Vec::new();
        let mut parent = self.object(id)?.parent_id;
        while let Some(above) = parent {
            let object = self.object(above)?;
            parent = object.parent_id;
            ancestors.push(object);
        }
        ancestors.reverse();
        Ok(ancestors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_end_with_the_id_in_the_same_direction() {
        let children = Source::Children { parent: None };
        let sort = |by, descending| Sort {
            by,
            field: Some("due".into()),
            descending,
        };
        assert_eq!(
            order_by(children, &sort(SortBy::Manual, false)).unwrap(),
            "o.sort_key, o.id"
        );
        let of_type = Source::Type {
            type_id: Id::from_parts(1, 1),
        };
        assert_eq!(
            order_by(of_type, &sort(SortBy::Manual, true)).unwrap(),
            "o.id DESC"
        );
        assert_eq!(
            order_by(of_type, &sort(SortBy::Field, true)).unwrap(),
            "(o.props_json ->> ?4) IS NULL, o.props_json ->> ?4 DESC, o.id DESC"
        );
        let unnamed = Sort {
            by: SortBy::Field,
            field: Some("not a key".into()),
            descending: false,
        };
        assert!(order_by(children, &unnamed).is_err());
    }
}
