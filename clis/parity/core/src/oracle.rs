//! `wf parity coverage --by-oracle`: for every served row, which test serves it and whether a recording could (class a to d below), and for
//! every behaviour test the host state it sets up that the mimic driver cannot yet. Everything is read from the checked-in table, the decisions,
//! the cassettes' interaction row labels and their `known-gaps.json`, and the behaviour tests' source; no server runs.
use crate::coverage::{Decisions, Problem};
use crate::ports::Tree;
use crate::{refusal, App, Mode, Outcome};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Clean,
    Differs,
    Missing,
    Unrecordable,
}

const CLASSES: [Class; 4] = [Class::Clean, Class::Differs, Class::Missing, Class::Unrecordable];

impl Class {
    pub fn key(self) -> &'static str {
        match self {
            Class::Clean => "a",
            Class::Differs => "b",
            Class::Missing => "c",
            Class::Unrecordable => "d",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Class::Clean => "(a) recorded, clean or drift-only gaps: switch the row to `\"oracle\": \"mimic\"`",
            Class::Differs => "(b) recorded, listed as a breaking difference in `known-gaps.json`: fix the adapter or list the gap",
            Class::Missing => "(c) no interaction in any recorded set: R3 to R7 add the case",
            Class::Unrecordable => "(d) a recording cannot show it: the reason is named",
        }
    }
}

/// The host state a behaviour test sets up, with the source text that shows it. Each needle is looked for on the lines that are not comments.
const HOST_STATE: [(&str, &[&str]); 5] = [
    ("scripted agent turn", &["Script::of(", ".script(", "Turn::"]),
    ("gh double", &["start_hosted(", "HostedClis", "hosted::Start"]),
    ("restart", &["restart_serve(", ".restart("]),
    ("clock", &["Clock::", "advance_clock(", "set_clock("]),
    ("service env", &["start_with_env("]),
];

struct Hit {
    set: String,
    /// `Some(breaking)` when the interaction is listed in the set's `known-gaps.json`.
    gap: Option<bool>,
}

struct Served {
    id: String,
    suite: String,
    test: String,
}

impl Served {
    fn chain_file(&self) -> Option<&str> {
        self.suite.ends_with("-integration").then(|| self.test.split("::").next().unwrap_or(""))
    }

    fn served_by(&self) -> String {
        match self.chain_file() {
            Some(file) => format!("chain `{file}`"),
            None => format!("replay `{}`", self.suite),
        }
    }
}

struct Scan {
    tests: usize,
    found: Vec<(&'static str, usize)>,
}

fn json_of(tree: &dyn Tree, path: &str) -> Option<Value> {
    serde_json::from_str(&tree.read(path)?).ok()
}

/// Interaction id to whether its listed difference is breaking, from either schema of `known-gaps.json`.
fn listed(file: Option<Value>) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    let Some(file) = file else { return out };
    let ids = |g: &Value| g["ids"].as_array().into_iter().flatten().filter_map(|i| i.as_str().map(String::from)).collect::<Vec<_>>();
    for g in file["groups"].as_array().into_iter().flatten() {
        let breaking = g["severity"].as_str() != Some("drift");
        out.extend(ids(g).into_iter().map(|id| (id, breaking)));
    }
    for g in file["gaps"].as_array().into_iter().flatten() {
        let breaking = g["diffs"].as_array().into_iter().flatten().filter_map(Value::as_str).any(|d| d.ends_with(" breaking"));
        if let Some(id) = g["id"].as_str() {
            out.insert(id.to_string(), breaking);
        }
    }
    out
}

/// The recorded sets of the app (`provenance.kind` is `recorded`; authored and imported sets are our own claims) by row label.
fn recorded(tree: &dyn Tree, app: App) -> (BTreeMap<String, Vec<Hit>>, Vec<String>) {
    let (mut rows, mut sets): (BTreeMap<String, Vec<Hit>>, Vec<String>) = (BTreeMap::new(), vec![]);
    for cassette in tree.files(&app.cassette_dir()).into_iter().filter(|p| p.ends_with("/cassette.json")) {
        let dir = cassette.trim_end_matches("cassette.json");
        if json_of(tree, &cassette).is_none_or(|c| c["provenance"]["kind"] != "recorded") {
            continue;
        }
        let set = dir.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        let gaps = listed(json_of(tree, &format!("{dir}known-gaps.json")));
        for line in tree.read(&format!("{dir}interactions.jsonl")).unwrap_or_default().lines() {
            let Ok(i) = serde_json::from_str::<Value>(line) else { continue };
            if let (Some(id), Some(row)) = (i["id"].as_str(), i["row"].as_str()) {
                rows.entry(row.to_string()).or_default().push(Hit { set: set.clone(), gap: gaps.get(id).copied() });
            }
        }
        sets.push(set);
    }
    sets.sort();
    (rows, sets)
}

