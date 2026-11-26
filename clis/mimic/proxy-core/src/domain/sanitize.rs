//! Keeps credentials off the disk. Every credential becomes a fixture value (`fx-access-token-1`), the same one wherever that
//! credential appears, so a replay keeps the relationship between a grant, the token it bought and the request that carries it.
use mimic_core::cassette::{Interaction, StepKind};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub const FIXTURE: &str = "fx-";
/// A shorter value is only replaced where a key, header or query name says it is a credential: anywhere else it is too likely a coincidence.
const MIN_ANYWHERE: usize = 6;
const MAX_NESTING: usize = 4;

pub const SECRET_KEYS: &[&str] = &[
    "access_token", "refresh_token", "id_token", "subject_token", "actor_token", "client_secret", "code_verifier", "password", "credential",
    "token", "ticket", "wsTicket", "csrfToken", "sessionToken", "session_token", "pairingCode", "pairing_code", "pairingToken", "pairing_token",
    "apiKey", "api_key",
];
pub const SECRET_HEADERS: &[&str] = &["x-api-key", "x-csrf-token", "x-auth-token"];

#[derive(Clone, Debug)]
pub struct Policy {
    /// Names of JSON keys, form keys and query parameters whose value is a credential; `_`, `-` and case do not matter.
    pub secret_keys: Vec<String>,
    /// Headers whose whole value is a credential, beside Authorization and the cookies.
    pub secret_headers: Vec<String>,
    /// Values the caller knows must never be written, by a name of the caller's.
    pub declared: Vec<(String, String)>,
    /// Values of this machine (home, temp directory, host name) and what stands for each.
    pub masks: Vec<(String, String)>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            secret_keys: SECRET_KEYS.iter().map(|s| s.to_string()).collect(),
            secret_headers: SECRET_HEADERS.iter().map(|s| s.to_string()).collect(),
            declared: Vec::new(),
            masks: Vec::new(),
        }
    }
}

fn fold(key: &str) -> String {
    key.chars().filter(|c| !matches!(c, '_' | '-')).flat_map(char::to_lowercase).collect()
}

fn slug(key: &str) -> String {
    let mut out = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() && !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        out.push(if c == '_' { '-' } else { c.to_ascii_lowercase() });
    }
    out
}

impl Policy {
    pub fn secret(&self, key: &str) -> bool {
        let key = fold(key);
        self.secret_keys.iter().any(|s| fold(s) == key)
    }

    fn secret_header(&self, name: &str) -> bool {
        self.secret_headers.iter().any(|h| h.eq_ignore_ascii_case(name)) || self.secret(name)
    }
}

/// Why a run cannot be written. It names where, never the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Leak {
    Encoded { interaction: String, step: usize },
    Value { interaction: String, step: usize, kind: String },
    Shape { interaction: String, step: usize, shape: &'static str },
}

impl std::fmt::Display for Leak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Leak::Encoded { interaction, step } => write!(f, "{interaction} step {step} carries an encoded body, which cannot be read for credentials"),
            Leak::Value { interaction, step, kind } => write!(f, "{interaction} step {step} still holds a credential ({kind}) after sanitising"),
            Leak::Shape { interaction, step, shape } => write!(f, "{interaction} step {step} still holds {shape} after sanitising"),
        }
    }
}

impl std::error::Error for Leak {}

#[derive(Default)]
struct Fixtures {
    by_value: HashMap<String, String>,
    order: Vec<String>,
    counts: BTreeMap<String, usize>,
}

impl Fixtures {
    fn learn(&mut self, value: &str, kind: &str) {
        if value.is_empty() || value.starts_with(FIXTURE) || self.by_value.contains_key(value) {
            return;
        }
        let n = self.counts.entry(kind.to_string()).or_default();
        *n += 1;
        self.by_value.insert(value.to_string(), format!("{FIXTURE}{kind}-{n}"));
        self.order.push(value.to_string());
    }

