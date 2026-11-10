//! The edit-latency record (`@lib/voice-edit-dsl`'s `editStats` contract and `lib/editStats.ts`): the client's turn read as the
//! contract describes it, table rows with every column, and a fire-and-forget writer that posts rows to the ORDS table.

use crate::domain::schema::parse;
use serde::de::{self, Deserialize, Deserializer};
use serde::Deserialize as DeserializeDerive;
use remote_chat_agent_core::application::ports::{Outbound, Payload, Post};
use super::log;
use super::settings::Settings;
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

pub const COLUMNS: [&str; 38] = [
    "edit_id", "side", "route", "source", "ok", "status", "error_code", "ms", "ffmpeg_ms", "whisper_ms", "whisper_model", "whisper_threads", "audio_bytes",
    "transcript_chars", "planner_model", "planner_ms", "session_reused", "prompt_tokens", "cache_read", "cache_write", "alternatives", "invalid_count",
    "retried", "code_chars", "language", "host", "problem_id", "platform", "transcribe_wall_ms", "plan_wall_ms", "post_confirm_wait_ms", "speculative_hit",
    "local_fast_path", "outcome", "picked_index", "code_chars_after", "chunk_index", "error_text",
];

/// How long a row may wait for others to share its flush.
pub const FLUSH_EVERY: Duration = Duration::from_secs(5);
const FLUSH_AT: usize = 20;
const MAX_QUEUE: usize = 500;
const ERROR_LOG_EVERY: Duration = Duration::from_secs(60);

/// A fully-null row with the required fields set; callers fill in what they know.
pub fn empty_row(edit_id: &str, side: &str, route: &str) -> Map<String, Value> {
    let mut row = Map::new();
    for c in COLUMNS {
        row.insert(c.into(), Value::Null);
    }
    row.insert("edit_id".into(), json!(edit_id));
    row.insert("side".into(), json!(side));
    row.insert("route".into(), json!(route));
    row.insert("ok".into(), json!(1));
    row
}

/// A string of `MIN` to `MAX` characters.
#[allow(dead_code)]
struct Text<const MIN: usize, const MAX: usize>(String);

impl<'de, const MIN: usize, const MAX: usize> Deserialize<'de> for Text<MIN, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let length = text.chars().count();
        if length < MIN || length > MAX {
            return Err(de::Error::invalid_length(length, &format!("a string of {MIN} to {MAX} characters").as_str()));
        }
        Ok(Self(text))
    }
}

/// A whole number from `MIN` to `MAX`.
#[allow(dead_code)]
struct Ranged<const MIN: i64, const MAX: i64>(i64);

impl<'de, const MIN: i64, const MAX: i64> Deserialize<'de> for Ranged<MIN, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let number = i64::deserialize(deserializer)?;
        if number < MIN || number > MAX {
            return Err(de::Error::invalid_value(de::Unexpected::Signed(number), &format!("an integer from {MIN} to {MAX}").as_str()));
        }
        Ok(Self(number))
    }
}

#[derive(DeserializeDerive)]
#[serde(rename_all = "kebab-case")]
enum Source {
    Voice,
    Text,
    PlanVoice,
    PlanText,
}

#[derive(DeserializeDerive)]
#[serde(rename_all = "kebab-case")]
enum Route {
    Turn,
    TranscribeChunk,
}

#[derive(DeserializeDerive)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Auto,
    Picked,
    Dismissed,
    Error,
    Cancelled,
}

const MS: i64 = 3_600_000;

/// `clientEditStatSchema`; the values are read back from the JSON when the row is built, so only the rules are used here.
#[derive(DeserializeDerive)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ClientStat {
    edit_id: Text<1, 64>,
    source: Source,
    route: Option<Route>,
    chunk_index: Option<Ranged<0, 10_000>>,
    problem_id: Option<Text<0, 128>>,
    platform: Text<0, 16>,
    transcribe_wall_ms: Option<Ranged<0, MS>>,
    plan_wall_ms: Option<Ranged<0, MS>>,
    post_confirm_wait_ms: Option<Ranged<0, MS>>,
    speculative_hit: bool,
    local_fast_path: bool,
    outcome: Outcome,
    picked_index: Option<Ranged<0, 99>>,
    alternatives: Ranged<0, 99>,
    transcript_chars: Option<Ranged<0, 100_000>>,
    code_chars_before: Ranged<0, 10_000_000>,
    code_chars_after: Option<Ranged<0, 10_000_000>>,
    error: Option<Text<0, 1000>>,
}

