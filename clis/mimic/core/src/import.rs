//! Legacy CLI fixtures to cassettes, and back. Pure: the caller reads the legacy files and writes the set. Two formats:
//! `exec-recording` (`recordings.json` plus `recordings/*.jsonl`, the stream-json each CLI prints) and `exec-hosted` (`hosted_*.json`, what
//! `gh` and `glab` print for an argv, with the executor request and response). The kind is always `imported`: nothing here ran a CLI, and
//! a legacy file that claims `recorded` is refused. Each legacy file's canonical sha256 goes into `provenance.imported_from`, and
//! `export` rebuilds the files from the cassette so `check` can show the conversion lost nothing.
use crate::cassette::{Cassette, Dir, Effect, Header, Interaction, Kind, Provenance, RefHash, Step, StepKind, ToolInfo, SCHEMA};
use crate::json::{canonical, hash_value, sha256_label};
use crate::ports::CassetteSet;
use crate::rules::RuleSet;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const EXEC_RECORDING: &str = "exec-recording";
pub const EXEC_HOSTED: &str = "exec-hosted";
pub const T3_GOLDEN: &str = crate::t3_golden::FORMAT;
pub const FORMATS: [&str; 3] = [EXEC_RECORDING, EXEC_HOSTED, T3_GOLDEN];

#[derive(Debug, PartialEq)]
pub struct ImportError(pub String);

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImportError {}

fn err<T>(msg: impl Into<String>) -> Result<T, ImportError> {
    Err(ImportError(msg.into()))
}

