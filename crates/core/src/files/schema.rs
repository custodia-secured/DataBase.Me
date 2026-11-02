// SPDX-License-Identifier: AGPL-3.0-or-later
//! The vault's types and their fields as the files folder needs them: which kind a frontmatter
//! key has for an object's types, and the JSON of `files/.dbme/schema.json` and of the export's
//! `types.json` (M5 Decision 19). `to_json` and `from_json` round-trip exactly.

use serde_json::{Map, Value};

use crate::model::{FieldRow, TypeRow};
use crate::{CoreError, FORMAT_VERSION, Id};

/// Every type of a vault with its fields. Types are kept sorted by name, fields by `sort_order`
/// then key, so two schemas with the same rows are equal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schema {
    /// The types, by name.
    pub types: Vec<SchemaType>,
}

/// One type with its own fields (not the inherited ones).
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaType {
    /// The type.
    pub ty: TypeRow,
    /// Its fields, by `sort_order` then key.
    pub fields: Vec<FieldRow>,
}

impl Schema {
    /// Groups `fields` under their types by `type_id`. A field whose `type_id` names no type is
    /// dropped: the schema cannot say where it belongs.
    pub fn new(mut types: Vec<TypeRow>, mut fields: Vec<FieldRow>) -> Self {
        types.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
        fields.sort_by(|a, b| (a.sort_order, &a.key, a.id).cmp(&(b.sort_order, &b.key, b.id)));
        let types = types
            .into_iter()
            .map(|ty| {
                let fields = fields
                    .iter()
                    .filter(|f| f.type_id == ty.id)
                    .cloned()
                    .collect();
                SchemaType { ty, fields }
            })
            .collect();
        Self { types }
    }

    /// The type called `name`, exactly.
    pub fn type_by_name(&self, name: &str) -> Option<&TypeRow> {
        self.entry_by_name(name).map(|t| &t.ty)
    }

    /// The type with this id.
    pub fn type_by_id(&self, id: Id) -> Option<&TypeRow> {
        self.entry_by_id(id).map(|t| &t.ty)
    }

    /// The field `key` of the first type in `type_names` that has one, own fields before inherited
    /// ones (following `extends`). A name the schema does not know is skipped.
    pub fn field(&self, type_names: &[String], key: &str) -> Option<&FieldRow> {
        type_names.iter().find_map(|name| {
            self.lineage(name)
                .into_iter()
                .find_map(|t| t.fields.iter().find(|f| f.key == key))
        })
    }

    /// Every field the objects of these types have, each key once: the types in the order given,
    /// own fields before inherited ones, by `sort_order` then key within a type. The first type
    /// that has a key wins.
    pub fn fields_of(&self, type_names: &[String]) -> Vec<&FieldRow> {
        let mut out: Vec<&FieldRow> = Vec::new();
        for name in type_names {
            for ty in self.lineage(name) {
                for field in &ty.fields {
                    if !out.iter().any(|f| f.key == field.key) {
                        out.push(field);
                    }
                }
            }
        }
        out
    }

    /// The field with this id and the type it belongs to.
    pub fn field_by_id(&self, id: Id) -> Option<(&TypeRow, &FieldRow)> {
        self.types
            .iter()
            .find_map(|t| t.fields.iter().find(|f| f.id == id).map(|f| (&t.ty, f)))
    }

    /// The JSON of `files/.dbme/schema.json`:
    /// `{"format_version": 0, "types": [{<TypeRow columns>, "fields": [<FieldRow>…]}]}`, types by
    /// name, fields by `sort_order` then key. The caller pretty-prints it.
    pub fn to_json(&self) -> Value {
        let types: Vec<Value> = self
            .types
            .iter()
            .map(|t| {
                let mut map = match serde_json::to_value(&t.ty) {
                    Ok(Value::Object(map)) => map,
                    _ => Map::new(),
                };
                let fields: Vec<Value> = t
                    .fields
                    .iter()
                    .map(|f| serde_json::to_value(f).unwrap_or(Value::Null))
                    .collect();
                map.insert("fields".to_owned(), Value::Array(fields));
                Value::Object(map)
            })
            .collect();
        let mut root = Map::new();
        root.insert("format_version".to_owned(), Value::from(FORMAT_VERSION));
        root.insert("types".to_owned(), Value::Array(types));
        Value::Object(root)
    }

