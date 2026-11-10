//! `wf mimic record|replay|diff|verify`: parse the line, run the core over the disk and a driver, print one JSON object (schema
//! `mimic-<verb>/1`). Exit 0 clean, 1 findings, 2 usage or environment; nothing prompts and every failure names its `next` step.
#[path = "crawl_verb.rs"]
mod crawl_verb;
#[path = "stabilize_verb.rs"]
mod stabilize_verb;
#[path = "bundle_verb.rs"]
mod bundle_verb;
#[path = "job_verb.rs"]
mod job_verb;
#[path = "provision_verb.rs"]
mod provision_verb;
#[path = "trigger_verb.rs"]
mod trigger_verb;

use crate::adapters::{FsCheckpoint, FsStore, StdioDriver};
use crate::config::{relative, Config, Target};
use mimic_core::allowlist::Allowlist;
use mimic_core::cassette::{Current, Kind, ToolInfo};
use mimic_core::corpus::{corpus_hash, expand, Case, CorpusSpec};
use mimic_core::import::{self as legacy, Legacy};
use mimic_core::differ::{diff_cassettes, Label, Verdict};
use mimic_core::fixtures::{scan, FixtureFile, FixtureRules};
use mimic_core::ports::{CassetteSet, Store, StoreError};
use mimic_core::rules::RuleSet;
use mimic_core::ports::Checkpoint;
use mimic_core::record::{record_planned, DoubleRun, Plan, Progress};
use mimic_core::usecase::{evidence, record as record_set, replay as replay_set, verify as verify_set, Meta};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

const RECORD: Spec = Spec { values: &["target", "seed", "recorded-on", "source", "stand-in", "chunk", "batch", "write-chunk", "stability", "max-minutes"], flags: &["keep-raw", "allow-nondeterministic", "resume", "fresh"], positional: Positional::Exactly(1), ..Spec::NONE };
const REPLAY: Spec = Spec { values: &["target", "candidate", "only", "allowlist", "suite"], positional: Positional::Exactly(1), ..Spec::NONE };
const DIFF: Spec = Spec { values: &["allowlist"], positional: Positional::Exactly(2), ..Spec::NONE };
const IMPORT: Spec = Spec { values: &["out", "root", "allowlist", "pin"], positional: Positional::Exactly(2), ..Spec::NONE };
const EXPORT: Spec = Spec { values: &["out"], positional: Positional::Exactly(2), ..Spec::NONE };
const CRAWL: Spec = Spec { values: &["root"], flags: &["all", "check", "fresh"], positional: Positional::Any, ..Spec::NONE };
const STABILIZE: Spec = Spec { values: &["allowlist", "min-pct", "report"], flags: &["write"], positional: Positional::Any, ..Spec::NONE };
const REMASK: Spec = Spec { positional: Positional::Exactly(1), ..Spec::NONE };
const TRIGGER: Spec = Spec { values: &["message", "file", "root"], positional: Positional::None, ..Spec::NONE };
const PROVISION: Spec = Spec { values: &["work-root", "root", "when"], flags: &["check", "force"], positional: Positional::Exactly(1), ..Spec::NONE };
const JOB: Spec = Spec { values: &["chunk", "budget", "out", "summary", "run", "candidate", "work-root", "root"], flags: &["fresh"], positional: Positional::Exactly(2), ..Spec::NONE };
const PUBLISH: Spec = Spec { values: &["run", "root", "summary"], positional: Positional::Any, ..Spec::NONE };
const FETCH: Spec = Spec { values: &["set", "root"], positional: Positional::Exactly(1), ..Spec::NONE };
const VERIFY: Spec = Spec { values: &["root", "fixtures", "rules"], flags: &["all", "live", "files"], positional: Positional::Any, ..Spec::NONE };

/// What a verb prints when it cannot run at all (exit 2).
struct Fail {
    error: String,
    next: String,
    evidence: Value,
}

fn fail(error: impl Into<String>, next: impl Into<String>) -> Fail {
    Fail { error: error.into(), next: next.into(), evidence: json!({}) }
}

type Done = Result<(Value, i32), Fail>;

fn finish(schema: &str, done: Done) -> Option<i32> {
    let (mut out, code) = match done {
        Ok(pair) => pair,
        Err(f) => (json!({"ok": false, "error": f.error, "next": f.next, "evidence": f.evidence, "warnings": []}), 2),
    };
    out["schema"] = json!(schema);
    println!("{out}");
    Some(code)
}

pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    match verb.as_str() {
        "record" => finish("mimic-record/1", record(&parse_spec(rest, &RECORD)?)),
        "replay" => finish("mimic-replay/1", replay(&parse_spec(rest, &REPLAY)?)),
        "diff" => finish("mimic-diff/1", diff(&parse_spec(rest, &DIFF)?)),
        "verify" => {
            let p = parse_spec(rest, &VERIFY)?;
            if p.val("fixtures").is_some() {
                return finish("mimic-fixtures/1", fixtures(&p));
            }
            finish("mimic-verify/1", verify(&p))
        }
        "crawl" => finish("mimic-crawl/1", crawl_verb::crawl_verb(&parse_spec(rest, &CRAWL)?)),
        "stabilize" => finish("mimic-stability/1", stabilize_verb::stabilize_verb(&parse_spec(rest, &STABILIZE)?)),
        "remask" => finish("mimic-remask/1", stabilize_verb::remask_verb(&parse_spec(rest, &REMASK)?)),
        "trigger" => finish("mimic-trigger/1", trigger_verb::trigger_verb(&parse_spec(rest, &TRIGGER)?)),
        "provision" => finish("mimic-provision/1", provision_verb::provision_verb(&parse_spec(rest, &PROVISION)?)),
        "job" => finish("mimic-job/1", job_verb::job_verb(&parse_spec(rest, &JOB)?)),
        "publish" => finish("mimic-publish/1", bundle_verb::publish_verb(&parse_spec(rest, &PUBLISH)?)),
        "fetch" => finish("mimic-fetch/1", bundle_verb::fetch_verb(&parse_spec(rest, &FETCH)?)),
        "import" => finish("mimic-import/1", import(&parse_spec(rest, &IMPORT)?)),
        "export" => finish("mimic-export/1", export(&parse_spec(rest, &EXPORT)?)),
        _ => None,
    }
}

struct SetRef {
    dir: PathBuf,
    name: String,
    store: FsStore,
}

fn set_ref(arg: &str) -> SetRef {
    let dir = std::path::absolute(arg).unwrap_or_else(|_| PathBuf::from(arg));
    let name = dir.file_name().map_or_else(|| arg.to_string(), |n| n.to_string_lossy().into_owned());
    let root = dir.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    SetRef { dir, name, store: FsStore::new(root) }
}

impl SetRef {
    fn config_path(&self, p: &Parsed) -> PathBuf {
        p.val("target").map_or_else(|| self.dir.join("mimic.toml"), PathBuf::from)
    }

    fn read(&self) -> Result<CassetteSet, Fail> {
        self.store.read(&self.name).map_err(|e| {
            let next = match e {
                StoreError::NotFound(_) => format!("wf mimic record {}", self.dir.display()),
                _ => format!("wf mimic verify {}", self.dir.display()),
            };
            fail(e.to_string(), next)
        })
    }
}

fn read_json<T: DeserializeOwned>(path: &Path, what: &str) -> Result<T, Fail> {
    let text = std::fs::read_to_string(path).map_err(|e| fail(format!("{what} {}: {e}", path.display()), format!("create {} (see wf mimic --help)", path.display())))?;
    serde_json::from_str(&text).map_err(|e| fail(format!("{what} {} is not valid: {e}", path.display()), format!("fix {}", path.display())))
}

fn load_config(path: &Path) -> Result<Config, Fail> {
    Config::load(path).map_err(|e| fail(e, "write the set's mimic.toml (wf mimic --help documents it)"))
}

fn reference(cfg: &Config) -> Result<&Target, Fail> {
    cfg.reference.as_ref().ok_or_else(|| fail("mimic.toml has no [reference]", "add a [reference] table with a command and a pin"))
}

fn tool() -> ToolInfo {
    ToolInfo { mimic: env!("CARGO_PKG_VERSION").into(), git: option_env!("WF_BUILD_SHA").map(String::from) }
}

fn ids(p: &Parsed, name: &str) -> Option<Vec<String>> {
    p.val(name).map(|v| v.split(',').filter(|s| !s.is_empty()).map(String::from).collect())
}

fn allowlist(path: Option<PathBuf>, pin_hash: Option<&str>) -> Result<(Allowlist, Vec<String>), Fail> {
    let Some(path) = path else { return Ok((Allowlist::default(), Vec::new())) };
    let text = std::fs::read_to_string(&path).map_err(|e| fail(format!("allowlist {}: {e}", path.display()), "fix the path or drop --allowlist"))?;
    let all = Allowlist::parse(&text).map_err(|e| fail(e.to_string(), format!("fix {}", path.display())))?;
    let (live, expired) = all.active(pin_hash);
    Ok((live, expired.iter().map(|e| format!("allowlist entry {} expired: it was written against another pin", e.path)).collect()))
}

fn meta(cfg: &Config, set_dir: &Path, notes: (Option<&str>, Option<&str>), spec: &CorpusSpec, target: mimic_core::cassette::Target) -> Meta {
    Meta {
        kind: Kind::Recorded,
        target: Some(target),
        tool: tool(),
        seed: Some(spec.seed),
        recorded_at: Some(wf_kit::time::iso(wf_kit::time::now_ms())),
        recorded_on: notes.0.map(String::from),
        source: notes.1.map(String::from),
        rules_path: relative(set_dir, &cfg.rules),
        corpus_path: cfg.corpus.as_ref().map(|c| relative(set_dir, c)),
    }
}

/// The set's corpus spec and the cases it expands to; the seed of the line replaces the file's.
fn cases(cfg: &Config, set: &SetRef, seed: Option<u64>) -> Result<(CorpusSpec, Vec<Case>), Fail> {
    let path = cfg.corpus.as_ref().ok_or_else(|| fail("mimic.toml names no corpus", "add corpus = \"corpus.json\" (schema mimic-corpus/1)"))?;
    let mut spec: CorpusSpec = read_json(path, "corpus")?;
    if let Some(seed) = seed {
        spec.seed = seed;
    }
    let cases = expand(&spec, &set.store, &set.name).map_err(|e| fail(e.to_string(), format!("fix {}", path.display())))?;
    Ok((spec, cases))
}

const MAX_REPORTED: usize = 40;

