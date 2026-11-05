// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reads for the visual views (M8, ADR-0032, ADR-0034): figures per group, the shape of a set of
//! results, and the graph of objects and relations. Each is bounded, and each is computed here
//! rather than in a view, so a view never holds a vault's worth of rows to draw a picture of it.

use std::collections::HashMap;

use rusqlite::params_from_iter;
use rusqlite::types::{Value as SqlValue, ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::Id;
use dbme_core::model::is_identifier;
use dbme_core::query::{
    AggregateQuery, MAX_GRAPH_NODES, MAX_PROFILE_FIELDS, PROFILE_SAMPLE, Query,
};

use crate::compile::{Select, compile_aggregate};
use crate::query::Reader;
use crate::sql;
use crate::{Result, StoreError};

/// One bucket of an aggregate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AggregateRow {
    /// The bucket's value for each group, in order; `null` for no value.
    pub keys: Vec<Value>,
    /// Each measure, in order; `null` where a bucket has no number to compute from.
    pub values: Vec<Option<f64>>,
}

/// The result of an aggregate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AggregateResult {
    /// The buckets, in the order of their keys.
    pub rows: Vec<AggregateRow>,
    /// Whether there were more buckets than the limit.
    pub truncated: bool,
}

/// What a field holds across the results a profile looked at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FieldProfile {
    /// The field's key.
    pub key: String,
    /// How many results have a value.
    pub non_empty: u32,
    /// How many different values there are.
    pub distinct: u32,
    /// How many values are numbers.
    pub numeric: u32,
    /// How many values are lists.
    pub lists: u32,
    /// The smallest value.
    pub min: Option<Value>,
    /// The largest value.
    pub max: Option<Value>,
}

/// The shape of a set of results, for the view recommender.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DataProfile {
    /// How many results there are in all.
    pub objects: u32,
    /// How many of them the figures below looked at.
    pub sampled: u32,
    /// How many have an event date.
    pub with_event: u32,
    /// How many of those end after they start: a span and not a moment.
    pub with_span: u32,
    /// The earliest event date, in milliseconds since 1970.
    pub event_first: Option<i64>,
    /// The latest event date, in milliseconds since 1970.
    pub event_last: Option<i64>,
    /// How many have a location.
    pub with_geo: u32,
    /// How many have children.
    pub with_children: u32,
    /// How many have a live relation.
    pub with_relations: u32,
    /// The fields asked about, in the order asked.
    pub fields: Vec<FieldProfile>,
}

/// Objects and what joins them, as flat arrays: a large graph crosses to the UI in one reply and
/// costs a few numbers per edge rather than an object per edge.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct GraphData {
    /// The nodes.
    pub ids: Vec<Id>,
    /// Each node's title, cut to a label's length.
    pub titles: Vec<String>,
    /// The types that appear, for `node_types` to point into.
    pub types: Vec<Id>,
    /// Each node's first type, as an index into `types`; -1 for none.
    pub node_types: Vec<i32>,
    /// Pairs of indexes into `ids`: source, target, source, target.
    pub edges: Vec<u32>,
    /// The relation types that appear, for `edge_types` to point into.
    pub rel_types: Vec<String>,
    /// Each edge's relation type, as an index into `rel_types`.
    pub edge_types: Vec<u32>,
    /// Pairs of indexes into `ids`: parent, child, parent, child.
    pub parent_edges: Vec<u32>,
    /// Whether there were more nodes than the limit.
    pub truncated: bool,
}

/// The longest title a graph carries.
const GRAPH_TITLE: usize = 80;

fn count(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn index_of(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

fn json_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Integer(n) => Value::from(n),
        ValueRef::Real(n) => serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number),
        ValueRef::Text(text) => Value::String(String::from_utf8_lossy(text).into_owned()),
        ValueRef::Null | ValueRef::Blob(_) => Value::Null,
    }
}

fn number(value: ValueRef<'_>) -> Option<f64> {
    match value {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a chart's figure; a sum past 2^53 is not read to the unit"
        )]
        ValueRef::Integer(n) => Some(n as f64),
        ValueRef::Real(n) => Some(n),
        _ => None,
    }
}