    /// Reads what [`Schema::to_json`] wrote. The format version must be this crate's; every type
    /// and field is checked against the data model; a field must sit under its own type; unknown
    /// keys are refused, so a file from a newer app is never half-read.
    pub fn from_json(v: &Value) -> Result<Self, CoreError> {
        let root = v
            .as_object()
            .ok_or_else(|| CoreError::invalid("schema", "not a JSON object"))?;
        match root.get("format_version").and_then(Value::as_u64) {
            Some(version) if version == u64::from(FORMAT_VERSION) => {}
            Some(version) => {
                return Err(CoreError::invalid(
                    "schema",
                    format!("format_version {version} is not {FORMAT_VERSION}"),
                ));
            }
            None => {
                return Err(CoreError::invalid(
                    "schema",
                    "format_version is missing or not an integer",
                ));
            }
        }
        for key in root.keys() {
            if key != "format_version" && key != "types" {
                return Err(CoreError::invalid("schema", format!("unknown key {key:?}")));
            }
        }
        let entries = root
            .get("types")
            .and_then(Value::as_array)
            .ok_or_else(|| CoreError::invalid("schema", "`types` must be an array"))?;
        let mut types = Vec::with_capacity(entries.len());
        let mut fields = Vec::new();
        for entry in entries {
            let mut map = entry
                .as_object()
                .cloned()
                .ok_or_else(|| CoreError::invalid("schema type", "not a JSON object"))?;
            let own = match map.remove("fields") {
                Some(Value::Array(own)) => own,
                Some(_) => {
                    return Err(CoreError::invalid(
                        "schema type",
                        "`fields` must be an array",
                    ));
                }
                None => Vec::new(),
            };
            let ty: TypeRow = serde_json::from_value(Value::Object(map))?;
            ty.validate()?;
            for value in own {
                let field: FieldRow = serde_json::from_value(value)?;
                field.validate()?;
                if field.type_id != ty.id {
                    return Err(CoreError::invalid(
                        format!("schema field {:?}", field.key),
                        format!(
                            "belongs to type {} but is listed under {}",
                            field.type_id, ty.id
                        ),
                    ));
                }
                fields.push(field);
            }
            if types.iter().any(|t: &TypeRow| t.id == ty.id) {
                return Err(CoreError::invalid(
                    "schema",
                    format!("type {} is listed twice", ty.id),
                ));
            }
            types.push(ty);
        }
        Ok(Self::new(types, fields))
    }

    fn entry_by_name(&self, name: &str) -> Option<&SchemaType> {
        self.types.iter().find(|t| t.ty.name == name)
    }

    fn entry_by_id(&self, id: Id) -> Option<&SchemaType> {
        self.types.iter().find(|t| t.ty.id == id)
    }