/// Which paths differed between the stability passes of each nondeterministic interaction: the evidence for the rule that masks them.
fn differences(recording: &mimic_core::usecase::Recording) -> Value {
    Value::Object(recording.differences.iter().take(MAX_REPORTED).map(|(id, d)| (id.clone(), json!(d))).collect())
}

fn record(p: &Parsed) -> Done {
    let set = set_ref(&p.positional[0]);
    let cfg = load_config(&set.config_path(p))?;
    let reference = reference(&cfg)?;
    let seed = p.val("seed").map(|s| s.parse::<u64>().map_err(|_| fail(format!("--seed {s} is not a number"), "wf mimic record --help"))).transpose()?;
    let rules: RuleSet = read_json(&cfg.rules, "rules")?;
    let (spec, cases) = cases(&cfg, &set, seed)?;
    let target = reference.provenance().map_err(|e| fail(e, "fix the [reference] pin in mimic.toml"))?;
    let stand_in = p.val("stand-in");
    if target.pin_hash.is_none() && stand_in.is_none() {
        return Err(fail("the [reference] has no pin: a recorded cassette names the pin file it was recorded against", "add pin = \"<pin file>\" to [reference] in mimic.toml"));
    }
    let number = |name: &str| -> Result<Option<usize>, Fail> { p.val(name).map(|v| v.parse::<usize>().map_err(|_| fail(format!("--{name} {v} is not a number"), "wf mimic record --help"))).transpose() };
    let batch = number("batch")?.or(cfg.record.batch).unwrap_or(1).max(1);
    let pct = number("stability")?.map_or(cfg.record.stability_pct.unwrap_or(100), |v| u8::try_from(v.min(100)).unwrap_or(100));
    let until = p
        .val("max-minutes")
        .map(|v| v.parse::<f64>().ok().filter(|m| m.is_finite() && *m > 0.0 && *m <= 600.0).map(|m| std::time::Instant::now() + std::time::Duration::from_secs_f64(m * 60.0)).ok_or_else(|| fail(format!("--max-minutes {v} is not a number of minutes above 0 and up to 600"), "wf mimic record --help")))
        .transpose()?;
    let plan = Plan {
        double_run: if pct >= 100 { DoubleRun::All } else { DoubleRun::Sample { pct, seed: spec.seed } },
        batch,
        write_chunk: number("write-chunk")?.or(cfg.record.write_chunk).unwrap_or(0),
        chunk: number("chunk")?,
        until,
        keep_raw: p.flag("keep-raw"),
    };
    let mut command = reference.spec.clone();
    if batch > 1 && !command.env.iter().any(|(k, _)| k == "MIMIC_DRIVER_CONCURRENCY") {
        command.env.push(("MIMIC_DRIVER_CONCURRENCY".into(), batch.to_string()));
    }
    let mut driver = StdioDriver::new(command);
    let mut checkpoint = FsCheckpoint::new(&set.dir);
    let again = format!("wf mimic record {} --resume", set.dir.display());
    if p.flag("fresh") {
        checkpoint.clear().map_err(|e| fail(e.to_string(), "make the set directory writable"))?;
    } else if checkpoint.exists() && !p.flag("resume") {
        return Err(fail("the set holds the checkpoint of an interrupted recording", format!("{again}  (carries on from it), or wf mimic record {} --fresh (starts over)", set.dir.display())));
    }
    let mut meta = meta(&cfg, &set.dir, (p.val("recorded-on"), p.val("source")), &spec, target);
    if let Some(what) = stand_in {
        // a double is no reference: the set claims no pin and can never serve a parity row
        meta.kind = Kind::Authored;
        meta.target = meta.target.map(|t| mimic_core::cassette::Target { pin_hash: None, ..t });
        meta.source.get_or_insert_with(|| format!("stand-in: {what}"));
    }
    let progress = record_planned(&mut driver, &cases, &rules, Some(&spec), meta, &plan, &mut checkpoint).map_err(|e| {
        let saved = if checkpoint.exists() { format!("; what was recorded is saved: {again}") } else { String::new() };
        fail(e.to_string(), format!("check the [reference] command of mimic.toml runs and speaks mimic-driver/1 (wf mimic --help){saved}"))
    })?;
    let mut recording = match progress {
        Progress::Done(r) => *r,
        Progress::Partial { recorded, total } => {
            let out = json!({"ok": true, "done": false, "complete": false, "set": set.name, "recorded": recorded, "total": total, "remaining": total - recorded,
                "checkpoint": set.dir.join(crate::adapters::CHECKPOINT_DIR).display().to_string(), "wrote": [], "warnings": [], "next": again});
            return Ok((out, 0));
        }
    };
    let mut store = FsStore::new(set.dir.parent().unwrap_or(Path::new(".")));
    let mut warnings: Vec<String> = Vec::new();
    let mut raw_path = None;
    if p.flag("keep-raw") || !recording.nondeterministic.is_empty() {
        let lines: String = recording.raw.iter().map(|(id, o)| format!("{}\n", json!({"id": id, "observed": o}))).collect();
        store.write_raw(&set.name, "capture.jsonl", lines.as_bytes()).map_err(|e| fail(e.to_string(), "make the set directory writable"))?;
        raw_path = Some(set.dir.join("raw/capture.jsonl").display().to_string());
    }
    let provenance = serde_json::to_value(&recording.set.cassette.header.provenance).expect("provenance serialises");
    if !recording.nondeterministic.is_empty() {
        if !p.flag("allow-nondeterministic") {
            let out = json!({
                "ok": false, "set": set.name, "interactions": cases.len(), "provenance": provenance, "nondeterministic": recording.nondeterministic, "differences": differences(&recording),
                "wrote": [], "evidence": {"raw": raw_path}, "warnings": warnings,
                "error": "two runs of the reference differ after masking: nothing was written",
                "next": "mask what varies with a rule in rules.json (wf mimic --help lists the kinds), or record it flagged with --allow-nondeterministic",
            });
            return Ok((out, 1));
        }
        for i in recording.set.cassette.interactions.iter_mut().filter(|i| recording.nondeterministic.contains(&i.id)) {
            i.tags.push("nondeterministic".into());
        }
        recording.set.cassette.seal();
        warnings.push(format!("{} interaction(s) are tagged nondeterministic: they replay only up to the masks", recording.nondeterministic.len()));
    }
    store.write(&set.name, &recording.set).map_err(|e| fail(e.to_string(), "make the set directory writable"))?;
    let mut wrote: Vec<String> = set.store.files_of(&set.name).iter().map(|f| f.display().to_string()).collect();
    wrote.extend(raw_path.clone());
    wf_kit::logfmt::mcp("info", "mimic", "record.done", &[("set", set.name.clone()), ("interactions", cases.len().to_string())]);
    let out = json!({
        "ok": true, "done": true, "complete": true, "set": set.name, "interactions": cases.len(), "provenance": provenance, "nondeterministic": recording.nondeterministic, "differences": differences(&recording),
        "wrote": wrote, "warnings": warnings, "next": format!("wf mimic verify {}", set.dir.display()),
    });
    Ok((out, 0))
}

