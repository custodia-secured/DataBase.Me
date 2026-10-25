// SPDX-License-Identifier: AGPL-3.0-or-later
//! Conversions between SQLite values and the domain types.

use rusqlite::types::Type;
use serde::Serialize;
use serde::de::DeserializeOwned;

use dbme_core::{ContentHash, CoreError, Id, SortKey};

use crate::Result;

fn failed(index: usize, error: CoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
}

fn text<'r>(row: &'r rusqlite::Row<'_>, index: usize) -> rusqlite::Result<&'r str> {
    Ok(row.get_ref(index)?.as_str()?)
}

fn optional_text<'r>(
    row: &'r rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<&'r str>> {
    Ok(row.get_ref(index)?.as_str_or_null()?)
}

pub(crate) fn id(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Id> {
    Id::parse(text(row, index)?).map_err(|e| failed(index, e))
}

pub(crate) fn optional_id(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<Id>> {
    optional_text(row, index)?
        .map(Id::parse)
        .transpose()
        .map_err(|e| failed(index, e))
}

pub(crate) fn sort_key(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<SortKey> {
    SortKey::parse(text(row, index)?).map_err(|e| failed(index, e))
}

pub(crate) fn hash(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<ContentHash> {
    ContentHash::parse(text(row, index)?).map_err(|e| failed(index, e))
}

pub(crate) fn optional_hash(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<ContentHash>> {
    optional_text(row, index)?
        .map(ContentHash::parse)
        .transpose()
        .map_err(|e| failed(index, e))
}

/// A JSON column.
pub(crate) fn json<T: DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    serde_json::from_str(text(row, index)?).map_err(|e| failed(index, e.into()))
}

/// A text column holding one of a fixed set of names.
pub(crate) fn name<T>(
    row: &rusqlite::Row<'_>,
    index: usize,
    parse: fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let text = text(row, index)?;
    parse(text).ok_or_else(|| {
        failed(
            index,
            CoreError::invalid("column", format!("unknown value {text:?}")),
        )
    })
}

/// A value as JSON text, for a JSON column.
pub(crate) fn to_json(value: &impl Serialize) -> Result<String> {
    Ok(serde_json::to_string(value).map_err(CoreError::from)?)
}

pub(crate) fn optional(id: Option<Id>) -> Option<String> {
    id.map(|id| id.to_string())
}