    fn get(&self, value: &str) -> Option<&String> {
        self.by_value.get(value)
    }
}

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '~' | '+' | '/' | '=')
}

fn bearer_tokens(text: &str) -> Vec<&str> {
    let (mut out, mut rest) = (Vec::new(), text);
    while let Some(i) = rest.find("Bearer ") {
        let tail = &rest[i + 7..];
        let end = tail.find(|c: char| !token_char(c)).unwrap_or(tail.len());
        if end >= 8 {
            out.push(&tail[..end]);
        }
        rest = &tail[end..];
    }
    out
}

fn jwts(text: &str) -> Vec<&str> {
    let (mut out, mut rest) = (Vec::new(), text);
    while let Some(i) = rest.find("eyJ") {
        let tail = &rest[i..];
        let end = tail.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))).unwrap_or(tail.len());
        let token = tail[..end].trim_end_matches('.');
        if token.len() >= 13 {
            out.push(token);
        }
        rest = &tail[end.max(3)..];
    }
    out
}

/// A token-shaped value: the same two shapes the publish scan of `wf mimic` looks for.
pub fn token_shape(text: &str) -> Option<&'static str> {
    let mut rest = text;
    while let Some(i) = rest.find("Bearer ") {
        let tail = &rest[i + 7..];
        if tail.bytes().take(8).filter(u8::is_ascii_alphanumeric).count() == 8 {
            return Some("a Bearer token");
        }
        rest = tail;
    }
    let mut rest = text;
    while let Some(i) = rest.find("eyJ") {
        let tail = &rest[i + 3..];
        if tail.bytes().take(10).filter(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-').count() == 10 {
            return Some("a JWT");
        }
        rest = tail;
    }
    None
}

/// Text for a report with every token-shaped value taken out.
pub fn without_tokens(text: &str) -> String {
    let mut found: Vec<&str> = bearer_tokens(text).into_iter().chain(jwts(text)).filter(|t| !t.starts_with(FIXTURE)).collect();
    found.sort_by_key(|t| std::cmp::Reverse(t.len()));
    found.iter().fold(text.to_string(), |out, token| out.replace(token, "<redacted>"))
}

fn percent_decode(text: &str) -> String {
    let (bytes, mut out, mut i) = (text.as_bytes(), Vec::new(), 0);
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| text.get(i + 1..i + 3)).flatten().and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn json_escape(text: &str) -> String {
    let quoted = serde_json::to_string(text).unwrap_or_default();
    quoted.trim_matches('"').to_string()
}

fn credential(value: &str) -> &str {
    match value.trim().split_once(char::is_whitespace) {
        Some((_, rest)) if !rest.trim().is_empty() => rest.trim(),
        _ => value.trim(),
    }
}

fn nested(text: &str) -> Option<Value> {
    let trimmed = text.trim_start();
    (trimmed.starts_with('{') || trimmed.starts_with('[')).then(|| serde_json::from_str(text).ok()).flatten()
}

fn form_pairs(text: &str) -> Option<Vec<(&str, &str)>> {
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    text.split('&').map(|pair| pair.split_once('=')).collect()
}

fn header_pair(pair: &Value) -> Option<(&str, &str)> {
    let pair = pair.as_array().filter(|p| p.len() == 2)?;
    Some((pair[0].as_str()?, pair[1].as_str()?))
}

fn learn_headers(headers: &Value, policy: &Policy, fx: &mut Fixtures) {
    for (name, value) in headers.as_array().into_iter().flatten().filter_map(header_pair) {
        match name.to_lowercase().as_str() {
            "authorization" | "proxy-authorization" => fx.learn(credential(value), "authorization"),
            "cookie" => value.split(';').filter_map(|part| part.split_once('=')).for_each(|(_, v)| fx.learn(v.trim(), "cookie")),
            "set-cookie" => {
                if let Some((_, v)) = value.split(';').next().and_then(|first| first.split_once('=')) {
                    fx.learn(v.trim(), "cookie");
                }
            }
            lower if policy.secret_header(lower) => fx.learn(value.trim(), &slug(lower)),
            _ => {}
        }
    }
}

