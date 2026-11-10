//! `mimic-rules/1`: normalisation as data. A rule set masks what a reference makes up on each run (ids, timestamps, tokens, ports,
//! sequence numbers) so two runs of equal behaviour produce equal text. The behaviour is lifted from the T3 conformance normaliser
//! (the old TS normaliser of tooling/t3-conformance, retired in X5), which a flow test holds this port to against the recorded goldens.
use crate::cassette::{Dir, Effect, Interaction, Step};
use crate::json::{hash_value, set_pointer};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "mimic-rules/1";
const UUID: &str = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const ISO_TS: &str = r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?Z";
const TRUNCATE_AT: usize = 2000;

fn ts_placeholder() -> String {
    "<ts>".into()
}
fn seq_placeholder() -> String {
    "<seq>".into()
}
fn volatile_placeholder() -> String {
    "<volatile>".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Rule {
    /// UUIDs become `<uuid:N>` by order of first appearance within one interaction.
    UuidOrdinal,
    /// ISO 8601 text, and numbers under `keys` (epoch milliseconds), become the placeholder.
    Timestamp {
        #[serde(default)]
        keys: Vec<String>,
        #[serde(default = "ts_placeholder")]
        placeholder: String,
    },
    /// Text matching a regex (signed tokens, tickets) becomes the placeholder.
    Token { pattern: String, placeholder: String },
    /// A string under one of these keys is replaced wholesale and learned as a secret for every other place it appears.
    SecretKey { keys: BTreeMap<String, String> },
    /// Numbers under these keys (sequence, offsets) become the placeholder.
    Sequence {
        keys: Vec<String>,
        #[serde(default = "seq_placeholder")]
        placeholder: String,
    },
    /// Numbers under a key (pid, expires_in) become that key's placeholder.
    VolatileNumber { keys: BTreeMap<String, String> },
    /// Strings under a key (trace and span ids) become that key's placeholder.
    VolatileString { keys: BTreeMap<String, String> },
    /// Headers that differ by machine or run are removed (names compare lower-case).
    DropHeader { names: Vec<String> },
    /// An absolute path of the run becomes a placeholder.
    Path { placeholder: String, value: String },
    /// One JSON pointer into the interaction is masked, optionally in one interaction only.
    Pointer {
        pointer: String,
        #[serde(default = "volatile_placeholder")]
        placeholder: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        interaction: Option<String>,
    },
    /// The server-to-client steps of a channel are ordered by the request they answer, so a different interleaving of replies to
    /// concurrent requests is the same recording; the frames of one request keep their order.
    CanonicalOrder { channel: String },
    /// A declared input the driver is given instead of randomness (fixed keys and nonces); documents the seed, masks nothing.
    Seeded { pointer: String, reason: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RuleSet {
    pub schema: String,
    pub rules: Vec<Rule>,
}

impl RuleSet {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { schema: SCHEMA.into(), rules }
    }

    pub fn hash(&self) -> String {
        hash_value(&serde_json::to_value(self).expect("rules serialise"))
    }

    pub fn compile(&self) -> Result<Compiled, RulesError> {
        if self.schema != SCHEMA {
            return Err(RulesError::Schema(self.schema.clone()));
        }
        let mut c = Compiled::default();
        for rule in &self.rules {
            match rule {
                Rule::UuidOrdinal => c.text.push(TextRule::Uuid(regex(UUID)?)),
                Rule::Timestamp { keys, placeholder } => {
                    c.text.push(TextRule::Replace(regex(ISO_TS)?, placeholder.clone()));
                    for k in keys {
                        c.number_keys.insert(k.clone(), placeholder.clone());
                    }
                }
                Rule::Token { pattern, placeholder } => c.text.push(TextRule::Replace(regex(pattern)?, placeholder.clone())),
                Rule::SecretKey { keys } => c.secret_keys.extend(keys.clone()),
                Rule::Sequence { keys, placeholder } => {
                    for k in keys {
                        c.number_keys.insert(k.clone(), placeholder.clone());
                    }
                }
                Rule::VolatileNumber { keys } => c.number_keys.extend(keys.clone()),
                Rule::VolatileString { keys } => c.string_keys.extend(keys.clone()),
                Rule::DropHeader { names } => c.dropped.extend(names.iter().map(|n| n.to_lowercase())),
                Rule::Path { placeholder, value } => c.paths.push((value.clone(), placeholder.clone())),
                Rule::Pointer { pointer, placeholder, interaction } => {
                    c.pointers.push((pointer.clone(), placeholder.clone(), interaction.clone()))
                }
                Rule::CanonicalOrder { channel } => c.canonical.push(channel.clone()),
                Rule::Seeded { .. } => {}
            }
        }
        c.paths.sort_by_key(|p| std::cmp::Reverse(p.0.len()));
        Ok(c)
    }

    /// Declared seeded inputs: pointer and why, for the report.
    pub fn seeded(&self) -> Vec<(&str, &str)> {
        self.rules
            .iter()
            .filter_map(|r| match r {
                Rule::Seeded { pointer, reason } => Some((pointer.as_str(), reason.as_str())),
                _ => None,
            })
            .collect()
    }
}

fn regex(pattern: &str) -> Result<Regex, RulesError> {
    Regex::new(pattern).map_err(|e| RulesError::Pattern(pattern.to_string(), e.to_string()))
}

#[derive(Debug, PartialEq)]
pub enum RulesError {
    Schema(String),
    Pattern(String, String),
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(s) => write!(f, "unsupported rules schema {s:?}, this tool reads {SCHEMA}"),
            Self::Pattern(p, e) => write!(f, "rule pattern {p:?} is not a valid regex: {e}"),
        }
    }
}

