//! The evidence a row is served by: a passing test of a complete chain run whose method-hit trace shows the row's call was answered and, for
//! a row that forwards to the agent API, that the target method answered in the call's window. A trace is the `method-trace/1` file a chain
//! writes beside its run record (`exec_testkit::trace`, bundled by `wf test publish`); the evidence of record is the checked-in
//! `<app>-evidence.json` that `wf parity coverage --traces` writes from one, so a check never needs the run.
use crate::coverage::Problem;
use crate::ports::Evidence;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const TRACE_SCHEMA: &str = "method-trace/1";
pub const EVIDENCE_SCHEMA: u64 = 1;

/// One answered call of a passing test: the rows it exercised and the agent API methods and executor tools that answered in its window.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub suite: String,
    pub test: String,
    pub index: usize,
    pub rows: Vec<String>,
    pub agent: BTreeSet<String>,
    pub executor: BTreeSet<String>,
    /// How an oracle replay labelled the call (`diff`) and the provenance of the cassette it replayed (`cassette`); a chain call has neither.
    pub diff: Option<String>,
    pub cassette: Option<String>,
}

/// A phone row as the resolver sees it.
#[derive(Debug, Clone, Copy)]
pub struct RowSpec<'a> {
    pub id: &'a str,
    pub target: Option<&'a str>,
    /// A reverify note holds the row pending.
    pub held: bool,
    /// The row's decision says `oracle: mimic`: only an admitted oracle replay serves it.
    pub mimic: bool,
}

/// What proves a row served: the test, and where its call went.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub suite: String,
    pub test: String,
    pub agent: Vec<String>,
    pub executor: Vec<String>,
    /// Set for a row an oracle replay served: `mimic`, the call's diff label and the cassette's provenance.
    pub oracle: Option<String>,
    pub diff: Option<String>,
    pub cassette: Option<String>,
}

impl Entry {
    pub fn to_json(&self) -> Value {
        let mut v = json!({"suite": self.suite, "test": self.test, "agent": self.agent, "executor": self.executor});
        for (key, value) in [("oracle", &self.oracle), ("diff", &self.diff), ("cassette", &self.cassette)] {
            if let Some(value) = value {
                v[key] = json!(value);
            }
        }
        v
    }

    fn from_json(v: &Value) -> Option<Entry> {
        let list = |k: &str| -> Vec<String> { v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default() };
        let text = |k: &str| v[k].as_str().map(String::from);
        Some(Entry { suite: v["suite"].as_str()?.into(), test: v["test"].as_str()?.into(), agent: list("agent"), executor: list("executor"), oracle: text("oracle"), diff: text("diff"), cassette: text("cassette") })
    }
}

/// The calls of every passing test of every complete run of `app` in `texts` (each trace file as `(where it was read, its text)`), and a
/// summary of the runs read. A trace of another app is skipped, an unreadable one is an error naming it.
pub fn calls(app: &str, texts: &[(String, String)]) -> Result<(Vec<Call>, Vec<Value>), String> {
    let (mut out, mut runs) = (vec![], vec![]);
    for (from, text) in texts {
        let trace: Value = serde_json::from_str(text).map_err(|e| format!("{from}: not JSON: {e}"))?;
        if trace["schema"] != TRACE_SCHEMA {
            return Err(format!("{from}: schema {} is not {TRACE_SCHEMA}", trace["schema"]));
        }
        if trace["app"] != app {
            continue;
        }
        let suite = trace["suite"].as_str().ok_or_else(|| format!("{from}: no suite"))?.to_string();
        let complete = trace["complete"] == true;
        let tests = trace["tests"].as_array().ok_or_else(|| format!("{from}: no tests"))?;
        runs.push(json!({"suite": suite, "sha": trace["sha"], "complete": complete, "tests": tests.len(), "passed": tests.iter().filter(|t| t["verdict"] == "pass").count()}));
        // a run that stopped partway is an error run: nothing in it counts
        if !complete {
            continue;
        }
        for test in tests.iter().filter(|t| t["verdict"] == "pass") {
            let name = test["test"].as_str().ok_or_else(|| format!("{from}: a test without a name"))?;
            let cassette = test["cassette"].as_str().map(String::from);
            for (index, call) in test["calls"].as_array().into_iter().flatten().enumerate().filter(|(_, c)| c["ok"] == true) {
                let names = |key: &str, field: &str| -> BTreeSet<String> { call[key].as_array().into_iter().flatten().filter(|h| h["ok"] == true).filter_map(|h| h[field].as_str().map(String::from)).collect() };
                out.push(Call {
                    suite: suite.clone(),
                    test: name.to_string(),
                    index,
                    rows: call["rows"].as_array().into_iter().flatten().filter_map(|r| r.as_str().map(String::from)).collect(),
                    agent: names("agent", "method"),
                    executor: names("executor", "tool"),
                    diff: call["diff"].as_str().map(String::from),
                    cassette: cassette.clone(),
                });
            }
        }
    }
    Ok((out, runs))
}

