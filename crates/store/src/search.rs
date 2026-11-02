// SPDX-License-Identifier: AGPL-3.0-or-later
//! Full-text search (ADR-0020).
//!
//! `search_text` holds one row per live object: its title, the plain text of its blocks and the
//! text of its field values. `fts` is an FTS5 index over that table, kept in step by triggers.
//! Every transaction notes the objects it touched in `search_dirty` ([`mark_dirty`]), which costs
//! it almost nothing, and the next search brings those rows up to date first ([`flush`]). So undo,
//! redo and the Trash are covered without the op log knowing anything about search, a bulk change
//! of 10,000 objects does not pay for 10,000 index writes, and a search never sees stale text.
//! All three tables are derived and can be rebuilt ([`rebuild`]).

use std::collections::BTreeSet;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use dbme_core::Id;
use dbme_core::block::plain_text;
use dbme_core::model::{ObjectRow, Props};
use dbme_core::op::{Entity, Row, RowKey};

use crate::list::{MAX_PAGE, ObjectSummary};
use crate::query::Reader;
use crate::{Result, rows, sql};

/// How much a match in each column counts for: the title, the body, the field values.
pub(crate) const WEIGHTS: &str = "bm25(fts, 10.0, 1.0, 2.0)";

/// One result of a search.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SearchHit {
    /// The object.
    pub summary: ObjectSummary,
    /// A piece of the text around the match, with the matching words in `[` and `]`. Empty when
    /// the search had no words.
    pub snippet: String,
    /// How well it matched: lower is better, as FTS5's BM25 reports it. Zero without words.
    pub rank: f64,
}

/// The text of `props` that a search can find: strings and numbers, and the strings in lists.
pub(crate) fn props_text(props: &Props) -> String {
    let mut text = String::new();
    let mut push = |value: &Value| {
        let piece = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return,
        };
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&piece);
    };
    for value in props.values() {
        match value {
            Value::Array(items) => items.iter().for_each(&mut push),
            other => push(other),
        }
    }
    text
}

/// What one block contributes to the body: its text with mentions left out, the code of a code
/// block, or the description of an image.
fn block_text(kind: &str, content: &Value) -> String {
    match kind {
        "code" => content
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        "image" => content
            .get("alt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        _ => plain_text(content),
    }
}

/// The three texts the index holds for one object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SearchText {
    pub title: String,
    /// The summary, then the blocks in tree order, one line each.
    pub body: String,
    pub props_text: String,
}

/// What the index should hold for a live object, read from the object and its blocks.
pub(crate) fn text_of(conn: &Connection, object: &ObjectRow) -> Result<SearchText> {
    let mut lines: Vec<String> = object.summary.iter().cloned().collect();
    for block in (Reader { conn }).blocks(object.id)? {
        let text = block_text(&block.kind, &block.content_json);
        if !text.is_empty() {
            lines.push(text);
        }
    }
    Ok(SearchText {
        title: object.title.clone(),
        body: lines.join("\n"),
        props_text: props_text(&object.props_json),
    })
}

/// Writes the row of a live object. A row whose text has not changed is left alone, so that the
/// index does no work for a move or a reorder.
fn upsert(conn: &Connection, object: &ObjectRow) -> Result<()> {
    let text = text_of(conn, object)?;
    conn.prepare_cached(
        "INSERT INTO search_text (object_id, title, body, props_text) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (object_id) DO UPDATE SET title = excluded.title, body = excluded.body, \
         props_text = excluded.props_text \
         WHERE (title, body, props_text) IS NOT (excluded.title, excluded.body, excluded.props_text)",
    )?
    .execute(params![
        object.id.to_string(),
        text.title,
        text.body,
        text.props_text
    ])?;
    Ok(())
}

/// Notes that the text of `ids` may have changed, for the next [`flush`].
pub(crate) fn mark_dirty(conn: &Connection, ids: &BTreeSet<Id>) -> Result<()> {
    let mut statement =
        conn.prepare_cached("INSERT OR IGNORE INTO search_dirty (object_id) VALUES (?1)")?;
    for id in ids {
        statement.execute([id.to_string()])?;
    }
    Ok(())
}