fn pick<'a>(cfg: &'a Config, wanted: Option<&str>) -> Result<(&'a String, &'a Target), Fail> {
    let names = || cfg.candidates.keys().cloned().collect::<Vec<_>>().join(", ");
    match wanted {
        Some(n) => cfg.candidates.get_key_value(n).ok_or_else(|| fail(format!("mimic.toml has no [candidate.{n}] (it has: {})", names()), "wf mimic replay <set> --candidate <name>")),
        None if cfg.candidates.len() == 1 => Ok(cfg.candidates.iter().next().expect("one")),
        None => Err(fail(format!("name the candidate with --candidate (mimic.toml has: {})", names()), "wf mimic replay <set> --candidate <name>")),
    }
}

fn labelled(verdict: &Verdict) -> Vec<&mimic_core::differ::InteractionResult> {
    verdict.results.iter().filter(|r| r.label != Label::Clean).collect()
}

fn replay(p: &Parsed) -> Done {
    let set = set_ref(&p.positional[0]);
    let content = set.read()?;
    let cfg = load_config(&set.config_path(p))?;
    let (name, candidate) = pick(&cfg, p.val("candidate"))?;
    let pin_hash = content.cassette.header.provenance.target.as_ref().and_then(|t| t.pin_hash.clone());
    let (allow, mut warnings) = allowlist(p.val("allowlist").map(PathBuf::from).or_else(|| cfg.allowlist.clone()), pin_hash.as_deref())?;
    let only = ids(p, "only");
    if let Some(only) = &only {
        let unknown: Vec<&String> = only.iter().filter(|id| !content.cassette.interactions.iter().any(|i| &i.id == *id)).collect();
        if !unknown.is_empty() {
            return Err(fail(format!("--only names interactions the cassette does not have: {unknown:?}"), format!("wf mimic replay {}", set.dir.display())));
        }
    }
    let started = wf_kit::time::now_ms();
    let mut driver = StdioDriver::new(candidate.spec.clone());
    let verdict = replay_set(&content, &mut driver, &allow, &cfg.diff, only.as_deref()).map_err(|e| fail(e.to_string(), "fix rules.json"))?;
    let rows = evidence(&verdict, &content.cassette);
    let mut evidence_paths = json!({"report": null, "trace": null});
    if let Some(suite) = p.val("suite") {
        let (report, trace) = write_suite(suite, &set.name, &content, &verdict, started).map_err(|e| fail(e, "set WF_TEST_OUTPUT_DIR to a writable directory"))?;
        evidence_paths = json!({"report": report.display().to_string(), "trace": trace.display().to_string()});
    }
    warnings.extend(verdict.unused_allowlist.iter().map(|u| format!("allowlist entry {u} matched nothing: stale or over-broad")));
    let ok = verdict.ok();
    wf_kit::logfmt::mcp("info", "mimic", "replay.done", &[("set", set.name.clone()), ("severity", format!("{:?}", verdict.severity))]);
    let first = labelled(&verdict).first().map(|r| r.id.clone());
    let next = match (&first, ok) {
        (_, true) => "nothing to do".to_string(),
        (Some(id), _) => format!("wf mimic replay {} --candidate {name} --only {id}  (fix the candidate, or allowlist the path with a reason)", set.dir.display()),
        _ => "see results".to_string(),
    };
    let out = json!({
        "ok": ok, "set": set.name, "candidate": name, "severity": verdict.severity, "summary": verdict.summary, "results": labelled(&verdict),
        "shrunk": verdict.shrunk, "evidence_rows": rows.len(), "unused_allowlist": verdict.unused_allowlist, "evidence": evidence_paths, "warnings": warnings, "next": next,
    });
    Ok((out, i32::from(!ok)))
}

