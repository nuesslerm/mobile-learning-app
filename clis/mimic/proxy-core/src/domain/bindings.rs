//! A candidate hands out credentials and ids of its own. Where a recorded answer holds a fixture value or an id and the candidate's
//! answer holds another value at the same place, the two are bound, and later requests carry the candidate's value.
use super::sanitize::FIXTURE;
use serde_json::Value;

#[derive(Default)]
pub struct Bindings {
    pairs: Vec<(String, String)>,
}

fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36 && bytes.iter().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { *b == b'-' } else { b.is_ascii_hexdigit() })
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '~' | '+' | '/' | '-'))).filter(|t| !t.is_empty()).collect()
}

fn nested(text: &str) -> Option<Value> {
    let trimmed = text.trim_start();
    (trimmed.starts_with('{') || trimmed.starts_with('[')).then(|| serde_json::from_str(text).ok()).flatten()
}

impl Bindings {
    pub fn new(known: Vec<(String, String)>) -> Bindings {
        Bindings { pairs: known }
    }

    fn bind(&mut self, recorded: &str, live: &str) {
        if recorded != live && !self.pairs.iter().any(|(have, _)| have == recorded) {
            self.pairs.push((recorded.to_string(), live.to_string()));
            self.pairs.sort_by_key(|(recorded, _)| std::cmp::Reverse(recorded.len()));
        }
    }

    /// Reads a recorded answer and the candidate's answer side by side and binds what stands at the same place.
    pub fn learn(&mut self, recorded: &Value, live: &Value) {
        match (recorded, live) {
            (Value::Object(a), Value::Object(b)) => a.iter().filter_map(|(key, item)| b.get(key).map(|other| (item, other))).for_each(|(item, other)| self.learn(item, other)),
            (Value::Array(a), Value::Array(b)) => a.iter().zip(b).for_each(|(item, other)| self.learn(item, other)),
            (Value::String(a), Value::String(b)) => match (nested(a), nested(b)) {
                (Some(a), Some(b)) => self.learn(&a, &b),
                _ => {
                    let (ours, theirs) = (tokens(a), tokens(b));
                    if ours.len() == theirs.len() {
                        for (a, b) in ours.iter().zip(&theirs).filter(|(a, b)| a.starts_with(FIXTURE) || (is_uuid(a) && is_uuid(b))) {
                            self.bind(a, b);
                        }
                    }
                }
            },
            _ => {}
        }
    }

    /// A recorded request as the candidate must get it: every bound value replaced by the candidate's.
    pub fn text(&self, text: &str) -> String {
        self.pairs.iter().fold(text.to_string(), |out, (recorded, live)| out.replace(recorded.as_str(), live))
    }

    pub fn value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Array(items) => Value::Array(items.iter().map(|item| self.value(item)).collect()),
            Value::Object(fields) => Value::Object(fields.iter().map(|(key, item)| (key.clone(), self.value(item))).collect()),
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_a_recorded_answer_and_a_candidates_answer_when_read_side_by_side_then_later_requests_carry_the_candidates_values() {
        // Given
        let recorded = json!({"body": r#"{"access_token":"fx-access-token-1","thread":"6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11","name":"fixture"}"#, "headers": [["set-cookie", "sid=fx-cookie-1; Path=/"]]});
        let live = json!({"body": r#"{"access_token":"live-token","thread":"0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c","name":"other"}"#, "headers": [["set-cookie", "sid=live-cookie; Path=/"]]});
        let mut bindings = Bindings::new(vec![("fx-subject-token-1".into(), "a-pairing-code".into())]);

        // When
        bindings.learn(&recorded, &live);
        let sent = bindings.value(&json!({"path": "/threads/6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11", "headers": [["authorization", "Bearer fx-access-token-1"], ["cookie", "sid=fx-cookie-1"]], "body": "subject_token=fx-subject-token-1&name=fixture"}));

        // Then
        assert_eq!(sent, json!({"path": "/threads/0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c", "headers": [["authorization", "Bearer live-token"], ["cookie", "sid=live-cookie"]], "body": "subject_token=a-pairing-code&name=fixture"}));
    }
}
