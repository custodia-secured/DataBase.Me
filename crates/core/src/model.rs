// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rows of the vault's materialised tables (FORMAT.md §3), and the rules each must obey.
//!
//! Field names match the SQL column names exactly. Columns whose names end in `_json` hold the
//! parsed JSON value here; the store writes them as JSON text.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ContentHash, CoreError, Id, SortKey};

/// Field values of an object, keyed by field key, or extra properties of a relation.
pub type Props = BTreeMap<String, Value>;

/// The kinds a field can have (FORMAT.md §3.3).
#[allow(missing_docs, reason = "each kind is described in FORMAT.md §3.3")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FieldKind {
    Text,
    RichText,
    Number,
    Currency,
    Boolean,
    Date,
    Datetime,
    Duration,
    Select,
    MultiSelect,
    Relation,
    Url,
    Email,
    Phone,
    Geo,
    File,
    Rating,
    Formula,
    Rollup,
    Json,
}

impl FieldKind {
    /// Every kind, in the order FORMAT.md lists them.
    pub const ALL: [FieldKind; 20] = [
        Self::Text,
        Self::RichText,
        Self::Number,
        Self::Currency,
        Self::Boolean,
        Self::Date,
        Self::Datetime,
        Self::Duration,
        Self::Select,
        Self::MultiSelect,
        Self::Relation,
        Self::Url,
        Self::Email,
        Self::Phone,
        Self::Geo,
        Self::File,
        Self::Rating,
        Self::Formula,
        Self::Rollup,
        Self::Json,
    ];

    /// The name stored in `fields.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::RichText => "rich_text",
            Self::Number => "number",
            Self::Currency => "currency",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::Datetime => "datetime",
            Self::Duration => "duration",
            Self::Select => "select",
            Self::MultiSelect => "multi_select",
            Self::Relation => "relation",
            Self::Url => "url",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Geo => "geo",
            Self::File => "file",
            Self::Rating => "rating",
            Self::Formula => "formula",
            Self::Rollup => "rollup",
            Self::Json => "json",
        }
    }

    /// Reads a name stored in `fields.kind`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// Whether values of this kind live in `objects.props_json`. Relation values live in
    /// `relations`; formulas and rollups are computed.
    pub fn is_stored(self) -> bool {
        !matches!(self, Self::Relation | Self::Formula | Self::Rollup)
    }
}

/// How carefully an object must be handled, for example by AI features (M11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Sensitivity {
    /// No special handling.
    #[default]
    Normal,
    /// Redacted before anything leaves the device, and never sent to an AI provider.
    Sensitive,
}

impl Sensitivity {
    /// The name stored in `objects.sensitivity`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Sensitive => "sensitive",
        }
    }

    /// Reads a name stored in `objects.sensitivity`.
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Normal, Self::Sensitive]
            .into_iter()
            .find(|s| s.as_str() == text)
    }
}

/// What a blob is to the object it is attached to.
#[allow(missing_docs, reason = "the roles are described in FORMAT.md §3.1")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AttachmentRole {
    Original,
    Thumbnail,
    Transcript,
    OcrText,
    Snapshot,
}

impl AttachmentRole {
    /// Every role.
    pub const ALL: [AttachmentRole; 5] = [
        Self::Original,
        Self::Thumbnail,
        Self::Transcript,
        Self::OcrText,
        Self::Snapshot,
    ];

    /// Reads a name stored in `attachments.role`.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == text)
    }

    /// The name stored in `attachments.role`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Thumbnail => "thumbnail",
            Self::Transcript => "transcript",
            Self::OcrText => "ocr_text",
            Self::Snapshot => "snapshot",
        }
    }
}

