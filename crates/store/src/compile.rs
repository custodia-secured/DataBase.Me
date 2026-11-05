// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiling a [`Query`] to SQL (ADR-0020). Every value from the query is bound as a parameter;
//! the only text that reaches the statement is the shape of the query. Golden tests in
//! `tests/golden/` hold the SQL and the parameters each query compiles to.

use rusqlite::types::Value as SqlValue;
use serde_json::Value;

use dbme_core::Id;
use dbme_core::query::{
    AggregateQuery, COLUMNS, Compare, Condition, Direction, Group, MeasureOp, Query, RelatedTo,
    SortBy, TimeBin,
};

use crate::search::{WEIGHTS, match_expression};
use crate::sql::to_json;
use crate::{Result, StoreError};

/// A statement and its parameters, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Compiled {
    /// The SQL, with one `?` per parameter.
    pub sql: String,
    /// The parameters.
    pub params: Vec<SqlValue>,
}

/// What a compiled query selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    /// The IDs of the results, in order, paged.
    Ids,
    /// How many results there are.
    Count,
    /// The results' types and how many results have each, most first.
    FacetTypes,
    /// The values of a field among the results and how many have each, most first. A list value
    /// counts each of its items.
    FacetField,
    /// The IDs of the results in no order and without paging, to build a larger statement on.
    Set,
}

/// A query value as a parameter: booleans as integers, as SQLite's JSON functions read them;
/// lists and objects as JSON text.
fn bind(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| n.as_f64().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

struct Builder {
    params: Vec<SqlValue>,
}

impl Builder {
    /// Binds `value` and returns its placeholder.
    fn param(&mut self, value: SqlValue) -> &'static str {
        self.params.push(value);
        "?"
    }

    /// The expression for a field name: a column of `objects`, or a value in `props_json`.
    fn field(&mut self, field: &str) -> String {
        if COLUMNS.contains(&field) {
            format!("o.{}", &field[1..])
        } else {
            let path = self.param(SqlValue::Text(format!("$.{field}")));
            format!("json_extract(o.props_json, {path})")
        }
    }

    /// The clause of one condition. A field expression binds its path, so an expression that
    /// appears twice in the clause is built twice, in the order the clause reads them.
    fn condition(&mut self, condition: &Condition) -> String {
        let field = condition.field.as_str();
        let value = &condition.value;
        match condition.op {
            Compare::Eq => {
                let expr = self.field(field);
                format!("{expr} = {}", self.param(bind(value)))
            }
            Compare::Ne => {
                let expr = self.field(field);
                format!("{expr} IS NOT {}", self.param(bind(value)))
            }
            Compare::Lt => {
                let expr = self.field(field);
                format!("{expr} < {}", self.param(bind(value)))
            }
            Compare::Le => {
                let expr = self.field(field);
                format!("{expr} <= {}", self.param(bind(value)))
            }
            Compare::Gt => {
                let expr = self.field(field);
                format!("{expr} > {}", self.param(bind(value)))
            }
            Compare::Ge => {
                let expr = self.field(field);
                format!("{expr} >= {}", self.param(bind(value)))
            }
            Compare::Contains => {
                let expr = self.field(field);
                format!(
                    "instr(lower({expr}), lower({})) > 0",
                    self.param(bind(value))
                )
            }
            Compare::StartsWith => {
                let expr = self.field(field);
                let needle = self.param(bind(value));
                let again = self.param(bind(value));
                format!("substr(lower({expr}), 1, length({needle})) = lower({again})")
            }
            Compare::IsEmpty => {
                let expr = self.field(field);
                let again = self.field(field);
                format!("({expr} IS NULL OR {again} = '')")
            }
            Compare::IsNotEmpty => {
                let expr = self.field(field);
                let again = self.field(field);
                format!("({expr} IS NOT NULL AND {again} != '')")
            }
            Compare::In => {
                let expr = self.field(field);
                format!(
                    "{expr} IN (SELECT value FROM json_each({}))",
                    self.param(bind(value))
                )
            }
            Compare::Has => {
                let list = self.field(field);
                let item = self.param(bind(value));
                format!("EXISTS (SELECT 1 FROM json_each({list}) WHERE value = {item})")
            }
        }
    }
}

