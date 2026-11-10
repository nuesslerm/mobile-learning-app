//! The voice-edit planner (`lib/voiceEditPlanner.ts`): one opencode turn that turns an instruction and the numbered code into edit
//! alternatives. A session is held per model, prompt variant and agent so the prompt prefix can hit the provider's cache; a held
//! session that dies under a turn is replaced, a malformed answer gets one more turn on the same session, and a turn that is not
//! answered in its time budget is stopped.

use crate::domain::voice_dsl::{self as dsl, Alternative};
use crate::application::one_shot::{self, Failure, Reply, Turn, Usage};
use super::opencode_client::Client;
use super::settings::Settings;
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// opencode's built-in tool ids, every one switched off: the planner returns one JSON object and never calls a tool, but without
/// this map opencode prefills the whole catalogue on every turn only for it to be ignored.
const DISABLED_TOOLS: [&str; 17] = [
    "bash", "edit", "multiedit", "write", "read", "grep", "glob", "list", "patch", "todowrite", "todoread", "webfetch", "websearch", "task", "question", "skill", "lsp_diagnostics",
];

#[derive(Debug)]
pub struct PlanError {
    pub code: &'static str,
    pub message: String,
}

impl PlanError {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }
}

#[derive(Debug)]
pub struct Meta {
    pub model: String,
    pub variant: String,
    pub agent: Option<String>,
    pub session_reused: bool,
    pub usage: Option<Usage>,
    pub ms: u64,
    pub retried: bool,
}

pub struct Planned {
    pub transcript: String,
    pub alternatives: Vec<Alternative>,
    pub meta: Meta,
}

struct Slot {
    session_id: String,
    turns: u32,
}

pub struct Planner {
    client: Client,
    sessions: Mutex<HashMap<String, Slot>>,
}


fn text_model(settings: &Settings) -> (String, String) {
    let raw = settings.opencode_text_model.clone();
    match raw.find('/').filter(|&i| i >= 1) {
        Some(i) => (raw[..i].to_string(), raw[i + 1..].to_string()),
        None => ("kimi-code-plan-global".into(), "kimi-for-coding-highspeed".into()),
    }
}

/// `voice-planner` in opencode's server config carries temperature 0 and no tools; empty disables it.
fn planner_agent(settings: &Settings) -> Option<String> {
    match &settings.voice_edit_agent {
        None => Some("voice-planner".into()),
        Some(value) => Some(value.trim().to_string()).filter(|v| !v.is_empty()),
    }
}

fn prompt_variant(settings: &Settings) -> &'static str {
    if settings.voice_edit_prompt_v1 {
        "v1"
    } else {
        "v2"
    }
}

impl Planner {
    pub fn new(client: Client) -> Self {
        Self { client, sessions: Mutex::new(HashMap::new()) }
    }

    async fn run_turn(&self, prompt: &str, session_id: Option<String>, variant: &str, agent: Option<&str>, model: &(String, String), budget: Duration) -> Result<Reply, Failure> {
        let tools: Map<String, Value> = DISABLED_TOOLS.iter().map(|t| ((*t).to_string(), json!(false))).collect();
        let cwd = self.client.env().opencode_cwd();
        let turn = Turn { session_id, cwd: cwd.clone(), model: Some(model.clone()), system: Some(dsl::system_prompt(variant).to_string()), tools: Some(tools), agent: agent.map(str::to_string), file_part: None, ..Turn::default() };
        let result = one_shot::run(&self.client, prompt, &turn, Some(Instant::now() + budget)).await;
        if let Err(Failure::Budget { session_id }) = &result {
            // Stop the model, not just the wait for it: an abandoned turn keeps billing and keeps the held session busy.
            one_shot::abort(&self.client, session_id, &cwd).await;
        }
        result
    }

    pub async fn plan(&self, transcript: &str, code: &str, language: &str) -> Result<Planned, PlanError> {
        let settings = self.client.env().settings();
        let model = text_model(&settings);
        let variant = prompt_variant(&settings);
        let agent = planner_agent(&settings);
        let budget = Duration::from_millis(settings.voice_edit_turn_timeout_ms as u64);
        let max_turns = settings.voice_edit_session_max_turns as u32;
        // A held session carries one prompt variant and one agent: never mixed.
        let key = format!("{}/{}#{variant}#{}", model.0, model.1, agent.as_deref().unwrap_or("-"));
        let prompt = dsl::user_prompt(variant, transcript, code, language);
        let started = Instant::now();

        let held = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).get(&key).filter(|s| s.turns < max_turns).map(|s| s.session_id.clone());
        let reusable = held.is_some();
        let mut session_reused = reusable;
        let budget_ms = budget.as_millis();