/// `$WF_TEST_OUTPUT_DIR/<suite>/{report.json, trace.json}`: the harness run record and the method trace (`method-trace/1`) with the
/// `diff` label and parity `row` of every interaction and the cassette's provenance `kind`, which `wf parity coverage --traces` reads.
fn write_suite(suite: &str, set: &str, content: &CassetteSet, verdict: &Verdict, started: i64) -> Result<(PathBuf, PathBuf), String> {
    let base = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()).ok_or("--suite needs WF_TEST_OUTPUT_DIR")?;
    let dir = PathBuf::from(base).join(suite);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let finished = wf_kit::time::now_ms();
    let passed = |l: Label| matches!(l, Label::Clean | Label::Allowlisted);
    // one test per interaction: a row is served by a passing test, so one drifting interaction must not fail the others
    let kind = &content.cassette.header.provenance.kind;
    let tests: Vec<Value> = verdict.results.iter().map(|r| json!({"test": format!("{set}::{}", r.id), "cassette": kind, "verdict": if passed(r.label) { "pass" } else { "fail" }, "ms": 0,
        "calls": [{"id": r.id, "rows": r.row.iter().collect::<Vec<_>>(), "ok": passed(r.label), "diff": r.label, "agent": [], "executor": []}]})).collect();
    let app = content.cassette.header.provenance.target.as_ref().map_or("mimic", |t| t.name.as_str());
    let trace = json!({"schema": "method-trace/1", "app": app, "suite": suite, "sha": Value::Null, "complete": true, "tests": tests});
    let cases: Vec<Value> = verdict.results.iter().map(|r| json!({"name": r.id, "verdict": if passed(r.label) { "pass" } else { "fail" }, "ms": 0,
        "message": r.error.clone().or_else(|| r.diffs.first().map(|d| format!("{:?} at {}", d.kind, d.path)))})).collect();
    let report = json!({
        "schema": 1, "verdict": if verdict.ok() { "pass" } else { "fail" }, "ok": verdict.ok(), "exitCode": i32::from(!verdict.ok()), "message": Value::Null,
        "expectedCases": verdict.results.len(), "script": suite, "platform": "integration", "sha": Value::Null,
        "startedMs": started, "finishedMs": finished, "started": wf_kit::time::iso(started), "finished": wf_kit::time::iso(finished),
        "durationS": ((finished - started).max(0) as f64 / 100.0).round() / 10.0, "recordDir": dir.display().to_string(),
        "cases": cases, "summary": verdict.summary, "warnings": [],
    });
    let write = |name: &str, v: &Value| -> Result<PathBuf, String> {
        let path = dir.join(name);
        std::fs::write(&path, serde_json::to_string_pretty(v).expect("prints") + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    };
    Ok((write("report.json", &report)?, write("trace.json", &trace)?))
}

/// A set without a mimic.toml has no config; one that exists but does not load is an error, never the same as absent.
fn load_if_present(dir: &Path) -> Result<Option<Config>, String> {
    let file = dir.join("mimic.toml");
    if file.exists() { Config::load(&file).map(Some) } else { Ok(None) }
}

fn diff(p: &Parsed) -> Done {
    let (a, b) = (set_ref(&p.positional[0]), set_ref(&p.positional[1]));
    let (ca, cb) = (a.read()?, b.read()?);
    let cfg = load_if_present(&a.dir).map_err(|e| fail(e, "fix the mimic.toml of <a> (wf mimic --help documents it)"))?;
    let pin_hash = ca.cassette.header.provenance.target.as_ref().and_then(|t| t.pin_hash.clone());
    let path = p.val("allowlist").map(PathBuf::from).or_else(|| cfg.as_ref().and_then(|c| c.allowlist.clone()));
    let (allow, mut warnings) = allowlist(path, pin_hash.as_deref())?;
    let opts = cfg.map(|c| c.diff).unwrap_or_default();
    let verdict = diff_cassettes(&ca.cassette, &cb.cassette, &allow, &opts);
    warnings.extend(verdict.unused_allowlist.iter().map(|u| format!("allowlist entry {u} matched nothing: stale or over-broad")));
    let ok = verdict.ok();
    let next = if ok { "nothing to do".to_string() } else { format!("review the results; a pin bump that is intended is re-recorded with wf mimic record {}", a.dir.display()) };
    let out = json!({"ok": ok, "a": a.name, "b": b.name, "severity": verdict.severity, "summary": verdict.summary, "results": labelled(&verdict),
        "unused_allowlist": verdict.unused_allowlist, "warnings": warnings, "next": next});
    Ok((out, i32::from(!ok)))
}

const SKIP: &[&str] = &[".git", "node_modules", "target", "references", ".nx", ".agents"];

fn find_sets(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut sorted: Vec<_> = entries.flatten().collect();
    sorted.sort_by_key(|e| e.file_name());
    for e in sorted {
        let path = e.path();
        if path.is_dir() && !SKIP.iter().any(|s| e.file_name() == *s) {
            if path.join("mimic.toml").is_file() && path.join("cassette.json").is_file() {
                out.push(path.clone());
            }
            find_sets(&path, out);
        }
    }
}