/// What a row's call must show: the agent method its target names, if it forwards.
pub fn required_method(target: Option<&str>) -> Option<&str> {
    target.and_then(|t| t.strip_prefix("agent:"))
}

/// The rows `calls` serve. A row no call serves is absent. Among the calls that serve a row the test whose name sorts first (then its first
/// call) is the evidence, so a refresh names the same test whatever order the tests finished in. A row decided `oracle: mimic` is served
/// only by a call the oracle replay labelled and `oracle` admits (a clean or allowlisted diff on a recorded cassette); a call without a label,
/// as a chain test's is, never serves it.
pub fn resolve(calls: &[Call], rows: &[RowSpec], oracle: &dyn Evidence) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for row in rows.iter().filter(|r| !r.held) {
        let required = required_method(row.target);
        let serves = |c: &&Call| {
            c.rows.iter().any(|r| r == row.id)
                && required.is_none_or(|m| c.agent.contains(m))
                && (!row.mimic || (c.diff.is_some() && oracle.admits(c.cassette.as_deref(), c.diff.as_deref())))
        };
        let best = calls.iter().filter(serves).min_by(|a, b| (&a.suite, &a.test, a.index).cmp(&(&b.suite, &b.test, b.index)));
        if let Some(c) = best {
            let mimic = |v: &Option<String>| v.clone().filter(|_| row.mimic);
            out.insert(
                row.id.to_string(),
                Entry {
                    suite: c.suite.clone(),
                    test: c.test.clone(),
                    agent: c.agent.iter().cloned().collect(),
                    executor: c.executor.iter().cloned().collect(),
                    oracle: row.mimic.then(|| "mimic".to_string()),
                    diff: mimic(&c.diff),
                    cassette: mimic(&c.cassette),
                },
            );
        }
    }
    out
}

/// Trace rows no phone row of the table has: a renamed or mistyped row a test names.
pub fn unknown_rows(calls: &[Call], known: &BTreeSet<&str>) -> Vec<String> {
    let named: BTreeSet<&String> = calls.iter().flat_map(|c| c.rows.iter()).collect();
    named.into_iter().filter(|r| !known.contains(r.as_str())).cloned().collect()
}

/// The checked-in evidence file: its rows, or an error naming what is wrong with it.
pub fn parse(text: &str) -> Result<BTreeMap<String, Entry>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    if v["schema"] != EVIDENCE_SCHEMA {
        return Err(format!("schema {} is not {EVIDENCE_SCHEMA}", v["schema"]));
    }
    let rows = v["rows"].as_object().ok_or("no rows object")?;
    rows.iter().map(|(id, e)| Entry::from_json(e).map(|e| (id.clone(), e)).ok_or_else(|| format!("{id}: an entry needs a suite and a test"))).collect()
}

/// The evidence file for `rows`, and where they came from (`source`: what `--traces` named and the runs read).
pub fn document(app: &str, source: Value, rows: &BTreeMap<String, Entry>) -> String {
    let rows: Map<String, Value> = rows.iter().map(|(id, e)| (id.clone(), e.to_json())).collect();
    crate::methods::pretty(&json!({
        "schema": EVIDENCE_SCHEMA,
        "app": app,
        "generatedBy": format!("wf parity coverage --app {app} --traces <dir|bundle>"),
        "source": source,
        "rows": rows,
    }))
}

/// A `--traces` source as the evidence file records it: a URL, or a path inside the repository. An absolute path or one through a scratch
/// directory names nothing a later check could read, so it is not recorded.
pub fn stable_source(source: &str) -> Option<String> {
    if source.starts_with("https://") || source.starts_with("http://") {
        return Some(source.to_string());
    }
    let scratch = source.starts_with(['/', '~', '\\']) || source.chars().nth(1) == Some(':') || source.split(['/', '\\']).any(|c| matches!(c, ".." | "tmp" | "temp"));
    (!scratch && !source.is_empty()).then(|| source.to_string())
}

/// What the checked-in evidence file recorded as its traces source when that is no stable path (see `stable_source`).
pub fn unstable_source(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let traces = v["source"]["traces"].as_str()?;
    stable_source(traces).is_none().then(|| traces.to_string())
}

