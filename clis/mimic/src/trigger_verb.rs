//! `wf mimic trigger`: the marker of a commit message (`marker.rs`) turned into the jobs the record-references workflow runs, one per set,
//! with every set checked on the way: it exists, its mimic.toml loads, and it can do what the marker asks.
use super::{fail, Done};
use crate::config::Config;
use crate::marker::{self, Action};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use wf_kit::args::Parsed;

fn message(p: &Parsed) -> Result<String, super::Fail> {
    match (p.val("message"), p.val("file")) {
        (Some(m), None) => Ok(m.to_string()),
        (None, Some("-")) => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|e| fail(format!("stdin: {e}"), "pipe the commit message in, or pass --message"))?;
            Ok(text)
        }
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|e| fail(format!("{path}: {e}"), "name a readable file, or - for stdin")),
        _ => Err(fail("give the commit message with --message <text> or --file <path|->", "wf mimic trigger --file - < message.txt")),
    }
}

fn check(root: &Path, dir: &Path, action: Action, name: &str) -> Result<Value, String> {
    let abs = root.join(dir);
    let cfg = Config::load(&abs.join("mimic.toml")).map_err(|e| format!("set {name}: {e}"))?;
    match action {
        Action::Record => {
            if cfg.reference.is_none() {
                return Err(format!("set {name} has no [reference] in its mimic.toml: there is no server to record"));
            }
            if cfg.corpus.is_none() {
                return Err(format!("set {name} has no corpus: a record needs corpus.json (schema mimic-corpus/1)"));
            }
            if cfg.provision.is_none() {
                return Err(format!("set {name} has no [provision] block (or provision_file) in its mimic.toml: the runner cannot install its reference; the schema is in docs/runbooks/record-reference-servers-on-the-vm.md"));
            }
        }
        Action::Replay => {
            if !abs.join("cassette.json").is_file() {
                return Err(format!("set {name} has no cassette.json: there is nothing recorded to replay (record it first: [record {name}])"));
            }
            if cfg.candidates.is_empty() {
                return Err(format!("set {name} has no [candidate.<name>] in its mimic.toml: there is nothing to replay against"));
            }
        }
    }
    let tool = |name: &str| cfg.provision.as_ref().and_then(|p| p.toolchain.get(name).cloned());
    Ok(json!({
        "name": abs.file_name().map_or_else(|| name.to_string(), |n| n.to_string_lossy().into_owned()),
        "dir": dir.display().to_string(),
        "provision": cfg.provision.is_some(),
        "drivers": cfg.provision.as_ref().is_some_and(|p| p.drivers),
        "node": tool("node"),
        "python": tool("python"),
        "go": tool("go"),
        "rust": tool("rust"),
        "checkpoint": dir.join("raw/checkpoint").display().to_string(),
        "candidates": cfg.candidates.keys().collect::<Vec<_>>(),
    }))
}

pub fn trigger_verb(p: &Parsed) -> Done {
    let text = message(p)?;
    let root: PathBuf = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    let marker = marker::parse(&text).map_err(|e| fail(e, "write the marker as [record <set>... [chunk=<minutes>] [fresh]] or [replay <set>...] (wf mimic trigger --help)"))?;
    let Some(m) = marker else {
        return Ok((json!({"ok": true, "go": false, "action": null, "sets": [], "matrix": {"include": []}, "warnings": [], "next": "nothing to do: the message carries no marker"}), 0));
    };
    let mut include = Vec::new();
    for word in &m.sets {
        let dir = marker::resolve(&root, word).map_err(|e| fail(e, "name a directory that holds a mimic.toml (wf mimic verify --all lists them)"))?;
        include.push(check(&root, &dir, m.action, word).map_err(|e| fail(e, "fix the set or drop it from the marker"))?);
    }
    let sets: Vec<&Value> = include.iter().map(|i| &i["name"]).collect();
    Ok((
        json!({
            "ok": true, "go": true, "action": m.action.word(), "chunk": m.chunk, "fresh": m.fresh, "sets": sets,
            "matrix": {"include": include}, "warnings": [],
            "next": format!("wf mimic job {} <set>, once per set, on the runner", m.action.word()),
        }),
        0,
    ))
}
