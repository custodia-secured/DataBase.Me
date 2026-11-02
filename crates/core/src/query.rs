// SPDX-License-Identifier: AGPL-3.0-or-later
//! The query DSL (ADR-0020): one JSON shape for every question asked of a vault's objects, shared
//! by the views, saved views, the command palette and, later, plugins, MCP and AI.
//!
//! A [`Query`] says where to look (`from`), what must hold (`where`), what text to search for,
//! what the results must be related to, and how to order and page them. The store compiles it to
//! SQL; this module only defines the shape and checks it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::is_identifier;
use crate::{CoreError, Id};

/// The most results one query returns.
pub const MAX_LIMIT: u32 = 1000;
/// The most conditions one query holds.
pub const MAX_CONDITIONS: usize = 32;
/// The longest search text.
pub const MAX_SEARCH: usize = 1000;

/// The built-in columns a condition or a sort may name, each written with a leading `$` so that
/// it can never be confused with a field key.
pub const COLUMNS: [&str; 8] = [
    "$title",
    "$summary",
    "$created_at",
    "$updated_at",
    "$captured_at",
    "$event_start",
    "$event_end",
    "$sensitivity",
];

/// What a list is ordered by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SortBy {
    /// The order people gave the children (sort keys), or creation order otherwise.
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
    /// How well the object matches the search text; without search text, most recently changed
    /// first.
    Relevance,
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

impl Sort {
    /// Checks the sort.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.by == SortBy::Field
            && !self
                .field
                .as_deref()
                .is_some_and(|key| is_identifier(key, 64))
        {
            return Err(CoreError::invalid(
                "sort",
                "sorting by a field needs its key",
            ));
        }
        Ok(())
    }
}

/// Where a query looks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct From {
    /// Objects of any of these types, or of types that extend them. Empty: every type and none.
    pub types: Vec<Id>,
    /// Objects anywhere below this one in the hierarchy, not counting itself.
    pub under: Option<Id>,
}

/// How a condition compares a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Compare {
    /// Equal to the value.
    Eq,
    /// Not equal to the value, or absent.
    Ne,
    /// Less than the value. Absent values never match.
    Lt,
    /// At most the value.
    Le,
    /// Greater than the value.
    Gt,
    /// At least the value.
    Ge,
    /// Text that contains the value, ignoring case.
    Contains,
    /// Text that starts with the value, ignoring case.
    StartsWith,
    /// Absent, or empty text. Takes no value.
    IsEmpty,
    /// Present and not empty text. Takes no value.
    IsNotEmpty,
    /// Equal to one of the values in a list.
    In,
    /// A list (such as a multi-select field) that holds the value.
    Has,
}

impl Compare {
    /// Whether this comparison takes a value at all.
    pub fn takes_value(self) -> bool {
        !matches!(self, Self::IsEmpty | Self::IsNotEmpty)
    }
}

/// One thing that must hold for every result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Condition {
    /// A built-in column ([`COLUMNS`]) or a field key.
    pub field: String,
    /// How to compare.
    pub op: Compare,
    /// What to compare with; `null` for comparisons that take no value.
    #[serde(default)]
    pub value: Value,
}

/// Which end of a relation the results are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Direction {
    /// Results are related to the object in either direction.
    #[default]
    Either,
    /// Results are what the object's relations point to.
    Outgoing,
    /// Results are what points to the object.
    Incoming,
}

/// Results must have a live relation with this object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RelatedTo {
    /// The object.
    pub id: Id,
    /// Only relations of this type; `None` for any.
    #[serde(default)]
    pub rel_type: Option<String>,
    /// Which end the results are.
    #[serde(default)]
    pub direction: Direction,
}

