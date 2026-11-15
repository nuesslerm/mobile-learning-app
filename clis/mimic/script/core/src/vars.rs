//! Variables of a run: `{name}` in an entry is filled from what earlier answers taught, and `capture` names JSON pointers into an answer.
use serde_json::{Map, Value};

pub type Vars = Map<String, Value>;

/// The text `String(v)` gives in JavaScript for the values a script holds.
pub fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Every `{name}` in `text`, in order.
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_name(&after[..close]) => {
                found.push(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    found
}

/// A string that is exactly `{name}` becomes the value itself (a number stays one); inside longer text the value's string form is put in.
/// A name with no value stays as it is.
pub fn fill(v: &Value, vars: &Vars) -> Value {
    match v {
        Value::String(s) => {
            if let Some(name) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')).filter(|n| is_name(n)) {
                if let Some(value) = vars.get(name) {
                    return value.clone();
                }
            }
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(open) = rest.find('{') {
                out.push_str(&rest[..open]);
                let after = &rest[open + 1..];
                match after.find('}') {
                    Some(close) if is_name(&after[..close]) && vars.contains_key(&after[..close]) => {
                        out.push_str(&js_string(&vars[&after[..close]]));
                        rest = &after[close + 1..];
                    }
                    _ => {
                        out.push('{');
                        rest = after;
                    }
                }
            }
            out.push_str(rest);
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| fill(x, vars)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), fill(x, vars))).collect()),
        other => other.clone(),
    }
}

/// The first name `names` declares that appears in `v` with no value yet.
pub fn unbound(v: &Value, vars: &Vars, names: &[String]) -> Option<String> {
    placeholders(&v.to_string()).into_iter().find(|name| names.iter().any(|n| n == name) && !vars.contains_key(*name)).map(String::from)
}

/// A JSON pointer: parts split on `/` with the first empty one dropped, `~1` and `~0` unescaped, arrays indexed by their text.
pub fn pointer<'a>(doc: &'a Value, ptr: &str) -> Option<&'a Value> {
    let mut at = doc;
    for part in ptr.split('/').skip(1) {
        let key = part.replace("~1", "/").replace("~0", "~");
        at = match at {
            Value::Object(o) => o.get(&key)?,
            Value::Array(a) => a.get(key.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(at)
}

/// What `capture` (`{name: {from: [pointer...], prefix?}}`) finds in `response`: per name the first pointer holding a string or a number.
pub fn captured(response: &Value, capture: &Value) -> Vars {
    let mut out = Vars::new();
    for (name, spec) in capture.as_object().into_iter().flatten() {
        let prefix = spec.get("prefix").and_then(Value::as_str).unwrap_or("");
        for ptr in spec.get("from").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            match pointer(response, ptr) {
                Some(Value::String(s)) => {
                    out.insert(name.clone(), Value::String(format!("{prefix}{s}")));
                    break;
                }
                Some(n @ Value::Number(_)) => {
                    out.insert(name.clone(), n.clone());
                    break;
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars() -> Vars {
        json!({"id": 7, "name": "ada"}).as_object().unwrap().clone()
    }

    #[test]
    fn given_a_whole_string_placeholder_when_filled_then_the_value_keeps_its_type_and_embedded_ones_become_text() {
        assert_eq!(fill(&json!("{id}"), &vars()), json!(7));
        assert_eq!(fill(&json!("run-{id}-{name}"), &vars()), json!("run-7-ada"));
        assert_eq!(fill(&json!({"a": ["{name}", "{missing}"]}), &vars()), json!({"a": ["ada", "{missing}"]}));
    }

    #[test]
    fn given_a_pointer_with_escapes_when_followed_then_arrays_and_objects_are_walked() {
        let doc = json!({"a/b": {"c~d": [10, {"x": "y"}]}});

        assert_eq!(pointer(&doc, "/a~1b/c~0d/1/x"), Some(&json!("y")));
        assert_eq!(pointer(&doc, "/a~1b/none"), None);
    }

    #[test]
    fn given_a_capture_when_the_answer_holds_a_string_or_a_number_then_the_first_pointer_that_matches_wins_and_strings_get_the_prefix() {
        let capture = json!({"token": {"from": ["/missing", "/access"], "prefix": "Bearer "}, "run": {"from": ["/n"]}, "none": {"from": ["/obj"]}});

        let found = captured(&json!({"access": "abc", "n": 3, "obj": {}}), &capture);

        assert_eq!(Value::Object(found), json!({"token": "Bearer abc", "run": 3}));
    }
}
