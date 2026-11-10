//! The meta an opencode run's start and finish write, built from `wf`'s posts as Node builds it: a field of the wrong type is null
//! (`''` for the prompt), a token count is `Number(x) || 0`, and the keys keep Node's order.
use crate::domain::js;
use serde_json::{Map, Value};

type Fields = Map<String, Value>;

fn text(fields: &Fields, key: &str) -> Value {
    fields.get(key).filter(|value| value.is_string()).cloned().unwrap_or(Value::Null)
}

fn number_or_null(fields: &Fields, key: &str) -> Value {
    fields.get(key).filter(|value| value.is_number()).cloned().unwrap_or(Value::Null)
}

/// A JavaScript number as a JSON value, the way `JSON.stringify` keeps it: whole numbers without a fraction, and NaN or an
/// infinity as null.
fn number_value(number: f64) -> Value {
    if !number.is_finite() {
        Value::Null
    } else if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
        Value::from(number as i64)
    } else {
        Value::from(number)
    }
}

/// `Array#toString`: the items joined with commas, null as nothing and a nested array flattened the same way.
fn array_text(items: &[Value]) -> String {
    let each = |item: &Value| match item {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => js::number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(text) => text.clone(),
        Value::Array(inner) => array_text(inner),
        Value::Object(_) => "[object Object]".to_string(),
    };
    items.iter().map(each).collect::<Vec<_>>().join(",")
}

/// `Number(value)` of a parsed JSON value; a missing key is `undefined`, which is NaN.
fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => js::to_number(text),
        Some(Value::Array(items)) => js::to_number(&array_text(items)),
    }
}

/// `Number(x) || 0`
fn count(tokens: &Fields, key: &str) -> Value {
    let number = to_number(tokens.get(key));
    number_value(if number.is_nan() || number == 0.0 { 0.0 } else { number })
}

fn object(pairs: Vec<(&str, Value)>) -> Fields {
    pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect()
}

/// What `start` stores: the agent is the caller, else the agent field.
pub fn started(run_id: &str, post: &Fields, started_at: &str) -> Fields {
    let caller = text(post, "caller");
    let agent = if caller.is_null() { text(post, "agent") } else { caller.clone() };
    let prompt = match text(post, "prompt") {
        Value::Null => Value::from(""),
        prompt => prompt,
    };
    object(vec![
        ("runId", Value::from(run_id)),
        ("prompt", prompt),
        ("agent", agent),
        ("caller", caller),
        ("claudeSessionId", text(post, "claudeSessionId")),
        ("model", text(post, "model")),
        ("startedAt", Value::from(started_at)),
        ("finishedAt", Value::Null),
        ("status", Value::from("running")),
        ("ok", Value::Null),
        ("tokens", Value::Null),
        ("cost", Value::Null),
        ("durationMs", Value::Null),
    ])
}

/// What `finish` stores over the run's meta; a run that never started gets the stub of its first five keys. The model of the post
/// replaces the stored one only when it is a string, and `tokens` is null unless the post's is an object or an array.
pub fn finished(run_id: &str, existing: Option<Fields>, post: &Fields, now: &str) -> Fields {
    let mut meta = existing.unwrap_or_else(|| {
        object(vec![
            ("runId", Value::from(run_id)),
            ("prompt", Value::from("")),
            ("agent", Value::Null),
            ("model", Value::Null),
            ("startedAt", Value::from(now)),
        ])
    });
    if let Some(model) = post.get("model").filter(|value| value.is_string()) {
        meta.insert("model".to_string(), model.clone());
    }
    let tokens = match post.get("tokens") {
        Some(Value::Object(tokens)) => Some(tokens.clone()),
        Some(Value::Array(_)) => Some(Fields::new()),
        _ => None,
    };
    let tokens = tokens.map_or(Value::Null, |tokens| Value::Object(object(vec![("total", count(&tokens, "total")), ("input", count(&tokens, "input")), ("output", count(&tokens, "output"))])));
    let ok = post.get("ok").filter(|value| value.is_boolean()).cloned().unwrap_or(Value::Null);
    for (key, value) in [
        ("finishedAt", Value::from(now)),
        ("status", Value::from("finished")),
        ("ok", ok),
        ("tokens", tokens),
        ("cost", number_or_null(post, "cost")),
        ("durationMs", number_or_null(post, "durationMs")),
    ] {
        meta.insert(key.to_string(), value);
    }
    meta
}