/// A query over the live objects of a vault.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Query {
    /// Where to look.
    pub from: From,
    /// What must hold, all of it.
    #[serde(rename = "where")]
    pub conditions: Vec<Condition>,
    /// Text the object must match in its title, body or field values.
    pub search: Option<String>,
    /// An object the results must be related to.
    pub related_to: Option<RelatedTo>,
    /// The order.
    pub sort: Sort,
    /// How many results at most, up to [`MAX_LIMIT`].
    pub limit: u32,
    /// How many results to skip.
    pub offset: u32,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            from: From::default(),
            conditions: Vec::new(),
            search: None,
            related_to: None,
            sort: Sort {
                by: SortBy::Relevance,
                field: None,
                descending: false,
            },
            limit: 50,
            offset: 0,
        }
    }
}

fn invalid(reason: impl Into<String>) -> CoreError {
    CoreError::invalid("query", reason)
}

/// Whether `field` names a built-in column or a field key.
pub fn is_field_name(field: &str) -> bool {
    COLUMNS.contains(&field) || is_identifier(field, 64)
}

impl Condition {
    /// Checks the condition: the field name, and that the value fits the comparison.
    pub fn validate(&self) -> Result<(), CoreError> {
        if !is_field_name(&self.field) {
            return Err(invalid(format!(
                "{:?} is not a column or a field key",
                self.field
            )));
        }
        let value = &self.value;
        let fits = match self.op {
            Compare::IsEmpty | Compare::IsNotEmpty => value.is_null(),
            Compare::Eq | Compare::Ne | Compare::Has => {
                value.is_string() || value.is_number() || value.is_boolean()
            }
            Compare::Lt | Compare::Le | Compare::Gt | Compare::Ge => {
                value.is_string() || value.is_number()
            }
            Compare::Contains | Compare::StartsWith => {
                value.as_str().is_some_and(|s| !s.is_empty())
            }
            Compare::In => value.as_array().is_some_and(|list| {
                !list.is_empty()
                    && list
                        .iter()
                        .all(|v| v.is_string() || v.is_number() || v.is_boolean())
            }),
        };
        if fits {
            Ok(())
        } else {
            Err(invalid(format!(
                "{} on {:?} does not take the value {value}",
                serde_json::to_string(&self.op).unwrap_or_default(),
                self.field
            )))
        }
    }
}