/// A legacy file as read: the path it is recorded under (relative to the repository) and its bytes.
#[derive(Clone, Debug)]
pub struct Legacy {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A legacy file rebuilt from a cassette, with the canonical hash of what was rebuilt.
#[derive(Clone, Debug, PartialEq)]
pub struct Rebuilt {
    pub path: String,
    pub bytes: Vec<u8>,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Lossless {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,
    pub ok: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexEntry {
    name: String,
    driver: String,
    file: String,
    scenario: String,
    provenance: String,
    source: String,
    expect: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    note: String,
    recordings: Vec<IndexEntry>,
}

const DOCUMENTED: &str = "documented-format";
const RECORDINGS_JSON: &str = "recordings.json";

fn text_of(f: &Legacy) -> Result<&str, ImportError> {
    std::str::from_utf8(&f.bytes).or_else(|e| err(format!("{} is not UTF-8: {e}", f.path)))
}

fn json_of(f: &Legacy) -> Result<Value, ImportError> {
    serde_json::from_str(text_of(f)?).or_else(|e| err(format!("{} is not JSON: {e}", f.path)))
}

fn lines_text(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

fn pretty(v: &Value) -> Vec<u8> {
    let mut s = serde_json::to_string_pretty(&canonical(v)).expect("JSON prints");
    s.push('\n');
    s.into_bytes()
}

fn sealed(source: String, note: String, imported_from: Vec<RefHash>, interactions: Vec<Interaction>, tool: ToolInfo) -> CassetteSet {
    let rules = RuleSet::new(Vec::new());
    let provenance = Provenance {
        kind: Kind::Imported,
        target: None,
        driver: None,
        tool,
        seed: None,
        rules_hash: rules.hash(),
        corpus_hash: None,
        recorded_at: None,
        recorded_on: None,
        source: Some(source),
        note: Some(note),
        imported_from,
    };
    let header = Header { schema: SCHEMA.into(), provenance, rules: RefHash { path: "rules.json".into(), hash: rules.hash() }, corpus: None, interactions: 0, content_hash: String::new() };
    let mut cassette = Cassette { header, interactions };
    cassette.seal();
    CassetteSet { cassette, rules, corpus: None }
}

fn step(dir: Dir, channel: Option<&str>, kind: StepKind, payload: Value) -> Step {
    Step { dir, channel: channel.map(String::from), kind, rid: None, len: None, payload }
}

pub fn import(format: &str, files: &[Legacy], tool: ToolInfo) -> Result<CassetteSet, ImportError> {
    match format {
        EXEC_RECORDING => import_recordings(files, tool),
        EXEC_HOSTED => import_hosted(files, tool),
        T3_GOLDEN => crate::t3_golden::import(files, tool),
        other => err(format!("unknown format {other:?}, this tool imports {}", FORMATS.join(", "))),
    }
}

pub fn export(format: &str, set: &CassetteSet) -> Result<Vec<Rebuilt>, ImportError> {
    match format {
        EXEC_RECORDING => export_recordings(set),
        EXEC_HOSTED => export_hosted(set),
        T3_GOLDEN => crate::t3_golden::export(set),
        other => err(format!("unknown format {other:?}, this tool exports {}", FORMATS.join(", "))),
    }
}

/// Rebuild every legacy file and compare its canonical hash with the one recorded at import; a file the set no longer rebuilds is a failure.
pub fn check(format: &str, set: &CassetteSet) -> Result<Vec<Lossless>, ImportError> {
    let rebuilt = export(format, set)?;
    let from = &set.cassette.header.provenance.imported_from;
    if from.is_empty() {
        return err("the set records no imported_from: it was not imported");
    }
    let mut out: Vec<Lossless> = from
        .iter()
        .map(|r| {
            let actual = rebuilt.iter().find(|b| b.path == r.path).map(|b| b.hash.clone());
            Lossless { path: r.path.clone(), expected: r.hash.clone(), ok: actual.as_deref() == Some(r.hash.as_str()), actual }
        })
        .collect();
    for b in rebuilt.iter().filter(|b| !from.iter().any(|r| r.path == b.path)) {
        out.push(Lossless { path: b.path.clone(), expected: String::new(), actual: Some(b.hash.clone()), ok: false });
    }
    Ok(out)
}

fn import_recordings(files: &[Legacy], tool: ToolInfo) -> Result<CassetteSet, ImportError> {
    let Some(index_file) = files.iter().find(|f| f.path == RECORDINGS_JSON || f.path.ends_with(&format!("/{RECORDINGS_JSON}"))) else {
        return err(format!("no {RECORDINGS_JSON} among the legacy files"));
    };
    let index: Index = serde_json::from_value(json_of(index_file)?).or_else(|e| err(format!("{}: {e}", index_file.path)))?;
    if index.note.is_empty() {
        return err(format!("{} needs its note", index_file.path));
    }
    let dir = &index_file.path[..index_file.path.len() - RECORDINGS_JSON.len()];
    let mut from = vec![RefHash { path: index_file.path.clone(), hash: hash_value(&json_of(index_file)?) }];
    let mut interactions = Vec::new();
    for e in &index.recordings {
        if e.provenance != DOCUMENTED {
            return err(format!("{}: provenance {:?} is not importable: only a documented-format recording is, a recorded one needs a real run of the CLI", e.name, e.provenance));
        }
        if e.file != format!("{}.jsonl", e.name) {
            return err(format!("{}: file {:?} is not <name>.jsonl", e.name, e.file));
        }
        let path = format!("{dir}recordings/{}", e.file);
        let Some(file) = files.iter().find(|f| f.path == path) else { return err(format!("{path} is missing")) };
        let lines: Vec<String> = text_of(file)?.lines().map(String::from).collect();
        if lines.is_empty() || lines.iter().any(|l| l.trim().is_empty()) || lines_text(&lines).as_bytes() != file.bytes.as_slice() {
            return err(format!("{path} does not round-trip: it needs one non-blank line per line, LF endings and a final newline"));
        }
        from.push(RefHash { path, hash: sha256_label(&file.bytes) });
        let steps = lines.into_iter().map(|l| step(Dir::Stdout, None, StepKind::Text, Value::String(l))).collect();
        let effect = Effect { kind: "neutral-events".into(), data: Map::from_iter([("events".to_string(), json!(e.expect))]) };
        interactions.push(Interaction {
            id: e.name.clone(),
            row: None,
            op: Some(e.driver.clone()),
            input: json!({"scenario": e.scenario, "source": e.source, "legacy_provenance": e.provenance}),
            steps,
            effects: vec![effect],
            outcome: Default::default(),
            tags: Vec::new(),
        });
    }
    let source = format!("{}: stream-json written from each CLI's documented format, not recorded from a run of claude, codex or opencode (docs/gaps/exec-testkit-recordings-not-from-real-clis.md)", index_file.path);
    Ok(sealed(source, index.note, from, interactions, tool))
}

fn export_recordings(set: &CassetteSet) -> Result<Vec<Rebuilt>, ImportError> {
    let p = &set.cassette.header.provenance;
    let Some(first) = p.imported_from.first() else { return err("the set records no imported_from") };
    let dir = &first.path[..first.path.len().saturating_sub(RECORDINGS_JSON.len())];
    let mut entries = Vec::new();
    let mut out = Vec::new();
    for i in &set.cassette.interactions {
        let field = |k: &str| i.input.get(k).and_then(Value::as_str).map(String::from).ok_or_else(|| ImportError(format!("{}: input has no {k}", i.id)));
        let events = i.effects.iter().find(|e| e.kind == "neutral-events").and_then(|e| e.data.get("events")).and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());
        let (Some(driver), Some(expect)) = (i.op.clone(), events) else { return err(format!("{}: no driver or neutral events", i.id)) };
        entries.push(IndexEntry { name: i.id.clone(), driver, file: format!("{}.jsonl", i.id), scenario: field("scenario")?, provenance: field("legacy_provenance")?, source: field("source")?, expect });
        let lines: Vec<String> = i.steps.iter().filter(|s| s.dir == Dir::Stdout).filter_map(|s| s.payload.as_str().map(String::from)).collect();
        let bytes = lines_text(&lines).into_bytes();
        out.push(Rebuilt { path: format!("{dir}recordings/{}.jsonl", i.id), hash: sha256_label(&bytes), bytes });
    }
    let index = serde_json::to_value(Index { note: p.note.clone().unwrap_or_default(), recordings: entries }).expect("index serialises");
    out.insert(0, Rebuilt { path: first.path.clone(), hash: hash_value(&index), bytes: pretty(&index) });
    Ok(out)
}

const EXCHANGE_KEYS: [&str; 6] = ["name", "files", "cli", "request", "starts", "response"];
const CLI_KEYS: [&str; 6] = ["program", "args", "stdout", "stderr", "exit_code", "stdin"];

fn object<'a>(v: &'a Value, what: &str, allowed: &[&str], required: &[&str]) -> Result<&'a Map<String, Value>, ImportError> {
    let Some(m) = v.as_object() else { return err(format!("{what} is not an object")) };
    if let Some(k) = m.keys().find(|k| !allowed.contains(&k.as_str())) {
        return err(format!("{what} has the key {k:?} this importer does not carry"));
    }
    if let Some(k) = required.iter().find(|k| !m.contains_key(**k)) {
        return err(format!("{what} lacks {k:?}"));
    }
    Ok(m)
}

fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".json").unwrap_or(name)
}

