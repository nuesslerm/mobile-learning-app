//! A request body read as Node's routes read it: `body ? JSON.parse(body) : {}`, then `asRecord`.
use serde_json::{Map, Value};

/// The body's top-level object. An empty body is `{}`, and valid JSON that is no object (an array, `null`, a number) reads as
/// `{}` too; none when the body is no JSON. A byte that is no UTF-8 reads as U+FFFD, as Node decodes it.
pub fn parse_record(body: &[u8]) -> Option<Map<String, Value>> {
    let text = String::from_utf8_lossy(body);
    if text.is_empty() {
        return Some(Map::new());
    }
    match serde_json::from_str::<Value>(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => Some(Map::new()),
    }
}

/// `typeof value === 'string' ? value : null`
pub fn string_or_null(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// `Number.isInteger(value)` for a JSON value: a number whose value has no fraction, so `0.0` and `1e0` are integers.
pub fn as_integer(value: Option<&Value>) -> Option<f64> {
    let number = value?.as_f64()?;
    (number.is_finite() && number.fract() == 0.0).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_bodies_when_read_as_a_record_then_only_invalid_json_is_none_and_non_objects_are_empty() {
        assert_eq!(parse_record(b"").map(Value::Object), Some(json!({})));
        assert_eq!(parse_record(br#"{"a":1}"#).map(Value::Object), Some(json!({ "a": 1 })));
        for body in ["[1]", "null", "5", "\"s\"", "true"] {
            assert_eq!(parse_record(body.as_bytes()).map(Value::Object), Some(json!({})), "{body}");
        }
        for body in ["{oops", " ", "{\"a\":1} x", "\u{feff}{}"] {
            assert_eq!(parse_record(body.as_bytes()), None, "{body:?}");
        }
    }

    #[test]
    fn given_numbers_when_checked_for_an_integer_then_a_fraction_free_value_counts_in_any_spelling() {
        let record = parse_record(br#"{"a":0,"b":0.0,"c":1e0,"d":-1,"e":0.5,"f":"0","g":null,"h":true}"#).expect("a record");
        let integer = |key: &str| as_integer(record.get(key));
        assert_eq!((integer("a"), integer("b"), integer("c"), integer("d")), (Some(0.0), Some(0.0), Some(1.0), Some(-1.0)));
        assert_eq!((integer("e"), integer("f"), integer("g"), integer("h"), integer("missing")), (None, None, None, None, None));
    }
}
