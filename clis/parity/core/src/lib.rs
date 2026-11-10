//! The pure core of `wf parity`: the reference scans of the pinned T3 Code and Orca sources, the coverage table built from the
//! scan, the decisions file, the evidence of the chain runs and the mapper's own tables, and the checks on it. It reads the world
//! only through its ports (`Tree`, `History`, `Traces`, `Upstream`); the `parity` crate implements them.
pub mod coverage;
pub mod evidence;
pub mod graph;
pub mod lex;
pub mod mapper;
pub mod methods;
pub mod orca;
pub mod ports;
pub mod resolve;
pub mod scan;
pub mod t3;
pub mod track;
pub mod track_report;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

use coverage::{Decisions, Problem, PIN_BUMP};
use methods::MethodsList;
use ports::{Evidence, History, Traces, Tree};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const COVERAGE_DIR: &str = "docs/plans/rca-parity/coverage";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    T3,
    Orca,
}

impl App {
    pub fn parse(name: &str) -> Option<App> {
        match name {
            "t3" => Some(App::T3),
            "orca" => Some(App::Orca),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            App::T3 => "t3",
            App::Orca => "orca",
        }
    }
    pub fn reference(self) -> &'static str {
        match self {
            App::T3 => t3::REFERENCE,
            App::Orca => orca::REFERENCE,
        }
    }
    pub fn pin_file(self) -> &'static str {
        match self {
            App::T3 => "tooling/t3-conformance/t3-pin.json",
            App::Orca => "tooling/orca-conformance/orca-pin.json",
        }
    }
    fn path(self, suffix: &str) -> String {
        format!("{COVERAGE_DIR}/{}{suffix}", self.name())
    }

    /// The agent API methods and executor tools the app's chain tests prove its rows served by.
    pub fn evidence_file(self) -> String {
        self.path("-evidence.json")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Write,
    Check,
}

pub struct Request {
    pub app: App,
    pub mode: Mode,
    /// The revision `pending` is compared against; by default HEAD when the table is dirty, else HEAD^.
    pub base: Option<String>,
    /// Where the method-hit traces of a chain run are (a test output dir, a trace file, a bundle or its URL): a write refreshes the evidence
    /// from them, a check compares the checked-in evidence with them. Without it the checked-in evidence stands.
    pub traces: Option<String>,
}

pub struct Outcome {
    pub ok: bool,
    pub report: Value,
    /// Files to write, with their content (write mode only).
    pub writes: Vec<(String, String)>,
}

fn refusal(app: App, error: String, next: String) -> Outcome {
    Outcome { ok: false, report: json!({"ok": false, "app": app.name(), "error": error, "next": next}), writes: vec![] }
}

/// `pending` may not rise against the base table: HEAD while the table differs from it, else HEAD^, unless `base` names one. A
/// commit since the base whose message carries the pin-bump mark may raise it.
pub fn pending_rose(history: &dyn History, table: &str, dirty: bool, base: Option<&str>, now: usize) -> Option<Problem> {
    let rev = base.map(String::from).unwrap_or_else(|| if dirty { "HEAD".into() } else { "HEAD^".into() });
    let before = coverage::pending_of(&history.show(&rev, table)?)?;
    if now <= before || history.messages_since(&rev).iter().any(|m| m.contains(PIN_BUMP)) {
        return None;
    }
    Some(Problem { rule: "pending-rose", message: format!("pending rose from {before} at {rev} to {now}; only a commit whose message carries '{PIN_BUMP}' may raise it, and its milestone lists the new rows"), rows: vec![] })
}