impl std::error::Error for RulesError {}

enum TextRule {
    Uuid(Regex),
    Replace(Regex, String),
}

/// A rule set with its patterns compiled; one `Normalizer` per interaction borrows it.
#[derive(Default)]
pub struct Compiled {
    text: Vec<TextRule>,
    secret_keys: BTreeMap<String, String>,
    number_keys: BTreeMap<String, String>,
    string_keys: BTreeMap<String, String>,
    dropped: BTreeSet<String>,
    paths: Vec<(String, String)>,
    pointers: Vec<(String, String, Option<String>)>,
    canonical: Vec<String>,
}

impl Compiled {
    pub fn normalizer(&self) -> Normalizer<'_> {
        Normalizer { rules: self, uuids: Vec::new(), secrets: Vec::new() }
    }

    /// The observed side of an interaction (steps and effects) masked; the input is what the driver was given and stays verbatim.
    pub fn interaction(&self, it: &Interaction) -> Interaction {
        let mut nz = self.normalizer();
        let mut out = it.clone();
        out.steps = it.steps.iter().map(|s| crate::wire::mask_step(&mut nz, s)).collect();
        for channel in &self.canonical {
            canonical_order(&mut out.steps, channel);
        }
        out.effects = it
            .effects
            .iter()
            .map(|e| Effect {
                kind: e.kind.clone(),
                data: e.data.iter().map(|(k, v)| (k.clone(), nz.json(v, Some(k)))).collect(),
            })
            .collect();
        let mut v = serde_json::to_value(&out).expect("interaction serialises");
        for (pointer, placeholder, only) in &self.pointers {
            if only.as_ref().is_none_or(|id| *id == it.id) {
                set_existing(&mut v, pointer, Value::String(placeholder.clone()));
            }
        }
        serde_json::from_value(v).expect("a masked interaction still parses")
    }
}

/// The server-to-client steps of one channel, sorted by request id (numeric ids by value, steps without one last) into the slots they held.
fn canonical_order(steps: &mut [Step], channel: &str) {
    let slots: Vec<usize> = (0..steps.len()).filter(|&i| steps[i].dir == Dir::S2c && steps[i].channel.as_deref() == Some(channel)).collect();
    let key = |s: &Step| match s.rid.as_deref() {
        None => (true, 0u64, String::new()),
        Some(r) => match r.parse::<u64>() {
            Ok(n) => (false, n, String::new()),
            Err(_) => (false, u64::MAX, r.to_string()),
        },
    };
    let mut ordered: Vec<Step> = slots.iter().map(|&i| steps[i].clone()).collect();
    ordered.sort_by_key(key);
    for (slot, step) in slots.into_iter().zip(ordered) {
        steps[slot] = step;
    }
}

fn set_existing(root: &mut Value, pointer: &str, value: Value) {
    if root.pointer(pointer).is_some() {
        set_pointer(root, pointer, value);
    }
}

pub struct Normalizer<'a> {
    rules: &'a Compiled,
    uuids: Vec<String>,
    secrets: Vec<(String, String)>,
}