impl Query {
    /// Checks the query against the limits and the field names.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(invalid(format!("limit must be 1 to {MAX_LIMIT}")));
        }
        if self.conditions.len() > MAX_CONDITIONS {
            return Err(invalid(format!("at most {MAX_CONDITIONS} conditions")));
        }
        for condition in &self.conditions {
            condition.validate()?;
        }
        if let Some(search) = &self.search
            && search.chars().count() > MAX_SEARCH
        {
            return Err(invalid(format!(
                "search text longer than {MAX_SEARCH} characters"
            )));
        }
        if let Some(related) = &self.related_to
            && let Some(rel_type) = &related.rel_type
            && !is_identifier(rel_type, 64)
        {
            return Err(invalid(format!("{rel_type:?} is not a relation type")));
        }
        self.sort.validate()?;
        if let Some(key) = &self.sort.field
            && self.sort.by == SortBy::Field
            && COLUMNS.contains(&key.as_str())
        {
            return Err(invalid(
                "sort by a column with its own `by`, not as a field",
            ));
        }
        Ok(())
    }

    /// Whether the query has search text that is not blank.
    pub fn has_search(&self) -> bool {
        self.search.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn condition(field: &str, op: Compare, value: Value) -> Condition {
        Condition {
            field: field.into(),
            op,
            value,
        }
    }

    #[test]
    fn the_default_query_is_everything_by_relevance() {
        let query = Query::default();
        assert_eq!(query.sort.by, SortBy::Relevance);
        assert_eq!(query.limit, 50);
        query.validate().unwrap();
        assert!(!query.has_search());
        let parsed: Query = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, query);
    }

    #[test]
    fn a_query_round_trips_through_json() {
        let query = Query {
            from: From {
                types: vec![Id::from_parts(1, 1)],
                under: Some(Id::from_parts(1, 2)),
            },
            conditions: vec![
                condition("status", Compare::Eq, json!("doing")),
                condition("$created_at", Compare::Ge, json!(1_700_000_000_000_i64)),
                condition("notes", Compare::IsEmpty, Value::Null),
            ],
            search: Some("budget".into()),
            related_to: Some(RelatedTo {
                id: Id::from_parts(1, 3),
                rel_type: Some("mentions".into()),
                direction: Direction::Incoming,
            }),
            sort: Sort {
                by: SortBy::Field,
                field: Some("due".into()),
                descending: true,
            },
            limit: 20,
            offset: 40,
        };
        query.validate().unwrap();
        let json = serde_json::to_value(&query).unwrap();
        assert_eq!(json["where"][0]["op"], "eq");
        assert_eq!(json["related_to"]["direction"], "incoming");
        assert_eq!(serde_json::from_value::<Query>(json).unwrap(), query);
        assert!(serde_json::from_str::<Query>(r#"{"limti": 3}"#).is_err());
    }

    #[test]
    fn conditions_check_their_fields_and_values() {
        let ok = [
            condition("$title", Compare::Contains, json!("a")),
            condition("done", Compare::Eq, json!(true)),
            condition("tags", Compare::Has, json!("urgent")),
            condition("status", Compare::In, json!(["todo", "doing"])),
            condition("$event_start", Compare::Lt, json!("2026-12-01")),
            condition("due", Compare::IsNotEmpty, Value::Null),
        ];
        for condition in ok {
            condition.validate().unwrap();
        }
        let bad = [
            condition("Title", Compare::Eq, json!("a")),
            condition("$nope", Compare::Eq, json!("a")),
            condition("a b", Compare::Eq, json!("a")),
            condition("done", Compare::IsEmpty, json!(true)),
            condition("done", Compare::Eq, Value::Null),
            condition("done", Compare::Eq, json!({"a": 1})),
            condition("status", Compare::In, json!([])),
            condition("status", Compare::In, json!("todo")),
            condition("status", Compare::In, json!([["nested"]])),
            condition("$title", Compare::Contains, json!("")),
            condition("$title", Compare::StartsWith, json!(1)),
            condition("n", Compare::Gt, json!(true)),
        ];
        for condition in bad {
            assert!(condition.validate().is_err(), "{condition:?}");
        }
    }

    #[test]
    fn queries_keep_to_their_limits() {
        let mut query = Query {
            limit: 0,
            ..Default::default()
        };
        assert!(query.validate().is_err());
        query.limit = MAX_LIMIT + 1;
        assert!(query.validate().is_err());
        query.limit = MAX_LIMIT;
        query.validate().unwrap();

        query.conditions = (0..=MAX_CONDITIONS)
            .map(|_| condition("a", Compare::IsEmpty, Value::Null))
            .collect();
        assert!(query.validate().is_err());
        query.conditions.clear();

        query.search = Some("x".repeat(MAX_SEARCH + 1));
        assert!(query.validate().is_err());
        query.search = Some("   ".into());
        query.validate().unwrap();
        assert!(!query.has_search());
        query.search = Some(" x ".into());
        assert!(query.has_search());

        query.related_to = Some(RelatedTo {
            id: Id::from_parts(1, 1),
            rel_type: Some("Not Valid".into()),
            direction: Direction::Either,
        });
        assert!(query.validate().is_err());
        query.related_to = None;

        query.sort = Sort {
            by: SortBy::Field,
            field: None,
            descending: false,
        };
        assert!(query.validate().is_err());
        query.sort.field = Some("$title".into());
        assert!(query.validate().is_err());
        query.sort.field = Some("due".into());
        query.validate().unwrap();
    }

    #[test]
    fn comparisons_say_whether_they_take_a_value() {
        assert!(Compare::Eq.takes_value());
        assert!(!Compare::IsEmpty.takes_value());
        assert!(!Compare::IsNotEmpty.takes_value());
    }
}