fn learn_pairs(pairs: &[(&str, &str)], policy: &Policy, fx: &mut Fixtures) {
    for (name, value) in pairs.iter().filter(|(name, _)| policy.secret(name)) {
        fx.learn(value, &slug(name));
        fx.learn(&percent_decode(value), &slug(name));
    }
}

fn learn(value: &Value, policy: &Policy, fx: &mut Fixtures, depth: usize) {
    match value {
        Value::Object(fields) => {
            for (key, item) in fields {
                match (key.as_str(), item) {
                    ("headers", Value::Array(_)) => learn_headers(item, policy, fx),
                    ("path", Value::String(path)) => {
                        if let Some(pairs) = path.split_once('?').and_then(|(_, query)| form_pairs(query)) {
                            learn_pairs(&pairs, policy, fx);
                        }
                    }
                    ("body", Value::String(body)) if nested(body).is_none() => {
                        if let Some(pairs) = form_pairs(body) {
                            learn_pairs(&pairs, policy, fx);
                        }
                    }
                    _ => {}
                }
                if policy.secret(key) {
                    match item {
                        Value::String(s) => fx.learn(s, &slug(key)),
                        Value::Array(items) => items.iter().filter_map(Value::as_str).for_each(|s| fx.learn(s, &slug(key))),
                        _ => {}
                    }
                }
                learn(item, policy, fx, depth);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| learn(item, policy, fx, depth)),
        Value::String(text) => {
            if depth < MAX_NESTING {
                if let Some(inner) = nested(text) {
                    learn(&inner, policy, fx, depth + 1);
                }
            }
            bearer_tokens(text).into_iter().for_each(|t| fx.learn(t, "bearer"));
            jwts(text).into_iter().for_each(|t| fx.learn(t, "jwt"));
        }
        _ => {}
    }
}

struct Table(Vec<(String, String)>);

impl Table {
    fn of(fx: &Fixtures, masks: &[(String, String)]) -> Table {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for value in fx.order.iter().filter(|v| v.len() >= MIN_ANYWHERE) {
            let fixture = fx.by_value[value].clone();
            for form in [value.clone(), json_escape(value), percent_encode(value)] {
                if !pairs.iter().any(|(have, _)| *have == form) {
                    pairs.push((form, fixture.clone()));
                }
            }
        }
        pairs.extend(masks.iter().filter(|(value, _)| !value.is_empty()).cloned());
        pairs.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
        Table(pairs)
    }

    fn swap(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (from, to) in &self.0 {
            if out.contains(from.as_str()) {
                out = out.replace(from.as_str(), to);
            }
        }
        out
    }

    fn rewrite(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.swap(text),
            Value::Array(items) => items.iter_mut().for_each(|item| self.rewrite(item)),
            Value::Object(fields) => fields.values_mut().for_each(|item| self.rewrite(item)),
            _ => {}
        }
    }
}

fn fix_header(name: &str, value: &str, policy: &Policy, fx: &Fixtures) -> Option<String> {
    match name.to_lowercase().as_str() {
        "authorization" | "proxy-authorization" => {
            let fixture = fx.get(credential(value))?;
            Some(match value.trim().split_once(char::is_whitespace) {
                Some((scheme, rest)) if !rest.trim().is_empty() => format!("{scheme} {fixture}"),
                _ => fixture.clone(),
            })
        }
        "cookie" | "set-cookie" => {
            let only_first = name.eq_ignore_ascii_case("set-cookie");
            let mut changed = false;
            let parts: Vec<String> = value
                .split(';')
                .enumerate()
                .map(|(i, part)| match part.split_once('=').filter(|_| i == 0 || !only_first).and_then(|(k, v)| fx.get(v.trim()).map(|f| (k, f))) {
                    Some((k, fixture)) => {
                        changed = true;
                        format!("{k}={fixture}")
                    }
                    None => part.to_string(),
                })
                .collect();
            changed.then(|| parts.join(";"))
        }
        lower if policy.secret_header(lower) => fx.get(value.trim()).cloned(),
        _ => None,
    }
}

