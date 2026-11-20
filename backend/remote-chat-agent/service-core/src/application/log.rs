//! Events for the service's own modules, written through `tracing` (the console line and the OTLP export are `service-kit`'s). The
//! event name is the message and the metadata rides in one `meta` object. Never pass a token, a cookie value or a credential.
//! A call site names typed fields, `log::info!("server.listening", port = port)`; the slice functions serve the sites whose fields are built at run time.

use agent_api_service::logs::{Level, LogLine};
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// How many events the service keeps for `service.logs`: the newest ones.
pub const TAIL_KEPT: usize = 2000;

/// The newest events of the service's own code, in memory: what `service.logs` reads. Nothing here survives a restart; the console line and the OTLP export
/// are the record.
pub struct Tail {
    kept: usize,
    lines: Mutex<VecDeque<LogLine>>,
}

/// A field that holds a credential is kept out of the tail whatever the call site passed.
fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["token", "secret", "password", "authorization", "cookie", "bearer", "credential", "api_key", "apikey"].iter().any(|word| key.contains(word))
}

const KNOWN_PREFIXES: [&str; 8] = ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xox", "AKIA", "eyJ"];

/// A value that carries a credential under a harmless key (an executor's stderr in a `message`) keeps its words and loses the credential: a token with a
/// well-known prefix anywhere in a word, the word after `Bearer` or after a secret-looking name that ends in `:` or `=`, and what follows `=` or `:` inside a word.
fn scrub_text(text: &str) -> String {
    let token_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut hide_next = false;
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end_matches(char::is_whitespace);
        let mut shown = word.to_string();
        for prefix in KNOWN_PREFIXES {
            while let Some(at) = shown.find(prefix).filter(|at| shown[*at..].chars().take_while(|c| token_char(*c)).count() > 8 && !shown[*at..].starts_with("<redacted>")) {
                let len: usize = shown[at..].chars().take_while(|c| token_char(*c)).map(char::len_utf8).sum();
                shown.replace_range(at..at + len, "<redacted>");
            }
        }
        if hide_next && !word.is_empty() {
            shown = "<redacted>".into();
        } else if let Some((name, value)) = word.split_once(['=', ':']).filter(|(name, _)| secret_key(name)) {
            if !value.is_empty() {
                shown = format!("{name}{}<redacted>", &word[name.len()..=name.len()]);
            }
        }
        let named = word.trim_end_matches([':', '=']);
        hide_next = word.eq_ignore_ascii_case("bearer") || (named.len() < word.len() && secret_key(named));
        out.push_str(&shown);
        out.push_str(&piece[word.len()..]);
    }
    out
}

fn scrub(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(scrub_text(text)),
        Value::Array(items) => Value::Array(items.iter().map(scrub).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), if secret_key(k) { Value::String("<redacted>".into()) } else { scrub(v) })).collect()),
        other => other.clone(),
    }
}

impl Tail {
    pub fn new(kept: usize) -> Tail {
        Tail { kept: kept.max(1), lines: Mutex::default() }
    }

    pub fn record(&self, level: Level, event: &str, meta: &[(&str, Value)]) {
        let at_ms = CLOCK.get().map_or(0, |now| now());
        let meta: Map<String, Value> = meta.iter().map(|(k, v)| ((*k).to_string(), if secret_key(k) { Value::String("<redacted>".into()) } else { scrub(v) })).collect();
        let mut lines = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        if lines.len() >= self.kept {
            lines.pop_front();
        }
        lines.push_back(LogLine { at_ms, level, event: event.to_string(), meta: Value::Object(meta) });
    }

    /// The last `limit` events at or above `min`, oldest first, and how many events are kept in all.
    pub fn recent(&self, limit: usize, min: Level) -> (Vec<LogLine>, usize) {
        let lines = self.lines.lock().unwrap_or_else(|p| p.into_inner());
        let mut picked: Vec<LogLine> = lines.iter().rev().filter(|l| l.level >= min).take(limit).cloned().collect();
        picked.reverse();
        (picked, lines.len())
    }
}

static CLOCK: OnceLock<fn() -> u64> = OnceLock::new();