impl Normalizer<'_> {
    pub fn add_secret(&mut self, value: &str, placeholder: &str) {
        if value.len() >= 6 && !self.secrets.iter().any(|(v, _)| v == value) {
            self.secrets.push((value.to_string(), placeholder.to_string()));
        }
    }

    pub fn text(&mut self, input: &str) -> String {
        let mut s = input.to_string();
        let mut secrets = self.secrets.clone();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.0.len()));
        for (secret, ph) in &secrets {
            s = s.replace(secret.as_str(), ph);
        }
        for (abs, ph) in &self.rules.paths {
            s = s.replace(abs.as_str(), ph);
        }
        for rule in &self.rules.text {
            s = match rule {
                TextRule::Replace(re, ph) => re.replace_all(&s, ph.as_str()).into_owned(),
                TextRule::Uuid(re) => {
                    let mut out = String::new();
                    let mut last = 0;
                    for m in re.find_iter(&s) {
                        out.push_str(&s[last..m.start()]);
                        out.push_str(&format!("<uuid:{}>", self.ordinal(m.as_str())));
                        last = m.end();
                    }
                    out.push_str(&s[last..]);
                    out
                }
            };
        }
        s
    }

    fn ordinal(&mut self, uuid: &str) -> usize {
        match self.uuids.iter().position(|u| u == uuid) {
            Some(i) => i + 1,
            None => {
                self.uuids.push(uuid.to_string());
                self.uuids.len()
            }
        }
    }

    pub fn json(&mut self, value: &Value, key: Option<&str>) -> Value {
        match value {
            Value::String(s) => {
                if let Some(k) = key {
                    if let Some(ph) = self.rules.secret_keys.get(k) {
                        if !s.is_empty() {
                            let ph = ph.clone();
                            self.add_secret(s, &ph);
                            return Value::String(ph);
                        }
                    }
                    if let Some(ph) = self.rules.string_keys.get(k) {
                        return Value::String(ph.clone());
                    }
                }
                Value::String(self.text(s))
            }
            Value::Number(_) => match key.and_then(|k| self.rules.number_keys.get(k)) {
                Some(ph) => Value::String(ph.clone()),
                None => value.clone(),
            },
            Value::Array(a) => Value::Array(a.iter().map(|v| self.json(v, key)).collect()),
            Value::Object(m) => {
                let mut out = Map::new();
                for (k, v) in m {
                    out.insert(k.clone(), self.json(v, Some(k)));
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }

    /// A JSON or form-urlencoded body; anything else as scrubbed text, truncated.
    pub fn body(&mut self, raw: &str, content_type: &str) -> Value {
        if raw.is_empty() {
            return Value::Null;
        }
        if content_type.contains("json") {
            return match serde_json::from_str::<Value>(raw) {
                Ok(v) => self.json(&v, None),
                Err(_) => Value::String(self.text(raw)),
            };
        }
        if content_type.contains("x-www-form-urlencoded") {
            let obj: Map<String, Value> = form_pairs(raw).into_iter().map(|(k, v)| (k, Value::String(v))).collect();
            return self.json(&Value::Object(obj), None);
        }
        let chars: Vec<char> = raw.chars().collect();
        let shown = if chars.len() > TRUNCATE_AT {
            format!("{}...<truncated {}>", chars[..TRUNCATE_AT].iter().collect::<String>(), chars.len() - TRUNCATE_AT)
        } else {
            raw.to_string()
        };
        Value::String(self.text(&shown))
    }

    /// Lower-cased, sorted headers without the dropped ones; credentials and cookies masked.
    pub fn headers(&mut self, headers: &[(String, String)]) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for (name, v) in headers {
            let k = name.to_lowercase();
            if self.rules.dropped.contains(&k) {
                continue;
            }
            let masked = if k == "authorization" {
                let secret = self.rules.secret_keys.get("access_token").cloned().unwrap_or_else(|| "<secret>".into());
                match v.split_once(char::is_whitespace) {
                    Some((scheme, rest)) if !rest.trim().is_empty() => format!("{scheme} {secret}"),
                    _ => self.text(v),
                }
            } else if k == "set-cookie" || k == "cookie" {
                "<cookie>".to_string()
            } else {
                self.text(v)
            };
            out.insert(k, masked);
        }
        out
    }

    /// A request path with its query: secret parameters replaced and learned, the rest scrubbed.
    pub fn path(&mut self, raw: &str) -> String {
        let (p, query) = raw.split_once('?').unwrap_or((raw, ""));
        let params: Vec<String> = form_pairs(query)
            .into_iter()
            .map(|(k, v)| match self.rules.secret_keys.get(&k) {
                Some(ph) => {
                    let ph = ph.clone();
                    self.add_secret(&v, &ph);
                    format!("{k}={ph}")
                }
                None => format!("{k}={}", self.text(&v)),
            })
            .collect();
        let p = self.text(p);
        if params.is_empty() {
            p
        } else {
            format!("{p}?{}", params.join("&"))
        }
    }
}

fn form_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if b.get(i + 1).zip(b.get(i + 2)).is_some_and(|(a, c)| a.is_ascii_hexdigit() && c.is_ascii_hexdigit()) => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).expect("hex digits are ascii");
                out.push(u8::from_str_radix(h, 16).expect("checked hex"));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
