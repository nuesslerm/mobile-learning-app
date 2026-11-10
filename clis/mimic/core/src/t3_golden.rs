//! The goldens of `tooling/t3-conformance` (one `<scenario>.jsonl` of normalised wire lines per scenario, recorded from `t3@0.1.6`) as one
//! imported cassette set, and back. Pure: the caller reads the legacy files and writes the set. Kind is always `imported`: nothing here ran
//! the reference. A descriptor and the 426 gate are one interaction each; the pairing, the token exchange and the socket (`auth`,
//! `rpc-phase1`, `rpc-errors`) are one `session`, because one credential lives through all three. The input of an interaction is the client's
//! side of the lines (what a driver sends), with the values only a run knows written as `{credential}`, `{access_token}`, `{ticket}`,
//! `{workspace}`. The `rpc-phase1.variant-order.jsonl` file holds the same frames in another racy order: it is a permutation in the input
//! and a `variant:order` tag, not a second copy. Timing (`t`) is run noise the old differ never compared and is not kept.
use crate::cassette::{Cassette, Dir, Header, Interaction, Kind, Provenance, RefHash, Step, StepKind, Target, ToolInfo, SCHEMA};
use crate::corpus::{corpus_hash, Case, CaseSpec, CorpusSpec, GeneratorSpec};
use crate::import::{ImportError, Legacy, Rebuilt};
use crate::json::{canonical, hash_value, sha256_label};
use crate::ports::CassetteSet;
use crate::presets;
use crate::wire::rid;
use serde_json::{json, Value};

pub const FORMAT: &str = "t3-golden";
pub const OP: &str = "script";
pub const SETTLE_MS: u64 = 250;
const TAG_VARIANT: &str = "variant:order";
const SESSION: &str = "session";
const SESSION_FILES: [&str; 3] = ["auth", "rpc-phase1", "rpc-errors"];
const VARIANT_FILE: &str = "rpc-phase1.variant-order.jsonl";
const ALLOWLIST: &str = "allowlist.json";
const PIN: &str = "t3-pin.json";

fn err<T>(msg: impl Into<String>) -> Result<T, ImportError> {
    Err(ImportError(msg.into()))
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parse_lines(f: &Legacy) -> Result<Vec<Value>, ImportError> {
    let text = std::str::from_utf8(&f.bytes).or_else(|e| err(format!("{} is not UTF-8: {e}", f.path)))?;
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).or_else(|e| err(format!("{} has a line that is not JSON: {e}", f.path)))).collect()
}

/// What a legacy file's hash covers: its lines without the run's timing.
pub fn lines_hash(lines: &[Value]) -> String {
    let bare: Vec<Value> = lines.iter().map(|l| without(l, "t")).collect();
    hash_value(&Value::Array(bare))
}

fn without(v: &Value, key: &str) -> Value {
    let mut v = v.clone();
    if let Some(m) = v.as_object_mut() {
        m.remove(key);
    }
    v
}

fn step(dir: Dir, channel: &str, kind: StepKind, rid: Option<String>, payload: Value) -> Step {
    Step { dir, channel: Some(channel.into()), kind, rid, len: None, payload }
}

fn dir_of(line: &Value) -> Result<Dir, ImportError> {
    match line["dir"].as_str() {
        Some("c2s") => Ok(Dir::C2s),
        Some("s2c") => Ok(Dir::S2c),
        other => err(format!("a ws line has dir {other:?}, not c2s or s2c")),
    }
}

/// One golden line as steps: an HTTP exchange is a call and a return, everything else one step.
fn line_steps(line: &Value) -> Result<Vec<Step>, ImportError> {
    match line["kind"].as_str() {
        Some("http") => Ok(vec![
            step(Dir::Call, "http", StepKind::Json, None, json!({"method": line["method"], "path": line["path"], "headers": line["reqHeaders"], "body": line["reqBody"]})),
            step(Dir::Ret, "http", StepKind::Json, None, json!({"status": line["status"], "headers": line["resHeaders"], "body": line["resBody"]})),
        ]),
        Some("ws-open") => Ok(vec![step(Dir::S2c, "ws:open", StepKind::Json, None, json!({"path": line["path"], "status": line["status"]}))]),
        Some("ws-close") => {
            let tcp = line["dir"] == "tcp";
            let dir = if tcp { Dir::S2c } else { dir_of(line)? };
            Ok(vec![step(dir, "ws:close", StepKind::Json, None, json!({"code": line["code"], "reason": line["reason"], "tcp": tcp}))])
        }
        Some("ws") => {
            if line["opcode"] != 1 {
                return err("a golden ws line is not a text frame: the goldens carry no binary frame");
            }
            let frame = &line["frame"];
            let kind = if frame.is_string() { StepKind::Text } else { StepKind::Json };
            Ok(vec![step(dir_of(line)?, "ws", kind, rid(frame), frame.clone())])
        }
        other => err(format!("a golden line has kind {other:?}")),
    }
}

