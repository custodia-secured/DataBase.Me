// SPDX-License-Identifier: AGPL-3.0-or-later
//! Checking a field value against its field's kind (FORMAT.md §3.3).
//!
//! Only the shape is checked: a URL field holds a string, not necessarily a reachable URL. Imported
//! data is often messy, and refusing it would lose it.

use serde_json::Value;

use crate::model::{FieldKind, FieldRow, select_option_ids};
use crate::{ContentHash, CoreError};

/// Checks `value` as a value of `field`. To clear a field, remove its key rather than storing
/// `null`.
pub fn validate_field_value(field: &FieldRow, value: &Value) -> Result<(), CoreError> {
    let fail = |reason: &str| {
        Err(CoreError::invalid(
            format!("value of field {:?}", field.key),
            reason,
        ))
    };
    if value.is_null() {
        return fail("null is not a value; remove the key to clear the field");
    }
    let ok = match field.kind {
        FieldKind::Text | FieldKind::Url | FieldKind::Email | FieldKind::Phone => value.is_string(),
        FieldKind::RichText => value
            .as_array()
            .is_some_and(|blocks| blocks.iter().all(Value::is_object)),
        FieldKind::Number => value.is_number(),
        FieldKind::Currency => is_currency(value),
        FieldKind::Boolean => value.is_boolean(),
        FieldKind::Date => value.as_str().is_some_and(is_date),
        FieldKind::Datetime => value.as_str().is_some_and(is_datetime),
        FieldKind::Duration => value.is_i64(),
        FieldKind::Select => value
            .as_str()
            .is_some_and(|choice| is_option(field, choice)),
        FieldKind::MultiSelect => value.as_array().is_some_and(|choices| {
            let mut seen = Vec::with_capacity(choices.len());
            choices.iter().all(|c| {
                c.as_str().is_some_and(|choice| {
                    is_option(field, choice) && !seen.contains(&choice) && {
                        seen.push(choice);
                        true
                    }
                })
            })
        }),
        FieldKind::Geo => is_geo(value),
        FieldKind::File => value.as_array().is_some_and(|hashes| {
            hashes
                .iter()
                .all(|h| h.as_str().is_some_and(|h| ContentHash::parse(h).is_ok()))
        }),
        FieldKind::Rating => {
            let max = field
                .options_json
                .get("max")
                .and_then(Value::as_i64)
                .unwrap_or(5);
            value.as_i64().is_some_and(|r| (0..=max).contains(&r))
        }
        FieldKind::Json => true,
        FieldKind::Relation | FieldKind::Formula | FieldKind::Rollup => {
            return fail("this kind of field is not stored in properties");
        }
    };
    if ok {
        Ok(())
    } else {
        fail(&format!("not a valid {} value", field.kind.as_str()))
    }
}

fn is_option(field: &FieldRow, choice: &str) -> bool {
    match field
        .options_json
        .get("options")
        .and_then(select_option_ids)
    {
        Some(ids) => ids.contains(&choice),
        None => true,
    }
}

fn is_currency(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    let amount = map.get("amount").and_then(Value::as_str);
    let code = map.get("currency").and_then(Value::as_str);
    map.len() == 2
        && amount.is_some_and(is_decimal)
        && code.is_some_and(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()))
}

/// `-?digits(.digits)?`
fn is_decimal(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, "0"));
    !whole.is_empty()
        && !fraction.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.bytes().all(|b| b.is_ascii_digit())
}

fn is_geo(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    let lat = map.get("lat").and_then(Value::as_f64);
    let lon = map.get("lon").and_then(Value::as_f64);
    map.len() == 2
        && lat.is_some_and(|lat| (-90.0..=90.0).contains(&lat))
        && lon.is_some_and(|lon| (-180.0..=180.0).contains(&lon))
}

fn number(text: &str) -> Option<u32> {
    if text.bytes().all(|b| b.is_ascii_digit()) {
        text.parse().ok()
    } else {
        None
    }
}