/// The `source` object of the evidence file.
pub fn source_json(traces: &str, runs: Vec<Value>) -> Value {
    match stable_source(traces) {
        Some(t) => json!({"traces": t, "runs": runs}),
        None => json!({"runs": runs}),
    }
}

pub fn unknown_problem(rows: Vec<String>) -> Option<Problem> {
    (!rows.is_empty()).then(|| Problem { rule: "unknown-trace-row", message: "the traces name rows the coverage table has no phone row for: fix the rows the chain test names".into(), rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::AdmitOnly;

    fn trace(complete: bool, tests: Value) -> (String, String) {
        ("trace.json".into(), json!({"schema": TRACE_SCHEMA, "app": "t3", "suite": "chain", "sha": "abc", "complete": complete, "tests": tests}).to_string())
    }

    fn call(rows: &[&str], ok: bool, agent: &[(&str, bool)], executor: &[(&str, bool)]) -> Value {
        json!({"rows": rows, "ok": ok, "agent": agent.iter().map(|(m, ok)| json!({"method": m, "ok": ok})).collect::<Vec<_>>(), "executor": executor.iter().map(|(t, ok)| json!({"tool": t, "ok": ok})).collect::<Vec<_>>()})
    }

    #[test]
    fn only_answered_calls_of_passing_tests_of_complete_runs_count() {
        // Given a complete run with a passing test, a failing one, a refused call, and an incomplete run of its own
        let complete = trace(
            true,
            json!([
                {"test": "a::passes", "verdict": "pass", "calls": [call(&["rpc:x"], true, &[], &[]), call(&["rpc:refused"], false, &[], &[])]},
                {"test": "a::fails", "verdict": "fail", "calls": [call(&["rpc:failed"], true, &[], &[])]},
            ]),
        );
        let stopped = trace(false, json!([{"test": "b::passes", "verdict": "pass", "calls": [call(&["rpc:stopped"], true, &[], &[])]}]));
        // When the calls are read
        let (read, runs) = calls("t3", &[complete, stopped]).unwrap();
        // Then only the answered call of the passing test of the complete run is there, and both runs are summarized
        assert_eq!(read.iter().map(|c| (c.test.as_str(), c.rows.clone())).collect::<Vec<_>>(), [("a::passes", vec!["rpc:x".to_string()])]);
        assert_eq!(runs.iter().map(|r| (r["complete"].clone(), r["passed"].clone())).collect::<Vec<_>>(), [(json!(true), json!(1)), (json!(false), json!(1))]);
    }

    #[test]
    fn a_forwarding_row_needs_its_target_answered_in_the_same_call_and_the_first_test_by_name_is_the_evidence() {
        // Given two tests that exercised a command, one whose window shows the target method failing and one where it answered, and a t3-local row
        let tests = json!([
            {"test": "b::second", "verdict": "pass", "calls": [call(&["command:message.dispatch", "rpc:local"], true, &[("turn.start", true)], &[("write_file", true)])]},
            {"test": "a::first", "verdict": "pass", "calls": [call(&["command:message.dispatch"], true, &[("turn.start", false)], &[]), call(&["command:message.dispatch", "rpc:local"], true, &[("thread.create", true), ("turn.start", true)], &[])]},
        ]);
        let (read, _) = calls("t3", &[trace(true, tests)]).unwrap();
        // When the rows are resolved, the command forwarding to turn.start and a row with no target
        let spec = |id, target, held| RowSpec { id, target, held, mimic: false };
        let rows = [spec("command:message.dispatch", Some("agent:turn.start"), false), spec("rpc:local", None, false), spec("command:unseen", Some("t3-local"), false)];
        let served = resolve(&read, &rows, &AdmitOnly::default());
        // Then the command is served by the first test by name whose call shows turn.start answered, the local row needs only its call, and an unseen row is absent
        assert_eq!(served["command:message.dispatch"].test, "a::first");
        assert_eq!(served["command:message.dispatch"].agent, ["thread.create", "turn.start"]);
        assert_eq!(served["rpc:local"].test, "a::first");
        assert!(!served.contains_key("command:unseen"));
        // And a row held by a reverify note is never served by a trace
        assert!(resolve(&read, &[spec("rpc:local", None, true)], &AdmitOnly::default()).is_empty());
    }

    #[test]
    fn a_call_that_is_answered_without_its_target_serving_does_not_serve_a_forwarding_row() {
        // Given a command whose target is turn.start, answered on the wire without turn.start in its window
        let (read, _) = calls("t3", &[trace(true, json!([{"test": "a::t", "verdict": "pass", "calls": [call(&["command:thread.create"], true, &[], &[])]}]))]).unwrap();
        // When resolved, Then it is not served: the wire answered it, but not through the agent API
        assert!(resolve(&read, &[RowSpec { id: "command:thread.create", target: Some("agent:thread.create"), held: false, mimic: false }], &AdmitOnly::default()).is_empty());
        // And the rows the traces name that the table lacks are found
        assert_eq!(unknown_rows(&read, &BTreeSet::from(["rpc:other"])), ["command:thread.create"]);
    }

    #[test]
    fn the_evidence_file_round_trips_and_a_file_of_another_schema_is_refused() {
        let rows = BTreeMap::from([("rpc:x".to_string(), Entry { suite: "chain".into(), test: "a::t".into(), agent: vec!["file.list".into()], executor: vec!["list_dir".into()], oracle: None, diff: None, cassette: None })]);
        assert_eq!(parse(&document("t3", json!({}), &rows)).unwrap(), rows);
        assert!(parse(r#"{"schema": 9, "rows": {}}"#).unwrap_err().contains("schema 9"));
        assert!(parse(r#"{"schema": 1, "rows": {"rpc:x": {}}}"#).unwrap_err().contains("rpc:x"));
        // and a trace of another schema or one that is not JSON is an error naming where it was read
        assert!(calls("t3", &[("t.json".into(), r#"{"schema": "other"}"#.into())]).unwrap_err().contains("t.json"));
        assert!(calls("t3", &[("t.json".into(), "x".into())]).unwrap_err().contains("not JSON"));
    }

    fn replayed(test: &str, cassette: &str, diff: &str) -> Value {
        json!({"test": test, "cassette": cassette, "verdict": "pass", "calls": [{"rows": ["rpc:x"], "ok": true, "diff": diff, "agent": [], "executor": []}]})
    }

    #[test]
    fn a_mimic_row_is_served_only_by_an_admitted_diff_of_a_recorded_cassette_and_names_it() {
        // Given replays of the same row: a drifted one, a clean one of an authored cassette, an unlabelled chain call, and a clean recorded one that sorts last
        let tests = json!([
            replayed("a::drift", "recorded", "drift"),
            replayed("b::authored", "authored", "clean"),
            {"test": "c::chain", "verdict": "pass", "calls": [{"rows": ["rpc:x"], "ok": true, "agent": [], "executor": []}]},
            replayed("d::clean", "recorded", "clean"),
        ]);
        let (read, _) = calls("t3", &[trace(true, tests)]).unwrap();
        let oracle = AdmitOnly::of(&[("recorded", "clean"), ("recorded", "allowlisted")]);
        let row = |mimic| [RowSpec { id: "rpc:x", target: None, held: false, mimic }];
        // When the row is decided oracle: mimic, Then only the clean recorded replay serves it, and the entry names the diff and the cassette
        let served = resolve(&read, &row(true), &oracle);
        assert_eq!((served["rpc:x"].test.as_str(), served["rpc:x"].oracle.as_deref(), served["rpc:x"].diff.as_deref(), served["rpc:x"].cassette.as_deref()), ("d::clean", Some("mimic"), Some("clean"), Some("recorded")));
        // And without the oracle decision the first test by name serves it, as before
        assert_eq!(resolve(&read, &row(false), &oracle)["rpc:x"].test, "a::drift");
        // And with no admitted replay at all the row is not served
        let only_drift = calls("t3", &[trace(true, json!([replayed("a::drift", "recorded", "drift")]))]).unwrap().0;
        assert!(resolve(&only_drift, &row(true), &oracle).is_empty());
        // And the entry round-trips through the evidence file
        assert_eq!(parse(&document("t3", json!({}), &served)).unwrap(), served);
    }

    #[test]
    fn the_recorded_source_is_a_url_or_a_path_in_the_repository_never_an_absolute_or_scratch_path() {
        // Given sources of each shape
        for (given, kept) in [("https://b.example/bundle.json", true), ("verification/runs", true), ("/tmp/orca-traces", false), ("/home/u/run", false), (".agents/tmp/run", false), ("../out", false), ("C:\\out", false)] {
            // When recorded, Then only a URL or a repository-relative path is kept
            assert_eq!(stable_source(given).is_some(), kept, "{given}");
        }
        assert_eq!(source_json("/tmp/x", vec![]), json!({"runs": []}));
        // And a checked-in file that recorded a scratch path is found
        assert_eq!(unstable_source(r#"{"source": {"traces": "/tmp/orca-traces"}}"#).as_deref(), Some("/tmp/orca-traces"));
        assert!(unstable_source(r#"{"source": {"runs": []}}"#).is_none());
    }
}
