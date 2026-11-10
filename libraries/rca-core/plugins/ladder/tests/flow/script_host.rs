//! The `RunHost` double: a scripted clock and opencode attempts, the ledger and run log kept in real files under a temp
//! dir (`FileLedgerStore`, `FileRunLog`, as `wf offload ask` keeps them).
use crate::golden::s;
use rca_core::file_store::FileRunLog;
use rca_ladder::file_store::FileLedgerStore;
use rca_ladder::pool_ledger::Ledger;
use rca_ladder::run::{AttemptMetrics, AttemptOutput, RunHost};
use rca_core::run_log::RunLogEntry;
use serde_json::Value;
use std::collections::HashMap;

pub struct ScriptHost<'a> {
    script: &'a [Value],
    pub executed: usize,
    t: i64,
    pub store: FileLedgerStore,
    pub log: FileRunLog,
    pub ledger_writes: Vec<Value>,
    pub rows: Vec<Value>,
    last_diff: Option<String>,
    bad_contains: Option<String>,
    metrics: HashMap<usize, AttemptMetrics>,
}

impl<'a> ScriptHost<'a> {
    pub fn new(script: &'a [Value], started_at: i64, store: FileLedgerStore, log: FileRunLog, bad_contains: Option<String>) -> Self {
        ScriptHost { script, executed: 0, t: started_at, store, log, ledger_writes: Vec::new(), rows: Vec::new(), last_diff: None, bad_contains, metrics: HashMap::new() }
    }
}

fn tool_event(v: &Value) -> rca_core::opencode::ToolEvent {
    rca_core::opencode::ToolEvent {
        tool: s(&v["tool"]).to_string(),
        status: match s(&v["status"]) {
            "completed" => "completed",
            "error" => "error",
            _ => "other",
        },
        input: v["input"].as_object().cloned().unwrap_or_default(),
        error: None,
    }
}

impl RunHost for ScriptHost<'_> {
    fn now(&mut self) -> i64 {
        self.t
    }
    fn read_ledger(&mut self) -> Ledger {
        self.store.read()
    }
    fn write_ledger(&mut self, ledger: &Ledger) {
        self.store.write(ledger).unwrap();
        self.ledger_writes.push(serde_json::to_value(ledger).unwrap());
    }
    fn append_run_log(&mut self, entry: &serde_json::Map<String, Value>) {
        self.rows.push(Value::Object(entry.clone()));
        let row: RunLogEntry = serde_json::from_value(Value::Object(entry.clone())).unwrap();
        self.log.append(&row).unwrap();
    }
    fn diff_stat(&mut self, _cwd: &str, _touched: &[String]) -> Option<String> {
        self.last_diff.clone()
    }
    fn bad_output(&mut self, text: &str) -> bool {
        self.bad_contains.as_deref().is_some_and(|needle| text.contains(needle))
    }
    fn attempt(&mut self, index: usize, _model: &str) -> AttemptOutput {
        let step = &self.script[self.executed];
        self.executed += 1;
        self.t += step["advanceMs"].as_i64().unwrap();
        self.last_diff = step["diffStat"].as_str().map(String::from);
        let m = &step["metrics"];
        if m.is_object() {
            self.metrics.insert(
                index,
                AttemptMetrics {
                    tokens: (!m["tokens"].is_null()).then(|| serde_json::from_value(m["tokens"].clone()).unwrap()),
                    cost: m["cost"].as_f64(),
                    opencode_session_id: m["opencodeSessionId"].as_str().map(String::from),
                    summary_chars: m["summaryChars"].as_f64(),
                },
            );
        }
        let o = &step["output"];
        AttemptOutput {
            text: s(&o["text"]).to_string(),
            tool_events: o["toolEvents"].as_array().unwrap().iter().map(tool_event).collect(),
            structured: o["structured"].as_bool().unwrap(),
            error: o["error"].as_object().map(|e| (s(&e["failureClass"]).to_string(), s(&e["message"]).to_string())),
        }
    }
    fn metrics(&mut self, index: usize) -> Option<AttemptMetrics> {
        self.metrics.get(&index).cloned()
    }
}
