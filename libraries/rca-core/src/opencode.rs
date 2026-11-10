//! Parsing and formatting around `opencode run --format json` (`data/opencodeRun/events.ts`, the argv and cap/crash
//! detectors of `spawnAttempt.ts`, and `formatDispatchSummary`). Pure: the process itself lives with the caller.
use crate::text::has_word;
use crate::stats::{js_num, js_round, to_fixed};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeTokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolEvent {
    pub tool: String,
    pub status: &'static str,
    pub input: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRun {
    pub text_parts: Vec<String>,
    pub tool_events: Vec<ToolEvent>,
    pub tokens: OpencodeTokens,
    pub cost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub errors: Vec<String>,
    pub non_json_lines: Vec<String>,
    pub saw_json: bool,
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).filter(|n| n.is_finite()).unwrap_or(0.0)
}

fn obj(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

/// NDJSON from `opencode run --format json`; lines that are not events (hook output, stray logs) are kept as text.
pub fn parse_opencode_json_stream(raw: &str) -> ParsedRun {
    let mut r = ParsedRun::default();
    for line in raw.split('\n').map(str::trim).filter(|l| !l.is_empty()) {
        if !line.starts_with('{') {
            r.non_json_lines.push(line.to_string());
            continue;
        }
        let event = match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(m)) => m,
            _ => {
                r.non_json_lines.push(line.to_string());
                continue;
            }
        };
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            r.non_json_lines.push(line.to_string());
            continue;
        };
        r.saw_json = true;
        if let Some(id) = event.get("sessionID").and_then(Value::as_str) {
            if r.session_id.as_deref().is_none_or(str::is_empty) {
                r.session_id = Some(id.to_string());
            }
        }
        let part = obj(event.get("part"));
        match kind {
            "text" => {
                if let Some(t) = part.get("text").and_then(Value::as_str) {
                    r.text_parts.push(t.to_string());
                }
            }
            "tool_use" => {
                let state = part.get("state").and_then(Value::as_object).cloned().unwrap_or_default();
                let status = match state.get("status").and_then(Value::as_str) {
                    Some("completed") => "completed",
                    Some("error") => "error",
                    _ => "other",
                };
                r.tool_events.push(ToolEvent {
                    tool: part.get("tool").and_then(Value::as_str).unwrap_or("unknown").to_string(),
                    status,
                    input: obj(state.get("input")),
                    error: state.get("error").and_then(Value::as_str).map(String::from),
                });
            }
            "step_finish" => {
                let tokens = obj(part.get("tokens"));
                let cache = obj(tokens.get("cache"));
                r.tokens.input += num(tokens.get("input"));
                r.tokens.output += num(tokens.get("output"));
                r.tokens.reasoning += num(tokens.get("reasoning"));
                r.tokens.cache_read += num(cache.get("read"));
                r.tokens.cache_write += num(cache.get("write"));
                r.tokens.total += num(tokens.get("total"));
                r.cost += num(part.get("cost"));
            }
            "error" => {
                let error = obj(event.get("error"));
                let data = obj(error.get("data"));
                let message = data
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| error.get("message").and_then(Value::as_str))
                    .map_or_else(
                        || match event.get("error").filter(|v| !v.is_null()) {
                            Some(v) => v.to_string(),
                            None => Value::Object(event.clone()).to_string(),
                        },
                        String::from,
                    );
                r.errors.push(match error.get("name").and_then(Value::as_str) {
                    Some(name) => format!("{name}: {message}"),
                    None => message,
                });
            }
            _ => {}
        }
    }
    r
}

const MAX_TOOL_LINE: usize = 140;

fn first_string_input(input: &Map<String, Value>) -> String {
    for key in ["filePath", "path", "command", "pattern", "url", "query"] {
        if let Some(s) = input.get(key).and_then(Value::as_str) {
            return s.to_string();
        }
    }
    input.values().find_map(Value::as_str).unwrap_or_default().to_string()
}

fn clip(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.encode_utf16().count() <= MAX_TOOL_LINE {
        return one_line;
    }
    let mut kept = String::new();
    let mut units = 0;
    for c in one_line.chars() {
        units += c.len_utf16();
        if units > MAX_TOOL_LINE {
            break;
        }
        kept.push(c);
    }
    format!("{kept}...")
}

/// One line per call (`[read] path`, `[shell] cmd`): what a supervising model skims instead of the transcript.
pub fn tool_lines_from_events(events: &[ToolEvent]) -> Vec<String> {
    events
        .iter()
        .map(|ev| {
            let target = clip(&first_string_input(&ev.input));
            let label = match ev.tool.as_str() {
                "bash" => "shell",
                "grep" => "search",
                other => other,
            };
            let mut line = format!("[{label}]{}", if target.is_empty() { String::new() } else { format!(" {target}") });
            if ev.status == "error" {
                line.push_str(&format!(" (error{})", ev.error.as_deref().map_or(String::new(), |e| format!(": {}", clip(e)))));
            }
            line
        })
        .collect()
}

