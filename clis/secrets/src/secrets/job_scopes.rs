//! `wf secret scope-check`: the keys each workflow job asks for against the scope of each key. A job's class (`open` or `deploy`) and
//! the keys it reads that no step names (a deploy leg verifying its service's group) are in `data/job-scopes.json`; the keys and
//! groups its own `run` steps name are read from the workflow files. An open job may read `ci-open` and `public` keys only; a deploy
//! job may also read `ci-deploy`; no job may read a `vm` key. Names only, never a value.
use super::groups::GROUPS;
use super::registry::{self, Entry, Scope};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wf_kit::args::Parsed;
use yaml_rust2::{Yaml, YamlLoader};

const RAW: &str = include_str!("../data/job-scopes.json");

struct Declared {
    class: String,
    keys: Vec<String>,
    groups: Vec<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect()
}

fn declared(raw: &str) -> Result<BTreeMap<String, Declared>, String> {
    let root: Value = serde_json::from_str(raw).map_err(|e| format!("job-scopes.json: {e}"))?;
    let jobs = root["jobs"].as_object().ok_or("job-scopes.json: expected a jobs object")?;
    jobs.iter()
        .map(|(id, j)| {
            let class = j["class"].as_str().unwrap_or("").to_string();
            if class != "open" && class != "deploy" {
                return Err(format!("job-scopes.json: {id}: class must be open or deploy"));
            }
            Ok((id.clone(), Declared { class, keys: strings(&j["keys"]), groups: strings(&j["groups"]) }))
        })
        .collect()
}

/// `workflow file name:job id` and the text of every `run` step of the job.
fn workflow_jobs(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    files.sort();
    let mut jobs = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let docs = YamlLoader::load_from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(Yaml::Hash(map)) = docs.first().map(|d| &d["jobs"]) else { continue };
        for (id, job) in map {
            let runs: Vec<&str> = job["steps"].as_vec().into_iter().flatten().filter_map(|s| s["run"].as_str()).collect();
            jobs.insert(format!("{name}:{}", id.as_str().unwrap_or("")), runs.join("\n"));
        }
    }
    Ok(jobs)
}

/// The groups a run text resolves with `--for <group>` and the registry keys it names on a `wf secret` line.
fn asked(run: &str, registry: &[Entry]) -> (Vec<String>, Vec<String>) {
    let for_group = Regex::new(r"--for[ =]([A-Za-z0-9_-]+)").expect("valid regex");
    let word = Regex::new(r"[A-Z][A-Z0-9_]{3,}").expect("valid regex");
    let (mut groups, mut keys) = (Vec::new(), Vec::new());
    for line in run.lines().filter(|l| l.contains("wf secret")) {
        groups.extend(for_group.captures_iter(line).map(|c| c[1].to_string()));
        keys.extend(word.find_iter(line).map(|m| m.as_str().to_string()).filter(|w| registry.iter().any(|e| e.key == *w)));
    }
    (groups, keys)
}

fn allowed(class: &str, scope: Scope) -> bool {
    match scope {
        Scope::Public | Scope::CiOpen => true,
        Scope::CiDeploy => class == "deploy",
        Scope::Vm => false,
    }
}

fn check(workflows: &Path, jobs_raw: &str) -> Result<(bool, Value), String> {
    let registry = registry::load()?;
    let declared = declared(jobs_raw)?;
    let found = workflow_jobs(workflows)?;
    let mut violations: Vec<String> = declared.keys().filter(|id| !found.contains_key(*id)).map(|id| format!("{id}: declared in job-scopes.json but not a job of any workflow")).collect();
    let mut report = Vec::new();
    for (id, run) in &found {
        let decl = declared.get(id);
        let class = decl.map_or("unclassified", |d| d.class.as_str());
        let (groups_named, keys_named) = asked(run, &registry);
        let mut asks: BTreeMap<String, String> = BTreeMap::new();
        let mut problems = Vec::new();
        for g in groups_named.iter().chain(decl.iter().flat_map(|d| &d.groups)) {
            match GROUPS.iter().find(|x| &x.app == g) {
                Some(group) => group.required.iter().chain(&group.optional).filter(|k| !group.vm_only.contains(k)).for_each(|k| {
                    asks.entry(k.clone()).or_insert_with(|| format!("group {g}"));
                }),
                None => problems.push(format!("{id}: asks for the unknown group {g}")),
            }
        }
        for k in &keys_named {
            asks.entry(k.clone()).or_insert_with(|| "workflow".to_string());
        }
        for k in decl.iter().flat_map(|d| &d.keys) {
            asks.entry(k.clone()).or_insert_with(|| "declared".to_string());
        }
        let mut keys = Vec::new();
        for (key, via) in &asks {
            let Some(entry) = registry.iter().find(|e| &e.key == key) else {
                problems.push(format!("{id}: asks for {key}, which is not a registry key"));
                continue;
            };
            if class == "unclassified" {
                problems.push(format!("{id}: asks for {key} but has no class in job-scopes.json"));
            } else if !allowed(class, entry.scope) {
                let why = if entry.scope == Scope::Vm { "not readable in CI" } else { "needs the ci-deploy token" };
                problems.push(format!("{id}: {class} job asks for {key} (scope {}, {why})", entry.scope.name()));
            }
            keys.push(json!({ "key": key, "scope": entry.scope.name(), "via": via }));
        }
        report.push(json!({ "id": id, "class": class, "ok": problems.is_empty(), "asks": keys, "violations": problems }));
        violations.extend(problems);
    }
    let ok = violations.is_empty();
    Ok((ok, json!({ "ok": ok, "jobs": report, "violations": violations })))
}

/// The violations of the committed job classes against the workflows in `workflows`; none when the directory is absent.
pub fn violations(workflows: &Path) -> Result<Vec<String>, String> {
    if !workflows.is_dir() {
        return Ok(Vec::new());
    }
    let (_, report) = check(workflows, RAW)?;
    Ok(strings(&report["violations"]))
}

pub fn cmd(p: &Parsed) -> Result<i32, String> {
    let workflows = p.opts.get("workflows").map_or_else(|| wf_kit::root::repo_root().join(".github/workflows"), PathBuf::from);
    let raw = match p.opts.get("jobs") {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?,
        None => RAW.to_string(),
    };
    let (ok, report) = check(&workflows, &raw)?;
    super::out(&format!("{}\n", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?));
    Ok(if ok { 0 } else { 1 })
}