impl Reader<'_> {
    /// Figures per group over a query's results.
    pub fn aggregate(&self, aggregate: &AggregateQuery) -> Result<AggregateResult> {
        if aggregate.query.has_search() {
            crate::search::flush(self.conn)?;
        }
        let types = self.query_types(&aggregate.query)?;
        let compiled = compile_aggregate(aggregate, &types)?;
        let mut statement = self.conn.prepare(&compiled.sql)?;
        let keys = aggregate.group_by.len();
        let values = aggregate.measures.len();
        let mut rows = statement
            .query_map(params_from_iter(compiled.params), |r| {
                let mut row = AggregateRow {
                    keys: Vec::with_capacity(keys),
                    values: Vec::with_capacity(values),
                };
                for column in 0..keys {
                    row.keys.push(json_value(r.get_ref(column)?));
                }
                for column in keys..keys + values {
                    row.values.push(number(r.get_ref(column)?));
                }
                Ok(row)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let limit = aggregate.bucket_limit() as usize;
        let truncated = rows.len() > limit;
        rows.truncate(limit);
        Ok(AggregateResult { rows, truncated })
    }

    /// The shape of a query's results: what the view recommender needs to know, read from a sample
    /// of them. `fields` are the field keys to look at.
    pub fn data_profile(&self, query: &Query, fields: &[String]) -> Result<DataProfile> {
        if fields.len() > MAX_PROFILE_FIELDS {
            return Err(StoreError::Rule(format!(
                "a profile looks at no more than {MAX_PROFILE_FIELDS} fields"
            )));
        }
        for key in fields {
            if !is_identifier(key, 64) {
                return Err(StoreError::Rule(format!("{key:?} is not a field key")));
            }
        }
        let objects = self.query_count(query)?;
        let set = self.compiled(query, Select::Set, "")?;
        let sample = format!("WITH s(id) AS MATERIALIZED ({} LIMIT ?)", set.sql);
        // The sample is first in each statement's text, so its parameters are first too.
        let mut sample_params = set.params;
        sample_params.push(SqlValue::Integer(i64::from(PROFILE_SAMPLE)));

        let event = self.conn.query_row(
            &format!(
                "{sample} SELECT coalesce(sum(o.event_end > o.event_start), 0), \
                 min(o.event_start), max(o.event_start) FROM s JOIN objects o ON o.id = s.id"
            ),
            params_from_iter(sample_params.clone()),
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )?;
        let (with_span, event_first, event_last) = event;

        let (sampled, with_event, with_geo, with_children, with_relations) = self.conn.query_row(
            &format!(
                "{sample} SELECT count(*), count(o.event_start), count(o.geo_lat), \
                 coalesce(sum(EXISTS (SELECT 1 FROM objects c WHERE c.parent_id = o.id AND c.deleted_at IS NULL)), 0), \
                 coalesce(sum(EXISTS (SELECT 1 FROM relations r WHERE r.deleted_at IS NULL AND (r.src_id = o.id OR r.dst_id = o.id))), 0) \
                 FROM s JOIN objects o ON o.id = s.id"
            ),
            params_from_iter(sample_params.clone()),
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )?;

        let statement = format!(
            "{sample} SELECT count(v), count(DISTINCT v), \
             coalesce(sum(typeof(v) IN ('integer', 'real')), 0), coalesce(sum(t = 'array'), 0), min(v), max(v) \
             FROM (SELECT nullif(json_extract(o.props_json, ?), '') AS v, json_type(o.props_json, ?) AS t \
             FROM s JOIN objects o ON o.id = s.id)"
        );
        let mut profiles = Vec::with_capacity(fields.len());
        for key in fields {
            let path = SqlValue::Text(format!("$.{key}"));
            let mut params = sample_params.clone();
            params.push(path.clone());
            params.push(path);
            profiles.push(
                self.conn
                    .query_row(&statement, params_from_iter(params), |r| {
                        Ok(FieldProfile {
                            key: key.clone(),
                            non_empty: count(r.get(0)?),
                            distinct: count(r.get(1)?),
                            numeric: count(r.get(2)?),
                            lists: count(r.get(3)?),
                            min: Some(json_value(r.get_ref(4)?)).filter(|value| !value.is_null()),
                            max: Some(json_value(r.get_ref(5)?)).filter(|value| !value.is_null()),
                        })
                    })?,
            );
        }
        Ok(DataProfile {
            objects: u32::try_from(objects).unwrap_or(u32::MAX),
            sampled: count(sampled),
            with_event: count(with_event),
            with_span: count(with_span),
            event_first,
            event_last,
            with_geo: count(with_geo),
            with_children: count(with_children),
            with_relations: count(with_relations),
            fields: profiles,
        })
    }

    /// The results of `query` as a graph, up to `limit` nodes (and never more than
    /// [`MAX_GRAPH_NODES`]): the live relations and parent links among them.
    pub fn graph(&self, query: &Query, limit: u32) -> Result<GraphData> {
        let limit = limit.clamp(1, MAX_GRAPH_NODES);
        let set = self.compiled(query, Select::Set, "")?;
        let mut params = set.params;
        params.push(SqlValue::Integer(i64::from(limit) + 1));
        let mut statement = self.conn.prepare(&format!(
            "SELECT o.id, o.title, o.parent_id FROM ({}) q JOIN objects o ON o.id = q.id ORDER BY o.id LIMIT ?",
            set.sql
        ))?;
        let mut rows: Vec<(Id, String, Option<Id>)> = statement
            .query_map(params_from_iter(params), |r| {
                let title: String = r.get(1)?;
                Ok((
                    sql::id(r, 0)?,
                    title.chars().take(GRAPH_TITLE).collect(),
                    sql::optional_id(r, 2)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let truncated = rows.len() > limit as usize;
        rows.truncate(limit as usize);

        let mut graph = GraphData {
            truncated,
            node_types: vec![-1; rows.len()],
            ..GraphData::default()
        };
        let index: HashMap<Id, u32> = rows
            .iter()
            .enumerate()
            .map(|(at, row)| (row.0, index_of(at)))
            .collect();
        for (at, (id, title, parent)) in rows.into_iter().enumerate() {
            graph.ids.push(id);
            graph.titles.push(title);
            if let Some(parent) = parent.and_then(|parent| index.get(&parent)) {
                graph.parent_edges.push(*parent);
                graph.parent_edges.push(index_of(at));
            }
        }

        let mut seen_types: HashMap<Id, i32> = HashMap::new();
        let mut types = self
            .conn
            .prepare("SELECT object_id, type_id FROM object_types ORDER BY object_id, type_id")?;
        let pairs = types.query_map([], |r| Ok((sql::id(r, 0)?, sql::id(r, 1)?)))?;
        for pair in pairs {
            let (object, type_id) = pair?;
            let Some(&at) = index.get(&object) else {
                continue;
            };
            let slot = &mut graph.node_types[at as usize];
            if *slot < 0 {
                let next = i32::try_from(seen_types.len()).unwrap_or(i32::MAX);
                *slot = *seen_types.entry(type_id).or_insert_with(|| {
                    graph.types.push(type_id);
                    next
                });
            }
        }

        let mut seen_rels: HashMap<String, u32> = HashMap::new();
        let mut relations = self.conn.prepare(
            "SELECT src_id, dst_id, rel_type FROM relations WHERE deleted_at IS NULL ORDER BY id",
        )?;
        let edges = relations.query_map([], |r| {
            Ok((sql::id(r, 0)?, sql::id(r, 1)?, r.get::<_, String>(2)?))
        })?;
        for edge in edges {
            let (src, dst, rel_type) = edge?;
            let (Some(&src), Some(&dst)) = (index.get(&src), index.get(&dst)) else {
                continue;
            };
            graph.edges.push(src);
            graph.edges.push(dst);
            let next = index_of(seen_rels.len());
            let at = *seen_rels.entry(rel_type.clone()).or_insert_with(|| {
                graph.rel_types.push(rel_type);
                next
            });
            graph.edge_types.push(at);
        }
        Ok(graph)
    }
}