fn scan(source: &str) -> Scan {
    let code: Vec<&str> = source.lines().filter(|l| !l.trim_start().starts_with("//")).collect();
    let found = HOST_STATE.iter().filter_map(|(name, needles)| Some((*name, code.iter().map(|l| needles.iter().filter(|n| l.contains(*n)).count()).sum::<usize>())).filter(|(_, n)| *n > 0)).collect();
    Scan { tests: code.iter().filter(|l| l.trim() == "#[test]").count(), found }
}

fn served_rows(table: &Value) -> Vec<Served> {
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    table["rows"].as_array().into_iter().flatten().filter(|r| r["state"] == "served").map(|r| Served { id: text(&r["id"]), suite: text(&r["evidence"]["suite"]), test: text(&r["evidence"]["test"]) }).collect()
}

struct Classified<'a> {
    row: &'a Served,
    class: Class,
    detail: String,
}

fn classify<'a>(rows: &'a [Served], recorded: &BTreeMap<String, Vec<Hit>>, decisions: &Decisions) -> Vec<Classified<'a>> {
    rows.iter()
        .map(|row| {
            let hits = recorded.get(&row.id).map(Vec::as_slice).unwrap_or_default();
            let (listed, breaking) = (hits.iter().filter(|h| h.gap.is_some()).count(), hits.iter().filter(|h| h.gap == Some(true)).count());
            let mut sets: BTreeMap<&str, usize> = BTreeMap::new();
            for h in hits {
                *sets.entry(h.set.as_str()).or_default() += 1;
            }
            let recording = format!("{}; {listed} gap-listed, {breaking} breaking", sets.iter().map(|(set, n)| format!("{set} x{n}")).collect::<Vec<_>>().join(", "));
            let (class, detail) = match (decisions.unrecordable(&row.id, &row.test), hits.is_empty(), breaking) {
                (Some(why), _, _) => (Class::Unrecordable, why),
                (None, true, _) => (Class::Missing, "no interaction".to_string()),
                (None, false, 0) => (Class::Clean, recording),
                (None, false, _) => (Class::Differs, recording),
            };
            Classified { row, class, detail }
        })
        .collect()
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn markdown(app: App, sets: &[String], rows: &[Classified], files: &BTreeMap<String, (Option<Scan>, usize)>) -> String {
    let title = if app == App::T3 { "T3 Code" } else { "Orca" };
    let count = |c: Class| rows.iter().filter(|r| r.class == c).count();
    let chain = rows.iter().filter(|r| r.row.chain_file().is_some()).count();
    let name = app.name();
    let mut md = format!(
        "# {title} parity by oracle\n\nGenerated by `wf parity coverage --app {name} --by-oracle` from `{name}.json`, `{name}-decisions.json`, the interaction row labels and `known-gaps.json` of the recorded sets under `{}` ({}) and the behaviour tests' source. Do not edit by hand.\n\n",
        app.cassette_dir(),
        sets.join(", ")
    );
    md.push_str(&format!("{} served rows, {chain} of them by a behaviour test.\n\n| Class | Rows | Behaviour-served rows |\n| --- | --- | --- |\n", rows.len()));
    for c in CLASSES {
        md.push_str(&format!("| {} | {} | {} |\n", c.title(), count(c), rows.iter().filter(|r| r.class == c && r.row.chain_file().is_some()).count()));
    }
    md.push_str("\nA row is class (a) when it has a recorded interaction and every listed gap of its interactions is drift; (b) when one is listed as breaking; (c) when no recorded set has an interaction labelled with it; (d) when its decision says `unrecordable` or its test is on the `unrecordableTests` list of the decisions file (both name the reason).\n");
    md.push_str("\n## Host state the behaviour tests set up that the mimic driver cannot\n\nFound by scanning each test's source for the exec-testkit helpers (comments skipped).\n\n| Behaviour test | Tests | Rows served | Scripted agent turn | gh double | Restart | Clock | Service env |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for (file, (scanned, served)) in files {
        let times = |what: &str| scanned.as_ref().and_then(|s| s.found.iter().find(|(n, _)| *n == what).map(|(_, n)| format!("{n}"))).unwrap_or_else(|| "none".into());
        let tests = scanned.as_ref().map_or("source missing".to_string(), |s| s.tests.to_string());
        md.push_str(&format!("| `{file}` | {tests} | {served} | {} | {} | {} | {} | {} |\n", times("scripted agent turn"), times("gh double"), times("restart"), times("clock"), times("service env")));
    }
    for c in CLASSES {
        md.push_str(&format!("\n## {}\n\n", c.title()));
        let mut any = false;
        for r in rows.iter().filter(|r| r.class == c) {
            if !any {
                md.push_str("| Row | Served by | Detail |\n| --- | --- | --- |\n");
                any = true;
            }
            md.push_str(&format!("| `{}` | {} | {} |\n", cell(&r.row.id), r.row.served_by(), cell(&r.detail)));
        }
        if !any {
            md.push_str("None.\n");
        }
    }
    md
}