/// `YYYY-MM-DD`, a real calendar date.
pub(crate) fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (
        number(&text[0..4]),
        number(&text[5..7]),
        number(&text[8..10]),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// `YYYY-MM-DDTHH:MM:SSZ` or `YYYY-MM-DDTHH:MM:SS.sssZ`, in UTC.
pub(crate) fn is_datetime(text: &str) -> bool {
    let Some(rest) = text.strip_suffix('Z') else {
        return false;
    };
    let Some((date, time)) = rest.split_once('T') else {
        return false;
    };
    let (time, millis) = match time.split_once('.') {
        Some((time, millis)) => (time, Some(millis)),
        None => (time, None),
    };
    let parts: Vec<&str> = time.split(':').collect();
    is_date(date)
        && parts.len() == 3
        && parts.iter().all(|p| p.len() == 2)
        && number(parts[0]).is_some_and(|h| h < 24)
        && number(parts[1]).is_some_and(|m| m < 60)
        && number(parts[2]).is_some_and(|s| s < 60)
        && millis.is_none_or(|ms| ms.len() == 3 && number(ms).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::field;
    use serde_json::json;

    fn valid(kind: FieldKind, options: Value, value: Value) -> bool {
        validate_field_value(&field(1, kind, options), &value).is_ok()
    }

    #[test]
    fn each_kind_accepts_its_shape_and_rejects_others() {
        let none = json!({});
        let cases: Vec<(FieldKind, Vec<Value>, Vec<Value>)> = vec![
            (FieldKind::Text, vec![json!("x"), json!("")], vec![json!(1)]),
            (
                FieldKind::Url,
                vec![json!("https://example.org")],
                vec![json!(true)],
            ),
            (
                FieldKind::RichText,
                vec![json!([]), json!([{"kind": "paragraph"}])],
                vec![json!("x"), json!([1])],
            ),
            (
                FieldKind::Number,
                vec![json!(1), json!(-2.5)],
                vec![json!("1")],
            ),
            (
                FieldKind::Currency,
                vec![
                    json!({"amount": "12.50", "currency": "EUR"}),
                    json!({"amount": "-3", "currency": "USD"}),
                ],
                vec![
                    json!({"amount": 12, "currency": "EUR"}),
                    json!({"amount": "1.", "currency": "EUR"}),
                    json!({"amount": "1", "currency": "eur"}),
                    json!({"amount": "1", "currency": "EUR", "x": 1}),
                    json!({"amount": "a", "currency": "EUR"}),
                    json!({"amount": "-", "currency": "EUR"}),
                    json!("1 EUR"),
                ],
            ),
            (FieldKind::Boolean, vec![json!(false)], vec![json!(0)]),
            (
                FieldKind::Date,
                vec![json!("2024-02-29"), json!("2026-12-31")],
                vec![
                    json!("2026-02-29"),
                    json!("2026-13-01"),
                    json!("2026-04-31"),
                    json!("2026-1-01"),
                    json!("2026/01/01"),
                    json!("20a6-01-01"),
                    json!("2026-00-10"),
                    json!(20260101),
                ],
            ),
            (
                FieldKind::Datetime,
                vec![
                    json!("2026-10-25T08:21:11Z"),
                    json!("2026-10-25T08:21:11.000Z"),
                ],
                vec![
                    json!("2026-10-25T08:21:11"),
                    json!("2026-10-25 08:21:11Z"),
                    json!("2026-10-25T24:00:00Z"),
                    json!("2026-10-25T08:21:11.0Z"),
                    json!("2026-10-25T08:21Z"),
                    json!("2026-10-25T08:61:11Z"),
                    json!("2026-10-25T08:21:61Z"),
                    json!("2026-10-25T8:21:11Z"),
                ],
            ),
            (FieldKind::Duration, vec![json!(1_500)], vec![json!(1.5)]),
            (
                FieldKind::Geo,
                vec![json!({"lat": 1.5, "lon": -2})],
                vec![
                    json!({"lat": 91, "lon": 0}),
                    json!({"lat": 0}),
                    json!({"lat": 0, "lon": 0, "alt": 1}),
                    json!([0, 0]),
                ],
            ),
            (
                FieldKind::File,
                vec![json!([]), json!([ContentHash::of(b"x").as_str()])],
                vec![json!(["md5:00"]), json!("blake3:00")],
            ),
            (
                FieldKind::Rating,
                vec![json!(0), json!(5)],
                vec![json!(6), json!(-1), json!(2.5)],
            ),
            (
                FieldKind::Json,
                vec![json!({"anything": [1, "two"]}), json!(3)],
                vec![],
            ),
        ];
        for (kind, good, bad) in cases {
            for value in good {
                assert!(
                    valid(kind, none.clone(), value.clone()),
                    "{kind:?} rejected {value}"
                );
            }
            for value in bad {
                assert!(
                    !valid(kind, none.clone(), value.clone()),
                    "{kind:?} accepted {value}"
                );
            }
        }
        assert!(valid(FieldKind::Rating, json!({"max": 10}), json!(10)));
    }

    #[test]
    fn selects_check_their_options_when_they_have_a_list() {
        let options = json!({"options": [{"id": "todo"}, {"id": "done"}]});
        assert!(valid(FieldKind::Select, options.clone(), json!("todo")));
        assert!(!valid(FieldKind::Select, options.clone(), json!("later")));
        assert!(valid(FieldKind::Select, json!({}), json!("anything")));
        assert!(valid(
            FieldKind::MultiSelect,
            options.clone(),
            json!(["todo", "done"])
        ));
        assert!(valid(FieldKind::MultiSelect, options.clone(), json!([])));
        assert!(!valid(
            FieldKind::MultiSelect,
            options.clone(),
            json!(["todo", "todo"])
        ));
        assert!(!valid(
            FieldKind::MultiSelect,
            options,
            json!(["todo", "later"])
        ));
        assert!(!valid(FieldKind::MultiSelect, json!({}), json!("todo")));
    }

    #[test]
    fn null_and_unstored_kinds_are_rejected() {
        assert!(!valid(FieldKind::Json, json!({}), Value::Null));
        for kind in [FieldKind::Relation, FieldKind::Formula, FieldKind::Rollup] {
            assert!(!valid(kind, json!({}), json!("x")));
        }
    }
}