fn import_hosted(files: &[Legacy], tool: ToolInfo) -> Result<CassetteSet, ImportError> {
    let mut sorted: Vec<&Legacy> = files.iter().filter(|f| f.path.ends_with(".json")).collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    if sorted.is_empty() {
        return err("no legacy .json file to import");
    }
    let (mut from, mut interactions) = (Vec::new(), Vec::new());
    for f in sorted {
        let doc = json_of(f)?;
        let top = object(&doc, &f.path, &["action", "exchanges"], &["action", "exchanges"])?;
        let action = top["action"].as_str().unwrap_or_default();
        if action != stem(&f.path) {
            return err(format!("{}: action {action:?} is not the file's name", f.path));
        }
        let Some(exchanges) = top["exchanges"].as_array() else { return err(format!("{}: exchanges is not an array", f.path)) };
        from.push(RefHash { path: f.path.clone(), hash: hash_value(&doc) });
        for (n, x) in exchanges.iter().enumerate() {
            let what = format!("{} exchange {n}", f.path);
            let m = object(x, &what, &EXCHANGE_KEYS, &["name", "cli", "request", "starts", "response"])?;
            let mut steps = Vec::new();
            for (j, c) in m["cli"].as_array().ok_or_else(|| ImportError(format!("{what}: cli is not an array")))?.iter().enumerate() {
                let c = object(c, &format!("{what} cli {j}"), &CLI_KEYS, &["program", "args", "stdout", "stderr", "exit_code"])?;
                let program = c["program"].as_str().ok_or_else(|| ImportError(format!("{what} cli {j}: program is not a string")))?;
                steps.push(step(Dir::Call, Some(program), StepKind::Json, json!({"args": c["args"]})));
                if let Some(stdin) = c.get("stdin") {
                    steps.push(step(Dir::Stdin, Some(program), StepKind::Text, stdin.clone()));
                }
                steps.push(step(Dir::Stdout, Some(program), StepKind::Text, c["stdout"].clone()));
                steps.push(step(Dir::Stderr, Some(program), StepKind::Text, c["stderr"].clone()));
                steps.push(step(Dir::Exit, Some(program), StepKind::Exit, c["exit_code"].clone()));
            }
            let mut response = step(Dir::S2c, None, StepKind::Json, m["response"].clone());
            response.rid = m["request"].get("id").and_then(Value::as_str).map(String::from);
            steps.push(response);
            let mut input = Map::new();
            for k in ["name", "request", "files"] {
                if let Some(v) = m.get(k) {
                    input.insert(k.to_string(), v.clone());
                }
            }
            interactions.push(Interaction {
                id: format!("{action}/{n:02}"),
                row: None,
                op: Some(action.to_string()),
                input: Value::Object(input),
                steps,
                effects: vec![Effect { kind: "cli-starts".into(), data: Map::from_iter([("starts".to_string(), m["starts"].clone())]) }],
                outcome: Default::default(),
                tags: Vec::new(),
            });
        }
    }
    let source = "the hosted_*.json fixtures of exec-hosted: what gh and glab print for an argv, written by RECORD_HOSTED_FIXTURES from what the executor did against a scripted fake, not recorded from a run of gh or glab (docs/gaps/exec-hosted-fixtures-not-from-real-clis.md)".to_string();
    let note = "imported without a run of gh or glab; the owner-gated real recording against a pinned CLI version is the open next step".to_string();
    Ok(sealed(source, note, from, interactions, tool))
}