/// True when the run completed at least one file-writing tool call.
pub fn has_completed_write(events: &[ToolEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e.tool.as_str(), "edit" | "write" | "patch" | "multiedit") && e.status == "completed")
}

/// The model's final prose: the last text part that is not pure whitespace, trimmed.
pub fn last_text_part(parts: &[String]) -> String {
    parts.iter().rev().map(|p| p.trim()).find(|t| !t.is_empty()).unwrap_or_default().to_string()
}

pub fn looks_like_cap_error(output: &str) -> bool {
    let lower = output.to_lowercase();
    ["rate limit", "quota exceeded", "usage limit", "high concurrency", "overloaded"].iter().any(|p| lower.contains(p))
        || has_word(output, "429")
}

/// A process crash is transient and model-independent, unlike a deterministic auth error.
pub fn looks_like_transient_crash(output: &str) -> bool {
    let lower = output.to_lowercase();
    ["segmentation fault", "core dumped", "out of memory"].iter().any(|p| lower.contains(p))
        || output.contains("sysdeps/")
        || output.contains("libc_start_call_main")
        || has_word(output, "ENOMEM")
        || has_word(output, "ECONNRESET")
}

/// The prompt is always the LAST argument: the mock opencode binary relies on that.
pub fn build_opencode_args(
    auto: bool,
    json: bool,
    continue_session_id: Option<&str>,
    agent: Option<&str>,
    cwd: &str,
    model: &str,
    prompt: &str,
) -> Vec<String> {
    let mut a = vec!["run".to_string()];
    if auto {
        a.push("--auto".into());
    }
    if json {
        a.extend(["--format".into(), "json".into()]);
    }
    if let Some(id) = continue_session_id.filter(|s| !s.is_empty()) {
        a.extend(["--session".into(), id.into()]);
    }
    if let Some(name) = agent.filter(|s| !s.is_empty()) {
        a.extend(["--agent".into(), name.into()]);
    }
    a.extend(["--dir".into(), cwd.into(), "--model".into(), model.into(), prompt.into()]);
    a
}

/// opencode reads OPENCODE_API_KEY; this repo's registry names it OPENCODE_GO_API_KEY.
pub fn opencode_env_overrides(get: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let truthy = |k: &str| get(k).filter(|v| !v.is_empty());
    let mut out = Vec::new();
    if let Some(v) = truthy("OPENCODE_GO_API_KEY") {
        out.push(("OPENCODE_API_KEY".to_string(), v));
    }
    for k in ["MINIMAX_API_KEY", "KIMI_API_KEY"] {
        if let Some(v) = truthy(k) {
            out.push((k.to_string(), v));
        }
    }
    out
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryInput {
    #[serde(default)]
    pub tool_lines: Vec<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub diff_stat: Option<String>,
    #[serde(default)]
    pub wrote_nothing: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tokens: Option<OpencodeTokens>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub duration_ms: f64,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub transcript_path: Option<String>,
    /// The opencode session that answered, for a later turn (`--session`).
    #[serde(default)]
    pub session: Option<String>,
}

/// `brief` is the model's own end-of-task message plus the meta line; `full` keeps tool lines and the diff.
pub fn format_dispatch_summary(r: &SummaryInput, brief: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !brief && !r.tool_lines.is_empty() {
        lines.extend(r.tool_lines.iter().cloned());
        lines.push(String::new());
    }
    if !r.summary.is_empty() {
        lines.push(r.summary.trim().to_string());
        lines.push(String::new());
    }
    if let Some(stat) = r.diff_stat.as_deref().filter(|s| !brief && !s.is_empty()) {
        lines.extend(["git diff --stat:".to_string(), stat.to_string(), String::new()]);
    }
    if r.wrote_nothing {
        lines.push("WROTE_NOTHING: exit 0 but no file changed -- fix the prompt (name the file, say \"make the edit now\") and relaunch.".into());
        lines.push(String::new());
    }
    let mut meta: Vec<String> = Vec::new();
    if let Some(m) = r.model.as_deref().filter(|m| !m.is_empty()) {
        meta.push(format!("served by {m}"));
    }
    if let Some(t) = &r.tokens {
        meta.push(format!("tokens={}", js_num(t.total)));
    }
    if let Some(c) = r.cost {
        meta.push(format!("cost=${}", to_fixed(c, 4)));
    }
    meta.push(format!("{}s", js_num(js_round(r.duration_ms / 1000.0))));
    meta.push(format!("run {}", r.run_id));
    if let Some(s) = r.session.as_deref().filter(|s| !s.is_empty()) {
        meta.push(format!("session {s}"));
    }
    if let Some(p) = r.transcript_path.as_deref().filter(|p| !p.is_empty()) {
        meta.push(format!("transcript {p}"));
    }
    lines.push(format!("[{}]", meta.join(" | ")));
    lines.join("\n")
}