fn verify_one(set: &SetRef, live: bool, named: bool) -> Value {
    let content = match set.read() {
        Ok(c) => c,
        Err(f) => return json!({"set": set.name, "dir": set.dir.display().to_string(), "ok": false, "error": f.error, "next": f.next}),
    };
    let cfg = match load_if_present(&set.dir) {
        Ok(c) => c,
        Err(e) => return json!({"set": set.name, "dir": set.dir.display().to_string(), "ok": false, "error": e, "next": "fix mimic.toml (wf mimic --help documents it)"}),
    };
    let mut warnings: Vec<String> = Vec::new();
    let mut current = Current { pin_hash: content.cassette.header.provenance.target.as_ref().and_then(|t| t.pin_hash.clone()), rules_hash: content.rules.hash(), corpus_hash: content.cassette.header.corpus.as_ref().map(|c| c.hash.clone()) };
    let mut live_part = Value::Null;
    match &cfg {
        None => warnings.push("no mimic.toml: the pin, the rules file and the corpus were not re-read, only the cassette's integrity was checked".into()),
        Some(cfg) => {
            let outcome: Result<(), String> = (|| {
                current.pin_hash = match cfg.reference.as_ref() {
                    Some(r) if content.cassette.header.provenance.kind != Kind::Authored => r.pin_hash()?,
                    _ => None,
                };
                let rules: RuleSet = std::fs::read_to_string(&cfg.rules).map_err(|e| format!("{}: {e}", cfg.rules.display())).and_then(|t| serde_json::from_str(&t).map_err(|e| format!("{}: {e}", cfg.rules.display())))?;
                current.rules_hash = rules.hash();
                if let Some(path) = &cfg.corpus {
                    let spec: CorpusSpec = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())).and_then(|t| serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display())))?;
                    let cases = expand(&spec, &set.store, &set.name).map_err(|e| e.to_string())?;
                    current.corpus_hash = Some(corpus_hash(&cases));
                    if live {
                        live_part = match cfg.reference.as_ref().and_then(|r| r.live_when_named.as_ref()).filter(|_| !named) {
                            Some(why) => json!({"ok": true, "skipped": why}),
                            None => live_check(cfg, set, &content, &rules, &spec, &cases)?,
                        };
                    }
                } else {
                    current.corpus_hash = None;
                    if live {
                        if named {
                            return Err("--live needs a corpus in mimic.toml".into());
                        }
                        live_part = json!({"ok": true, "skipped": "no corpus in mimic.toml: nothing to run live"});
                    }
                }
                Ok(())
            })();
            if let Err(e) = outcome {
                return json!({"set": set.name, "dir": set.dir.display().to_string(), "ok": false, "error": e, "next": "fix mimic.toml or the reference command it names"});
            }
        }
    }
    let mut report = verify_set(&content, &current);
    let unaccounted = stabilize_verb::accounting_problems(&set.dir, &content.cassette.interactions);
    report.ok &= unaccounted.is_empty();
    report.problems.extend(unaccounted);
    let live_ok = live_part.get("ok").and_then(Value::as_bool).unwrap_or(true);
    json!({
        "set": set.name, "dir": set.dir.display().to_string(), "ok": report.ok && live_ok, "kind": content.cassette.header.provenance.kind,
        "interactions": content.cassette.interactions.len(), "checks": report.checks, "problems": report.problems, "live": live_part, "warnings": warnings,
    })
}

/// Re-record the reference into memory and diff it with the committed cassette: the pin is still what the cassette says it is.
fn live_check(cfg: &Config, set: &SetRef, content: &CassetteSet, rules: &RuleSet, spec: &CorpusSpec, cases: &[Case]) -> Result<Value, String> {
    let reference = cfg.reference.as_ref().ok_or("--live needs a [reference] in mimic.toml")?;
    let target = reference.provenance()?;
    let mut driver = StdioDriver::new(reference.spec.clone());
    let recording = record_set(&mut driver, cases, rules, Some(spec), meta(cfg, &set.dir, (Some("verify --live"), None), spec, target)).map_err(|e| e.to_string())?;
    let pin_hash = content.cassette.header.provenance.target.as_ref().and_then(|t| t.pin_hash.clone());
    let (allow, _) = allowlist(cfg.allowlist.clone(), pin_hash.as_deref()).map_err(|f| f.error)?;
    let verdict = diff_cassettes(&content.cassette, &recording.set.cassette, &allow, &cfg.diff);
    Ok(json!({"ok": verdict.ok() && recording.nondeterministic.is_empty(), "severity": verdict.severity, "summary": verdict.summary, "results": labelled(&verdict), "nondeterministic": recording.nondeterministic}))
}

fn verify(p: &Parsed) -> Done {
    let mut dirs: Vec<PathBuf> = p.positional.iter().map(PathBuf::from).collect();
    let named = dirs.len();
    if p.flag("all") {
        let root = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
        find_sets(&root, &mut dirs);
    }
    if dirs.is_empty() {
        return Err(fail("no cassette set to verify: name sets or pass --all", "wf mimic verify <set>... | --all [--root <dir>]"));
    }
    let sets: Vec<Value> = dirs.iter().enumerate().map(|(i, d)| verify_one(&set_ref(&d.display().to_string()), p.flag("live"), i < named)).collect();
    let ok = sets.iter().all(|s| s["ok"] == true);
    let stale = sets.iter().find(|s| s["ok"] != true).and_then(|s| s["dir"].as_str());
    let next = stale.map_or_else(|| "nothing to do".to_string(), |d| format!("wf mimic record {d}  (re-record against the current pin, rules and corpus), then commit the set"));
    Ok((json!({"ok": ok, "sets": sets, "next": next, "warnings": []}), i32::from(!ok)))
}