/// Brings the row of one object up to date: a live object gets its current text, and anything
/// else (a trashed or deleted object) loses its row.
fn reindex(conn: &Connection, id: Id) -> Result<()> {
    match rows::load(conn, &RowKey::Id(Entity::Object, id))? {
        Some(Row::Object(object)) if object.deleted_at.is_none() => upsert(conn, &object),
        _ => {
            conn.prepare_cached("DELETE FROM search_text WHERE object_id = ?1")?
                .execute([id.to_string()])?;
            Ok(())
        }
    }
}

/// Brings the index up to date with every object noted since the last flush. Reads call this
/// first, so a search never sees stale text. The work is done in one transaction, unless the
/// caller is already in one.
pub(crate) fn flush(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare_cached("SELECT object_id FROM search_dirty")?;
    let dirty: Vec<Id> = statement
        .query_map([], |r| sql::id(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
    drop(statement);
    if dirty.is_empty() {
        return Ok(());
    }
    let own = if conn.is_autocommit() {
        Some(conn.unchecked_transaction()?)
    } else {
        None
    };
    for id in &dirty {
        reindex(conn, *id)?;
    }
    conn.execute_batch("DELETE FROM search_dirty")?;
    if let Some(tx) = own {
        tx.commit()?;
    }
    Ok(())
}

/// Rebuilds the index from the objects and blocks: for a vault upgraded to migration 2, or one
/// whose index is suspect.
pub(crate) fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute_batch("DELETE FROM search_dirty; DELETE FROM search_text")?;
    let mut statement = conn.prepare("SELECT id FROM objects WHERE deleted_at IS NULL")?;
    let ids: Vec<Id> = statement
        .query_map([], |r| sql::id(r, 0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        if let Some(Row::Object(object)) = rows::load(conn, &RowKey::Id(Entity::Object, id))? {
            upsert(conn, &object)?;
        }
    }
    conn.execute_batch("INSERT INTO fts (fts) VALUES ('optimize')")?;
    Ok(())
}

/// Turns what a person typed into an FTS5 query: every word quoted, so none of FTS5's own syntax
/// gets through, joined as "all of these"; a word still being typed (the last, with no space after
/// it) matches as a prefix; text in double quotes stays a phrase. `None` when there is nothing to
/// search for.
pub fn match_expression(text: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut phrase: Option<String> = None;
    let flush = |word: &mut String, terms: &mut Vec<String>| {
        if !word.is_empty() {
            terms.push(format!("\"{word}\""));
            word.clear();
        }
    };
    for c in text.chars() {
        match (&mut phrase, c) {
            (Some(inside), '"') => {
                let inside = inside.replace('"', "\"\"");
                if inside.chars().any(char::is_alphanumeric) {
                    terms.push(format!("\"{inside}\""));
                }
                phrase = None;
            }
            (Some(inside), c) => inside.push(c),
            (None, '"') => {
                flush(&mut word, &mut terms);
                phrase = Some(String::new());
            }
            (None, c) if c.is_alphanumeric() => word.push(c),
            (None, _) => flush(&mut word, &mut terms),
        }
    }
    // An unfinished phrase counts as words.
    if let Some(inside) = phrase.take() {
        for piece in inside.split(|c: char| !c.is_alphanumeric()) {
            if !piece.is_empty() {
                terms.push(format!("\"{piece}\""));
            }
        }
    } else if !word.is_empty() {
        terms.push(format!("\"{word}\"*"));
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

impl Reader<'_> {
    /// Up to `limit` (at most [`MAX_PAGE`]) live objects matching `text`, best match first. With no
    /// words to search for, the most recently changed objects.
    pub fn search(&self, text: &str, limit: u32) -> Result<Vec<SearchHit>> {
        flush(self.conn)?;
        let limit = limit.min(MAX_PAGE);
        let Some(expression) = match_expression(text) else {
            return Ok(self
                .search_titles("", limit)?
                .into_iter()
                .map(|summary| SearchHit {
                    summary,
                    snippet: String::new(),
                    rank: 0.0,
                })
                .collect());
        };
        let mut statement = self.conn.prepare_cached(
            "SELECT st.object_id, bm25(fts, 10.0, 1.0, 2.0), snippet(fts, -1, '[', ']', '…', 12) \
             FROM fts JOIN search_text st ON st.doc = fts.rowid WHERE fts MATCH ?1 \
             ORDER BY 2, st.object_id LIMIT ?2",
        )?;
        let found: Vec<(Id, f64, String)> = statement
            .query_map(params![expression, limit], |r| {
                Ok((sql::id(r, 0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let ids: Vec<Id> = found.iter().map(|(id, _, _)| *id).collect();
        let summaries = self.summaries(&ids)?;
        Ok(summaries
            .into_iter()
            .zip(found)
            .map(|(summary, (_, rank, snippet))| SearchHit {
                summary,
                snippet,
                rank,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn typed_text_becomes_quoted_terms_with_a_prefix_on_the_last() {
        assert_eq!(match_expression("proj"), Some("\"proj\"*".into()));
        assert_eq!(match_expression("proj "), Some("\"proj\"".into()));
        assert_eq!(
            match_expression("mind map"),
            Some("\"mind\" \"map\"*".into())
        );
        assert_eq!(
            match_expression("  Alpha,  beta  "),
            Some("\"Alpha\" \"beta\"".into())
        );
        assert_eq!(match_expression("e-mail"), Some("\"e\" \"mail\"*".into()));
    }

    #[test]
    fn fts_syntax_never_gets_through() {
        assert_eq!(
            match_expression("a AND (b OR c) NOT d*"),
            Some("\"a\" \"AND\" \"b\" \"OR\" \"c\" \"NOT\" \"d\"".into())
        );
        assert_eq!(match_expression("col: ^x"), Some("\"col\" \"x\"*".into()));
        assert_eq!(match_expression("***"), None);
        assert_eq!(match_expression(""), None);
        assert_eq!(match_expression("   \"  \" "), None);
    }

    #[test]
    fn quotes_make_phrases() {
        assert_eq!(
            match_expression("\"mind map\" tool"),
            Some("\"mind map\" \"tool\"*".into())
        );
        // A quote always ends a phrase: there is no escaping, so none can go wrong.
        assert_eq!(
            match_expression("he said \"mind map\" and \"node\""),
            Some("\"he\" \"said\" \"mind map\" \"and\" \"node\"".into())
        );
        assert_eq!(match_expression("don\"t"), Some("\"don\" \"t\"".into()));
        assert_eq!(
            match_expression("\"unfinished phra"),
            Some("\"unfinished\" \"phra\"".into())
        );
    }

    #[test]
    fn field_values_give_their_text() {
        let props: Props = [
            ("status".to_owned(), json!("doing")),
            ("estimate".to_owned(), json!(3.5)),
            ("done".to_owned(), json!(true)),
            ("tags".to_owned(), json!(["urgent", "home", 7])),
            ("geo".to_owned(), json!({"lat": 1, "lon": 2})),
        ]
        .into_iter()
        .collect();
        // In key order, as the map keeps them: done (skipped), estimate, geo (skipped), status, tags.
        assert_eq!(props_text(&props), "3.5\ndoing\nurgent\nhome\n7");
    }

    #[test]
    fn blocks_give_their_text() {
        assert_eq!(
            block_text("code", &json!({"code": "let x = 1;", "language": "rs"})),
            "let x = 1;"
        );
        assert_eq!(
            block_text("image", &json!({"blob": "h", "alt": "A cat"})),
            "A cat"
        );
        assert_eq!(block_text("image", &json!({"blob": "h"})), "");
        assert_eq!(
            block_text(
                "paragraph",
                &json!({"inline": [{"text": "Hello "}, {"text": "there"}]})
            ),
            "Hello there"
        );
        assert_eq!(block_text("divider", &json!({})), "");
    }
}
