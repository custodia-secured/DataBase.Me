// SPDX-License-Identifier: AGPL-3.0-or-later
//! Block content (FORMAT.md §3.5, ADR-0016): the kinds of block this version knows, the shape of
//! each kind's `content_json`, and the objects a block mentions.
//!
//! A kind this version does not know is kept as it is. Its name must be a lower-case identifier, like
//! every kind's, and its content a JSON object; nothing else is checked, and it may have children. A newer app or a
//! plugin can add kinds without an older app refusing them. Keys a known kind does not use are
//! allowed too, for the same reason.

use serde_json::{Map, Value};

use crate::model::is_identifier;
use crate::{ContentHash, CoreError, Id};

/// The kinds of block this version knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    /// A paragraph of text.
    Paragraph,
    /// A heading, level 1 to 6.
    Heading,
    /// An item of a bulleted list.
    Bullet,
    /// An item of a numbered list.
    Numbered,
    /// An item of a to-do list, checked or not.
    Todo,
    /// A quotation.
    Quote,
    /// Source code or other preformatted text.
    Code,
    /// A horizontal rule.
    Divider,
    /// An image from the blob store.
    Image,
}

impl BlockKind {
    /// Every kind.
    pub const ALL: [BlockKind; 9] = [
        Self::Paragraph,
        Self::Heading,
        Self::Bullet,
        Self::Numbered,
        Self::Todo,
        Self::Quote,
        Self::Code,
        Self::Divider,
        Self::Image,
    ];

    /// The name stored in `blocks.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paragraph => "paragraph",
            Self::Heading => "heading",
            Self::Bullet => "bullet",
            Self::Numbered => "numbered",
            Self::Todo => "todo",
            Self::Quote => "quote",
            Self::Code => "code",
            Self::Divider => "divider",
            Self::Image => "image",
        }
    }

    /// The kind named `name`, if this version knows it.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }

    /// Whether blocks of this kind hold text in `inline`.
    pub fn has_inline(self) -> bool {
        !matches!(self, Self::Code | Self::Divider | Self::Image)
    }

    /// Whether this kind is a list item. List items are the only known kinds that may have child
    /// blocks, as in Markdown.
    pub fn is_list_item(self) -> bool {
        matches!(self, Self::Bullet | Self::Numbered | Self::Todo)
    }
}

/// The `rel_type` of the relations that record a block's mentions.
pub const MENTIONS: &str = "mentions";

/// The marks a text run may carry, each stored as `true` when set and left out when not.
pub const MARKS: [&str; 5] = ["bold", "italic", "underline", "strike", "code"];

/// Whether a block of kind `kind` may have child blocks: list items, and kinds this version does
/// not know.
pub fn may_have_children(kind: &str) -> bool {
    BlockKind::parse(kind).is_none_or(BlockKind::is_list_item)
}

/// Checks a block's kind name and its content.
pub fn validate_block(kind: &str, content: &Value) -> Result<(), CoreError> {
    if !is_identifier(kind, 64) {
        return Err(CoreError::invalid(
            "block kind",
            format!("{kind:?} is not a lower-case identifier of at most 64 characters"),
        ));
    }
    let what = format!("content of a {kind} block");
    let fail = |reason: String| Err(CoreError::invalid(what.clone(), reason));
    let Some(map) = content.as_object() else {
        return fail("not a JSON object".into());
    };
    let Some(known) = BlockKind::parse(kind) else {
        return Ok(());
    };
    if known.has_inline() {
        match map.get("inline") {
            Some(Value::Array(runs)) => {
                for (index, run) in runs.iter().enumerate() {
                    if let Err(reason) = check_run(run) {
                        return fail(format!("run {index}: {reason}"));
                    }
                }
            }
            Some(_) => return fail("\"inline\" is not an array".into()),
            None => return fail("\"inline\" is missing".into()),
        }
    }
    let checked = match known {
        BlockKind::Heading => required(map, "level", |v| {
            v.as_u64().is_some_and(|level| (1..=6).contains(&level))
        })
        .map_err(|_| "\"level\" must be an integer from 1 to 6"),
        BlockKind::Todo => required(map, "checked", Value::is_boolean)
            .map_err(|_| "\"checked\" must be true or false"),
        BlockKind::Code => required(map, "code", Value::is_string)
            .and_then(|()| optional(map, "language", Value::is_string))
            .map_err(|_| "\"code\" must be a string, and \"language\", if present, a string"),
        BlockKind::Image => required(map, "blob", |v| {
            v.as_str()
                .is_some_and(|hash| ContentHash::parse(hash).is_ok())
        })
        .and_then(|()| optional(map, "alt", Value::is_string))
        .map_err(|_| "\"blob\" must be a blob hash, and \"alt\", if present, a string"),
        BlockKind::Paragraph
        | BlockKind::Bullet
        | BlockKind::Numbered
        | BlockKind::Quote
        | BlockKind::Divider => Ok(()),
    };
    checked.or_else(|reason| fail(reason.into()))
}