pub fn run(app: App, mode: Mode, tree: &dyn Tree) -> Outcome {
    let table_path = app.table_file();
    let Some(table) = json_of(tree, &table_path) else {
        return refusal(app, format!("{table_path} is missing or not JSON"), format!("wf parity coverage --app {}", app.name()));
    };
    let decisions = tree.read(&app.decisions_file()).and_then(|t| Decisions::parse(&t).ok()).unwrap_or_default();
    let served = served_rows(&table);
    let (by_row, sets) = recorded(tree, app);
    let rows = classify(&served, &by_row, &decisions);
    let mut problems = vec![];
    let mut files: BTreeMap<String, (Option<Scan>, usize)> = BTreeMap::new();
    for row in &served {
        if let Some(file) = row.chain_file() {
            let entry = files.entry(file.to_string()).or_insert_with(|| (tree.read(&format!("{}/{file}.rs", app.chain_dir())).map(|s| scan(&s)), 0));
            entry.1 += 1;
        }
    }
    let missing: Vec<String> = files.iter().filter(|(_, (s, _))| s.is_none()).map(|(f, _)| f.clone()).collect();
    if !missing.is_empty() {
        problems.push(Problem { rule: "chain-source-missing", message: format!("the evidence names behaviour tests whose source is not under {}", app.chain_dir()), rows: missing });
    }
    let text = markdown(app, &sets, &rows, &files);
    let path = app.oracle_file();
    let stale = tree.read(&path).as_deref() != Some(text.as_str());
    if mode == Mode::Check && stale {
        problems.push(Problem { rule: "stale", message: "the checked-in table differs from what the generator builds".into(), rows: vec![path.clone()] });
    }
    let count = |c: Class, chain_only: bool| rows.iter().filter(|r| r.class == c && (!chain_only || r.row.chain_file().is_some())).count();
    let by_class = |chain_only: bool| CLASSES.iter().map(|c| (c.key().to_string(), json!(count(*c, chain_only)))).collect::<serde_json::Map<_, _>>();
    let chain_tests: Vec<Value> = files
        .iter()
        .map(|(file, (s, n))| json!({"file": file, "rows": n, "tests": s.as_ref().map(|s| s.tests), "hostState": s.as_ref().map(|s| s.found.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>())}))
        .collect();
    let ok = mode == Mode::Write || problems.is_empty();
    let mut report = json!({
        "ok": ok,
        "app": app.name(),
        "mode": if mode == Mode::Write { "write" } else { "check" },
        "by": "oracle",
        "served": rows.len(),
        "counts": by_class(false),
        "chainCounts": by_class(true),
        "recordedSets": sets,
        "chainTests": chain_tests,
        "problems": problems.iter().map(Problem::to_json).collect::<Vec<_>>(),
    });
    let writes = if mode == Mode::Write && stale { vec![(path.clone(), text)] } else { vec![] };
    if mode == Mode::Write {
        report["wrote"] = json!(writes.iter().map(|(p, _)| p).collect::<Vec<_>>());
    }
    Outcome { ok, report, writes }
}