fn fix_pairs(text: &str, policy: &Policy, fx: &Fixtures) -> Option<String> {
    let pairs = form_pairs(text)?;
    let mut changed = false;
    let rebuilt: Vec<String> = pairs
        .iter()
        .map(|(name, value)| match policy.secret(name).then(|| fx.get(value).or_else(|| fx.get(&percent_decode(value)))).flatten() {
            Some(fixture) => {
                changed = true;
                format!("{name}={fixture}")
            }
            None => format!("{name}={value}"),
        })
        .collect();
    changed.then(|| rebuilt.join("&"))
}

/// Replaces what a name marks as a credential, whatever its length. Long values are already gone: this is for the short ones.
fn fix(value: &mut Value, policy: &Policy, fx: &Fixtures, depth: usize) {
    match value {
        Value::Object(fields) => {
            for (key, item) in fields.iter_mut() {
                match (key.as_str(), &mut *item) {
                    ("headers", Value::Array(pairs)) => {
                        for pair in pairs.iter_mut() {
                            let fixed = header_pair(pair).and_then(|(name, value)| fix_header(name, value, policy, fx));
                            if let (Some(fixed), Some(slot)) = (fixed, pair.get_mut(1)) {
                                *slot = Value::String(fixed);
                            }
                        }
                    }
                    ("path", Value::String(path)) => {
                        let fixed = path.split_once('?').and_then(|(before, query)| fix_pairs(query, policy, fx).map(|q| format!("{before}?{q}")));
                        if let Some(fixed) = fixed {
                            *path = fixed;
                        }
                    }
                    ("body", Value::String(body)) if nested(body).is_none() => {
                        if let Some(fixed) = fix_pairs(body, policy, fx) {
                            *body = fixed;
                        }
                    }
                    _ => {}
                }
                if policy.secret(key) {
                    match item {
                        Value::String(s) => {
                            if let Some(fixture) = fx.get(s) {
                                *s = fixture.clone();
                            }
                        }
                        Value::Array(items) => {
                            for s in items.iter_mut() {
                                if let Some(fixture) = s.as_str().and_then(|s| fx.get(s)) {
                                    *s = Value::String(fixture.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                fix(item, policy, fx, depth);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| fix(item, policy, fx, depth)),
        Value::String(text) if depth < MAX_NESTING => {
            if let Some(mut inner) = nested(text) {
                let before = inner.clone();
                fix(&mut inner, policy, fx, depth + 1);
                if inner != before {
                    *text = inner.to_string();
                }
            }
        }
        _ => {}
    }
}

fn leaves(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push_str(text);
            out.push('\n');
        }
        Value::Array(items) => items.iter().for_each(|item| leaves(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| leaves(item, out)),
        _ => {}
    }
}

fn decoded(text: &str, out: &mut String) {
    use base64::Engine;
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(text) {
        out.push_str(&String::from_utf8_lossy(&bytes));
        out.push('\n');
    }
}

fn encoded(interactions: &[Interaction]) -> Result<(), Leak> {
    for it in interactions {
        for (n, step) in it.steps.iter().enumerate() {
            let has_body = !step.payload["body"].is_null() || !step.payload["body_b64"].is_null();
            let coded = step.payload["headers"].as_array().into_iter().flatten().filter_map(header_pair).any(|(name, value)| {
                name.eq_ignore_ascii_case("content-encoding") && !value.trim().is_empty() && !value.trim().eq_ignore_ascii_case("identity")
            });
            if has_body && coded {
                return Err(Leak::Encoded { interaction: it.id.clone(), step: n });
            }
        }
    }
    Ok(())
}

fn verify(interactions: &[Interaction], fx: &Fixtures) -> Result<(), Leak> {
    for it in interactions {
        for (n, step) in it.steps.iter().enumerate() {
            let mut text = String::new();
            leaves(&step.payload, &mut text);
            if step.kind == StepKind::Binary {
                decoded(step.payload.as_str().unwrap_or_default(), &mut text);
            }
            if let Some(body) = step.payload["body_b64"].as_str() {
                decoded(body, &mut text);
            }
            if let Some(value) = fx.order.iter().find(|v| v.len() >= MIN_ANYWHERE && text.contains(v.as_str())) {
                let kind = fx.by_value[value].trim_start_matches(FIXTURE).to_string();
                return Err(Leak::Value { interaction: it.id.clone(), step: n, kind });
            }
            if let Some(shape) = token_shape(&text) {
                return Err(Leak::Shape { interaction: it.id.clone(), step: n, shape });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sanitised {
    /// How many fixture values of each kind stand in for a credential.
    pub fixtures: BTreeMap<String, usize>,
}

/// Rewrites the interactions in place. On `Err` they are half rewritten and must be dropped, never written.
pub fn sanitise(interactions: &mut [Interaction], policy: &Policy) -> Result<Sanitised, Leak> {
    encoded(interactions)?;
    let mut fx = Fixtures::default();
    for (_, value) in &policy.declared {
        fx.learn(value, "declared");
    }
    for it in interactions.iter() {
        for step in &it.steps {
            learn(&step.payload, policy, &mut fx, 0);
        }
    }
    let table = Table::of(&fx, &policy.masks);
    for it in interactions.iter_mut() {
        it.row = it.row.take().map(|row| table.swap(&row));
        table.rewrite(&mut it.input);
        for step in &mut it.steps {
            if step.kind != StepKind::Binary {
                table.rewrite(&mut step.payload);
                fix(&mut step.payload, policy, &fx, 0);
            }
        }
    }
    verify(interactions, &fx)?;
    Ok(Sanitised { fixtures: fx.counts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::interactions::interactions;
    use crate::domain::traffic::Event;
    use mimic_core::cassette::Dir;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn request(exchange: u32, target: &str, head: &[(&str, &str)], body: &str) -> Event {
        Event::HttpRequest { exchange, method: "POST".into(), target: target.into(), headers: headers(head), body: body.as_bytes().to_vec() }
    }

    fn answer(exchange: u32, head: &[(&str, &str)], body: &str) -> Event {
        Event::HttpResponse { exchange, status: 200, headers: headers(head), body: body.as_bytes().to_vec() }
    }

    fn text(its: &[Interaction]) -> String {
        its.iter().map(|i| i.to_value().to_string()).collect()
    }

    #[test]
    fn given_a_grant_the_token_it_bought_and_the_requests_that_carry_it_when_sanitised_then_each_credential_has_one_fixture_value_everywhere() {
        // Given
        let events = vec![
            request(1, "/oauth/token", &[("content-type", "application/x-www-form-urlencoded")], "grant_type=pairing&subject_token=PAIRING-GRANT-CANARY"),
            answer(1, &[("set-cookie", "sid=COOKIE-CANARY-VALUE; Path=/")], r#"{"access_token":"ACCESS-TOKEN-CANARY","expires_in":3600}"#),
            request(2, "/api/auth/websocket-ticket/rpc", &[("authorization", "Bearer ACCESS-TOKEN-CANARY"), ("cookie", "sid=COOKIE-CANARY-VALUE")], ""),
            answer(2, &[], r#"{"ticket":"TICKET-CANARY-VALUE"}"#),
            Event::WsOpen { connection: 1, target: "/ws?wsTicket=TICKET-CANARY-VALUE".into(), status: 101 },
            Event::WsFrame { connection: 1, direction: Dir::S2c, opcode: 1, data: br#"{"note":"seen ACCESS-TOKEN-CANARY in a log line"}"#.to_vec() },
        ];
        let mut its = interactions(&events);

        // When
        let report = sanitise(&mut its, &Policy::default()).expect("sanitised");

        // Then
        let all = text(&its);
        assert!(!all.contains("CANARY"), "{all}");
        assert_eq!(its[0].steps[0].payload["body"], "grant_type=pairing&subject_token=fx-subject-token-1");
        assert_eq!(its[0].steps[1].payload["headers"][0][1], "sid=fx-cookie-1; Path=/");
        assert_eq!(its[1].steps[0].payload["headers"], serde_json::json!([["authorization", "Bearer fx-access-token-1"], ["cookie", "sid=fx-cookie-1"]]));
        assert_eq!(its[2].steps[0].payload["path"], "/ws?wsTicket=fx-ticket-1");
        assert!(its[2].steps[1].payload["note"].as_str().unwrap().contains("fx-access-token-1"));
        assert_eq!(report.fixtures["access-token"], 1);
    }

    #[test]
    fn given_a_short_pairing_code_when_sanitised_then_it_is_replaced_under_its_name_and_left_alone_elsewhere() {
        // Given
        let events = vec![request(1, "/pair", &[], r#"{"pairingCode":"4821","count":"4821 items"}"#), answer(1, &[], "{}")];
        let mut its = interactions(&events);

        // When
        sanitise(&mut its, &Policy::default()).expect("sanitised");

        // Then
        let body: Value = serde_json::from_str(its[0].steps[0].payload["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["pairingCode"], "fx-pairing-code-1");
        assert_eq!(body["count"], "4821 items");
    }

    #[test]
    fn given_a_token_under_a_name_nobody_listed_when_sanitised_then_its_shape_gives_it_away() {
        // Given
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJjYW5hcnkifQ.c2lnbmF0dXJl";
        let events = vec![request(1, "/x", &[], &format!(r#"{{"blob":"{jwt}","line":"sent Bearer abcdefgh12345678 upstream"}}"#)), answer(1, &[], "{}")];
        let mut its = interactions(&events);

        // When
        sanitise(&mut its, &Policy::default()).expect("sanitised");

        // Then
        let all = text(&its);
        assert!(!all.contains("eyJ") && !all.contains("abcdefgh12345678"), "{all}");
        assert!(all.contains("fx-jwt-1") && all.contains("Bearer fx-bearer-1"), "{all}");
    }

    #[test]
    fn given_a_declared_secret_and_a_home_directory_when_sanitised_then_neither_is_written() {
        // Given
        let events = vec![request(1, "/x", &[], r#"{"cwd":"/Users/someone/repo","env":"VAULT-VALUE-CANARY"}"#), answer(1, &[], "{}")];
        let mut its = interactions(&events);
        let policy = Policy { declared: vec![("VAULT_KEY".into(), "VAULT-VALUE-CANARY".into())], masks: vec![("/Users/someone".into(), "/fixture/home".into())], ..Policy::default() };

        // When
        sanitise(&mut its, &policy).expect("sanitised");

        // Then
        let all = text(&its);
        assert!(!all.contains("CANARY") && !all.contains("/Users/someone"), "{all}");
        assert!(all.contains("/fixture/home/repo") && all.contains("fx-declared-1"), "{all}");
    }

    #[test]
    fn given_a_compressed_body_when_sanitised_then_the_run_is_refused() {
        // Given
        let events = vec![request(1, "/x", &[], ""), Event::HttpResponse { exchange: 1, status: 200, headers: headers(&[("content-encoding", "gzip")]), body: vec![0x1f, 0x8b, 0x08] }];
        let mut its = interactions(&events);

        // When
        let refused = sanitise(&mut its, &Policy::default());

        // Then
        assert_eq!(refused, Err(Leak::Encoded { interaction: "h0001".into(), step: 1 }));
    }

    #[test]
    fn given_a_report_line_with_a_live_token_when_tokens_are_taken_out_then_only_the_token_goes() {
        // Given
        let line = "refused: Bearer abcdefgh12345678 is not known";

        // When
        let shown = without_tokens(line);

        // Then
        assert_eq!(shown, "refused: Bearer <redacted> is not known");
    }
}