/// The objects a block's content mentions, each once, in the order they first appear. Runs that
/// are not well formed are skipped; [`validate_block`] reports them.
pub fn mentions(content: &Value) -> Vec<Id> {
    let mut ids = Vec::new();
    let runs = content.get("inline").and_then(Value::as_array);
    for run in runs.into_iter().flatten() {
        if let Some(id) = run
            .get("mention")
            .and_then(Value::as_str)
            .and_then(|text| Id::parse(text).ok())
            && !ids.contains(&id)
        {
            ids.push(id);
        }
    }
    ids
}

/// The text of a block's content with its mentions left out: what a search index or a snippet
/// shows.
pub fn plain_text(content: &Value) -> String {
    if let Some(code) = content.get("code").and_then(Value::as_str) {
        return code.to_owned();
    }
    let runs = content.get("inline").and_then(Value::as_array);
    runs.into_iter()
        .flatten()
        .filter_map(|run| run.get("text").and_then(Value::as_str))
        .collect()
}

fn required(map: &Map<String, Value>, key: &str, ok: fn(&Value) -> bool) -> Result<(), ()> {
    map.get(key).filter(|v| ok(v)).map(|_| ()).ok_or(())
}

fn optional(map: &Map<String, Value>, key: &str, ok: fn(&Value) -> bool) -> Result<(), ()> {
    map.get(key)
        .map_or(Ok(()), |v| if ok(v) { Ok(()) } else { Err(()) })
}

