//! What a replay says about a request the recording does not have: what was asked, the closest recording and where it is in
//! the cassette. Every value in it is a normalised form, so a live credential cannot ride on a failure.
use super::sanitize::without_tokens;
use mimic_core::allowlist::Allowlist;
use mimic_core::differ::{diff_json, Diff, DiffOptions};
use serde::Serialize;
use serde_json::Value;

const MAX_DIFFS: usize = 20;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Nearest {
    pub interaction: String,
    /// `interactions.jsonl:<line>`.
    pub cassette: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    pub diff: Vec<Diff>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Mismatch {
    pub protocol: &'static str,
    /// The route of an HTTP request or the method of a socket request.
    pub what: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    pub reason: String,
    pub received: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nearest: Option<Nearest>,
}

fn clean(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(without_tokens(text)),
        Value::Array(items) => Value::Array(items.iter().map(clean).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(k, v)| (k.clone(), clean(v))).collect()),
        other => other.clone(),
    }
}

pub fn location(line: usize) -> String {
    format!("interactions.jsonl:{line}")
}

pub fn nearest(interaction: &str, line: usize, step: Option<usize>, expected: &Value, actual: &Value) -> Nearest {
    let mut diff = diff_json(&clean(expected), &clean(actual), &Allowlist::default(), &DiffOptions::default());
    diff.truncate(MAX_DIFFS);
    Nearest { interaction: interaction.to_string(), cassette: location(line), step, diff }
}

impl Mismatch {
    pub fn new(protocol: &'static str, what: impl Into<String>, connection: Option<String>, reason: impl Into<String>, received: &Value, nearest: Option<Nearest>) -> Mismatch {
        Mismatch { protocol, what: without_tokens(&what.into()), connection, reason: reason.into(), received: clean(received), nearest }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_a_request_that_differs_in_one_field_when_reported_then_the_diff_names_the_field_and_the_cassette_line() {
        // Given
        let expected = json!({"tag": "git.diff", "payload": {"path": "a.rs"}});
        let actual = json!({"tag": "git.diff", "payload": {"path": "b.rs", "note": "Bearer abcdefgh12345678"}});

        // When
        let near = nearest("c01", 3, Some(12), &expected, &actual);
        let miss = Mismatch::new("ws", "git.diff", Some("c2".into()), "no recorded request of this method has this payload", &actual, Some(near));

        // Then
        let text = serde_json::to_string(&miss).unwrap();
        assert!(text.contains("interactions.jsonl:3") && text.contains("payload.path"), "{text}");
        assert!(!text.contains("abcdefgh12345678"), "{text}");
    }
}