/// An object type, such as Task or Person. Types can extend one parent type.
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TypeRow {
    pub id: Id,
    pub name: String,
    pub plural: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub extends: Option<Id>,
    pub description: Option<String>,
    pub is_system: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A field definition belonging to a type.
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FieldRow {
    pub id: Id,
    pub type_id: Id,
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub options_json: Value,
    pub required: bool,
    pub sort_order: i64,
}

/// An object: the unit of everything in a vault.
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ObjectRow {
    pub id: Id,
    pub title: String,
    pub summary: Option<String>,
    pub parent_id: Option<Id>,
    pub sort_key: SortKey,
    pub props_json: Props,
    pub created_at: i64,
    pub updated_at: i64,
    pub captured_at: Option<i64>,
    pub event_start: Option<i64>,
    pub event_end: Option<i64>,
    pub geo_lat: Option<f64>,
    pub geo_lon: Option<f64>,
    pub deleted_at: Option<i64>,
    pub sensitivity: Sensitivity,
}

/// One of an object's types (an object can have several).
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ObjectTypeRow {
    pub object_id: Id,
    pub type_id: Id,
}

/// A node of the content tree inside an object.
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlockRow {
    pub id: Id,
    pub object_id: Id,
    pub parent_block_id: Option<Id>,
    pub sort_key: SortKey,
    pub kind: String,
    pub content_json: Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A typed, directed link between two objects.
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RelationRow {
    pub id: Id,
    pub src_id: Id,
    pub dst_id: Id,
    pub rel_type: String,
    pub field_id: Option<Id>,
    pub src_block_id: Option<Id>,
    pub label: Option<String>,
    pub props_json: Props,
    pub created_at: i64,
    pub deleted_at: Option<i64>,
}

/// A blob attached to an object, with the media type it has for this object.
///
/// `blob_size` is the blob's size from the `blobs` table, not a column of `attachments`. It travels
/// with the op so that the first attachment of a blob can create the blob's row; a blob's size is
/// fixed by its content, so it is the same for every attachment.
#[allow(
    missing_docs,
    reason = "columns are documented in FORMAT.md §3.1 and §5"
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AttachmentRow {
    pub object_id: Id,
    pub blob_hash: ContentHash,
    pub role: AttachmentRole,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub blob_size: i64,
}

/// A file in the blob store.
#[allow(missing_docs, reason = "columns are documented in FORMAT.md §3.1")]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlobRow {
    pub hash: ContentHash,
    pub size: i64,
    pub mime: Option<String>,
    pub encrypted: bool,
    pub created_at: i64,
}

/// `true` for a lower-case identifier: a letter, then letters, digits or `_`, at most `max` long.
pub fn is_identifier(text: &str, max: usize) -> bool {
    let mut bytes = text.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && text.len() <= max
        && bytes.all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn check(ok: bool, what: &str, reason: &str) -> Result<(), CoreError> {
    if ok {
        Ok(())
    } else {
        Err(CoreError::invalid(what, reason))
    }
}

fn check_text(text: &str, what: &str, max_chars: usize) -> Result<(), CoreError> {
    check(
        text.chars().count() <= max_chars,
        what,
        &format!("longer than {max_chars} characters"),
    )
}

fn check_optional(text: Option<&str>, what: &str, max_chars: usize) -> Result<(), CoreError> {
    text.map_or(Ok(()), |t| check_text(t, what, max_chars))
}

fn check_props(props: &Props, what: &str) -> Result<(), CoreError> {
    for key in props.keys() {
        check(
            !key.is_empty() && key.chars().count() <= 256,
            what,
            "property keys must be 1 to 256 characters",
        )?;
    }
    Ok(())
}

impl TypeRow {
    /// Checks the row against the data model.
    pub fn validate(&self) -> Result<(), CoreError> {
        check(
            !self.name.trim().is_empty(),
            "type name",
            "must not be blank",
        )?;
        check_text(&self.name, "type name", 200)?;
        check_optional(self.plural.as_deref(), "type plural", 200)?;
        check_optional(self.icon.as_deref(), "type icon", 64)?;
        check_optional(self.color.as_deref(), "type color", 32)?;
        check_optional(self.description.as_deref(), "type description", 10_000)?;
        check(
            self.extends != Some(self.id),
            "type",
            "cannot extend itself",
        )
    }
}

impl FieldRow {
    /// Checks the row against the data model.
    pub fn validate(&self) -> Result<(), CoreError> {
        check(
            is_identifier(&self.key, 64),
            "field key",
            "must be a lower-case identifier of at most 64 characters",
        )?;
        check(
            !self.label.trim().is_empty(),
            "field label",
            "must not be blank",
        )?;
        check_text(&self.label, "field label", 200)?;
        let options = self.options_json.as_object();
        check(options.is_some(), "field options", "must be a JSON object")?;
        if let Some(choices) = options.and_then(|o| o.get("options")) {
            let ids = select_option_ids(choices);
            check(
                ids.is_some(),
                "field options",
                "`options` must be a list of objects with unique string `id`s",
            )?;
        }
        if let Some(max) = options.and_then(|o| o.get("max")) {
            let max = max.as_i64().unwrap_or(0);
            check(
                (1..=100).contains(&max),
                "field options",
                "`max` must be an integer from 1 to 100",
            )?;
        }
        Ok(())
    }
}

/// The option IDs of a select field's `options` list, or `None` if the list is malformed.
pub(crate) fn select_option_ids(choices: &Value) -> Option<Vec<&str>> {
    let list = choices.as_array()?;
    let mut ids = Vec::with_capacity(list.len());
    for choice in list {
        let id = choice.as_object()?.get("id")?.as_str()?;
        if ids.contains(&id) {
            return None;
        }
        ids.push(id);
    }
    Some(ids)
}

impl ObjectRow {
    /// Checks the row against the data model. Field values are checked against field definitions
    /// by the store, which knows the object's types.
    pub fn validate(&self) -> Result<(), CoreError> {
        check_text(&self.title, "object title", 10_000)?;
        check_optional(self.summary.as_deref(), "object summary", 100_000)?;
        check(
            self.parent_id != Some(self.id),
            "object",
            "cannot be its own parent",
        )?;
        check_props(&self.props_json, "object properties")?;
        match (self.geo_lat, self.geo_lon) {
            (None, None) => {}
            (Some(lat), Some(lon)) => {
                check(
                    (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon),
                    "object location",
                    "latitude must be within ±90 and longitude within ±180",
                )?;
            }
            _ => {
                return Err(CoreError::invalid(
                    "object location",
                    "latitude and longitude go together",
                ));
            }
        }
        if let (Some(start), Some(end)) = (self.event_start, self.event_end) {
            check(
                end >= start,
                "object event",
                "must not end before it starts",
            )?;
        }
        Ok(())
    }
}

impl BlockRow {
    /// Checks the row against the data model.
    pub fn validate(&self) -> Result<(), CoreError> {
        check(
            is_identifier(&self.kind, 64),
            "block kind",
            "must be a lower-case identifier of at most 64 characters",
        )?;
        check(
            self.parent_block_id != Some(self.id),
            "block",
            "cannot be its own parent",
        )
    }
}

impl RelationRow {
    /// Checks the row against the data model.
    pub fn validate(&self) -> Result<(), CoreError> {
        check(
            is_identifier(&self.rel_type, 64),
            "relation type",
            "must be a lower-case identifier of at most 64 characters",
        )?;
        check_optional(self.label.as_deref(), "relation label", 1_000)?;
        check_props(&self.props_json, "relation properties")
    }
}

impl AttachmentRow {
    /// Checks the row against the data model.
    pub fn validate(&self) -> Result<(), CoreError> {
        check(
            self.blob_size >= 0,
            "attachment",
            "blob size must not be negative",
        )?;
        check_optional(self.mime.as_deref(), "attachment media type", 255)?;
        if let Some(name) = &self.filename {
            check(
                !name.is_empty()
                    && name.len() <= 255
                    && !name.contains(['/', '\\', '\0'])
                    && name != "."
                    && name != "..",
                "attachment file name",
                "must be a plain file name of at most 255 bytes",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn id(n: u128) -> Id {
        Id::from_parts(1_000, n)
    }

    pub(crate) fn object(n: u128) -> ObjectRow {
        ObjectRow {
            id: id(n),
            title: format!("Object {n}"),
            summary: None,
            parent_id: None,
            sort_key: SortKey::first(),
            props_json: Props::new(),
            created_at: 1,
            updated_at: 1,
            captured_at: None,
            event_start: None,
            event_end: None,
            geo_lat: None,
            geo_lon: None,
            deleted_at: None,
            sensitivity: Sensitivity::Normal,
        }
    }

    pub(crate) fn type_row(n: u128) -> TypeRow {
        TypeRow {
            id: id(n),
            name: format!("Type {n}"),
            plural: None,
            icon: None,
            color: None,
            extends: None,
            description: None,
            is_system: false,
            created_at: 1,
            updated_at: 1,
        }
    }

    pub(crate) fn field(n: u128, kind: FieldKind, options: Value) -> FieldRow {
        FieldRow {
            id: id(n),
            type_id: id(1),
            key: format!("f{n}"),
            label: format!("Field {n}"),
            kind,
            options_json: options,
            required: false,
            sort_order: 0,
        }
    }

    #[test]
    fn field_kinds_have_stable_names() {
        for kind in FieldKind::ALL {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, json!(kind.as_str()));
            assert_eq!(serde_json::from_value::<FieldKind>(json).unwrap(), kind);
            assert_eq!(FieldKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(FieldKind::parse("Text"), None);
        assert!(
            !FieldKind::Relation.is_stored()
                && !FieldKind::Formula.is_stored()
                && !FieldKind::Rollup.is_stored()
        );
        assert!(FieldKind::Text.is_stored());
        for role in AttachmentRole::ALL {
            assert_eq!(serde_json::to_value(role).unwrap(), json!(role.as_str()));
            assert_eq!(AttachmentRole::parse(role.as_str()), Some(role));
        }
        assert_eq!(AttachmentRole::parse("cover"), None);
        for sensitivity in [Sensitivity::Normal, Sensitivity::Sensitive] {
            assert_eq!(
                serde_json::to_value(sensitivity).unwrap(),
                json!(sensitivity.as_str())
            );
            assert_eq!(Sensitivity::parse(sensitivity.as_str()), Some(sensitivity));
        }
        assert_eq!(Sensitivity::parse("secret"), None);
    }

    #[test]
    fn identifiers() {
        assert!(is_identifier("due_date2", 64));
        for bad in ["", "Due", "2due", "due-date", "dué", "_x"] {
            assert!(!is_identifier(bad, 64), "{bad:?}");
        }
        assert!(!is_identifier("abc", 2));
    }

    #[test]
    fn type_rules() {
        let mut row = type_row(1);
        row.validate().unwrap();
        row.extends = Some(row.id);
        assert!(row.validate().is_err());
        let mut row = type_row(1);
        row.name = "  ".into();
        assert!(row.validate().is_err());
        let mut row = type_row(1);
        row.icon = Some("x".repeat(65));
        assert!(row.validate().is_err());
    }

    #[test]
    fn field_rules() {
        field(1, FieldKind::Text, json!({})).validate().unwrap();
        assert!(field(1, FieldKind::Text, json!([])).validate().is_err());
        let mut bad_key = field(1, FieldKind::Text, json!({}));
        bad_key.key = "Due Date".into();
        assert!(bad_key.validate().is_err());
        let mut blank = field(1, FieldKind::Text, json!({}));
        blank.label = String::new();
        assert!(blank.validate().is_err());
        field(
            1,
            FieldKind::Select,
            json!({"options": [{"id": "a"}, {"id": "b", "label": "B"}]}),
        )
        .validate()
        .unwrap();
        assert!(
            field(
                1,
                FieldKind::Select,
                json!({"options": [{"id": "a"}, {"id": "a"}]})
            )
            .validate()
            .is_err()
        );
        assert!(
            field(1, FieldKind::Select, json!({"options": "a,b"}))
                .validate()
                .is_err()
        );
        assert!(
            field(1, FieldKind::Select, json!({"options": [{"label": "x"}]}))
                .validate()
                .is_err()
        );
        field(1, FieldKind::Rating, json!({"max": 10}))
            .validate()
            .unwrap();
        assert!(
            field(1, FieldKind::Rating, json!({"max": 0}))
                .validate()
                .is_err()
        );
        assert!(
            field(1, FieldKind::Rating, json!({"max": "5"}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn object_rules() {
        object(1).validate().unwrap();
        let mut row = object(1);
        row.parent_id = Some(row.id);
        assert!(row.validate().is_err());
        let mut row = object(1);
        row.geo_lat = Some(10.0);
        assert!(row.validate().is_err());
        row.geo_lon = Some(200.0);
        assert!(row.validate().is_err());
        row.geo_lon = Some(20.0);
        row.validate().unwrap();
        let mut row = object(1);
        row.event_start = Some(10);
        row.event_end = Some(5);
        assert!(row.validate().is_err());
        row.event_end = Some(10);
        row.validate().unwrap();
        let mut row = object(1);
        row.props_json.insert(String::new(), json!(1));
        assert!(row.validate().is_err());
        let mut row = object(1);
        row.title = "x".repeat(10_001);
        assert!(row.validate().is_err());
    }

    #[test]
    fn block_relation_and_attachment_rules() {
        let block = BlockRow {
            id: id(1),
            object_id: id(2),
            parent_block_id: None,
            sort_key: SortKey::first(),
            kind: "paragraph".into(),
            content_json: json!({}),
            created_at: 1,
            updated_at: 1,
        };
        block.validate().unwrap();
        assert!(
            BlockRow {
                kind: "Para".into(),
                ..block.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            BlockRow {
                parent_block_id: Some(id(1)),
                ..block
            }
            .validate()
            .is_err()
        );

        let relation = RelationRow {
            id: id(1),
            src_id: id(2),
            dst_id: id(3),
            rel_type: "links_to".into(),
            field_id: None,
            src_block_id: None,
            label: None,
            props_json: Props::new(),
            created_at: 1,
            deleted_at: None,
        };
        relation.validate().unwrap();
        assert!(
            RelationRow {
                rel_type: "links to".into(),
                ..relation.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            RelationRow {
                label: Some("x".repeat(1_001)),
                ..relation
            }
            .validate()
            .is_err()
        );

        let attachment = AttachmentRow {
            object_id: id(1),
            blob_hash: ContentHash::of(b"x"),
            role: AttachmentRole::Original,
            filename: Some("scan.pdf".into()),
            blob_size: 1,
            mime: Some("application/pdf".into()),
        };
        attachment.validate().unwrap();
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(
                AttachmentRow {
                    filename: Some(bad.into()),
                    ..attachment.clone()
                }
                .validate()
                .is_err(),
                "{bad:?}"
            );
        }
        assert!(
            AttachmentRow {
                blob_size: -1,
                ..attachment
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rows_reject_unknown_columns() {
        let mut json = serde_json::to_value(object(1)).unwrap();
        json["colour"] = json!("red");
        assert!(serde_json::from_value::<ObjectRow>(json).is_err());
    }
}