/// The binary hands the tail its wall clock (epoch milliseconds); until then events carry 0.
pub fn install_clock(now: fn() -> u64) {
    CLOCK.set(now).ok();
}

/// The tail the service's `log::` functions write to.
pub fn tail() -> &'static Tail {
    static TAIL: OnceLock<Tail> = OnceLock::new();
    TAIL.get_or_init(|| Tail::new(TAIL_KEPT))
}

fn meta_of(meta: &[(&str, Value)]) -> Value {
    Value::Object(meta.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect::<Map<_, _>>())
}

/// A field's value as the `meta` object carries it.
pub fn field<T: serde::Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[macro_export]
macro_rules! log_event {
    ($level:ident, $event:expr $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::application::log::$level($event, &[$((stringify!($key), $crate::application::log::field(&$value))),*])
    };
}

#[macro_export]
macro_rules! log_info {
    ($($args:tt)*) => { $crate::log_event!(info, $($args)*) };
}
#[macro_export]
macro_rules! log_warn {
    ($($args:tt)*) => { $crate::log_event!(warn, $($args)*) };
}
#[macro_export]
macro_rules! log_error {
    ($($args:tt)*) => { $crate::log_event!(error, $($args)*) };
}
pub use crate::{log_error as error, log_info as info, log_warn as warn};

pub fn info(event: &str, meta: &[(&str, Value)]) {
    tail().record(Level::Info, event, meta);
    tracing::info!(meta = %meta_of(meta), "{event}");
}

pub fn warn(event: &str, meta: &[(&str, Value)]) {
    tail().record(Level::Warn, event, meta);
    tracing::warn!(meta = %meta_of(meta), "{event}");
}

pub fn error(event: &str, meta: &[(&str, Value)]) {
    tail().record(Level::Error, event, meta);
    tracing::error!(meta = %meta_of(meta), "{event}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_more_events_than_it_keeps_when_read_then_the_newest_come_back_oldest_first_filtered_by_level_and_a_credential_field_is_never_kept() {
        // Given a tail of three events that has seen five, one of them with a token field
        let tail = Tail::new(3);
        tail.record(Level::Info, "a", &[]);
        tail.record(Level::Warn, "b", &[]);
        tail.record(Level::Info, "c", &[("count", json!(2))]);
        tail.record(Level::Error, "d", &[("api_token", json!("tok-123")), ("why", json!("no"))]);
        tail.record(Level::Info, "e", &[]);
        // When the last two, and the warnings and above, are read
        let (last, kept) = tail.recent(2, Level::Info);
        let (loud, _) = tail.recent(10, Level::Warn);
        // Then the oldest two were dropped, the newest two come back in order, only the error is loud, and the token was redacted
        assert_eq!((last.iter().map(|l| l.event.as_str()).collect::<Vec<_>>(), kept), (vec!["d", "e"], 3));
        assert_eq!(loud.iter().map(|l| l.event.as_str()).collect::<Vec<_>>(), ["d"]);
        assert_eq!(loud[0].meta, json!({"api_token": "<redacted>", "why": "no"}));
        assert!(!serde_json::to_string(&loud).unwrap().contains("tok-123"));
    }

    #[test]
    fn given_an_error_message_that_echoes_a_credential_under_a_harmless_key_when_recorded_then_the_credential_is_gone_and_the_words_stay() {
        // Given a tail and a failure message that echoes a bearer header, a prefixed token and a name=value pair
        let tail = Tail::new(4);
        let message = "git failed: Authorization: Bearer abc123def456 remote https://x:ghp_0123456789abcdef@host GH_TOKEN=s3cr3t done";
        // When it is recorded under a plain `message` key, also nested
        tail.record(Level::Error, "run.preparation.failed", &[("message", json!(message)), ("detail", json!({"stderr": ["password: hunter2"]}))]);
        // Then no credential is kept and the surrounding words are
        let (lines, _) = tail.recent(1, Level::Info);
        let text = serde_json::to_string(&lines).unwrap();
        for secret in ["abc123def456", "ghp_0123456789abcdef", "s3cr3t", "hunter2"] {
            assert!(!text.contains(secret), "{secret} kept in {text}");
        }
        assert!(text.contains("git failed") && text.contains("done"), "{text}");
    }
}