/// The inverse: the lines of a run of steps, numbered from 0.
fn steps_lines(steps: &[Step]) -> Result<Vec<Value>, ImportError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < steps.len() {
        let s = &steps[i];
        let line = match (s.channel.as_deref(), s.dir) {
            (Some("http"), Dir::Call) => {
                let Some(ret) = steps.get(i + 1).filter(|r| r.dir == Dir::Ret) else { return err("an http call has no return step") };
                i += 1;
                json!({"kind": "http", "method": s.payload["method"], "path": s.payload["path"], "reqHeaders": s.payload["headers"], "reqBody": s.payload["body"],
                    "status": ret.payload["status"], "resHeaders": ret.payload["headers"], "resBody": ret.payload["body"]})
            }
            (Some("ws:open"), _) => json!({"kind": "ws-open", "path": s.payload["path"], "status": s.payload["status"]}),
            (Some("ws:close"), dir) => {
                let who = if s.payload["tcp"] == true { "tcp" } else if dir == Dir::C2s { "c2s" } else { "s2c" };
                json!({"kind": "ws-close", "dir": who, "code": s.payload["code"], "reason": s.payload["reason"]})
            }
            (Some("ws"), dir) => json!({"kind": "ws", "dir": if dir == Dir::C2s { "c2s" } else { "s2c" }, "opcode": 1, "frame": s.payload}),
            _ => return err("a step is not part of the golden dialect"),
        };
        let mut line = line;
        line["n"] = json!(out.len());
        out.push(line);
        i += 1;
    }
    Ok(out)
}

struct Subst {
    ticket_known: bool,
    credential: &'static str,
}

fn subst(v: &Value, s: &Subst) -> Value {
    match v {
        Value::String(t) => {
            let ticket = if s.ticket_known { "{ticket}" } else { "none" };
            Value::String(t.replace("<pairing-credential>", s.credential).replace("<access-token>", "{access_token}").replace("<ws-ticket>", ticket).replace("<workspace>", "{workspace}"))
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| subst(x, s)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), subst(x, s))).collect()),
        other => other.clone(),
    }
}

/// The client's side of the lines, as a driver script.
fn script(lines: &[Value]) -> Vec<Value> {
    let exchanges = lines.iter().filter(|l| l["kind"] == "http" && l["path"] == "/oauth/token").count();
    let (mut seen, mut ticket_known) = (0, false);
    let mut out = Vec::new();
    for l in lines {
        match l["kind"].as_str() {
            Some("http") => {
                let exchange = l["path"] == "/oauth/token";
                // the third exchange of the recorded session posted a wrong credential, which the recorder masked like the right one
                let credential = if exchange && exchanges >= 3 && seen == exchanges - 1 { "{bad-credential}" } else { "{credential}" };
                seen += usize::from(exchange);
                let s = Subst { ticket_known, credential };
                out.push(json!({"http": {"method": l["method"], "path": subst(&l["path"], &s), "headers": subst(&l["reqHeaders"], &s), "body": subst(&l["reqBody"], &s)}}));
                ticket_known |= l["resBody"].get("ticket").is_some();
            }
            Some("ws-open") => out.push(json!({"ws-open": {"path": subst(&l["path"], &Subst { ticket_known, credential: "{credential}" })}})),
            Some("ws") if l["dir"] == "c2s" && l["frame"]["_tag"] != "Ack" => out.push(json!({"ws-send": subst(&l["frame"], &Subst { ticket_known, credential: "{credential}" })})),
            Some("ws-close") if l["dir"] == "c2s" => out.push(json!({"ws-close": {"code": l["code"], "reason": l["reason"]}})),
            _ => {}
        }
    }
    out
}