/// Compiles `query` to the statement that reads `select`. `types` are the types of `query.from`,
/// each with the types that extend it, already looked up by the caller. `field` is the field a
/// [`Select::FacetField`] counts; `"$type"` is written as [`Select::FacetTypes`] instead.
pub fn compile(query: &Query, types: &[Id], select: Select, field: &str) -> Result<Compiled> {
    query.validate()?;
    let mut b = Builder { params: Vec::new() };
    let searching = query.has_search();
    // With search text, the index must be the outer loop: CROSS JOIN fixes that order, as SQLite
    // documents. Left to itself the planner can run the full-text match once per object instead,
    // which took 29 s on 100,000 objects where this takes milliseconds.
    let mut sql = if searching {
        let expression = query
            .search
            .as_deref()
            .and_then(match_expression)
            .unwrap_or_default();
        let param = b.param(SqlValue::Text(expression));
        format!(
            "SELECT o.id FROM fts CROSS JOIN search_text st ON st.doc = fts.rowid \
             CROSS JOIN objects o ON o.id = st.object_id WHERE fts MATCH {param} AND o.deleted_at IS NULL"
        )
    } else {
        String::from("SELECT o.id FROM objects o WHERE o.deleted_at IS NULL")
    };
    if !query.from.types.is_empty() {
        let list = b.param(SqlValue::Text(to_json(&types)?));
        if searching {
            // The index leaves a few candidates: one indexed lookup each. The IN form below would
            // first gather every object of the types, which for a common type is the whole vault.
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM object_types ot WHERE ot.object_id = o.id \
                 AND ot.type_id IN (SELECT value FROM json_each({list})))"
            ));
        } else {
            sql.push_str(&format!(
                " AND o.id IN (SELECT object_id FROM object_types WHERE type_id IN \
                 (SELECT value FROM json_each({list})))"
            ));
        }
    }
    if let Some(under) = query.from.under {
        let root = b.param(SqlValue::Text(under.to_string()));
        sql.push_str(&format!(
            " AND o.id IN (WITH RECURSIVE below(id) AS (SELECT id FROM objects WHERE parent_id = {root} \
             UNION ALL SELECT c.id FROM objects c JOIN below ON c.parent_id = below.id) \
             SELECT id FROM below)"
        ));
    }
    for condition in &query.conditions {
        let clause = b.condition(condition);
        sql.push_str(" AND ");
        sql.push_str(&clause);
    }
    if let Some(related) = query
        .related_to
        .as_ref()
        .filter(|related| related.depth > 1)
    {
        sql.push_str(" AND ");
        sql.push_str(&reach(&mut b, related));
    } else if let Some(related) = &query.related_to {
        let id = related.id.to_string();
        let ends = match related.direction {
            Direction::Either => {
                let a = b.param(SqlValue::Text(id.clone()));
                let c = b.param(SqlValue::Text(id));
                format!(
                    "((r.src_id = {a} AND r.dst_id = o.id) OR (r.dst_id = {c} AND r.src_id = o.id))"
                )
            }
            Direction::Outgoing => {
                let a = b.param(SqlValue::Text(id));
                format!("(r.src_id = {a} AND r.dst_id = o.id)")
            }
            Direction::Incoming => {
                let a = b.param(SqlValue::Text(id));
                format!("(r.dst_id = {a} AND r.src_id = o.id)")
            }
        };
        let rel_type = match &related.rel_type {
            Some(rel_type) => format!(
                " AND r.rel_type = {}",
                b.param(SqlValue::Text(rel_type.clone()))
            ),
            None => String::new(),
        };
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM relations r WHERE r.deleted_at IS NULL AND {ends}{rel_type})"
        ));
    }

    let sql = match select {
        Select::Ids => {
            let order = order_by(&mut b, query, searching)?;
            let limit = b.param(SqlValue::Integer(i64::from(query.limit)));
            let offset = b.param(SqlValue::Integer(i64::from(query.offset)));
            format!("{sql} ORDER BY {order} LIMIT {limit} OFFSET {offset}")
        }
        Select::Count => format!("SELECT count(*) FROM ({sql})"),
        Select::Set => sql,
        Select::FacetTypes => format!(
            "SELECT ot.type_id, count(*) FROM ({sql}) q JOIN object_types ot ON ot.object_id = q.id \
             GROUP BY ot.type_id ORDER BY 2 DESC, 1 LIMIT 50"
        ),
        Select::FacetField => {
            if !dbme_core::model::is_identifier(field, 64) {
                return Err(StoreError::Rule(format!(
                    "{field:?} is not a field key to count values of"
                )));
            }
            let path = SqlValue::Text(format!("$.{field}"));
            let kind = b.param(path.clone());
            let list = b.param(path.clone());
            let one = b.param(path);
            format!(
                "SELECT j.value, count(*) FROM ({sql}) q JOIN objects p ON p.id = q.id, \
                 json_each(CASE WHEN json_type(p.props_json, {kind}) = 'array' \
                 THEN json_extract(p.props_json, {list}) ELSE json_array(json_extract(p.props_json, {one})) END) j \
                 WHERE j.value IS NOT NULL GROUP BY j.value ORDER BY 2 DESC, 1 LIMIT 50"
            )
        }
    };
    Ok(Compiled {
        sql,
        params: b.params,
    })
}

