// SPDX-License-Identifier: AGPL-3.0-or-later
//! `files::Schema` (M5 Task 2, spec §6): the JSON of `.dbme/schema.json` round-trips, field
//! lookup follows `extends`, malformed JSON is refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests: a failure here is the finding"
)]

use dbme_core::files::{Schema, SchemaType};
use dbme_core::model::{FieldKind, FieldRow, TypeRow};
use dbme_core::{FORMAT_VERSION, Id};
use serde_json::json;

fn id(n: u128) -> Id {
    Id::from_parts(1_000, n)
}

fn ty(n: u128, name: &str, extends: Option<u128>) -> TypeRow {
    TypeRow {
        id: id(n),
        name: name.into(),
        plural: Some(format!("{name}s")),
        icon: None,
        color: Some("#abc".into()),
        extends: extends.map(id),
        description: None,
        is_system: n < 10,
        created_at: 1,
        updated_at: 2,
    }
}

fn field(n: u128, type_id: u128, key: &str, kind: FieldKind, sort_order: i64) -> FieldRow {
    FieldRow {
        id: id(n),
        type_id: id(type_id),
        key: key.into(),
        label: key.to_uppercase(),
        kind,
        options_json: json!({}),
        required: false,
        sort_order,
    }
}

fn sample() -> Schema {
    Schema::new(
        vec![
            ty(1, "Item", None),
            ty(2, "Task", Some(1)),
            ty(3, "Bug", Some(2)),
            ty(4, "Person", None),
        ],
        vec![
            field(10, 1, "status", FieldKind::Select, 0),
            field(11, 1, "notes", FieldKind::RichText, 1),
            field(20, 2, "due", FieldKind::Date, 0),
            field(21, 2, "status", FieldKind::Text, 1),
            field(30, 3, "severity", FieldKind::Rating, 0),
            field(40, 4, "email", FieldKind::Email, 0),
        ],
    )
}

#[test]
fn to_json_and_from_json_round_trip_exactly() {
    let schema = sample();
    let json = schema.to_json();
    assert_eq!(json["format_version"], json!(FORMAT_VERSION));
    let names: Vec<&str> = json["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Bug", "Item", "Person", "Task"]);
    assert_eq!(json["types"][3]["fields"][0]["key"], json!("due"));
    assert_eq!(json["types"][3]["fields"][1]["kind"], json!("text"));
    assert_eq!(json["types"][3]["extends"], json!(id(1).to_string()));
    let back = Schema::from_json(&json).unwrap();
    assert_eq!(back, schema);
    assert_eq!(back.to_json(), json);
    // Pretty-printed text, as the file is written, reads back the same.
    let text = serde_json::to_string_pretty(&json).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(Schema::from_json(&parsed).unwrap(), schema);
    // The grouping is visible in the public structure.
    let task: &SchemaType = schema.types.iter().find(|t| t.ty.name == "Task").unwrap();
    assert_eq!(task.fields.len(), 2);
}

#[test]
fn field_lookup_follows_extends() {
    let schema = sample();
    let bug = vec!["Bug".to_owned()];
    assert_eq!(schema.field(&bug, "severity").map(|f| f.id), Some(id(30)));
    assert_eq!(schema.field(&bug, "due").map(|f| f.id), Some(id(20)));
    assert_eq!(schema.field(&bug, "notes").map(|f| f.id), Some(id(11)));
    // The nearest type's definition of a key wins over an ancestor's.
    assert_eq!(
        schema.field(&bug, "status").map(|f| f.kind),
        Some(FieldKind::Text)
    );
    assert_eq!(
        schema.field(&["Item".to_owned()], "status").map(|f| f.kind),
        Some(FieldKind::Select)
    );
    assert_eq!(schema.field(&bug, "email"), None);
    // Several types: the first that has the key wins.
    let both = vec!["Person".to_owned(), "Bug".to_owned()];
    assert_eq!(schema.field(&both, "email").map(|f| f.id), Some(id(40)));
    assert_eq!(schema.field(&both, "severity").map(|f| f.id), Some(id(30)));
    assert_eq!(schema.field(&[], "status"), None);
    assert_eq!(schema.field(&["Nope".to_owned()], "status"), None);
    let keys: Vec<&str> = schema
        .fields_of(&bug)
        .into_iter()
        .map(|f| f.key.as_str())
        .collect();
    assert_eq!(keys, ["severity", "due", "status", "notes"]);
    assert_eq!(
        schema
            .field_by_id(id(21))
            .map(|(t, f)| (t.name.as_str(), f.key.as_str())),
        Some(("Task", "status"))
    );
    assert_eq!(schema.field_by_id(id(99)), None);
    assert_eq!(schema.type_by_name("Bug").map(|t| t.id), Some(id(3)));
}

#[test]
fn malformed_json_is_refused() {
    let good = sample().to_json();
    let mut other_version = good.clone();
    other_version["format_version"] = json!(1);
    let mut string_version = good.clone();
    string_version["format_version"] = json!("0");
    let mut extra_root_key = good.clone();
    extra_root_key["extra"] = json!(true);
    let mut unknown_type_column = good.clone();
    unknown_type_column["types"][0]["colour"] = json!("red");
    let mut unknown_field_column = good.clone();
    unknown_field_column["types"][0]["fields"][0]["colour"] = json!("red");
    let mut bad_kind = good.clone();
    bad_kind["types"][0]["fields"][0]["kind"] = json!("Text");
    let mut bad_id = good.clone();
    bad_id["types"][0]["id"] = json!("not-a-ulid");
    let mut wrong_owner = good.clone();
    wrong_owner["types"][0]["fields"][0]["type_id"] = json!(id(4).to_string());
    let mut self_extends = good.clone();
    self_extends["types"][2]["extends"] = self_extends["types"][2]["id"].clone();
    let mut missing_column = good.clone();
    missing_column["types"][0]
        .as_object_mut()
        .unwrap()
        .remove("name");
    for (what, bad) in [
        ("null", json!(null)),
        ("a string", json!("schema")),
        ("an array", json!([])),
        ("empty object", json!({})),
        ("types missing", json!({"format_version": 0})),
        (
            "types not an array",
            json!({"format_version": 0, "types": null}),
        ),
        (
            "a type that is a number",
            json!({"format_version": 0, "types": [1]}),
        ),
        ("other version", other_version),
        ("string version", string_version),
        ("extra root key", extra_root_key),
        ("unknown type column", unknown_type_column),
        ("unknown field column", unknown_field_column),
        ("bad kind", bad_kind),
        ("bad id", bad_id),
        ("field under the wrong type", wrong_owner),
        ("type extends itself", self_extends),
        ("missing column", missing_column),
    ] {
        assert!(Schema::from_json(&bad).is_err(), "{what} was accepted");
    }
    assert!(serde_json::from_str::<serde_json::Value>("{not json").is_err());
}

#[test]
fn an_empty_schema_round_trips() {
    let empty = Schema::default();
    let json = empty.to_json();
    assert_eq!(json, json!({"format_version": FORMAT_VERSION, "types": []}));
    assert_eq!(Schema::from_json(&json).unwrap(), empty);
    assert_eq!(Schema::new(vec![], vec![]), empty);
}