fn content(l: &Value) -> String {
    without(&without(l, "t"), "n").to_string()
}

/// For each line of `variant`, the index of the line of `main` it repeats (the k-th copy of a line maps to the k-th copy).
fn permutation(main: &[Value], variant: &[Value]) -> Result<Vec<usize>, ImportError> {
    let mut used = vec![false; main.len()];
    variant
        .iter()
        .map(|v| {
            let c = content(v);
            let at = (0..main.len()).find(|&i| !used[i] && content(&main[i]) == c);
            match at {
                Some(i) => {
                    used[i] = true;
                    Ok(i)
                }
                None => err("the variant file has a line the main file lacks: it is not a reordering"),
            }
        })
        .collect()
}

fn find<'a>(files: &'a [Legacy], name: &str) -> Result<&'a Legacy, ImportError> {
    files.iter().find(|f| name_of(&f.path) == name).map_or_else(|| err(format!("no {name} among the legacy files")), Ok)
}

pub fn import(files: &[Legacy], tool: ToolInfo) -> Result<CassetteSet, ImportError> {
    let pin_file = find(files, PIN)?;
    let pin: Value = serde_json::from_slice(&pin_file.bytes).or_else(|e| err(format!("{PIN} is not JSON: {e}")))?;
    let allowlist = find(files, ALLOWLIST)?;
    let mut from = Vec::new();
    let mut interactions = Vec::new();
    let mut cases = Vec::new();
    let take = |ids: &[&str], from: &mut Vec<RefHash>| -> Result<Vec<Vec<Value>>, ImportError> {
        let mut parts = Vec::new();
        for name in ids {
            let f = find(files, &format!("{name}.jsonl"))?;
            let lines = parse_lines(f)?;
            if lines.is_empty() {
                return err(format!("{} has no line", f.path));
            }
            from.push(RefHash { path: f.path.clone(), hash: lines_hash(&lines) });
            parts.push(lines);
        }
        Ok(parts)
    };
    for (id, ids) in [("descriptor", vec!["descriptor"]), ("protocol-gate-426", vec!["protocol-gate-426"]), (SESSION, SESSION_FILES.to_vec())] {
        let parts = take(&ids, &mut from)?;
        let all: Vec<Value> = parts.iter().flatten().cloned().collect();
        let mut steps = Vec::new();
        for l in &all {
            steps.extend(line_steps(l)?);
        }
        let mut input = json!({"script": script(&all), "settle_ms": SETTLE_MS, "scenarios": ids.iter().zip(&parts).map(|(n, p)| json!([n, p.len()])).collect::<Vec<_>>()});
        let mut tags = Vec::new();
        if id == SESSION {
            let variant = find(files, VARIANT_FILE)?;
            let vlines = parse_lines(variant)?;
            let main = &parts[1];
            let perm = permutation(main, &vlines)?;
            from.push(RefHash { path: variant.path.clone(), hash: lines_hash(&vlines) });
            input["variants"] = json!({"order": {"file": VARIANT_FILE, "scenario": "rpc-phase1", "permutation": perm}});
            tags.push(TAG_VARIANT.to_string());
        }
        cases.push(Case { id: id.into(), op: Some(OP.into()), row: None, input: input.clone(), tags: tags.clone() });
        interactions.push(Interaction { id: id.into(), row: None, op: Some(OP.into()), input, steps, effects: Vec::new(), outcome: Default::default(), tags });
    }
    let rules = presets::t3();
    let spec = CorpusSpec {
        schema: crate::corpus::SCHEMA.into(),
        seed: 0,
        generators: cases.iter().map(|c| GeneratorSpec::Corpus { name: c.id.clone(), op: c.op.clone(), files: Vec::new(), cases: vec![CaseSpec { id: c.id.clone(), row: None, input: c.input.clone(), tags: c.tags.clone() }] }).collect(),
    };
    let target = Target {
        name: "t3".into(),
        version: pin["version"].as_str().map(String::from),
        commit: pin["commit"].as_str().map(String::from),
        pin_file: Some(pin_file.path.clone()),
        pin_hash: Some(sha256_label(&pin_file.bytes)),
    };
    let provenance = Provenance {
        kind: Kind::Imported,
        target: Some(target),
        driver: None,
        tool,
        seed: None,
        rules_hash: rules.hash(),
        corpus_hash: Some(corpus_hash(&cases)),
        recorded_at: None,
        recorded_on: None,
        source: Some(format!("the goldens of tooling/t3-conformance recorded from the pinned t3 release by its own recorder; allowlist {} {}", allowlist.path, sha256_label(&allowlist.bytes))),
        note: Some("imported without a run: the transcripts are what the old recorder wrote, converted line for line, and the owner or the VM re-records them against the pinned server with `wf mimic verify --live`; manifest.json, a summary of them, is not carried".into()),
        imported_from: from,
    };
    let header = Header {
        schema: SCHEMA.into(),
        provenance,
        rules: RefHash { path: "rules.json".into(), hash: rules.hash() },
        corpus: Some(RefHash { path: "corpus.json".into(), hash: corpus_hash(&cases) }),
        interactions: 0,
        content_hash: String::new(),
    };
    let mut cassette = Cassette { header, interactions };
    cassette.seal();
    Ok(CassetteSet { cassette, rules, corpus: Some(spec) })
}