/// The clause for a [`RelatedTo`] of more than one step: a walk along the live relations from the
/// object, as far as its depth. The object itself is not a result, even when a cycle reaches it.
fn reach(b: &mut Builder, related: &RelatedTo) -> String {
    let start = b.param(SqlValue::Text(related.id.to_string()));
    let (next, on) = match related.direction {
        Direction::Either => (
            "CASE WHEN r.src_id = reach.id THEN r.dst_id ELSE r.src_id END",
            "(r.src_id = reach.id OR r.dst_id = reach.id)",
        ),
        Direction::Outgoing => ("r.dst_id", "r.src_id = reach.id"),
        Direction::Incoming => ("r.src_id", "r.dst_id = reach.id"),
    };
    let depth = b.param(SqlValue::Integer(i64::from(related.depth)));
    let rel_type = match &related.rel_type {
        Some(rel_type) => format!(
            " AND r.rel_type = {}",
            b.param(SqlValue::Text(rel_type.clone()))
        ),
        None => String::new(),
    };
    let again = b.param(SqlValue::Text(related.id.to_string()));
    format!(
        "o.id IN (WITH RECURSIVE reach(id, depth) AS (SELECT {start}, 0 UNION \
         SELECT {next}, reach.depth + 1 FROM reach JOIN relations r ON {on} \
         WHERE r.deleted_at IS NULL AND reach.depth < {depth}{rel_type}) \
         SELECT id FROM reach WHERE id != {again})"
    )
}

/// The moment a date value stands for and the modifier that reads it, as the two arguments of
/// SQLite's date functions. Columns hold milliseconds since the epoch and date fields hold text.
fn moment(b: &mut Builder, field: &str) -> String {
    let (one, two, three, four) = (
        b.field(field),
        b.field(field),
        b.field(field),
        b.field(field),
    );
    format!(
        "CASE WHEN typeof({one}) IN ('integer', 'real') THEN {two} / 1000 ELSE {three} END, \
         CASE WHEN typeof({four}) IN ('integer', 'real') THEN 'unixepoch' ELSE '+0 seconds' END"
    )
}

/// The bucket of a date: text that sorts in time order. A value that is not a date has none.
fn time_bucket(b: &mut Builder, field: &str, bin: TimeBin) -> String {
    match bin {
        TimeBin::Year => format!("strftime('%Y', {})", moment(b, field)),
        TimeBin::Month => format!("strftime('%Y-%m', {})", moment(b, field)),
        TimeBin::Day => format!("strftime('%Y-%m-%d', {})", moment(b, field)),
        TimeBin::Hour => format!("strftime('%Y-%m-%dT%H:00', {})", moment(b, field)),
        // Sunday on or after the date, less six days: the Monday that starts its week.
        TimeBin::Week => format!("date({}, 'weekday 0', '-6 days')", moment(b, field)),
        TimeBin::Quarter => {
            let year = moment(b, field);
            let month = moment(b, field);
            format!(
                "strftime('%Y', {year}) || '-Q' || ((CAST(strftime('%m', {month}) AS INTEGER) + 2) / 3)"
            )
        }
    }
}

