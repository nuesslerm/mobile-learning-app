//! Why an Nx task missed the cache (issue #364, owner 8 Nov): every run leaves, per task, the input digests Nx hashed, in the
//! verification-artifacts bucket (`verification/nx-inputs/<target>.json`, the last few runs of each task). A miss is then explained against the
//! nearest earlier entry of the same task: a file, an env var (by name, never by value), a runtime tool, `expired` (an earlier run stored this
//! very hash) or `never-stored` (no earlier run, or the run that computed this hash failed and stored nothing). Digests only: Nx's own
//! per-input hashes, and the npm packages folded into one group per task (listed once per distinct group, in `groups`).
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::SystemTime;

const SCRIPT: &str = include_str!("nx_hashes.mjs");
pub const PREFIX: &str = "verification/nx-inputs";
/// Entries kept per task: enough to find the nearest earlier run without growing the shard without bound.
const KEEP: usize = 6;
const SHOWN: usize = 12;

/// `{schema, runtime: {<command>: <output>}, tasks: {<project>:<target>: {hash, inputs: {<input>: <hash>}}}}` from Nx's own hasher.
pub fn hash_table(root: &Path, targets: &str, projects: Option<&str>) -> Result<Value, String> {
    let node = std::env::var_os("WF_NODE").unwrap_or_else(|| "node".into());
    let mut cmd = Command::new(node);
    cmd.args(["--input-type=module", "-"]).arg(root).arg(targets);
    if let Some(projects) = projects {
        cmd.arg(projects);
    }
    // Git's own environment inside a hook points at the commit's index: Nx must list the files the way an ordinary run does.
    for key in ["GIT_INDEX_FILE", "GIT_DIR", "GIT_WORK_TREE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_PREFIX"] {
        cmd.env_remove(key);
    }
    let mut child = cmd.current_dir(root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("cannot start node: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(SCRIPT.as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| format!("node: {e}"))?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = said.lines().filter(|l| !l.trim().is_empty()).rev().take(6).collect();
        return Err(format!("hashing the Nx tasks failed with exit {}: {}", out.status.code().unwrap_or(1), tail.into_iter().rev().collect::<Vec<_>>().join(" | ")));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.lines().last().unwrap_or_default()).map_err(|e| format!("the hasher printed unreadable JSON: {e}"))
}

/// One task of an Nx run, as `run.json` tells it.
#[derive(Debug, Clone)]
pub struct RunTask {
    pub task: String,
    pub hash: String,
    /// Nx executed it (a cache miss), as opposed to replaying it.
    pub ran: bool,
    pub passed: bool,
}

/// The tasks of the newest `run.json` under `home/.nx/*/cache` written at or after `since`.
pub fn run_tasks(home: &Path, since: SystemTime) -> Vec<RunTask> {
    let Ok(workspaces) = std::fs::read_dir(home.join(".nx")) else { return Vec::new() };
    let newest = workspaces
        .filter_map(Result::ok)
        .map(|w| w.path().join("cache/run.json"))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .filter(|(at, _)| *at >= since)
        .max_by_key(|(at, _)| *at);
    let Some(doc) = newest.and_then(|(_, path)| std::fs::read_to_string(path).ok()).and_then(|raw| serde_json::from_str::<Value>(&raw).ok()) else { return Vec::new() };
    doc["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| Some(RunTask { task: t["taskId"].as_str()?.to_string(), hash: t["hash"].as_str()?.to_string(), ran: t["cacheStatus"] == "cache-miss", passed: t["status"].as_i64() == Some(0) }))
        .collect()
}

/// Whether a run explains and records its misses: always where `WF_NX_WHY=1`, never where it is `0`, else where the environment can
/// reach the bucket (a CI job, or a shell holding the write link).
pub fn enabled() -> bool {
    let var = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    match var("WF_NX_WHY").as_deref() {
        Some("0") => false,
        Some(_) => true,
        None => var("GITHUB_RUN_ID").is_some() || var("VERIFICATION_ARTIFACTS_PAR_URL").is_some(),
    }
}

/// Who ran: the CI run id and commit, or the clock and the checkout's head for a sandbox run (the push gate).
fn identity(root: &Path) -> (String, String, &'static str, i64) {
    let var = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let ts = wf_kit::time::now_ms() as i64;
    let sha = var("GITHUB_SHA").or_else(|| git(root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())).unwrap_or_default();
    match var("GITHUB_RUN_ID") {
        Some(run) => (run, sha, "ci", ts),
        None => (format!("local-{ts}"), sha, "local", ts),
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// The digest of an npm packages group and its members.
type Group = (String, BTreeMap<String, String>);

/// What a task's hash was made of, from either side of a comparison.
#[derive(Debug, Clone, Default)]
struct Snapshot {
    run: String,
    sha: String,
    ts: i64,
    source: String,
    hash: String,
    passed: bool,
    outcome: String,
    inputs: BTreeMap<String, String>,
    /// The digest of the npm packages group and, when the shard still lists it, its members.
    npm: Option<Group>,
    runtime: BTreeMap<String, String>,
}

fn strings(v: &Value) -> BTreeMap<String, String> {
    v.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()
}

impl Snapshot {
    fn to_json(&self) -> Value {
        json!({ "run": self.run, "sha": self.sha, "ts": self.ts, "source": self.source, "hash": self.hash, "outcome": self.outcome, "passed": self.passed,
                "inputs": self.inputs, "npm": self.npm.as_ref().map(|(d, _)| d), "runtime": self.runtime })
    }

    fn of_json(v: &Value, groups: &Value) -> Option<Snapshot> {
        let text = |k: &str| v[k].as_str().map(str::to_string);
        let npm = v["npm"].as_str().map(|d| (d.to_string(), strings(&groups[d])));
        Some(Snapshot { run: text("run")?, sha: text("sha")?, ts: v["ts"].as_i64()?, source: text("source")?, hash: text("hash")?, passed: v["passed"].as_bool()?, outcome: text("outcome")?, inputs: strings(&v["inputs"]), npm, runtime: strings(&v["runtime"]) })
    }

    /// Every input by name; the npm packages one by one when both sides know them, else as the one group they were folded into.
    fn flat(&self, with_members: bool) -> BTreeMap<String, String> {
        let mut all = self.inputs.clone();
        if let Some((digest, members)) = &self.npm {
            if with_members {
                all.extend(members.iter().map(|(k, v)| (k.clone(), v.clone())));
            } else {
                all.insert("npm:*".to_string(), digest.clone());
            }
        }
        all
    }
}

/// The npm packages of a hash table's task folded into one digest, so a shard stays small.
fn fold_npm(inputs: &Map<String, Value>) -> (BTreeMap<String, String>, Option<Group>) {
    let (mut rest, mut npm) = (BTreeMap::new(), BTreeMap::new());
    for (k, v) in inputs {
        let digest = v.as_str().unwrap_or_default().to_string();
        if k.starts_with("npm:") {
            npm.insert(k.clone(), digest);
        } else {
            rest.insert(k.clone(), digest);
        }
    }
    if npm.is_empty() {
        return (rest, None);
    }
    let mut hasher = Sha256::new();
    for (k, v) in &npm {
        hasher.update(format!("{k}={v}\n"));
    }
    (rest, Some((hex::encode(hasher.finalize())[..12].to_string(), npm)))
}

fn target_of(task: &str) -> &str {
    task.split_once(':').map_or("", |(_, target)| target)
}

fn project_of(task: &str) -> &str {
    task.split_once(':').map_or(task, |(project, _)| project)
}

fn shard_name(target: &str) -> String {
    target.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

fn shard_url(base: &str, target: &str) -> String {
    format!("{base}/{PREFIX}/{}.json", shard_name(target))
}

/// The target's shard (`{tasks: {<task>: [entry]}, groups: {<digest>: {<package>: <digest>}}}`); a target never recorded has an empty one.
fn read_shard(target: &str) -> Result<Value, String> {
    let url = shard_url(&crate::artifacts::public_base(), target);
    match crate::artifacts::read_public(&url)? {
        None => Ok(json!({ "schema": 1, "tasks": {}, "groups": {} })),
        Some(text) => serde_json::from_str::<Value>(&text).ok().filter(|v| v["tasks"].is_object()).ok_or_else(|| format!("{url} is not an input shard; it was left as it is")),
    }
}

/// The paths an input digests, for `git diff`: the files a `workspace:[...]` input names, or the directory a project's glob starts at.
fn pathspecs(input: &str) -> Vec<String> {
    if let Some(list) = input.strip_prefix("workspace:[").and_then(|l| l.strip_suffix(']')) {
        return list.split(',').map(|f| f.trim_start_matches("{workspaceRoot}/").to_string()).collect();
    }
    let globs = input.split_once(':').map_or(input, |(_, g)| g);
    globs.split(',').filter(|g| !g.starts_with('!')).filter_map(|g| g.split("/*").next().map(str::to_string)).filter(|p| !p.is_empty()).take(1).collect()
}

fn kind_of(input: &str) -> &'static str {
    match input {
        "command" => "command",
        "AllExternalDependencies" => "dependencies",
        i if i.starts_with("runtime:") => "runtime",
        i if i.starts_with("env:") => "env",
        i if i.starts_with("npm:") => "npm",
        i if i.ends_with(":ProjectConfiguration") || i.ends_with(":TsConfig") => "config",
        _ => "files",
    }
}

/// Where two snapshots' inputs differ. An env var is named and its change is only said (a digest differs); a runtime tool carries both
/// outputs; a file input carries the files `git diff` names between the two commits, or why it cannot.
fn changes(root: &Path, now: &Snapshot, then: &Snapshot) -> Vec<Value> {
    let members = now.npm.as_ref().is_some_and(|(_, m)| !m.is_empty()) && then.npm.as_ref().is_some_and(|(_, m)| !m.is_empty());
    let (a, b) = (now.flat(members), then.flat(members));
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let mut out = Vec::new();
    for input in keys.into_iter().filter(|k| a.get(*k) != b.get(*k)) {
        let how = match (a.contains_key(input), b.contains_key(input)) {
            (true, false) => "added",
            (false, true) => "removed",
            _ => "changed",
        };
        let kind = kind_of(input);
        let mut change = json!({ "input": input, "kind": kind, "change": how });
        match kind {
            "env" => change["name"] = json!(input.trim_start_matches("env:")),
            "runtime" => {
                let command = input.trim_start_matches("runtime:");
                change["was"] = json!(then.runtime.get(command));
                change["now"] = json!(now.runtime.get(command));
            }
            "files" => {
                let specs = pathspecs(input);
                let named = (then.sha != now.sha && !then.sha.is_empty()).then(|| {
                    let mut args = vec!["diff", "--name-only", then.sha.as_str(), now.sha.as_str(), "--"];
                    args.extend(specs.iter().map(String::as_str));
                    git(root, &args)
                });
                match named.flatten() {
                    Some(list) if !list.trim().is_empty() => change["files"] = json!(list.lines().take(SHOWN).collect::<Vec<_>>()),
                    Some(_) => change["note"] = json!("no committed file differs between the two commits: an uncommitted or ignored file, or a file Nx reads that git does not list, differs"),
                    None if then.sha == now.sha => change["note"] = json!("the same commit on both sides: an uncommitted or ignored file differs (a dirty tree, a generated file)"),
                    None => change["note"] = json!("the earlier commit is not in this checkout, so the files are not named"),
                }
                change["paths"] = json!(specs);
            }
            _ => {}
        }
        out.push(change);
    }
    out
}

fn age(ms: i64) -> String {
    let secs = (wf_kit::time::now_ms() as i64 - ms).max(0) / 1000;
    match secs {
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 172_800 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

fn who(s: &Snapshot) -> String {
    format!("{} run {} at {} ({})", s.source, s.run, s.sha.get(..9).unwrap_or(&s.sha), age(s.ts))
}

fn against(s: &Snapshot) -> Value {
    json!({ "run": s.run, "sha": s.sha, "ts": s.ts, "source": s.source, "hash": s.hash, "outcome": s.outcome, "passed": s.passed })
}

/// Why `now` missed, against the earlier entries of the task (oldest first). `stored: false` always says a failed run left nothing.
fn explain(root: &Path, task: &str, now: &Snapshot, earlier: &[Snapshot]) -> Value {
    let mut out = json!({ "task": task, "hash": now.hash });
    if !now.passed {
        out["thisRun"] = json!(format!("failed: it stored nothing under {}, so the next run misses again", now.hash));
    }
    if let Some(same) = earlier.iter().rev().find(|e| e.hash == now.hash) {
        let stored = same.passed;
        out["against"] = against(same);
        if stored {
            out["verdict"] = json!("expired");
            out["because"] = json!(format!("{} stored this very hash and the cache no longer has it: expired or evicted, or this run reads a cache that run did not write", who(same)));
        } else {
            out["verdict"] = json!("never-stored");
            out["because"] = json!(format!("{} computed this hash but failed, so it stored nothing", who(same)));
        }
        out["stored"] = json!(stored);
        return out;
    }
    let mut ranked: Vec<(usize, &Snapshot, Vec<Value>)> = earlier.iter().map(|e| { let c = changes(root, now, e); (c.len(), e, c) }).collect();
    ranked.sort_by_key(|(n, e, _)| (*n, std::cmp::Reverse(e.ts)));
    let Some((count, nearest, found)) = ranked.into_iter().next() else {
        out["verdict"] = json!("never-stored");
        out["because"] = json!("no earlier run of this task is recorded, so nothing was stored under this hash");
        out["stored"] = json!(false);
        return out;
    };
    let names: Vec<String> = found.iter().take(3).map(|c| match c["kind"].as_str() {
        Some("env") => format!("env var {}", c["name"].as_str().unwrap_or_default()),
        Some("runtime") => format!("runtime `{}`", c["input"].as_str().unwrap_or_default().trim_start_matches("runtime:")),
        Some("files") => match c["files"].as_array() {
            Some(f) if !f.is_empty() => format!("file {}", f.iter().filter_map(Value::as_str).take(3).collect::<Vec<_>>().join(", ")),
            _ => format!("files of {}", c["input"].as_str().unwrap_or_default().split(',').next().unwrap_or_default()),
        },
        _ => c["input"].as_str().unwrap_or_default().chars().take(60).collect(),
    }).collect();
    out["verdict"] = json!("input-changed");
    out["against"] = against(nearest);
    out["because"] = json!(format!("{count} input(s) differ from {}: {}{}", who(nearest), names.join("; "), if nearest.passed { "" } else { " (that run failed and stored nothing)" }));
    out["changedCount"] = json!(count);
    out["changed"] = json!(found.into_iter().take(SHOWN).collect::<Vec<_>>());
    out["stored"] = json!(false);
    out
}

struct Current {
    task: RunTask,
    now: Snapshot,
    /// Nx's own hash of the task as the hasher computed it, when it is not the hash the run reported.
    computed: Option<String>,
}

/// Hashes every task once per target (one `node` run each) and keeps the input digests.
fn currents(root: &Path, tasks: &[RunTask], id: &(String, String, &'static str, i64)) -> Result<Vec<Current>, String> {
    let mut out = Vec::new();
    let mut targets: Vec<&str> = tasks.iter().map(|t| target_of(&t.task)).collect();
    targets.sort_unstable();
    targets.dedup();
    for target in targets {
        let wanted: Vec<&RunTask> = tasks.iter().filter(|t| target_of(&t.task) == target).collect();
        let mut projects: Vec<&str> = wanted.iter().map(|t| project_of(&t.task)).collect();
        projects.sort_unstable();
        projects.dedup();
        let table = hash_table(root, target, Some(&projects.join(",")))?;
        for task in wanted {
            let Some(entry) = table["tasks"].get(&task.task) else { continue };
            let (inputs, npm) = fold_npm(entry["inputs"].as_object().unwrap_or(&Map::new()));
            let computed = entry["hash"].as_str().unwrap_or_default().to_string();
            let hash = if task.hash.is_empty() { computed.clone() } else { task.hash.clone() };
            let runtime = strings(&table["runtime"]).into_iter().filter(|(c, _)| inputs.contains_key(&format!("runtime:{c}"))).collect();
            let mut shown = task.clone();
            shown.hash = hash.clone();
            let now = Snapshot { run: id.0.clone(), sha: id.1.clone(), ts: id.3, source: id.2.to_string(), hash: hash.clone(), passed: task.passed, outcome: if task.ran { "ran" } else { "hit" }.to_string(), inputs, npm, runtime };
            out.push(Current { task: shown, now, computed: (computed != hash).then_some(computed) });
        }
    }
    Ok(out)
}

fn earlier_of(shard: &Value, task: &str) -> Vec<Snapshot> {
    shard["tasks"][task].as_array().into_iter().flatten().filter_map(|e| Snapshot::of_json(e, &shard["groups"])).collect()
}

/// Adds this run's entries to the target's shard, keeping the last few per task and the npm groups they name.
fn store(shard: &mut Value, target: &str, mine: &[&Current]) -> Result<(), String> {
    for c in mine {
        let list = shard["tasks"][&c.task.task].as_array().cloned().unwrap_or_default();
        let mut list: Vec<Value> = list.into_iter().filter(|e| e["run"] != c.now.run.as_str()).collect();
        let mut entry = c.now.to_json();
        if let Some(computed) = &c.computed {
            entry["computed"] = json!(computed);
        }
        list.push(entry);
        let from = list.len().saturating_sub(KEEP);
        shard["tasks"][&c.task.task] = Value::Array(list.split_off(from));
        if let Some((digest, members)) = &c.now.npm {
            shard["groups"][digest] = json!(members);
        }
    }
    let used: std::collections::BTreeSet<String> = shard["tasks"].as_object().into_iter().flatten().flat_map(|(_, l)| l.as_array().into_iter().flatten()).filter_map(|e| e["npm"].as_str().map(str::to_string)).collect();
    if let Some(groups) = shard["groups"].as_object_mut() {
        groups.retain(|digest, _| used.contains(digest));
    }
    let par = crate::artifacts::par_url()?;
    crate::artifacts::put_par(&format!("{par}{PREFIX}/{}.json", shard_name(target)), shard.to_string().as_bytes(), "application/json")
}

/// Explains every miss of `tasks` against the bucket's history and, with `record`, stores this run's entries (hits too: a hit proves
/// the hash is in the cache). `{schema, ok, run, source, misses: [{task, hash, verdict, because, against, changed[]}], recorded}`; the
/// verdicts are `input-changed`, `expired` and `never-stored`.
pub fn report(root: &Path, tasks: &[RunTask], record: bool) -> Value {
    let id = identity(root);
    let currents = match currents(root, tasks, &id) {
        Ok(c) => c,
        Err(error) => return json!({ "schema": 1, "ok": false, "error": error, "next": "pnpm install --frozen-lockfile, then wf test drift shows the hasher alone" }),
    };
    let mut misses = Vec::new();
    let mut recorded = json!(false);
    let mut targets: Vec<&str> = currents.iter().map(|c| target_of(&c.task.task)).collect();
    targets.sort_unstable();
    targets.dedup();
    let mut problems = Vec::new();
    for target in targets {
        let mine: Vec<&Current> = currents.iter().filter(|c| target_of(&c.task.task) == target).collect();
        let mut shard = match read_shard(target) {
            Ok(shard) => shard,
            Err(error) => {
                problems.push(format!("{target}: {error}"));
                continue;
            }
        };
        for c in mine.iter().filter(|c| c.task.ran) {
            let mut why = explain(root, &c.task.task, &c.now, &earlier_of(&shard, &c.task.task));
            if let Some(computed) = &c.computed {
                why["hasherDisagrees"] = json!(format!("the hasher computed {computed} for this task, Nx ran it under {}: the input table may not be what Nx hashed", c.now.hash));
            }
            misses.push(why);
        }
        if record {
            match store(&mut shard, target, &mine) {
                Ok(()) => recorded = json!(true),
                Err(error) => problems.push(format!("not recorded: {error}")),
            }
        }
    }
    json!({ "schema": 1, "ok": problems.is_empty(), "run": id.0, "source": id.2, "misses": misses, "recorded": recorded, "problems": problems })
}

/// `report` for tasks that have not run: the hash each would run under, explained as a miss (`wf test run --probe`).
pub fn probe(root: &Path, task_ids: &[String]) -> Value {
    if std::env::var("WF_NX_WHY").is_ok_and(|v| v == "0") {
        return Value::Null;
    }
    let tasks: Vec<RunTask> = task_ids.iter().map(|t| RunTask { task: t.clone(), hash: String::new(), ran: true, passed: true }).collect();
    report(root, &tasks, false)
}
