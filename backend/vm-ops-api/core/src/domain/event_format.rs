//! How an event reads outside the service, in the bytes Node produced: the JSON line on stdout, and the attributes `wf log emit`
//! was handed for the log sink, where every value is the text of a JavaScript `String(value)`.
use super::js;
use serde_json::{Map, Value};

/// `JSON.stringify({ level, message, ...attributes })` and a newline. An attribute named like a leading key replaces its value
/// and keeps the key's place; keys that are array indexes come first, as V8 orders them.
pub fn stdout_line(level: &str, message: &str, attributes: &[(&str, Value)]) -> String {
    let mut fields = Map::new();
    fields.insert("level".to_string(), Value::from(level));
    fields.insert("message".to_string(), Value::from(message));
    for (key, value) in attributes {
        fields.insert((*key).to_string(), value.clone());
    }
    let mut line = js::stringify(&Value::Object(fields));
    line.push('\n');
    line
}

/// `String(value)`: an array joins its items with commas, a null item is empty, and an object is `[object Object]`.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => js::number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => items.iter().map(|item| if item.is_null() { String::new() } else { js_string(item) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// What the sink receives for an event, as `wf log emit` built it: `source`, the service; `hostLabel`, unless it is empty; then
/// each attribute as text, in the order written. A name that comes twice keeps its first place and takes the last value.
pub fn sink_attributes(service: &str, host_label: &str, attributes: &[(&str, Value)]) -> Vec<(String, String)> {
    let mut fields = vec![("source".to_string(), service.to_string())];
    if !host_label.is_empty() {
        fields.push(("hostLabel".to_string(), host_label.to_string()));
    }
    for (key, value) in attributes {
        let text = js_string(value);
        match fields.iter_mut().find(|(name, _)| name == key) {
            Some(field) => field.1 = text,
            None => fields.push(((*key).to_string(), text)),
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_numbers_when_a_line_is_made_then_they_print_as_javascript_prints_them() {
        let attributes = [("whole", json!(5.0)), ("fraction", json!(12.5)), ("big", json!(1e21)), ("small", json!(0.000001)), ("minus_zero", json!(-0.0)), ("count", json!(8080))];

        let line = stdout_line("info", "self_build.waiting_for_headroom", &attributes);

        assert_eq!(
            line,
            "{\"level\":\"info\",\"message\":\"self_build.waiting_for_headroom\",\"whole\":5,\"fraction\":12.5,\"big\":1e+21,\"small\":0.000001,\"minus_zero\":0,\"count\":8080}\n"
        );
    }

    #[test]
    fn given_an_attribute_named_like_a_leading_key_when_a_line_is_made_then_its_value_wins_in_the_keys_place() {
        let line = stdout_line("info", "a", &[("level", json!("custom")), ("message", json!(2)), ("n", json!(1))]);

        assert_eq!(line, "{\"level\":\"custom\",\"message\":2,\"n\":1}\n");
    }

    #[test]
    fn given_keys_that_are_array_indexes_when_a_line_is_made_then_they_come_first_as_in_v8() {
        let line = stdout_line("warn", "a", &[("b", json!(1)), ("10", json!(2)), ("2", json!(3))]);

        assert_eq!(line, "{\"2\":3,\"10\":2,\"level\":\"warn\",\"message\":\"a\",\"b\":1}\n");
    }

    #[test]
    fn given_values_of_every_json_kind_when_turned_to_text_then_they_read_as_string_of_the_value() {
        let cases = [
            (json!("text"), "text"),
            (json!(8080), "8080"),
            (json!(5.0), "5"),
            (json!(0.1), "0.1"),
            (json!(1e21), "1e+21"),
            (json!(true), "true"),
            (json!(false), "false"),
            (json!(null), "null"),
            (json!([1, "a", null, [2, 3], true]), "1,a,,2,3,true"),
            (json!([]), ""),
            (json!({ "a": 1 }), "[object Object]"),
            (json!([{ "a": 1 }]), "[object Object]"),
        ];
        for (value, want) in cases {
            assert_eq!(js_string(&value), want, "String({value})");
        }
    }

    #[test]
    fn given_an_event_when_its_sink_attributes_are_made_then_source_and_host_label_lead_and_every_value_is_text() {
        let attributes = [("port", json!(8080)), ("ok", json!(true)), ("avg10", json!(12.5))];

        let made = sink_attributes("vm-ops-api", "main", &attributes);

        let text = |key: &str, value: &str| (key.to_string(), value.to_string());
        assert_eq!(made, [text("source", "vm-ops-api"), text("hostLabel", "main"), text("port", "8080"), text("ok", "true"), text("avg10", "12.5")]);
    }

    #[test]
    fn given_an_empty_host_label_when_sink_attributes_are_made_then_host_label_is_left_out() {
        let made = sink_attributes("vm-ops-api", "", &[("a", json!("b"))]);

        let names: Vec<&str> = made.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["source", "a"]);
    }

    #[test]
    fn given_attributes_named_like_the_leading_ones_or_repeated_when_sink_attributes_are_made_then_the_first_place_takes_the_last_value() {
        let attributes = [("source", json!("other")), ("a", json!(1)), ("hostLabel", json!("h2")), ("a", json!(2))];

        let made = sink_attributes("vm-ops-api", "main", &attributes);

        let text = |key: &str, value: &str| (key.to_string(), value.to_string());
        assert_eq!(made, [text("source", "other"), text("hostLabel", "h2"), text("a", "2")]);
    }
}