fn export_hosted(set: &CassetteSet) -> Result<Vec<Rebuilt>, ImportError> {
    let from = &set.cassette.header.provenance.imported_from;
    let mut docs: Vec<(String, Vec<Value>)> = Vec::new();
    for i in &set.cassette.interactions {
        let Some(action) = i.op.clone() else { return err(format!("{}: no op", i.id)) };
        let mut cli: Vec<Map<String, Value>> = Vec::new();
        let mut response = None;
        for s in &i.steps {
            let program = s.channel.clone().map(Value::String).unwrap_or(Value::Null);
            match s.dir {
                Dir::Call => cli.push(Map::from_iter([("program".to_string(), program), ("args".to_string(), s.payload["args"].clone())])),
                Dir::Stdin | Dir::Stdout | Dir::Stderr | Dir::Exit => {
                    let Some(c) = cli.last_mut() else { return err(format!("{}: a step comes before its call", i.id)) };
                    let key = match s.dir {
                        Dir::Stdin => "stdin",
                        Dir::Stdout => "stdout",
                        Dir::Stderr => "stderr",
                        _ => "exit_code",
                    };
                    c.insert(key.to_string(), s.payload.clone());
                }
                Dir::S2c => response = Some(s.payload.clone()),
                _ => return err(format!("{}: a {:?} step is not part of this format", i.id, s.dir)),
            }
        }
        let starts = i.effects.iter().find(|e| e.kind == "cli-starts").and_then(|e| e.data.get("starts")).cloned();
        let (Some(response), Some(starts)) = (response, starts) else { return err(format!("{}: no response or starts", i.id)) };
        let mut x = Map::new();
        for k in ["name", "files", "request"] {
            if let Some(v) = i.input.get(k) {
                x.insert(k.to_string(), v.clone());
            }
        }
        x.insert("cli".into(), Value::Array(cli.into_iter().map(Value::Object).collect()));
        x.insert("starts".into(), starts);
        x.insert("response".into(), response);
        match docs.iter_mut().find(|d| d.0 == action) {
            Some(d) => d.1.push(Value::Object(x)),
            None => docs.push((action, vec![Value::Object(x)])),
        }
    }
    docs.into_iter()
        .map(|(action, exchanges)| {
            let Some(r) = from.iter().find(|r| stem(&r.path) == action) else { return err(format!("{action}: no imported_from path names it")) };
            let doc = json!({"action": action, "exchanges": exchanges});
            Ok(Rebuilt { path: r.path.clone(), hash: hash_value(&doc), bytes: pretty(&doc) })
        })
        .collect()
}
