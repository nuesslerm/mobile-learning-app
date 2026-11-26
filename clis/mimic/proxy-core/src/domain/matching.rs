//! Tells whether a live request is one the recording has. Two tiers: equal once credentials are masked, then equal once the
//! set's rules have masked what varies between runs (ids a client makes up, timestamps).
use super::sanitize::without_machine_values;
use mimic_core::json::{canonical, canonical_string};
use mimic_core::rules::{Compiled, Normalizer, Rule, RuleSet, RulesError};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const SECRET: &str = "<secret>";

#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub strict: String,
    pub loose: String,
    /// The loose form as a value: what a report may show and a diff compares.
    pub shown: Value,
}

/// What a set allows to differ between its recording and a live request, beside what its rules mask.
#[derive(Clone, Debug, Default)]
pub struct Tolerance {
    /// Keys a client sends on some runs and not on others: taken out of both sides before they are compared.
    pub ignore_keys: Vec<String>,
    /// Values of this machine a client may still hold from the run that was recorded, and what stands for each in the recording.
    pub machine: Vec<(String, String)>,
}

pub struct Keys {
    strict: Compiled,
    loose: Compiled,
    tolerance: Tolerance,
}

fn body_value(nz: &mut Normalizer, body: &str, content_type: &str) -> Value {
    if body.is_empty() {
        return Value::Null;
    }
    match serde_json::from_str::<Value>(body) {
        // sorted keys first: the rules number ids in the order they meet them, which must not depend on how a client orders its keys
        Ok(value) => nz.json(&canonical(&value), None),
        Err(_) => nz.body(body, content_type),
    }
}

impl Tolerance {
    fn text(&self, text: &str) -> String {
        if self.machine.is_empty() { text.to_string() } else { without_machine_values(text, &self.machine) }
    }

    fn value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Array(items) => Value::Array(items.iter().map(|item| self.value(item)).collect()),
            Value::Object(fields) => Value::Object(fields.iter().filter(|(key, _)| !self.ignore_keys.contains(key)).map(|(key, item)| (key.clone(), self.value(item))).collect()),
            other => other.clone(),
        }
    }
}

impl Keys {
    pub fn new(rules: &RuleSet, secret_keys: &[String], tolerance: Tolerance) -> Result<Keys, RulesError> {
        let mut keys: BTreeMap<String, String> = secret_keys.iter().map(|k| (k.clone(), SECRET.to_string())).collect();
        for rule in &rules.rules {
            if let Rule::SecretKey { keys: named } = rule {
                keys.extend(named.keys().map(|k| (k.clone(), SECRET.to_string())));
            }
        }
        let secret = Rule::SecretKey { keys };
        let strict = RuleSet::new(vec![secret.clone()]).compile()?;
        let loose = RuleSet::new(rules.rules.iter().cloned().chain([secret]).collect()).compile()?;
        Ok(Keys { strict, loose, tolerance })
    }

    fn both(&self, form: impl Fn(&mut Normalizer) -> Value) -> Key {
        let strict = form(&mut self.strict.normalizer());
        let loose = form(&mut self.loose.normalizer());
        Key { strict: canonical_string(&strict), loose: canonical_string(&loose), shown: loose }
    }

    pub fn http(&self, method: &str, target: &str, body: &str, content_type: &str) -> Key {
        let (target, body) = (self.tolerance.text(target), self.tolerance.text(body));
        let tolerated = |value: Value| self.tolerance.value(&value);
        self.both(|nz| json!({"method": method.to_uppercase(), "path": nz.path(&target), "body": tolerated(body_value(nz, &body, content_type))}))
    }

    pub fn target(&self, target: &str) -> Key {
        let target = self.tolerance.text(target);
        self.both(|nz| json!(nz.path(&target)))
    }

    pub fn frame(&self, frame: &Value) -> Key {
        let frame = canonical(&self.tolerance.value(frame));
        self.both(|nz| nz.json(&frame, None))
    }
}

/// A recording that could answer: where it is, its two forms and whether it already answered once.
pub struct Candidate<'a> {
    pub at: usize,
    pub key: &'a Key,
    pub used: bool,
}

/// The first unused recording that is equal, strictly before loosely; when every equal one is used, the first of them again.
pub fn pick(candidates: &[Candidate], key: &Key) -> Option<(usize, bool)> {
    for used in [false, true] {
        for strict in [true, false] {
            let equal = |c: &&Candidate| c.used == used && if strict { c.key.strict == key.strict } else { c.key.loose == key.loose };
            if let Some(found) = candidates.iter().find(equal) {
                return Some((found.at, used));
            }
        }
    }
    None
}

/// The envelopes of one socket message: a message holds one value or an array of them.
pub fn envelopes(message: &Value) -> Vec<&Value> {
    match message {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    }
}

pub fn id_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// The id a client's envelope asks under: its own `id`, when it does not answer another request.
pub fn request_id(envelope: &Value) -> Option<String> {
    let fields = envelope.as_object()?;
    if fields.contains_key("requestId") {
        return None;
    }
    fields.get("id").and_then(id_text)
}

/// The request an envelope belongs to: the `requestId` it names, else its `id`.
pub fn reply_id(envelope: &Value) -> Option<String> {
    let fields = envelope.as_object()?;
    fields.get("requestId").or_else(|| fields.get("id")).and_then(id_text)
}

pub fn tag(envelope: &Value) -> &str {
    envelope["_tag"].as_str().unwrap_or_default()
}