/// The client's turn as a table row, or the first thing wrong with it. Unknown fields are dropped.
pub fn client_stat_to_row(body: &Value, host: &str) -> Result<Map<String, Value>, String> {
    let fields = parse::<Map<String, Value>>(body)?;
    parse::<ClientStat>(body)?;
    let fields = &fields;
    let get = |k: &str| fields.get(k).cloned().unwrap_or(Value::Null);
    let flag = |k: &str| json!(i64::from(fields.get(k).and_then(Value::as_bool).unwrap_or(false)));
    let route = fields.get("route").and_then(Value::as_str).unwrap_or("turn");
    let mut row = empty_row(fields.get("editId").and_then(Value::as_str).unwrap_or_default(), "client", route);
    let set = |row: &mut Map<String, Value>, k: &str, v: Value| {
        row.insert(k.into(), v);
    };
    set(&mut row, "source", get("source"));
    set(&mut row, "chunk_index", get("chunkIndex"));
    set(&mut row, "ok", json!(i64::from(fields.get("outcome").and_then(Value::as_str) != Some("error"))));
    set(&mut row, "problem_id", get("problemId"));
    set(&mut row, "platform", get("platform"));
    set(&mut row, "transcribe_wall_ms", get("transcribeWallMs"));
    set(&mut row, "plan_wall_ms", get("planWallMs"));
    set(&mut row, "post_confirm_wait_ms", get("postConfirmWaitMs"));
    set(&mut row, "speculative_hit", flag("speculativeHit"));
    set(&mut row, "local_fast_path", flag("localFastPath"));
    set(&mut row, "outcome", get("outcome"));
    set(&mut row, "picked_index", get("pickedIndex"));
    set(&mut row, "alternatives", get("alternatives"));
    set(&mut row, "transcript_chars", get("transcriptChars"));
    set(&mut row, "code_chars", get("codeCharsBefore"));
    set(&mut row, "code_chars_after", get("codeCharsAfter"));
    set(&mut row, "error_text", get("error"));
    set(&mut row, "host", json!(host));
    Ok(row)
}

/// The kernel's host name, `unknown` when it cannot be read.
pub fn host_name(files: &dyn remote_chat_agent_core::application::ports::WorkspaceFiles) -> String {
    files.read(std::path::Path::new("/proc/sys/kernel/hostname")).and_then(|b| String::from_utf8(b).ok()).map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).unwrap_or_else(|| "unknown".into())
}

struct Config {
    url: String,
    user: String,
    password: String,
}

fn placeholder(value: &str) -> bool {
    let v = value.trim();
    v.is_empty() || v == "PLACEHOLDER_FILL_ME" || v.starts_with("PLACEHOLDER_FILL_ME_") || (v.starts_with('<') && v.ends_with('>') && v.len() > 2 && !v[1..v.len() - 1].contains(['<', '>']))
}

fn read_config(settings: &Settings) -> Option<Config> {
    let (url, password) = (settings.edit_stats_ords_base_url.clone(), settings.health_reports_ords_password.clone());
    if placeholder(&url) || placeholder(&password) {
        return None;
    }
    Some(Config { url: if url.ends_with('/') { url } else { format!("{url}/") }, user: settings.health_reports_ords_user.clone(), password })
}

pub use crate::domain::util::iso_now;

struct State {
    queue: VecDeque<Map<String, Value>>,
    /// When the oldest queued row is due to be sent.
    due: Option<Instant>,
}

struct Shared {
    state: Mutex<State>,
    wake: Notify,
    flush_every: Duration,
}