    /// The type called `name` and then each ancestor through `extends`, stopping at a type with
    /// no parent, an unknown parent, or one already seen (a cycle).
    fn lineage(&self, name: &str) -> Vec<&SchemaType> {
        let mut chain: Vec<&SchemaType> = Vec::new();
        let mut next = self.entry_by_name(name);
        while let Some(ty) = next {
            if chain.iter().any(|seen| seen.ty.id == ty.ty.id) {
                break;
            }
            chain.push(ty);
            next = ty.ty.extends.and_then(|parent| self.entry_by_id(parent));
        }
        chain
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::FieldKind;
    use crate::model::tests::{id, type_row};

    fn field(n: u128, type_id: u128, key: &str, sort_order: i64) -> FieldRow {
        FieldRow {
            id: id(n),
            type_id: id(type_id),
            key: key.into(),
            label: key.into(),
            kind: FieldKind::Text,
            options_json: json!({}),
            required: false,
            sort_order,
        }
    }

    fn named(n: u128, name: &str, extends: Option<u128>) -> TypeRow {
        TypeRow {
            name: name.into(),
            extends: extends.map(id),
            ..type_row(n)
        }
    }

    fn sample() -> Schema {
        Schema::new(
            vec![
                named(2, "Task", Some(1)),
                named(1, "Item", None),
                named(3, "Person", None),
            ],
            vec![
                field(20, 2, "due", 1),
                field(21, 2, "status", 0),
                field(10, 1, "status", 5),
                field(11, 1, "notes", 3),
                field(30, 3, "email", 0),
                field(99, 77, "orphan", 0),
            ],
        )
    }

    #[test]
    fn new_sorts_and_groups() {
        let schema = sample();
        let names: Vec<&str> = schema.types.iter().map(|t| t.ty.name.as_str()).collect();
        assert_eq!(names, ["Item", "Person", "Task"]);
        let task_keys: Vec<&str> = schema.types[2]
            .fields
            .iter()
            .map(|f| f.key.as_str())
            .collect();
        assert_eq!(task_keys, ["status", "due"]);
        assert!(
            schema.field_by_id(id(99)).is_none(),
            "orphan fields are dropped"
        );
        assert_eq!(schema.type_by_name("Task").map(|t| t.id), Some(id(2)));
        assert_eq!(
            schema.type_by_id(id(3)).map(|t| t.name.as_str()),
            Some("Person")
        );
        assert!(schema.type_by_name("task").is_none());
    }

    #[test]
    fn field_lookup_follows_extends_and_type_order() {
        let schema = sample();
        let task = vec!["Task".to_owned()];
        assert_eq!(schema.field(&task, "status").map(|f| f.id), Some(id(21)));
        assert_eq!(schema.field(&task, "notes").map(|f| f.id), Some(id(11)));
        assert_eq!(schema.field(&task, "email"), None);
        let both = vec!["Person".to_owned(), "Task".to_owned()];
        assert_eq!(schema.field(&both, "email").map(|f| f.id), Some(id(30)));
        assert_eq!(schema.field(&both, "due").map(|f| f.id), Some(id(20)));
        assert_eq!(schema.field(&["Unknown".to_owned()], "status"), None);
        let keys: Vec<&str> = schema
            .fields_of(&both)
            .into_iter()
            .map(|f| f.key.as_str())
            .collect();
        assert_eq!(keys, ["email", "status", "due", "notes"]);
        assert_eq!(
            schema
                .field_by_id(id(11))
                .map(|(t, f)| (t.name.as_str(), f.key.as_str())),
            Some(("Item", "notes"))
        );
    }

    #[test]
    fn a_cycle_in_extends_ends() {
        let schema = Schema::new(
            vec![named(1, "A", Some(2)), named(2, "B", Some(1))],
            vec![field(10, 2, "x", 0)],
        );
        assert_eq!(
            schema.field(&["A".to_owned()], "x").map(|f| f.id),
            Some(id(10))
        );
        assert_eq!(schema.fields_of(&["A".to_owned()]).len(), 1);
    }

    #[test]
    fn json_round_trips() {
        let schema = sample();
        let json = schema.to_json();
        assert_eq!(json["format_version"], json!(FORMAT_VERSION));
        assert_eq!(json["types"][2]["name"], json!("Task"));
        assert_eq!(json["types"][2]["fields"][0]["key"], json!("status"));
        assert_eq!(Schema::from_json(&json).unwrap(), schema);
        assert_eq!(Schema::default().to_json()["types"], json!([]));
        assert_eq!(
            Schema::from_json(&Schema::default().to_json()).unwrap(),
            Schema::default()
        );
    }

    #[test]
    fn malformed_json_is_refused() {
        let good = sample().to_json();
        let mut wrong_version = good.clone();
        wrong_version["format_version"] = json!(99);
        let mut no_version = good.clone();
        no_version.as_object_mut().unwrap().remove("format_version");
        let mut extra_key = good.clone();
        extra_key["colour"] = json!("red");
        let mut unknown_column = good.clone();
        unknown_column["types"][0]["colour"] = json!("red");
        let mut bad_fields = good.clone();
        bad_fields["types"][0]["fields"] = json!("none");
        let mut wrong_owner = good.clone();
        wrong_owner["types"][2]["fields"][0]["type_id"] = json!(id(1).to_string());
        let mut bad_key = good.clone();
        bad_key["types"][2]["fields"][0]["key"] = json!("Due Date");
        let mut twice = good.clone();
        let first = twice["types"][0].clone();
        twice["types"].as_array_mut().unwrap().push(first);
        for bad in [
            json!(null),
            json!([]),
            json!({"format_version": 0, "types": {}}),
            json!({"format_version": 0, "types": [1]}),
            wrong_version,
            no_version,
            extra_key,
            unknown_column,
            bad_fields,
            wrong_owner,
            bad_key,
            twice,
        ] {
            assert!(Schema::from_json(&bad).is_err(), "{bad}");
        }
        // A type without a `fields` key has none.
        let mut no_fields = good;
        no_fields["types"][2]
            .as_object_mut()
            .unwrap()
            .remove("fields");
        let schema = Schema::from_json(&no_fields).unwrap();
        assert!(schema.types[2].fields.is_empty());
    }
}