/// What a request asks for: the `tag` of an orchestration request or the `method` of a JSON-RPC one.
pub fn method(envelope: &Value) -> String {
    envelope["tag"].as_str().or_else(|| envelope["method"].as_str()).unwrap_or_default().to_string()
}

pub fn without_id(envelope: &Value) -> Value {
    let mut copy = envelope.clone();
    if let Some(fields) = copy.as_object_mut() {
        fields.remove("id");
    }
    copy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::sanitize::SECRET_KEYS;

    fn keys() -> Keys {
        let secret: Vec<String> = SECRET_KEYS.iter().map(|s| s.to_string()).collect();
        Keys::new(&RuleSet::new(vec![Rule::UuidOrdinal]), &secret, Tolerance::default()).expect("rules compile")
    }

    #[test]
    fn given_a_key_the_set_ignores_and_a_path_of_this_machine_when_keyed_then_the_live_request_equals_the_recording() {
        // Given
        let tolerance = Tolerance { ignore_keys: vec!["httpSnapshotFallback".into()], machine: vec![("/Users/jane".into(), "/fixture/home".into())] };
        let keys = Keys::new(&RuleSet::new(Vec::new()), &[], tolerance).expect("rules compile");

        // When
        let recorded = keys.frame(&json!({"tag": "subscribe", "payload": {"cwd": "/fixture/home/repo"}}));
        let live = keys.frame(&json!({"tag": "subscribe", "payload": {"cwd": "/Users/jane/repo", "httpSnapshotFallback": true}}));
        let other = keys.frame(&json!({"tag": "subscribe", "payload": {"cwd": "/Users/jane/other"}}));

        // Then
        assert_eq!(recorded.strict, live.strict);
        assert_ne!(recorded.strict, other.strict);
    }

    #[test]
    fn given_a_live_credential_and_its_fixture_value_when_keyed_then_the_two_requests_are_strictly_equal() {
        // Given
        let keys = keys();

        // When
        let recorded = keys.http("post", "/ws?wsTicket=fx-ticket-1", r#"{"access_token":"fx-access-token-1","thread":"a"}"#, "application/json");
        let live = keys.http("POST", "/ws?wsTicket=live-ticket-value", r#"{"access_token":"live-token-value","thread":"a"}"#, "application/json");
        let other = keys.http("POST", "/ws?wsTicket=live-ticket-value", r#"{"access_token":"live-token-value","thread":"b"}"#, "application/json");

        // Then
        assert_eq!(recorded.strict, live.strict);
        assert_ne!(recorded.strict, other.strict);
    }

    #[test]
    fn given_an_id_the_client_makes_up_when_keyed_then_only_the_loose_form_is_equal() {
        // Given
        let keys = keys();

        // When
        let recorded = keys.frame(&json!({"tag": "dispatch", "payload": {"commandId": "6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11"}}));
        let live = keys.frame(&json!({"tag": "dispatch", "payload": {"commandId": "0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c"}}));

        // Then
        assert_ne!(recorded.strict, live.strict);
        assert_eq!(recorded.loose, live.loose);
    }

    #[test]
    fn given_the_same_request_with_its_keys_in_another_order_when_keyed_then_the_ids_it_makes_up_are_numbered_alike() {
        // Given
        let keys = keys();
        let recorded: Value = serde_json::from_str(r#"{"clientId":"mobile-6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11","environmentId":"0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c","at":1}"#).unwrap();
        let live: Value = serde_json::from_str(r#"{"environmentId":"0b7c2f4d-1a3e-4f5a-9b6c-7d8e9f0a1b2c","at":2,"clientId":"mobile-6f1e6a2e-9d0b-4d7c-8a51-0c2a4b9f1e11"}"#).unwrap();

        // When
        let (recorded, live) = (keys.frame(&recorded), keys.frame(&live));

        // Then
        assert_ne!(recorded.strict, live.strict);
        assert_eq!(recorded.shown["clientId"], live.shown["clientId"]);
        assert_eq!(recorded.shown["environmentId"], live.shown["environmentId"]);
    }

    #[test]
    fn given_two_equal_recordings_when_picked_three_times_then_each_answers_once_and_the_first_answers_again() {
        // Given
        let keys = keys();
        let key = keys.frame(&json!({"tag": "subscribe"}));
        let pick_with = |used: [bool; 2]| pick(&[Candidate { at: 4, key: &key, used: used[0] }, Candidate { at: 9, key: &key, used: used[1] }], &key);

        // When / Then
        assert_eq!(pick_with([false, false]), Some((4, false)));
        assert_eq!(pick_with([true, false]), Some((9, false)));
        assert_eq!(pick_with([true, true]), Some((4, true)));
    }

    #[test]
    fn given_envelopes_of_both_dialects_when_read_then_a_request_and_its_reply_name_the_same_id() {
        // Given
        let ask = json!({"_tag": "Request", "id": "7", "tag": "git.diff"});
        let chunk = json!({"_tag": "Chunk", "requestId": "7"});
        let rpc = json!({"jsonrpc": "2.0", "id": 3, "method": "ping"});

        // When / Then
        assert_eq!((request_id(&ask), reply_id(&chunk), method(&ask)), (Some("7".into()), Some("7".into()), "git.diff".into()));
        assert_eq!((request_id(&chunk), request_id(&rpc), method(&rpc)), (None, Some("3".into()), "ping".into()));
        assert_eq!(without_id(&ask), json!({"_tag": "Request", "tag": "git.diff"}));
    }
}