/// A run is `{"text": "<not empty>", <marks>, "link"?: "<url>"}` or `{"mention": "<object ID>"}`.
fn check_run(run: &Value) -> Result<(), String> {
    let Some(map) = run.as_object() else {
        return Err("not a JSON object".into());
    };
    match (map.get("text"), map.get("mention")) {
        (Some(text), None) => {
            if !text.as_str().is_some_and(|t| !t.is_empty()) {
                return Err("\"text\" must be a string that is not empty".into());
            }
            for mark in MARKS {
                if map.get(mark).is_some_and(|v| v != &Value::Bool(true)) {
                    return Err(format!("mark {mark:?} must be true or left out"));
                }
            }
            if map
                .get("link")
                .is_some_and(|v| !v.as_str().is_some_and(|url| !url.is_empty()))
            {
                return Err("\"link\" must be a string that is not empty".into());
            }
            Ok(())
        }
        (None, Some(mention)) => match mention.as_str().map(Id::parse) {
            Some(Ok(_)) => Ok(()),
            _ => Err("\"mention\" must be an object ID".into()),
        },
        _ => Err("a run has either \"text\" or \"mention\"".into()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ID: &str = "01JB3R7Q8Y5K3M2N4P6Q8R0S2T";

    fn ok(kind: &str, content: Value) {
        if let Err(error) = validate_block(kind, &content) {
            panic!("{kind} {content} was refused: {error}");
        }
    }

    fn bad(kind: &str, content: Value) {
        assert!(
            validate_block(kind, &content).is_err(),
            "{kind} {content} was accepted"
        );
    }

    #[test]
    fn kinds_round_trip_by_name() {
        for kind in BlockKind::ALL {
            assert_eq!(BlockKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(BlockKind::parse("table"), None);
    }

    #[test]
    fn only_list_items_and_unknown_kinds_have_children() {
        let parents: Vec<&str> = BlockKind::ALL
            .into_iter()
            .map(BlockKind::as_str)
            .filter(|kind| may_have_children(kind))
            .collect();
        assert_eq!(parents, ["bullet", "numbered", "todo"]);
        assert!(may_have_children("kanban"));
    }

    #[test]
    fn well_formed_content_is_accepted() {
        let text = json!([
            {"text": "Hello "},
            {"text": "world", "bold": true, "italic": true, "link": "https://example.org"},
            {"mention": ID},
            {"text": "line\nbreak", "code": true, "strike": true, "underline": true}
        ]);
        for kind in ["paragraph", "bullet", "numbered", "quote"] {
            ok(kind, json!({"inline": text}));
            ok(kind, json!({"inline": []}));
        }
        ok("heading", json!({"level": 1, "inline": text}));
        ok("heading", json!({"level": 6, "inline": []}));
        ok("todo", json!({"checked": false, "inline": text}));
        ok("code", json!({"code": "fn main() {}", "language": "rust"}));
        ok("code", json!({"code": ""}));
        ok("divider", json!({}));
        ok(
            "image",
            json!({"blob": format!("blake3:{}", "a".repeat(64)), "alt": "A cat"}),
        );
        // Keys a kind does not use are kept, for newer versions.
        ok("paragraph", json!({"inline": [], "color": "red"}));
        ok("kanban", json!({"anything": [1, 2, 3]}));
    }

    #[test]
    fn malformed_content_is_refused() {
        bad("Paragraph", json!({"inline": []}));
        bad("", json!({}));
        bad(&"a".repeat(65), json!({}));
        bad("2col", json!({}));
        bad("paragraph", json!([]));
        bad("plugin:x", json!({}));
        bad("chart", json!("text"));
        bad("paragraph", json!({}));
        bad("paragraph", json!({"inline": "text"}));
        bad("paragraph", json!({"inline": [{"text": ""}]}));
        bad("paragraph", json!({"inline": [{"text": 1}]}));
        bad("paragraph", json!({"inline": ["text"]}));
        bad(
            "paragraph",
            json!({"inline": [{"text": "a", "bold": false}]}),
        );
        bad("paragraph", json!({"inline": [{"text": "a", "link": ""}]}));
        bad("paragraph", json!({"inline": [{"mention": "nope"}]}));
        bad(
            "paragraph",
            json!({"inline": [{"mention": ID, "text": "both"}]}),
        );
        bad("paragraph", json!({"inline": [{"bold": true}]}));
        bad("heading", json!({"inline": []}));
        bad("heading", json!({"level": 0, "inline": []}));
        bad("heading", json!({"level": 7, "inline": []}));
        bad("todo", json!({"inline": []}));
        bad("todo", json!({"checked": "yes", "inline": []}));
        bad("code", json!({}));
        bad("code", json!({"code": "x", "language": 3}));
        bad("image", json!({"blob": "not a hash"}));
        bad(
            "image",
            json!({"blob": format!("blake3:{}", "a".repeat(64)), "alt": 1}),
        );
    }

    #[test]
    fn mentions_are_listed_once_in_order() {
        let other = "01JB3R7Q8Y5K3M2N4P6Q8R0S2V";
        let content = json!({"inline": [
            {"mention": other}, {"text": "and "}, {"mention": ID}, {"mention": other},
            {"mention": "not an id"}
        ]});
        let ids: Vec<String> = mentions(&content).iter().map(Id::to_string).collect();
        assert_eq!(ids, [other, ID]);
        assert!(mentions(&json!({"code": "x"})).is_empty());
    }

    #[test]
    fn plain_text_joins_the_text_runs() {
        let content =
            json!({"inline": [{"text": "a "}, {"mention": ID}, {"text": "b", "bold": true}]});
        assert_eq!(plain_text(&content), "a b");
        assert_eq!(plain_text(&json!({"code": "let x;"})), "let x;");
        assert_eq!(plain_text(&json!({})), "");
    }
}
