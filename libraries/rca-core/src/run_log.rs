//! One JSONL row per run (`plugins/ledger/runLog.ts`).
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTokens {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub reasoning: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
    #[serde(default)]
    pub total: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunLogAttempt {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub capped: bool,
    #[serde(default)]
    pub reason: String,
}

fn dispatch() -> String {
    "dispatch".to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogEntry {
    pub ts: String,
    pub ok: bool,
    #[serde(default)]
    pub run_id: String,
    #[serde(default = "dispatch")]
    pub caller: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub attempts: Vec<RunLogAttempt>,
    #[serde(default)]
    pub duration_ms: f64,
    #[serde(default)]
    pub prompt_sha256: String,
    #[serde(default)]
    pub prompt_chars: f64,
    #[serde(default)]
    pub prompt_head: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<RunTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode_session_id: Option<String>,
    #[serde(default)]
    pub output_chars: f64,
    #[serde(default)]
    pub summary_chars: f64,
    #[serde(default)]
    pub tool_calls: f64,
    #[serde(default)]
    pub wrote_nothing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    // Not defaulted by the TypeScript either: a legacy row simply lacks them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalated: Option<bool>,
}

/// Torn lines and rows without a string `ts` or boolean `ok` are skipped; legacy rows get the defaults.
/// A row whose other fields have the wrong type is skipped too, where the TypeScript would pass it through.
pub fn parse_run_log(text: &str) -> Vec<RunLogEntry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub fn serialize_run_log_entry(entry: &RunLogEntry) -> String {
    format!("{}\n", serde_json::to_string(entry).expect("entry serializes"))
}
