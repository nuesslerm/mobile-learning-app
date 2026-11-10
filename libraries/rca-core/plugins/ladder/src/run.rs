//! One headless delegation run: the one-shot plugin chain (ledger, cooldowns, ladder, output checks) fused with
//! `runWithPlugins` (`plugins/runWithPlugins.ts`, `use-cases/oneShotPlugins.ts`). The caller supplies the process
//! attempt and the stores through [`RunHost`]; everything else is decided here, in the same order the TypeScript's
//! plugin hooks fire.
use crate::ladder::{next_rung_index, Attempt, LadderFile, Verdict};
use rca_core::opencode::{OpencodeTokens, ToolEvent};
use crate::pool_ledger::{cold_pools, next_ledger, Ledger};
use rca_core::run_log::RunLogAttempt;
use rca_core::time::iso;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const DEFAULT_MAX_ATTEMPTS: usize = 20;
const PROMPT_HEAD_CHARS: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct AttemptOutput {
    pub text: String,
    pub tool_events: Vec<ToolEvent>,
    pub structured: bool,
    /// `(failureClass, message)` of a turn that failed before producing prose.
    pub error: Option<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct AttemptMetrics {
    pub tokens: Option<OpencodeTokens>,
    pub cost: Option<f64>,
    pub opencode_session_id: Option<String>,
    pub summary_chars: Option<f64>,
}

pub struct RunInput {
    pub run_id: String,
    pub cwd: String,
    pub writes_expected: bool,
    pub prompt: String,
    pub shape: Option<String>,
    /// The models to try, in order (already resolved by the caller).
    pub pins: Vec<String>,
    pub honor_cold: bool,
    pub caller: String,
    pub tier: Option<String>,
    pub claude_session_id: Option<String>,
    pub run_started_at: i64,
}

pub trait RunHost {
    fn now(&mut self) -> i64;
    fn read_ledger(&mut self) -> Ledger;
    fn write_ledger(&mut self, ledger: &Ledger);
    fn append_run_log(&mut self, entry: &Map<String, Value>);
    /// The `git diff --stat` of the files this run wrote; only asked when writes are expected.
    fn diff_stat(&mut self, cwd: &str, touched: &[String]) -> Option<String>;
    fn bad_output(&mut self, text: &str) -> bool;
    fn attempt(&mut self, index: usize, model: &str) -> AttemptOutput;
    fn metrics(&mut self, index: usize) -> Option<AttemptMetrics>;
    /// Called after every judged attempt, for logging.
    fn judged(&mut self, _model: &str, _verdict: &Verdict) {}
}

#[derive(Clone, Debug)]
pub struct AttemptRecord {
    pub index: usize,
    pub model: String,
    pub pool: Option<String>,
    pub skipped: Option<(String, String)>,
    pub verdict: Option<Verdict>,
}

#[derive(Debug)]
pub struct RunOutcome {
    pub ok: bool,
    pub ladder_exhausted: bool,
    pub attempts: Vec<AttemptRecord>,
    pub ok_attempt: Option<usize>,
    pub last_output: Option<AttemptOutput>,
}

fn fail(class: &str, reason: &str) -> Verdict {
    Verdict { ok: false, failure_class: Some(class.into()), reason: Some(reason.into()) }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn head_utf16(s: &str, units: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in s.chars() {
        n += c.len_utf16();
        if n > units {
            break;
        }
        out.push(c);
    }
    out
}

/// Mirrors the harness rotation: escalation fires on a repeated non-fatal class, never on model-missing.
fn escalated_in(reasons: &[String]) -> bool {
    let mut previous: Option<&str> = None;
    for current in reasons {
        if current == "ok" || current == "fatal" {
            return false;
        }
        if current != "model-missing" && previous == Some(current.as_str()) {
            return true;
        }
        previous = Some(current);
    }
    false
}

fn touched_files(events: &[ToolEvent]) -> Vec<String> {
    let mut seen = HashSet::new();
    events
        .iter()
        .filter(|e| matches!(e.tool.as_str(), "edit" | "write" | "patch" | "multiedit") && e.status == "completed")
        .filter_map(|e| e.input.get("filePath").and_then(Value::as_str))
        .filter(|p| seen.insert(p.to_string()))
        .map(String::from)
        .collect()
}

fn has_completed_write(events: &[ToolEvent]) -> bool {
    rca_core::opencode::has_completed_write(events)
}

struct RowState {
    started_at: i64,
    attempts: Vec<RunLogAttempt>,
    last_output: Option<AttemptOutput>,
}

#[allow(clippy::too_many_arguments)]
fn build_row(
    input: &RunInput,
    ladders: &LadderFile,
    state: &RowState,
    finished_at: i64,
    ok_model: Option<&str>,
    metrics: Option<&AttemptMetrics>,
) -> Map<String, Value> {
    let ok = ok_model.is_some();
    let last = state.attempts.last();
    let pool = last.map(|a| crate::ladder::pool_of(ladders, &a.model)).filter(|p| !p.is_empty());
    let reason = last.map(|a| a.reason.clone()).unwrap_or_else(|| if ok { "ok".into() } else { "ladder-exhausted".into() });
    let out = state.last_output.as_ref();
    let mut m = Map::new();
    m.insert("ts".into(), json!(iso(state.started_at)));
    m.insert("runId".into(), json!(input.run_id));
    m.insert("caller".into(), json!(input.caller));
    if let Some(id) = input.claude_session_id.as_deref().filter(|s| !s.is_empty()) {
        m.insert("claudeSessionId".into(), json!(id));
    }
    m.insert("cwd".into(), json!(input.cwd));
    if let Some(model) = ok_model.filter(|s| !s.is_empty()) {
        m.insert("model".into(), json!(model));
    }
    m.insert("ok".into(), json!(ok));
    m.insert("attempts".into(), serde_json::to_value(&state.attempts).expect("attempts"));
    m.insert("durationMs".into(), json!(finished_at - state.started_at));
    let sha: String = Sha256::digest(input.prompt.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    m.insert("promptSha256".into(), json!(sha));
    m.insert("promptChars".into(), json!(utf16_len(&input.prompt)));
    m.insert("promptHead".into(), json!(head_utf16(&input.prompt, PROMPT_HEAD_CHARS)));
    if ok {
        if let Some(t) = metrics.and_then(|x| x.tokens.as_ref()) {
            m.insert("tokens".into(), serde_json::to_value(t).expect("tokens"));
        }
        if let Some(c) = metrics.and_then(|x| x.cost) {
            m.insert("cost".into(), json!(c));
        }
        if let Some(id) = metrics.and_then(|x| x.opencode_session_id.as_deref()).filter(|s| !s.is_empty()) {
            m.insert("opencodeSessionId".into(), json!(id));
        }
    }
    m.insert("outputChars".into(), json!(out.map_or(0, |o| utf16_len(&o.text))));
    let summary_chars = if ok {
        metrics.and_then(|x| x.summary_chars).unwrap_or_else(|| out.map_or(0.0, |o| utf16_len(o.text.trim()) as f64))
    } else {
        0.0
    };
    m.insert("summaryChars".into(), json!(summary_chars));
    m.insert("toolCalls".into(), json!(if ok { out.map_or(0, |o| o.tool_events.len()) } else { 0 }));
    m.insert("wroteNothing".into(), json!(reason == "wrote-nothing"));
    if let Some(shape) = input.shape.as_deref().filter(|s| !s.is_empty()) {
        m.insert("shape".into(), json!(shape));
    }
    if let Some(tier) = input.tier.as_deref().filter(|s| !s.is_empty()) {
        m.insert("tier".into(), json!(tier));
    }
    if let Some(p) = pool {
        m.insert("pool".into(), json!(p));
    }
    let reasons: Vec<String> = state.attempts.iter().map(|a| a.reason.clone()).collect();
    m.insert("reason".into(), json!(reason));
    m.insert("escalated".into(), json!(escalated_in(&reasons)));
    m
}

/// Runs the ladder to the first accepted attempt, a fatal failure or the end of the rungs.
pub fn run_once(host: &mut dyn RunHost, input: &RunInput, ladders: &LadderFile) -> RunOutcome {
    let pool_of_model = |m: &str| crate::ladder::pool_of(ladders, m).to_string();
    let reset_hint_of = |pool: &str| ladders.pools.get(pool).and_then(|p| p.reset_hint.clone());

    let mut tried: Vec<AttemptRecord> = Vec::new();
    let mut ladder_attempts: Vec<Attempt> = Vec::new();
    let mut last_output: Option<AttemptOutput> = None;
    let mut resolved: Option<Vec<String>> = None;
    // Ledger plugin: one row state per run, written once.
    let mut row = RowState { started_at: input.run_started_at, attempts: Vec::new(), last_output: None };
    let mut row_written = false;
    // Cooldowns plugin: a per-run snapshot of the cold pools, the working ledger and per-attempt start times.
    let mut ledger: Option<Ledger> = None;
    let mut snapshot: Option<HashSet<String>> = None;
    let mut started: HashMap<usize, i64> = HashMap::new();
    let mut ok_attempt = None;

    while tried.len() < DEFAULT_MAX_ATTEMPTS {
        // Ladder: resolved once per run, against a fresh read of the pool ledger.
        let ladder = resolved.get_or_insert_with(|| {
            let cold: HashSet<String> = if input.honor_cold {
                let l = host.read_ledger();
                let now = host.now();
                let set = cold_pools(&l, now).into_iter().collect();
                ledger = Some(l);
                set
            } else {
                HashSet::new()
            };
            input.pins.iter().filter(|m| !cold.contains(&pool_of_model(m))).cloned().collect()
        });
        let index = next_rung_index(ladders, ladder, &ladder_attempts);
        let Some(i) = index else {
            if tried.is_empty() {
                // finishRun still gives an empty ladder its row.
                if !row_written {
                    let at = host.now();
                    let entry = build_row(input, ladders, &row, at, None, None);
                    host.append_run_log(&entry);
                }
                return RunOutcome { ok: false, ladder_exhausted: true, attempts: tried, ok_attempt: None, last_output };
            }
            break;
        };
        let model = ladder[i].clone();
        let pool = Some(pool_of_model(&model)).filter(|p| !p.is_empty());
        let attempt_index = tried.len();

        // beforeAttempt: ledger.stateFor (no-op here), then cooldowns.
        let snap = match &snapshot {
            Some(s) => s.clone(),
            None => {
                let l = host.read_ledger();
                let now = host.now();
                let set: HashSet<String> = cold_pools(&l, now).into_iter().collect();
                ledger = Some(l);
                snapshot = Some(set.clone());
                started.clear();
                set
            }
        };
        if input.honor_cold {
            if let Some(p) = pool.as_deref().filter(|p| snap.contains(*p)) {
                let until = ledger.get_or_insert_with(|| host.read_ledger()).get(p).map(|e| e.cold_until.clone());
                let reason = format!("pool {p} cold until {}", until.as_deref().unwrap_or("unknown"));
                tried.push(AttemptRecord { index: attempt_index, model, pool, skipped: Some(("cooldowns".into(), reason)), verdict: None });
                ladder_attempts.push(Attempt { skipped: true, verdict: None });
                continue;
            }
        }
        started.insert(attempt_index, host.now());

        let output = host.attempt(attempt_index, &model);
        last_output = Some(output.clone());
        row.last_output = Some(output.clone());

        // validateOutput: the turn-error plugin, the ledger (records the output), then the output checks.
        let mut verdict = Verdict { ok: true, failure_class: None, reason: None };
        if let Some((class, message)) = &output.error {
            verdict = Verdict { ok: false, failure_class: Some(class.clone()), reason: Some(message.clone()) };
        }
        let diff = if input.writes_expected {
            let files = touched_files(&output.tool_events);
            host.diff_stat(&input.cwd, &files)
        } else {
            None
        };
        let wrote_nothing = input.writes_expected && output.structured && !has_completed_write(&output.tool_events) && diff.as_deref().is_none_or(str::is_empty);
        let checks = if wrote_nothing {
            Some(fail("wrote-nothing", "run completed without a file write or diff"))
        } else if host.bad_output(&output.text) {
            Some(fail("bad-output", "caller output gate rejected the transcript"))
        } else {
            None
        };
        if verdict.ok {
            if let Some(v) = checks {
                verdict = v;
            }
        }

        // afterAttempt: ledger plugin, then cooldowns, then the logger.
        let class = verdict.failure_class.clone();
        let reason_name = if verdict.ok { "ok".to_string() } else { class.clone().unwrap_or_default() };
        row.attempts.push(RunLogAttempt { model: model.clone(), capped: reason_name == "cap", reason: reason_name.clone() });
        if verdict.ok || reason_name == "fatal" {
            let at = host.now();
            let metrics = if verdict.ok { host.metrics(attempt_index) } else { None };
            let entry = build_row(input, ladders, &row, at, verdict.ok.then_some(model.as_str()), metrics.as_ref());
            row_written = true;
            host.append_run_log(&entry);
        }
        if let Some(p) = pool.as_deref() {
            let at = host.now();
            let attempt_ms = started.remove(&attempt_index).map_or(f64::INFINITY, |s| (at - s) as f64);
            let current = ledger.get_or_insert_with(|| host.read_ledger()).clone();
            let next = next_ledger(&current, p, class.as_deref().filter(|_| !verdict.ok), at, attempt_ms, reset_hint_of(p).as_deref());
            if next != current {
                ledger = Some(next.clone());
                host.write_ledger(&next);
            }
        }
        host.judged(&model, &verdict);

        tried.push(AttemptRecord { index: attempt_index, model, pool, skipped: None, verdict: Some(verdict.clone()) });
        ladder_attempts.push(Attempt { skipped: false, verdict: Some(verdict.clone()) });
        if verdict.ok {
            ok_attempt = Some(attempt_index);
            break;
        }
    }

    // finishRun: a run that ended without an ok or fatal attempt still gets its row.
    if !row_written {
        let at = host.now();
        let entry = build_row(input, ladders, &row, at, None, None);
        host.append_run_log(&entry);
    }
    RunOutcome { ok: ok_attempt.is_some(), ladder_exhausted: false, attempts: tried, ok_attempt, last_output }
}