const FIXTURE_RULES: &str = "fixture-provenance.json";

fn fixture_paths(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if !SKIP.iter().any(|s| e.file_name() == *s) {
                fixture_paths(base, &path, out);
            }
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
        }
    }
}

/// `verify --fixtures <dir>`: classify every fixture file under the include globs of the rules file by provenance (offline).
fn fixtures(p: &Parsed) -> Done {
    let dir = PathBuf::from(p.val("fixtures").unwrap_or_default());
    if !p.positional.is_empty() || p.flag("all") || p.flag("live") {
        return Err(fail("--fixtures scans one tree and takes no set, --all or --live", "wf mimic verify --fixtures <dir> [--rules <file>] [--files]"));
    }
    if !dir.is_dir() {
        return Err(fail(format!("--fixtures {} is not a directory", dir.display()), "pass the root of the fixture tree, e.g. libraries/agent-api"));
    }
    let rules_path = p.val("rules").map_or_else(|| dir.join(FIXTURE_RULES), PathBuf::from);
    let text = std::fs::read_to_string(&rules_path).map_err(|e| fail(format!("rules {}: {e}", rules_path.display()), format!("write {} (schema mimic-fixture-rules/1, wf mimic verify --help)", rules_path.display())))?;
    let rules = FixtureRules::parse(&text).map_err(|e| fail(format!("rules {}: {e}", rules_path.display()), format!("fix {}", rules_path.display())))?;
    let mut paths = Vec::new();
    fixture_paths(&dir, &dir, &mut paths);
    let files: Vec<FixtureFile> = paths.into_iter().filter(|rel| rules.includes(rel)).map(|rel| FixtureFile { bytes: std::fs::read(dir.join(&rel)).unwrap_or_default(), path: rel }).collect();
    let report = scan(&rules, &files);
    let ok = report.ok();
    let warnings: Vec<String> = report.unused_rules.iter().map(|r| format!("rule {r} matched no file: stale, or waiting for a file of its kind")).collect();
    let next = match report.unclassified.first() {
        None => "nothing to do".to_string(),
        Some(first) => format!("add a rule for {first} (and its siblings) to {}: kind authored, mirrors-reference (with a row) or cli, and a reason", rules_path.display()),
    };
    let mut out = json!({"ok": ok, "dir": dir.display().to_string(), "rules": rules_path.display().to_string(), "total": report.total, "groups": report.groups,
        "unclassified": report.unclassified, "unused_rules": report.unused_rules, "warnings": warnings, "next": next});
    if p.flag("files") {
        out["files"] = json!(report.files);
    }
    Ok((out, i32::from(!ok)))
}

/// The legacy files of `dir` a format reads, under the path they are recorded by (relative to `root`).
fn legacy_files(format: &str, dir: &Path, root: &Path, (allowlist, pin): (Option<&str>, Option<&str>)) -> Result<Vec<Legacy>, Fail> {
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let wanted: Vec<PathBuf> = match format {
        legacy::EXEC_RECORDING => {
            let mut v = vec![dir.join("recordings.json")];
            v.extend(sorted_children(&dir.join("recordings"), "jsonl"));
            v
        }
        legacy::T3_GOLDEN => {
            let mut v = sorted_children(&dir, "jsonl");
            v.extend([allowlist.map_or_else(|| dir.join("../../allowlist.json"), PathBuf::from), pin.map_or_else(|| dir.join("../../t3-pin.json"), PathBuf::from)]);
            v
        }
        _ => sorted_children(&dir, "json"),
    };
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    wanted
        .into_iter()
        .map(|p| {
            let p = std::fs::canonicalize(&p).unwrap_or(p);
            let bytes = std::fs::read(&p).map_err(|e| fail(format!("{}: {e}", p.display()), format!("pass the directory that holds the legacy files of {format}")))?;
            let rel = p.strip_prefix(&root).unwrap_or(&p).display().to_string();
            Ok(Legacy { path: rel, bytes })
        })
        .collect()
}

fn sorted_children(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == ext)).collect();
    v.sort();
    v
}

fn known_format(p: &Parsed) -> Result<&str, Fail> {
    let format = p.positional[0].as_str();
    if legacy::FORMATS.contains(&format) {
        Ok(format)
    } else {
        Err(fail(format!("unknown format {format}"), format!("one of {}", legacy::FORMATS.join(", "))))
    }
}

fn refs(set: &CassetteSet) -> Value {
    json!(set.cassette.header.provenance.imported_from.iter().map(|r| json!({"path": r.path, "hash": r.hash})).collect::<Vec<_>>())
}

const IMPORTED_TOML: &str = "# An imported set: converted from legacy files, never recorded from a reference, so it has no [reference] and no pin.\nrules = \"rules.json\"\n";