/// Compiles an aggregate. The inner query's parameters sit between the groups' and the joins', in
/// the order the statement reads them.
pub fn compile_aggregate(aggregate: &AggregateQuery, types: &[Id]) -> Result<Compiled> {
    aggregate.validate()?;
    let inner = compile(&aggregate.query, types, Select::Set, "")?;
    let mut columns = Builder { params: Vec::new() };
    let mut joins = Builder { params: Vec::new() };
    let mut inner_terms = Vec::new();
    let mut join_clauses = String::new();
    for (index, Group { field, bin }) in aggregate.group_by.iter().enumerate() {
        let term = match bin {
            Some(bin) => time_bucket(&mut columns, field, *bin),
            None if COLUMNS.contains(&field.as_str()) => columns.field(field),
            // A list counts once per item; a value counts once; no value is one empty bucket.
            None => {
                let path = SqlValue::Text(format!("$.{field}"));
                let kind = joins.param(path.clone());
                let list = joins.param(path.clone());
                let one = joins.param(path);
                join_clauses.push_str(&format!(
                    " LEFT JOIN json_each(CASE WHEN json_type(o.props_json, {kind}) = 'array' \
                     THEN json_extract(o.props_json, {list}) \
                     ELSE json_array(json_extract(o.props_json, {one})) END) j{index} ON 1"
                ));
                format!("j{index}.value")
            }
        };
        inner_terms.push(format!("{term} AS k{index}"));
    }
    let mut outer_terms: Vec<String> = (0..aggregate.group_by.len())
        .map(|index| format!("k{index}"))
        .collect();
    let keys = outer_terms.join(", ");
    for (index, measure) in aggregate.measures.iter().enumerate() {
        let function = match measure.op {
            MeasureOp::Count => {
                outer_terms.push("count(*)".into());
                continue;
            }
            MeasureOp::Sum => "sum",
            MeasureOp::Avg => "avg",
            MeasureOp::Min => "min",
            MeasureOp::Max => "max",
        };
        let field = measure.field.as_deref().unwrap_or_default();
        // Numbers only: SQLite would otherwise read the text "n/a" as 0.
        let (kind, value) = (columns.field(field), columns.field(field));
        inner_terms.push(format!(
            "CASE WHEN typeof({kind}) IN ('integer', 'real') THEN {value} END AS v{index}"
        ));
        outer_terms.push(format!("{function}(v{index})"));
    }
    // A subquery with no named column of its own still needs one to select.
    inner_terms.push("1 AS one".into());
    let grouping = if keys.is_empty() {
        String::new()
    } else {
        format!(" GROUP BY {keys} ORDER BY {keys}")
    };
    let sql = format!(
        "SELECT {} FROM (SELECT {} FROM ({}) q JOIN objects o ON o.id = q.id{join_clauses}){grouping} LIMIT ?",
        outer_terms.join(", "),
        inner_terms.join(", "),
        inner.sql,
    );
    let mut params = columns.params;
    params.extend(inner.params);
    params.extend(joins.params);
    // One more than asked, so the caller can say the list was cut.
    params.push(SqlValue::Integer(i64::from(aggregate.bucket_limit()) + 1));
    Ok(Compiled { sql, params })
}

/// The `ORDER BY` terms. Ties are broken by ID, in the same direction.
fn order_by(b: &mut Builder, query: &Query, searching: bool) -> Result<String> {
    let sort = &query.sort;
    let mut direction = if sort.descending { " DESC" } else { "" };
    let mut lead = String::new();
    let terms: Vec<String> = match sort.by {
        SortBy::Manual => vec!["o.sort_key".into()],
        SortBy::Title => vec!["o.title COLLATE NOCASE".into()],
        SortBy::Created => vec!["o.created_at".into()],
        SortBy::Updated => vec!["o.updated_at".into()],
        SortBy::Relevance if searching => vec![WEIGHTS.into()],
        SortBy::Relevance => {
            // Best first means most recent first.
            direction = if sort.descending { "" } else { " DESC" };
            vec!["o.updated_at".into()]
        }
        SortBy::Field => {
            let key = sort.field.clone().unwrap_or_default();
            let value = b.field(&key);
            lead = format!("({value} IS NULL), ");
            vec![b.field(&key)]
        }
    };
    let mut clause = lead;
    for term in terms.into_iter().chain(["o.id".to_owned()]) {
        clause.push_str(&term);
        clause.push_str(direction);
        clause.push_str(", ");
    }
    clause.truncate(clause.len() - 2);
    Ok(clause)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_bind_as_sqlite_reads_them() {
        assert_eq!(bind(&json!(true)), SqlValue::Integer(1));
        assert_eq!(bind(&json!(3)), SqlValue::Integer(3));
        assert_eq!(bind(&json!(2.5)), SqlValue::Real(2.5));
        assert_eq!(bind(&json!("x")), SqlValue::Text("x".into()));
        assert_eq!(bind(&Value::Null), SqlValue::Null);
        assert_eq!(bind(&json!(["a", 1])), SqlValue::Text("[\"a\",1]".into()));
    }

    #[test]
    fn a_facet_needs_a_field_key() {
        let query = Query::default();
        assert!(compile(&query, &[], Select::FacetField, "$title").is_err());
        assert!(compile(&query, &[], Select::FacetField, "status").is_ok());
    }
}