/// The post's `lines` that are strings; anything else, including a missing list, stores nothing.
pub fn string_lines(post: &Fields) -> Vec<String> {
    match post.get("lines") {
        Some(Value::Array(items)) => items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(value: Value) -> Fields {
        value.as_object().cloned().expect("an object")
    }

    fn keys(meta: &Fields) -> Vec<&str> {
        meta.keys().map(String::as_str).collect()
    }

    #[test]
    fn given_wfs_start_post_when_started_then_the_agent_is_the_caller_and_the_keys_keep_nodes_order() {
        let post = record(json!({ "prompt": "p", "caller": "implementer", "agent": "researcher", "claudeSessionId": "s", "model": "m" }));

        let meta = started("run-1", &post, "2026-10-22T10:00:00.000Z");

        assert_eq!(
            keys(&meta),
            ["runId", "prompt", "agent", "caller", "claudeSessionId", "model", "startedAt", "finishedAt", "status", "ok", "tokens", "cost", "durationMs"]
        );
        assert_eq!((&meta["agent"], &meta["caller"], &meta["status"], &meta["finishedAt"]), (&json!("implementer"), &json!("implementer"), &json!("running"), &Value::Null));
    }

    #[test]
    fn given_a_start_post_of_the_wrong_types_when_started_then_the_prompt_is_empty_and_the_rest_null() {
        let post = record(json!({ "prompt": 5, "caller": 7, "agent": true, "claudeSessionId": {}, "model": ["m"] }));

        let meta = started("r", &post, "t");

        assert_eq!((&meta["prompt"], &meta["agent"], &meta["caller"], &meta["claudeSessionId"], &meta["model"]), (&json!(""), &Value::Null, &Value::Null, &Value::Null, &Value::Null));
        assert_eq!(started("r", &record(json!({ "agent": "researcher" })), "t")["agent"], json!("researcher"));
    }

    #[test]
    fn given_a_finish_with_no_start_when_finished_then_the_stub_comes_first_and_the_fields_coerce_as_javascript_does() {
        let post = record(json!({ "ok": "yes", "model": "m2", "tokens": { "total": "120", "output": true, "input": [5] }, "cost": "1", "durationMs": 12.5 }));

        let meta = finished("run-x", None, &post, "2026-10-22T10:00:00.000Z");

        assert_eq!(keys(&meta), ["runId", "prompt", "agent", "model", "startedAt", "finishedAt", "status", "ok", "tokens", "cost", "durationMs"]);
        assert_eq!((&meta["model"], &meta["ok"], &meta["cost"], &meta["durationMs"]), (&json!("m2"), &Value::Null, &Value::Null, &json!(12.5)));
        assert_eq!(meta["tokens"], json!({ "total": 120, "input": 5, "output": 1 }));
    }

    #[test]
    fn given_token_counts_javascript_reads_oddly_when_finished_then_each_is_number_or_zero() {
        let tokens = |post: Value| finished("r", None, &record(json!({ "tokens": post })), "t")["tokens"].clone();

        assert_eq!(tokens(json!({ "total": " 12 ", "input": "0x10", "output": [5] })), json!({ "total": 12, "input": 16, "output": 5 }));
        assert_eq!(tokens(json!({ "total": "1e400", "input": null, "output": "12abc" })), json!({ "total": null, "input": 0, "output": 0 }));
        assert_eq!(tokens(json!([7])), json!({ "total": 0, "input": 0, "output": 0 }));
        assert_eq!(tokens(json!("5")), Value::Null);
        assert_eq!(tokens(Value::Null), Value::Null);
    }

    #[test]
    fn given_a_started_run_when_finished_then_its_keys_and_model_stay_and_only_the_finish_fields_change() {
        let first = started("run-1", &record(json!({ "prompt": "p", "caller": "c", "claudeSessionId": "s", "model": "m1" })), "t0");

        let meta = finished("run-1", Some(first), &record(json!({ "ok": true, "cost": 0.0123, "durationMs": 4567 })), "t1");

        assert_eq!((&meta["model"], &meta["claudeSessionId"], &meta["startedAt"], &meta["finishedAt"]), (&json!("m1"), &json!("s"), &json!("t0"), &json!("t1")));
        assert_eq!((&meta["status"], &meta["ok"], &meta["cost"], &meta["durationMs"], &meta["tokens"]), (&json!("finished"), &json!(true), &json!(0.0123), &json!(4567), &Value::Null));
        assert_eq!(keys(&meta)[..6], ["runId", "prompt", "agent", "caller", "claudeSessionId", "model"]);
    }

    #[test]
    fn given_a_lines_post_when_read_then_only_the_strings_of_a_list_are_kept() {
        assert_eq!(string_lines(&record(json!({ "lines": ["a", 5, null, { "a": 1 }, "b"] }))), ["a", "b"]);
        assert_eq!(string_lines(&record(json!({ "lines": "x" }))), Vec::<String>::new());
        assert_eq!(string_lines(&record(json!({}))), Vec::<String>::new());
    }
}
