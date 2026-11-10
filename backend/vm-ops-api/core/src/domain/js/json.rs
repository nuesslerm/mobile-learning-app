//! `JSON.stringify` of a parsed JSON value, byte for byte: a number prints as JavaScript prints it (`1`, not `1.0`; `0.000001`;
//! `1e+21`), and an object lists its array-index keys first, ascending, as V8 does, then the rest in the order they were written.
use super::{json_number, JsString};
use serde_json::{Map, Value};
use std::fmt::Write as _;

const INDENT: &str = "  ";
const MAX_ARRAY_INDEX: u64 = 4_294_967_294;

/// `JSON.stringify(value)`.
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, false, 0);
    out
}

/// `JSON.stringify(value, null, 2)`.
pub fn stringify_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, true, 0);
    out
}

fn write_value(out: &mut String, value: &Value, pretty: bool, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&json_number(number.as_f64().unwrap_or(f64::NAN))),
        Value::String(text) => out.push_str(&JsString::from(text.as_str()).to_json()),
        Value::Array(items) => write_list(out, items.iter().map(|item| (None, item)), ('[', ']'), pretty, depth),
        Value::Object(map) => write_list(out, ordered(map).into_iter().map(|(key, item)| (Some(key), item)), ('{', '}'), pretty, depth),
    }
}

fn write_list<'a>(out: &mut String, entries: impl Iterator<Item = (Option<&'a str>, &'a Value)>, brackets: (char, char), pretty: bool, depth: usize) {
    out.push(brackets.0);
    let mut any = false;
    for (key, value) in entries {
        if any {
            out.push(',');
        }
        any = true;
        if pretty {
            out.push('\n');
            out.push_str(&INDENT.repeat(depth + 1));
        }
        if let Some(key) = key {
            out.push_str(&JsString::from(key).to_json());
            out.push(':');
            if pretty {
                out.push(' ');
            }
        }
        write_value(out, value, pretty, depth + 1);
    }
    if any && pretty {
        let _ = write!(out, "\n{}", INDENT.repeat(depth));
    }
    out.push(brackets.1);
}

/// A canonical array index: `0`, or digits with no leading zero, no greater than 2^32 - 2.
fn array_index(key: &str) -> Option<u64> {
    let digits = key.as_bytes();
    if digits.is_empty() || digits.len() > 10 || !digits.iter().all(u8::is_ascii_digit) || (digits.len() > 1 && digits[0] == b'0') {
        return None;
    }
    key.parse::<u64>().ok().filter(|index| *index <= MAX_ARRAY_INDEX)
}

fn ordered(map: &Map<String, Value>) -> Vec<(&str, &Value)> {
    let mut indexed: Vec<(u64, &str, &Value)> = Vec::new();
    let mut named: Vec<(&str, &Value)> = Vec::new();
    for (key, value) in map {
        match array_index(key) {
            Some(index) => indexed.push((index, key, value)),
            None => named.push((key, value)),
        }
    }
    indexed.sort_by_key(|(index, _, _)| *index);
    indexed.into_iter().map(|(_, key, value)| (key, value)).chain(named).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).expect("test JSON")
    }

    #[test]
    fn given_numbers_when_stringified_then_they_print_as_javascript_prints_them() {
        let value = parsed(r#"{"a":1e21,"b":1e-7,"c":0.000001,"d":-0.0,"e":1.0,"f":12345678901234567890,"g":5e-324,"h":123456789012345680000,"i":1e300,"j":1500,"k":0.0123,"l":-3}"#);

        assert_eq!(
            stringify(&value),
            r#"{"a":1e+21,"b":1e-7,"c":0.000001,"d":0,"e":1,"f":12345678901234567000,"g":5e-324,"h":123456789012345680000,"i":1e+300,"j":1500,"k":0.0123,"l":-3}"#
        );
    }

    #[test]
    fn given_integer_like_keys_when_stringified_then_the_array_indices_come_first_in_ascending_order() {
        let value = parsed(r#"{"b":1,"2":"two","a":3,"1":"one","01":"x","4294967294":"max","4294967295":"over","-1":"neg"}"#);

        assert_eq!(stringify(&value), r#"{"1":"one","2":"two","4294967294":"max","b":1,"a":3,"01":"x","4294967295":"over","-1":"neg"}"#);
    }

    #[test]
    fn given_nested_containers_when_stringified_pretty_then_they_are_indented_by_two_and_empty_ones_stay_closed() {
        let value = parsed(r#"{"a":[],"b":{},"c":[1,[2,[]],{"x":{}}]}"#);

        assert_eq!(
            stringify_pretty(&value),
            "{\n  \"a\": [],\n  \"b\": {},\n  \"c\": [\n    1,\n    [\n      2,\n      []\n    ],\n    {\n      \"x\": {}\n    }\n  ]\n}"
        );
        assert_eq!(stringify(&value), r#"{"a":[],"b":{},"c":[1,[2,[]],{"x":{}}]}"#);
    }

    #[test]
    fn given_scalars_and_strings_when_stringified_then_they_are_what_json_stringify_writes() {
        assert_eq!(stringify(&Value::Null), "null");
        assert_eq!(stringify(&Value::Bool(true)), "true");
        assert_eq!(stringify(&parsed("{}")), "{}");
        assert_eq!(stringify_pretty(&parsed("[]")), "[]");
        assert_eq!(stringify(&Value::from("quote\" back\\ \n \u{1} \u{e9} \u{1f600} \u{2028}")), "\"quote\\\" back\\\\ \\n \\u0001 \u{e9} \u{1f600} \u{2028}\"");
        assert_eq!(stringify(&parsed(r#"{"k\n":"v"}"#)), "{\"k\\n\":\"v\"}");
    }
}