pub fn run(req: &Request, tree: &dyn Tree, history: &dyn History, traces: &dyn Traces, oracle: &dyn Evidence) -> Outcome {
    let app = req.app;
    let (reference, pin_file) = (app.reference(), app.pin_file());
    let Some(pin) = tree.read(pin_file).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        return refusal(app, format!("{pin_file} is missing or not JSON"), format!("restore {pin_file}: it names the reference commit the scan reads"));
    };
    let Some(commit) = pin["commit"].as_str().map(String::from) else {
        return refusal(app, format!("{pin_file} has no commit"), format!("add the reference's full commit to {pin_file}"));
    };
    let pin_summary = json!({"file": pin_file, "version": pin["version"], "commit": commit});
    let head = history.checkout_head(reference);
    let at_pin = head.as_deref() == Some(commit.as_str());
    if req.mode == Mode::Write && head.is_some() && !at_pin {
        return refusal(
            app,
            format!("{reference} is at {}, not the pin {commit} ({pin_file})", head.as_deref().unwrap_or("?")),
            format!("git -C {reference} checkout {commit}, or bump the pin in a commit marked '{PIN_BUMP}'; `--check` compares without writing"),
        );
    }
    let methods_path = app.path("-methods.json");
    let (list, source) = if at_pin {
        let scanned = match app {
            App::T3 => t3::scan(tree, pin_summary.clone()),
            App::Orca => orca::scan(tree, pin_summary.clone()),
        };
        match scanned {
            Ok(list) => (list, "reference-scan"),
            Err(e) => return refusal(app, e, format!("the pinned {reference} moved its sources; update the scan in clis/parity/core/src/{}.rs", app.name())),
        }
    } else {
        match tree.read(&methods_path).map(|t| MethodsList::parse(&t)) {
            Some(Ok(list)) => (list, "methods-list"),
            Some(Err(e)) => return refusal(app, format!("{methods_path}: {e}"), format!("git -C {reference} checkout {commit} (pnpm run submodule:init), then wf parity coverage --app {}", app.name())),
            None => return refusal(app, format!("{methods_path} is missing and {reference} is not checked out at the pin {commit}"), format!("pnpm run submodule:init, then wf parity coverage --app {}", app.name())),
        }
    };
    let decisions_path = app.path("-decisions.json");
    let decisions = match tree.read(&decisions_path).map(|t| Decisions::parse(&t)) {
        Some(Ok(d)) => d,
        Some(Err(e)) => return refusal(app, format!("{decisions_path}: {e}"), format!("fix {decisions_path}")),
        None => Decisions::default(),
    };
    let answered = match app {
        App::T3 => mapper::t3_answers(tree, &list.rows),
        App::Orca => BTreeMap::new(),
    };
    // the evidence of record: what the traces prove when asked for them, else the checked-in file
    let evidence_path = app.evidence_file();
    let checked_in = tree.read(&evidence_path);
    let mut problems = vec![];
    let refreshed = req.traces.is_some();
    let (evidence, evidence_text) = match &req.traces {
        None => match checked_in.as_deref().map(evidence::parse) {
            Some(Ok(rows)) => (rows, None),
            Some(Err(e)) => return refusal(app, format!("{evidence_path}: {e}"), format!("refresh it with wf parity coverage --app {} --traces <dir|bundle>", app.name())),
            None => (BTreeMap::new(), None),
        },
        Some(source) => {
            let loaded = match traces.load(source) {
                Ok(texts) if texts.is_empty() => return refusal(app, format!("{source}: no trace.json there"), "run the integration chain with WF_TEST_OUTPUT_DIR set (wf test run --stage integration-tests), or give the dir of a published bundle".into()),
                Ok(texts) => texts,
                Err(e) => return refusal(app, e, "give --traces a test output dir, a trace.json, or a bundle.json (a path or its URL)".into()),
            };
            let (calls, runs) = match evidence::calls(app.name(), &loaded) {
                Ok(read) => read,
                Err(e) => return refusal(app, e, "the traces are written by exec_testkit::trace (method-trace/1): rerun the chain".into()),
            };
            let targets = &decisions.targets;
            let rows: Vec<evidence::RowSpec> = list
                .phone_rows()
                .filter(|r| !matches!(r.class.as_deref(), Some("flagged" | "offAllowlist")))
                .map(|r| evidence::RowSpec { id: r.id.as_str(), target: targets.get(&r.id).map(String::as_str), held: decisions.reverify.contains_key(&r.id), mimic: decisions.mimic(&r.id) })
                .collect();
            let known: std::collections::BTreeSet<&str> = list.phone_rows().map(|r| r.id.as_str()).collect();
            problems.extend(evidence::unknown_problem(evidence::unknown_rows(&calls, &known).into_iter().filter(|r| !decisions.is_trace_noise(r)).collect()));
            let mut served = evidence::resolve(&calls, &rows, oracle);
            // a row decided not offered is not served, whatever a call of it did
            served.retain(|id, _| !decisions.rows.get(id).is_some_and(|d| matches!(d.get("state").and_then(Value::as_str), Some("not-offered" | "excluded"))));
            let text = evidence::document(app.name(), evidence::source_json(source, runs), &served);
            (served, Some(text))
        }
    };
    let cov = coverage::build(&list, &decisions, &answered, &evidence, oracle);
    let (json_path, md_path) = (app.path(".json"), app.path(".md"));
    problems.extend(cov.problems.clone());
    // an evidence file that records where a run was read from by an absolute or scratch path cannot be re-read by the next check
    let unstable = checked_in.as_deref().and_then(evidence::unstable_source);
    if let Some(path) = unstable.as_ref().filter(|_| !(refreshed && req.mode == Mode::Write)) {
        problems.push(Problem { rule: "scratch-trace-path", message: format!("{evidence_path} records its traces as {path}, an absolute or scratch path: refresh it with --traces, which records only a URL or a path in the repository"), rows: vec![] });
    }
    let json_text = coverage::json_text(&cov);
    // A write makes the table what it built, so it is dirty when that differs from the file.
    let dirty = history.is_dirty(&json_path) || (req.mode == Mode::Write && tree.read(&json_path).as_deref() != Some(json_text.as_str()));
    problems.extend(pending_rose(history, &json_path, dirty, req.base.as_deref(), cov.pending));
    let mut files = vec![(json_path, json_text), (md_path, cov.markdown.clone())];
    if source == "reference-scan" {
        files.insert(0, (methods_path, list.to_text()));
    }
    if let Some(text) = evidence_text {
        files.insert(0, (evidence_path.clone(), text));
    }
    // the evidence file is its rows: the same rows read from another run (its path, its commit) leave it as it is
    let mut changed: Vec<String> = files.iter().filter(|(p, text)| *p != evidence_path && tree.read(p).as_deref() != Some(text.as_str())).map(|(p, _)| p.clone()).collect();
    if refreshed && (unstable.is_some() || checked_in.as_deref().map(evidence::parse).and_then(Result::ok).as_ref() != Some(&evidence)) {
        changed.insert(0, evidence_path.clone());
    }
    if req.mode == Mode::Check && !changed.is_empty() {
        problems.push(Problem { rule: "stale", message: "the checked-in files differ from what the generator builds".into(), rows: changed.clone() });
    }
    let ok = req.mode == Mode::Write || problems.is_empty();
    let mut report = json!({
        "ok": ok,
        "app": app.name(),
        "mode": if req.mode == Mode::Write { "write" } else { "check" },
        "source": source,
        "reference": {"dir": reference, "head": head, "pin": commit, "atPin": at_pin},
        "counts": list.counts,
        "summary": cov.json["summary"],
        "evidence": json!({"file": app.evidence_file(), "rows": evidence.len(), "refreshedFrom": req.traces.as_deref().and_then(evidence::stable_source)}),
        "problems": problems.iter().map(Problem::to_json).collect::<Vec<_>>(),
    });
    let writes = if req.mode == Mode::Write { files.into_iter().filter(|(p, _)| changed.contains(p)).collect() } else { vec![] };
    if req.mode == Mode::Write {
        report["wrote"] = json!(changed);
    }
    if !ok {
        report["next"] = json!(format!("assign the rows in {decisions_path}, then wf parity coverage --app {} and commit the tables", app.name()));
    }
    Outcome { ok, report, writes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testing::{AdmitOnly, MemoryHistory, MemoryTraces, MemoryTree};

    #[test]
    fn pending_may_rise_only_in_a_pin_bump_commit() {
        let table = "docs/plans/rca-parity/coverage/t3.json";
        let committed = |rev: &str| (format!("{rev}:{table}"), r#"{"summary": {"pending": 5}}"#.to_string());
        let mut h = MemoryHistory { committed: [committed("HEAD^"), committed("HEAD")].into(), ..Default::default() };
        // Given a clean table, pending at 5 a commit ago: 6 is a rise and 5 is not
        assert_eq!(pending_rose(&h, table, false, None, 6).map(|p| p.rule), Some("pending-rose"));
        assert!(pending_rose(&h, table, false, None, 5).is_none());
        // When the commit since HEAD^ is a pin bump, Then the rise is allowed
        h.messages.insert("HEAD^".into(), vec!["parity: pin-bump to v0.1.7".into()]);
        assert!(pending_rose(&h, table, false, None, 6).is_none());
        // When the table is dirty, Then it is compared with HEAD, where no commit can excuse it yet
        assert!(pending_rose(&h, table, true, None, 6).is_some());
        // And with no base table there is nothing to compare
        assert!(pending_rose(&MemoryHistory::default(), table, false, None, 9).is_none());
    }

    #[test]
    fn a_reference_off_its_pin_is_refused_for_a_write_and_checked_from_the_list() {
        // Given the T3 pin and a checkout at another commit, with no methods list yet
        let tree = MemoryTree::of(&[("tooling/t3-conformance/t3-pin.json", r#"{"version": "0.1.6", "commit": "aaa"}"#)]);
        let history = MemoryHistory { heads: [("references/t3code".to_string(), "bbb".to_string())].into(), ..Default::default() };
        // When a write is asked, Then it is refused naming both commits
        let write = run(&Request { app: App::T3, mode: Mode::Write, base: None, traces: None }, &tree, &history, &MemoryTraces::default(), &AdmitOnly::default());
        assert!(!write.ok && write.writes.is_empty());
        assert!(write.report["error"].as_str().unwrap().contains("bbb") && write.report["error"].as_str().unwrap().contains("aaa"));
        // When a check is asked, Then it falls back to the methods list, which is missing here
        let check = run(&Request { app: App::T3, mode: Mode::Check, base: None, traces: None }, &tree, &history, &MemoryTraces::default(), &AdmitOnly::default());
        assert!(!check.ok);
        assert!(check.report["error"].as_str().unwrap().contains("t3-methods.json is missing"), "{}", check.report);
    }
}