/// The config of an imported T3 set: the differ's T3 semantics, and the pinned server as the reference `verify --live` re-records.
fn t3_toml(set: &Path, pin: &Path, driver: &Path) -> String {
    let f = mimic_core::presets::t3_fold();
    let list = |items: Vec<String>| format!("[{}]", items.join(", "));
    let once = list(f.once.iter().map(|(p, v)| format!("[{p:?}, {v:?}]")).collect());
    let squash = list(f.squash.iter().map(|p| format!("{p:?}")).collect());
    let fold = format!("channel = {:?}\ntag = {:?}\nchunk = {:?}\nvalues = {:?}\nonce = {once}\nsquash = {squash}\n", f.channel, f.tag, f.chunk, f.values);
    let paths = mimic_core::presets::T3_BREAKING_PATHS.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(", ");
    format!(
        "# The imported T3 goldens. The [reference] is the pinned server, driven by tooling/mimic-drivers (src/t3); `wf mimic verify --live` re-records it.\nrules = \"rules.json\"\ncorpus = \"corpus.json\"\nallowlist = \"allowlist.json\"\n\n[diff]\nignore_dirs = [\"c2s\"]\nbreaking_paths = [{paths}]\nadvertised_prefixes = [\"capabilities.\"]\n\n[diff.fold]\n{fold}\n[reference]\nname = \"t3\"\ncommand = [\"node\", \"{}\", \"serve\", \"t3\"]\npin = \"{}\"\nversion_pointer = \"/version\"\ncommit_pointer = \"/commit\"\ntimeout_ms = 900000\nlive_when_named = \"downloads and runs the pinned release: `wf mimic verify <set> --live`, on the VM\"\n",
        relative(set, driver),
        relative(set, pin)
    )
}

fn import(p: &Parsed) -> Done {
    let format = known_format(p)?;
    let out = set_ref(p.val("out").ok_or_else(|| fail("--out <set dir> is required", "wf mimic import <format> <legacy dir> --out <set dir>"))?);
    let root = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    let files = legacy_files(format, Path::new(&p.positional[1]), &root, (p.val("allowlist"), p.val("pin")))?;
    let set = legacy::import(format, &files, tool()).map_err(|e| fail(e.to_string(), "fix the legacy files; a recorded one is not importable, it needs a real run"))?;
    let mut store = FsStore::new(out.dir.parent().unwrap_or(Path::new(".")));
    store.write(&out.name, &set).map_err(|e| fail(e.to_string(), "make the set directory writable"))?;
    let toml = out.dir.join("mimic.toml");
    let mut wrote: Vec<String> = out.store.files_of(&out.name).iter().map(|f| f.display().to_string()).collect();
    let mut extra: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    if format == legacy::T3_GOLDEN {
        let named = |suffix: &str| files.iter().find(|f| f.path.ends_with(suffix));
        let (Some(allowlist), Some(pin)) = (named("allowlist.json"), named("t3-pin.json")) else { return Err(fail("the legacy files lack the allowlist or the pin", "wf mimic import --help")) };
        Allowlist::parse(&String::from_utf8_lossy(&allowlist.bytes)).map_err(|e| fail(e.to_string(), "fix the allowlist"))?;
        extra.push((out.dir.join("allowlist.json"), allowlist.bytes.clone()));
        if !toml.exists() {
            extra.push((toml.clone(), t3_toml(&out.dir, &root.join(&pin.path), &root.join("tooling/mimic-drivers/src/cli.ts")).into_bytes()));
        }
    } else if !toml.exists() {
        extra.push((toml.clone(), IMPORTED_TOML.as_bytes().to_vec()));
    }
    for (path, bytes) in extra {
        std::fs::write(&path, bytes).map_err(|e| fail(format!("{}: {e}", path.display()), "make the set directory writable"))?;
        wrote.push(path.display().to_string());
    }
    wf_kit::logfmt::mcp("info", "mimic", "import.done", &[("set", out.name.clone()), ("format", format.to_string())]);
    let report = json!({
        "ok": true, "format": format, "set": out.name, "kind": "imported", "interactions": set.cassette.interactions.len(), "imported_from": refs(&set),
        "wrote": wrote, "warnings": [], "next": format!("wf mimic export {format} {}", out.dir.display()),
    });
    Ok((report, 0))
}

fn export(p: &Parsed) -> Done {
    let format = known_format(p)?;
    let set = set_ref(&p.positional[1]);
    let content = set.read()?;
    let files = legacy::check(format, &content).map_err(|e| fail(e.to_string(), format!("wf mimic import {format} <legacy dir> --out {}", set.dir.display())))?;
    let mut wrote = Vec::new();
    if let Some(dir) = p.val("out") {
        let rebuilt = legacy::export(format, &content).map_err(|e| fail(e.to_string(), "re-import the set"))?;
        for b in rebuilt {
            let path = Path::new(dir).join(&b.path);
            std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).and_then(|()| std::fs::write(&path, &b.bytes)).map_err(|e| fail(format!("{}: {e}", path.display()), "make --out writable"))?;
            wrote.push(path.display().to_string());
        }
    }
    let ok = files.iter().all(|f| f.ok);
    let next = if ok { "nothing to do".to_string() } else { format!("wf mimic import {format} <legacy dir> --out {}  (the set no longer rebuilds its legacy files)", set.dir.display()) };
    Ok((json!({"ok": ok, "format": format, "set": set.name, "files": files, "wrote": wrote, "warnings": [], "next": next}), i32::from(!ok)))
}