/// Rows are queued and a background task posts them through `outbound`; a full queue drops the oldest, a failing endpoint is logged
/// at most once a minute, and without configuration every call is a no-op. Must be made inside the tokio runtime.
pub struct EditStats {
    config: Option<Arc<Config>>,
    shared: Arc<Shared>,
    pub host: String,
}

impl EditStats {
    /// The table the settings name, flushed a row's `flush_every` after it was queued (or at once when twenty are waiting).
    pub fn new(settings: &Settings, outbound: Arc<dyn Outbound>, flush_every: Duration, host: String) -> Self {
        let config = read_config(settings).map(Arc::new);
        if config.is_none() {
            log::info!("edit_stats.disabled", reason = "EDIT_STATS_ORDS_BASE_URL or HEALTH_REPORTS_ORDS_PASSWORD not set");
        }
        let shared = Arc::new(Shared { state: Mutex::new(State { queue: VecDeque::new(), due: None }), wake: Notify::new(), flush_every });
        if let Some(config) = &config {
            tokio::spawn(flush_loop(outbound, Arc::clone(config), Arc::clone(&shared)));
        }
        Self { config, shared, host }
    }

    pub fn enabled(&self) -> bool {
        self.config.is_some()
    }

    pub fn record(&self, mut row: Map<String, Value>) {
        if self.config.is_none() {
            return;
        }
        if row.get("host").is_none_or(Value::is_null) {
            row.insert("host".into(), json!(self.host));
        }
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.queue.len() >= MAX_QUEUE {
                state.queue.pop_front();
            }
            state.queue.push_back(row);
            if state.due.is_none() {
                state.due = Some(Instant::now() + self.shared.flush_every);
            }
            if state.queue.len() >= FLUSH_AT {
                state.due = Some(Instant::now());
            }
        }
        self.shared.wake.notify_one();
    }
}

/// The oldest due batch, waiting for rows and for their time to come.
async fn next_batch(shared: &Shared) -> Vec<Map<String, Value>> {
    loop {
        let due = shared.state.lock().unwrap_or_else(|e| e.into_inner()).due;
        match due {
            None => shared.wake.notified().await,
            Some(due) if Instant::now() < due => {
                tokio::select! {
                    () = tokio::time::sleep_until(due.into()) => {}
                    () = shared.wake.notified() => {}
                }
            }
            Some(_) => {
                let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                let take = state.queue.len().min(FLUSH_AT);
                let batch = state.queue.drain(..take).collect();
                state.due = if state.queue.is_empty() { None } else { Some(Instant::now()) };
                return batch;
            }
        }
    }
}

async fn flush_loop(outbound: Arc<dyn Outbound>, config: Arc<Config>, shared: Arc<Shared>) {
    let mut last_error_log: Option<Instant> = None;
    loop {
        let batch = next_batch(&shared).await;
        let (mut failed, mut first_error) = (0usize, String::new());
        for row in &batch {
            // ORDS auto-REST binds every omitted column to NULL rather than its default, so the NOT NULL timestamp travels with the row.
            let mut payload = Map::new();
            for c in COLUMNS {
                payload.insert(c.into(), row.get(c).cloned().unwrap_or(Value::Null));
            }
            payload.insert("ts".into(), json!(iso_now()));
            let post = Post { url: config.url.clone(), headers: Vec::new(), basic_auth: Some((config.user.clone(), config.password.clone())), payload: Payload::Json(Value::Object(payload)) };
            match outbound.post(post).await {
                Ok(reply) if (200..300).contains(&reply.status) => {}
                Ok(reply) => {
                    failed += 1;
                    if first_error.is_empty() {
                        first_error = format!("ORDS {}: {}", reply.status, String::from_utf8_lossy(&reply.body).chars().take(200).collect::<String>());
                    }
                }
                Err(e) => {
                    failed += 1;
                    if first_error.is_empty() {
                        first_error = format!("fetch failed: {}", e.message);
                    }
                }
            }
        }
        if failed > 0 && last_error_log.is_none_or(|t| t.elapsed() >= ERROR_LOG_EVERY) {
            last_error_log = Some(Instant::now());
            log::warn!("edit_stats.flush_failed", failed = failed, sent = batch.len() - failed, error = first_error);
        }
    }
}