fn jsonl(lines: &[Value]) -> Vec<u8> {
    lines.iter().map(|l| format!("{}\n", canonical(l))).collect::<String>().into_bytes()
}

/// The six legacy files rebuilt from the set (the lines without their timing), each with the hash `import` recorded for it.
pub fn export(set: &CassetteSet) -> Result<Vec<Rebuilt>, ImportError> {
    let from = &set.cassette.header.provenance.imported_from;
    let dir_of_path = |name: &str| -> Result<String, ImportError> {
        match from.iter().find(|r| name_of(&r.path) == name) {
            Some(r) => Ok(r.path[..r.path.len() - name.len()].to_string()),
            None => err(format!("no imported_from path names {name}")),
        }
    };
    let mut out = Vec::new();
    let mut push = |name: &str, lines: Vec<Value>| -> Result<(), ImportError> {
        let path = format!("{}{name}", dir_of_path(name)?);
        out.push(Rebuilt { path, hash: lines_hash(&lines), bytes: jsonl(&lines) });
        Ok(())
    };
    for i in &set.cassette.interactions {
        let lines = steps_lines(&i.steps)?;
        let Some(scenarios) = i.input["scenarios"].as_array() else { return err(format!("{}: input names no scenarios", i.id)) };
        let mut at = 0;
        let mut parts: Vec<(String, Vec<Value>)> = Vec::new();
        for s in scenarios {
            let (Some(name), Some(n)) = (s[0].as_str(), s[1].as_u64()) else { return err(format!("{}: a scenario entry is not [name, count]", i.id)) };
            let slice = lines.get(at..at + n as usize).ok_or_else(|| ImportError(format!("{}: the steps hold fewer lines than the scenarios count", i.id)))?;
            let numbered = slice.iter().enumerate().map(|(k, l)| {
                let mut l = l.clone();
                l["n"] = json!(k);
                l
            });
            parts.push((name.to_string(), numbered.collect()));
            at += n as usize;
        }
        for (name, lines) in &parts {
            push(&format!("{name}.jsonl"), lines.clone())?;
        }
        if let Some(v) = i.input["variants"]["order"].as_object() {
            let (Some(file), Some(scenario), Some(perm)) = (v.get("file").and_then(Value::as_str), v.get("scenario").and_then(Value::as_str), v.get("permutation").and_then(Value::as_array)) else {
                return err(format!("{}: the variant entry is incomplete", i.id));
            };
            let main = parts.iter().find(|p| p.0 == scenario).map(|p| &p.1).ok_or_else(|| ImportError(format!("{}: the variant names scenario {scenario}", i.id)))?;
            let mut reordered = Vec::new();
            for (k, ix) in perm.iter().enumerate() {
                let line = ix.as_u64().and_then(|ix| main.get(ix as usize)).ok_or_else(|| ImportError(format!("{}: the variant permutation points outside the lines", i.id)))?;
                let mut line = line.clone();
                line["n"] = json!(k);
                reordered.push(line);
            }
            push(file, reordered)?;
        }
    }
    Ok(out)
}
