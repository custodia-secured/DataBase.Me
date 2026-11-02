// SPDX-License-Identifier: AGPL-3.0-or-later
//! Running a [`Query`] (ADR-0020): its results, how many there are, and facets over them.

use rusqlite::types::{Value as SqlValue, ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::Id;
use dbme_core::model::ViewRow;
use dbme_core::query::Query;

use crate::compile::{Compiled, Select, compile};
use crate::list::ObjectSummary;
use crate::query::Reader;
use crate::rows::{view_columns, view_row};
use crate::sql;
use crate::{Result, StoreError};

/// One value among a query's results, and how many of them have it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FacetBucket {
    /// The value: a type ID for the type facet, otherwise a field value.
    pub value: Value,
    /// How many results have it.
    pub count: u32,
}

/// The facet that counts results per type.
pub const TYPE_FACET: &str = "$type";

fn json_of(value: ValueRef<'_>) -> Option<Value> {
    match value {
        ValueRef::Null | ValueRef::Blob(_) => None,
        ValueRef::Integer(i) => Some(Value::from(i)),
        ValueRef::Real(f) => Some(Value::from(f)),
        ValueRef::Text(text) => Some(Value::String(String::from_utf8_lossy(text).into_owned())),
    }
}

impl Reader<'_> {
    /// The types of `query.from`, each with the types that extend it.
    fn query_types(&self, query: &Query) -> Result<Vec<Id>> {
        let mut types = Vec::new();
        for type_id in &query.from.types {
            for id in self.type_and_subtypes(*type_id)? {
                if !types.contains(&id) {
                    types.push(id);
                }
            }
        }
        Ok(types)
    }

    fn compiled(&self, query: &Query, select: Select, field: &str) -> Result<Compiled> {
        if query.has_search() {
            crate::search::flush(self.conn)?;
        }
        let types = self.query_types(query)?;
        compile(query, &types, select, field)
    }

    /// The results of `query`, in its order, from its offset and up to its limit.
    pub fn query(&self, query: &Query) -> Result<Vec<ObjectSummary>> {
        let compiled = self.compiled(query, Select::Ids, "")?;
        let mut statement = self.conn.prepare_cached(&compiled.sql)?;
        let ids: Vec<Id> = statement
            .query_map(rusqlite::params_from_iter(compiled.params), |r| {
                sql::id(r, 0)
            })?
            .collect::<rusqlite::Result<_>>()?;
        self.summaries(&ids)
    }

    /// How many results `query` has in all, whatever its limit and offset.
    pub fn query_count(&self, query: &Query) -> Result<u64> {
        let compiled = self.compiled(query, Select::Count, "")?;
        crate::query::count(
            self.conn,
            &compiled.sql,
            rusqlite::params_from_iter(compiled.params),
        )
    }

    /// The values of `field` among all of `query`'s results, most common first, up to 50. `field`
    /// is a field key, or [`TYPE_FACET`] for the results' types.
    pub fn facets(&self, query: &Query, field: &str) -> Result<Vec<FacetBucket>> {
        let select = if field == TYPE_FACET {
            Select::FacetTypes
        } else {
            Select::FacetField
        };
        let compiled = self.compiled(query, select, field)?;
        let mut statement = self.conn.prepare_cached(&compiled.sql)?;
        let buckets = statement.query_map(rusqlite::params_from_iter(compiled.params), |r| {
            let count: i64 = r.get(1)?;
            Ok((
                json_of(r.get_ref(0)?),
                u32::try_from(count).unwrap_or(u32::MAX),
            ))
        })?;
        Ok(buckets
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(value, count)| value.map(|value| FacetBucket { value, count }))
            .collect())
    }

    /// Every saved view, by name.
    pub fn views(&self) -> Result<Vec<ViewRow>> {
        let mut statement = self.conn.prepare_cached(concat!(
            "SELECT ",
            view_columns!(),
            " FROM views ORDER BY name COLLATE NOCASE, id"
        ))?;
        let views = statement.query_map([], view_row)?;
        Ok(views.collect::<rusqlite::Result<_>>()?)
    }

    /// A saved view.
    pub fn view(&self, id: Id) -> Result<ViewRow> {
        use dbme_core::op::{Entity, Row, RowKey};
        match crate::rows::load(self.conn, &RowKey::Id(Entity::View, id))? {
            Some(Row::View(view)) => Ok(view),
            _ => Err(StoreError::not_found(Entity::View.as_str(), id)),
        }
    }
}

impl From<SqlValue> for FacetBucket {
    fn from(value: SqlValue) -> Self {
        Self {
            value: json_of(ValueRef::from(&value)).unwrap_or(Value::Null),
            count: 0,
        }
    }
}