        let mut turn = match self.run_turn(&prompt, held, variant, agent.as_deref(), &model, budget).await {
            Ok(turn) => turn,
            // A held session can die under us (a server restart, a context overflow): a reason to start a fresh one.
            Err(Failure::Session { .. }) if reusable => {
                self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
                session_reused = false;
                self.run_turn(&prompt, None, variant, agent.as_deref(), &model, budget).await.map_err(|e| self.failed(&key, e, budget_ms))?
            }
            Err(e) => return Err(self.failed(&key, e, budget_ms)),
        };

        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
            match sessions.get_mut(&key).filter(|s| s.session_id == turn.session_id) {
                Some(slot) => slot.turns += 1,
                None => {
                    sessions.insert(key.clone(), Slot { session_id: turn.session_id.clone(), turns: 1 });
                }
            }
        }

        let mut retried = false;
        let parsed = match parse_response(&turn.text, transcript) {
            Ok(parsed) => parsed,
            // Seen live: the model closed an alternative with one brace too many. A malformed answer is the model's slip, and one
            // more turn on the same session is cheaper than a re-recorded instruction.
            Err(e) if e.code == "parse_failed" => {
                let session = self.sessions.lock().unwrap_or_else(|s| s.into_inner()).get(&key).map(|s| s.session_id.clone());
                let retry = self.run_turn(&prompt, session, variant, agent.as_deref(), &model, budget).await.map_err(|f| {
                    let message = if let Failure::Budget { .. } = f { format!("planner turn exceeded {budget_ms}ms") } else { f.message() };
                    PlanError::new("llm_error", format!("LLM stream failed: {message}"))
                })?;
                retried = true;
                turn = Reply { usage: retry.usage.or(turn.usage), text: retry.text, session_id: retry.session_id };
                parse_response(&turn.text, transcript)?
            }
            Err(e) => return Err(e),
        };
        let alternatives = parsed.alternatives.iter().map(|a| dsl::complete_block_bodies(a, code, language)).collect();
        Ok(Planned {
            transcript: parsed.transcript,
            alternatives,
            meta: Meta { model: format!("{}/{}", model.0, model.1), variant: variant.to_string(), agent, session_reused, usage: turn.usage, ms: started.elapsed().as_millis() as u64, retried },
        })
    }

    fn failed(&self, key: &str, failure: Failure, budget_ms: u128) -> PlanError {
        if let Failure::Budget { .. } = failure {
            // The held session may still be mid-turn; a fresh one is safer next time.
            self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
            return PlanError::new("timeout", format!("Planner did not answer within {budget_ms}ms"));
        }
        PlanError::new("llm_error", format!("LLM stream failed: {}", failure.message()))
    }
}

fn fenced() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"```(?:json)?\s*([\s\S]*?)\s*```").unwrap_or_else(|e| panic!("bad pattern: {e}")))
}

/// The JSON of a reply: inside a fence when there is one, else from the first `{` to the last `}`, else the whole text.
fn json_text(text: &str) -> &str {
    if let Ok(Some(caps)) = fenced().captures(text) {
        if let Some(m) = caps.get(1) {
            return m.as_str();
        }
    }
    match (text.find('{'), text.rfind('}')) {
        (Some(open), Some(close)) if close > open => &text[open..=close],
        _ => text,
    }
}

#[derive(Debug)]
pub struct Parsed {
    pub transcript: String,
    pub alternatives: Vec<Alternative>,
}

pub fn parse_response(text: &str, transcript: &str) -> Result<Parsed, PlanError> {
    let parsed = dsl::parse_lenient_json(json_text(text)).map(dsl::normalize_response).map_err(|message| PlanError::new("parse_failed", format!("Failed to parse LLM response as JSON: {message}\n\nRaw:\n{text}")))?;
    match dsl::parse_response(&parsed) {
        Ok(response) => {
            if response.alternatives.is_empty() {
                return Err(PlanError::new("no_alternatives", "LLM returned no alternatives".into()));
            }
            Ok(Parsed { transcript: response.transcript, alternatives: response.alternatives })
        }
        Err(message) => {
            // One malformed alternative must not sink the turn: keep every one that validates on its own, fail only when none does.
            let Some(items) = parsed.get("alternatives").and_then(Value::as_array).filter(|_| parsed.is_object()) else {
                return Err(PlanError::new("parse_failed", format!("Schema validation failed: {message}")));
            };
            let kept: Vec<Alternative> = items.iter().filter_map(|item| dsl::parse_alternative(item).ok()).collect();
            if kept.is_empty() {
                return Err(PlanError::new("parse_failed", format!("Schema validation failed for every alternative: {message}\n\nRaw:\n{text}")));
            }
            let own = parsed.get("transcript").and_then(Value::as_str).map(str::to_string);
            Ok(Parsed { transcript: own.unwrap_or_else(|| transcript.to_string()), alternatives: kept })
        }
    }
}
