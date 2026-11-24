//! `wf ci plan`: everything the pipeline decides before its jobs run, in one place. `tags` reads the commit's `[ci:]`, `[services:]` and `[cache:]`
//! tags, `tests` plans the tests by stage, `legs` the deploy legs and the EAS Update, and `mimic` the capture jobs of a `[record ...]` or
//! `[replay ...]` marker. Each prints one JSON object and, with `--github-output`, writes the step outputs the workflow's jobs read.
//! The marker grammar and the plan over the sets are `mimic-plan-core`, pure; this adapter reads the commit and the checkout and writes the outputs.
use mimic_plan_core::{MarkerPlan, Refusal, SetFacts, SetSource};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use wf_kit::args::{parse_spec, Positional, Spec};

const WORDS: &str = "tags | tests | shard-keys | needs | shard-weights | legs | mimic";
const TAGS: Spec = Spec { values: &["sha", "services"], flags: &["fresh", "github-output"], positional: Positional::None, ..Spec::NONE };
const LEGS: Spec = Spec { values: &["since", "also"], flags: &["force", "github-output", "no-lookup"], positional: Positional::None, ..Spec::NONE };
const MIMIC: Spec = Spec { values: &["message", "file", "sha", "root"], flags: &["github-output"], positional: Positional::None, ..Spec::NONE };

fn say(value: &Value, code: i32) -> Option<i32> {
    println!("{value}");
    Some(code)
}

fn refused(error: &str, next: &str) -> Option<i32> {
    say(&json!({ "schema": "ci-plan/1", "ok": false, "error": error, "next": next, "warnings": [] }), 2)
}

/// Appends `key=value` lines to the file `GITHUB_OUTPUT` names: a value never holds a newline.
pub(crate) fn github_output<K: AsRef<str>>(pairs: &[(K, String)]) -> Result<(), String> {
    let path = std::env::var_os("GITHUB_OUTPUT").ok_or("--github-output needs GITHUB_OUTPUT")?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("cannot write GITHUB_OUTPUT: {e}"))?;
    for (key, value) in pairs {
        writeln!(file, "{}={value}", key.as_ref()).map_err(|e| format!("cannot write GITHUB_OUTPUT: {e}"))?;
    }
    Ok(())
}

pub fn run(rest: &[String]) -> Option<i32> {
    let (word, args) = rest.split_first()?;
    match word.as_str() {
        "tags" => tags(args),
        "tests" => {
            let spec = Spec { values: &["sha", "base", "platform", "force", "area"], flags: &["json", "changed", "all", "github-output"], positional: Positional::Any, ..Spec::NONE };
            let words: Vec<String> = std::iter::once("plan".to_string()).chain(args.iter().cloned()).collect();
            Some(crate::e2e_tests::plan(&parse_spec(&words, &spec)?))
        }
        "shard-keys" => shard_keys(args),
        "needs" => crate::ci_needs::run(args),
        "shard-weights" => crate::shard_weights::run(args),
        "legs" => legs(args),
        "mimic" => mimic(args),
        other => refused(&format!("wf ci plan {other}: no such plan"), &format!("wf ci plan <{WORDS}> (wf ci plan --help)")),
    }
}

/// `wf ci plan shard-keys --stage <stage> --count <n> --plan <file>`: one key per shard, a JSON array, from the crates each shard runs. A workflow
/// makes it the concurrency group, so a later commit's shard of the same crates queues behind the earlier one (a newer pending job replaces an older pending one).
fn shard_keys(args: &[String]) -> Option<i32> {
    let spec = Spec { values: &["stage", "count", "plan", "repo"], flags: &["units"], positional: Positional::None, ..Spec::NONE };
    let p = parse_spec(args, &spec)?;
    let (Some(stage), Some(plan_path)) = (p.val("stage"), p.val("plan")) else { return refused("shard-keys wants --stage and --plan", "wf ci plan shard-keys --stage integration-tests --count 4 --plan plan.json") };
    let count = p.val("count").and_then(|c| c.parse::<usize>().ok()).filter(|c| *c >= 1).unwrap_or(1);
    let plan: Value = match std::fs::read_to_string(plan_path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(plan) => plan,
        Err(e) => return refused(&format!("cannot read the plan {plan_path}: {e}"), "wf ci plan tests --json > plan.json"),
    };
    let repo = p.val("repo").map(PathBuf::from).unwrap_or_else(wf_kit::root::repo_root);
    let sha = plan["sha"].as_str().filter(|s| !s.is_empty()).unwrap_or("HEAD").to_string();
    if p.flag("units") {
        let units: Vec<Vec<String>> = (1..=count).map(|i| crate::test_plan::shard_unit_keys_in_plan(stage, &plan, i, count)).collect();
        println!("{}", json!(units));
        return Some(0);
    }
    let keys: Vec<String> = (1..=count).map(|i| crate::test_plan::shard_key_in_plan(stage, &plan, i, count, &repo, &sha)).collect();
    println!("{}", json!(keys));
    Some(0)
}

/// The last `[<name>:...]` of the message, its words split on commas and spaces.
fn tag(message: &str, name: &str) -> Vec<String> {
    let re = Regex::new(&format!(r"(?i)\[{name}:[^\]]*\]")).expect("a tag pattern");
    let Some(last) = re.find_iter(message).last() else { return Vec::new() };
    let inner = last.as_str().split_once(':').map_or("", |(_, v)| v).replace(['[', ']'], "");
    inner.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn commit_message(sha: &str) -> Result<String, String> {
    let out = Command::new("git").args(["log", "-1", "--format=%B", sha]).current_dir(wf_kit::root::repo_root()).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git log {sha}: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `[ci:a,b]` limits the test jobs; `[services:all|changed|a,b]` and `--services` pick the tests and legs forced on; `[cache:no]` and `--fresh` drop the caches.
fn tags(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &TAGS)?;
    let sha = p.val("sha").unwrap_or("HEAD");
    let message = match commit_message(sha) {
        Ok(m) => m,
        Err(e) => return refused(&e, "wf ci plan tags --sha <commit>"),
    };
    let ci = tag(&message, "ci");
    let run: Vec<(&str, bool)> = ["flow", "unit", "service", "browser", "android"].iter().map(|j| (*j, ci.is_empty() || ci.iter().any(|w| w == j))).collect();
    let name_ok = Regex::new(r"^[A-Za-z0-9_@][A-Za-z0-9_@/.:-]*$").expect("a name pattern");
    let mut warnings = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let given = p.val("services").unwrap_or("").replace(',', " ");
    for w in tag(&message, "services").into_iter().chain(given.split_whitespace().map(str::to_string)) {
        if name_ok.is_match(&w) {
            names.push(w);
        } else {
            warnings.push(format!("[services:] ignores the name '{w}'"));
        }
    }
    let joined = names.join(",");
    let wrapped = format!(",{joined},");
    let (force_tests, force_ship) = if wrapped.contains(",all,") {
        ("all".to_string(), "--force".to_string())
    } else if wrapped == ",," || wrapped == ",changed," {
        (String::new(), String::new())
    } else {
        let t = joined.replace("changed,", "");
        let t = t.strip_suffix(",changed").unwrap_or(&t).to_string();
        let ship = format!("--also {t}");
        (t, ship)
    };
    let fresh = p.flag("fresh") || matches!(tag(&message, "cache").join(" ").as_str(), "no" | "false" | "off");
    for w in &warnings {
        eprintln!("::warning::{w}");
    }
    let mut pairs: Vec<(String, String)> = run.iter().map(|(j, r)| (format!("run_{j}"), r.to_string())).collect();
    pairs.push(("force_tests".into(), force_tests.clone()));
    pairs.push(("force_ship".into(), force_ship.clone()));
    pairs.push(("fresh".into(), fresh.to_string()));
    if p.flag("github-output") {
        if let Err(e) = github_output(&pairs) {
            return refused(&e, "run it in a workflow step, where GITHUB_OUTPUT is set");
        }
    }
    say(
        &json!({
            "schema": "ci-plan/1", "ok": true, "plan": "tags", "sha": sha,
            "ci": ci, "run": run.iter().map(|(j, r)| (j.to_string(), json!(r))).collect::<serde_json::Map<_, _>>(),
            "services": if names.is_empty() { vec!["changed".to_string()] } else { names }, "force_tests": force_tests, "force_ship": force_ship, "fresh": fresh,
            "warnings": warnings, "next": "wf ci plan tests --changed --base last-green --github-output",
        }),
        0,
    )
}

/// The commit the latest EAS Update was published from, and the backend hosts it dials that have moved.
fn latest_update() -> Result<(String, Vec<String>), String> {
    wf_kit::wf_err!("wf: reading the latest EAS Update pointer from refs/wf/state");
    let started = std::time::Instant::now();
    let read = crate::eas::latest_pointer("dev");
    wf_kit::wf_err!("wf: read the latest EAS Update pointer in {} s ({})", crate::eas::seconds(started.elapsed()), match &read { Ok(_) => "found".to_string(), Err(e) if e.contains(crate::eas::NO_POINTER) => "not found".to_string(), Err(e) => format!("failed: {e}") });
    let pointer = read?;
    let commit = pointer["commit"].as_str().ok_or_else(|| "it names no commit".to_string())?;
    Ok((commit.to_string(), crate::eas::moved_hosts(&pointer)))
}

fn legs(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &LEGS)?;
    let also: Vec<String> = p.val("also").map(|a| a.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
    match crate::legs::plan(p.flag("force"), &also, p.val("since"), p.flag("no-lookup"), p.flag("github-output"), latest_update) {
        Ok(code) => code,
        Err(message) => {
            wf_kit::wf_err!("wf: {message}");
            Some(1)
        }
    }
}

fn message_of(p: &wf_kit::args::Parsed) -> Result<String, (String, String)> {
    let bad = |e: String, n: &str| (e, n.to_string());
    match (p.val("message"), p.val("file"), p.val("sha")) {
        (Some(m), None, None) => Ok(m.to_string()),
        (None, Some("-"), None) => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|e| bad(format!("stdin: {e}"), "pipe the commit message in, or pass --message"))?;
            Ok(text)
        }
        (None, Some(path), None) => std::fs::read_to_string(path).map_err(|e| bad(format!("{path}: {e}"), "name a readable file, or - for stdin")),
        (None, None, sha) => commit_message(sha.unwrap_or("HEAD")).map_err(|e| bad(e, "wf ci plan mimic --sha <commit>")),
        _ => Err(bad("give one of --message <text>, --file <path|->, --sha <commit>".into(), "wf ci plan mimic --file - < message.txt")),
    }
}

/// The mimic.toml and directories of a checkout, as the plan core reads them.
struct Disk {
    root: PathBuf,
}

const SKIP: [&str; 7] = ["node_modules", "target", ".git", "references", "dist", "tmp", ".nx"];

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(kind) = e.file_type() else { continue };
        let name = e.file_name();
        if kind.is_dir() && !SKIP.iter().any(|s| name == *s) {
            walk(root, &e.path(), out);
        } else if kind.is_file() && name == "mimic.toml" {
            out.push(dir.strip_prefix(root).unwrap_or(dir).to_path_buf());
        }
    }
}

fn read_table(path: &Path) -> Result<toml::Table, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// What a `[provision]` table pins: its toolchains and whether the drivers are installed (default: when node is pinned).
/// Toolchains, whether the drivers are installed, and the agent CLIs run for real.
type Provisioned = (BTreeMap<String, String>, bool, Vec<String>);

fn provision_facts(table: &toml::Table, origin: &str) -> Result<Provisioned, String> {
    let toolchain: BTreeMap<String, String> = table.get("toolchain").and_then(|t| t.as_table()).map(|t| t.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()).unwrap_or_default();
    let drivers = table.get("drivers").and_then(|d| d.as_bool()).unwrap_or(toolchain.contains_key("node"));
    let real_agents: Vec<String> = table.get("real_agents").and_then(|s| s.as_array()).map(|s| s.iter().filter_map(|a| a.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let real = !real_agents.is_empty();
    if table.get("step").and_then(|s| s.as_array()).is_none_or(|s| s.is_empty()) && !drivers && !real {
        return Err(format!("{origin}: no [[step]] and no drivers: a reference with nothing to install or build needs no [provision]"));
    }
    Ok((toolchain, drivers, real_agents))
}

impl SetSource for Disk {
    fn sets(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out.sort();
        out
    }

    fn facts(&self, dir: &Path) -> Result<SetFacts, String> {
        let abs = self.root.join(dir);
        let file = abs.join("mimic.toml");
        let table = read_table(&file)?;
        let mut facts = SetFacts {
            reference: table.contains_key("reference"),
            corpus: table.contains_key("corpus") || abs.join("corpus.json").is_file(),
            cassette: abs.join("cassette.json").is_file(),
            candidates: table.get("candidate").and_then(|c| c.as_table()).map(|c| c.keys().cloned().collect()).unwrap_or_default(),
            ..SetFacts::default()
        };
        let named = table.get("provision_file").and_then(|f| f.as_str());
        let found = match (table.get("provision").and_then(|p| p.as_table()), named) {
            (Some(_), Some(_)) => return Err(format!("{}: both [provision] and provision_file: a set has one", file.display())),
            (Some(inline), None) => Some(provision_facts(inline, &format!("{} [provision]", file.display()))?),
            (None, Some(name)) => {
                let path = name.strip_prefix("//").map_or_else(|| abs.join(name), |from_root| self.root.join(from_root));
                Some(provision_facts(&read_table(&path)?, &path.display().to_string())?)
            }
            (None, None) => None,
        };
        if let Some((toolchain, drivers, real_agents)) = found {
            facts.provision = true;
            facts.toolchain = toolchain;
            facts.drivers = drivers;
            facts.real_agents = real_agents;
        }
        Ok(facts)
    }
}

fn matrix_job(s: &mimic_plan_core::SetPlan) -> Value {
    let tool = |name: &str| s.facts.toolchain.get(name).cloned();
    json!({
        "name": s.name, "dir": s.dir.display().to_string(), "provision": s.facts.provision, "drivers": s.facts.drivers,
        "node": tool("node"), "python": tool("python"), "go": tool("go"), "rust": tool("rust"),
        "codex": s.facts.real_agents.iter().any(|a| a == "codex"),
        "checkpoint": s.dir.join("raw/checkpoint").display().to_string(), "candidates": s.facts.candidates,
    })
}

fn mimic_json(plan: Option<&MarkerPlan>) -> Value {
    let Some(plan) = plan else {
        return json!({"schema": "ci-plan/1", "ok": true, "plan": "mimic", "go": false, "action": null, "sets": [], "matrix": {"include": []}, "warnings": [], "next": "nothing to do: the message carries no marker"});
    };
    let m = &plan.marker;
    json!({
        "schema": "ci-plan/1", "ok": true, "plan": "mimic", "go": true, "action": m.action.word(), "chunk": m.chunk, "fresh": m.fresh, "rows": m.rows.join(","),
        "sets": plan.sets.iter().map(|s| &s.name).collect::<Vec<_>>(), "matrix": {"include": plan.sets.iter().map(matrix_job).collect::<Vec<_>>()}, "warnings": [],
        "next": format!("wf mimic capture {} <set>, once per set, on the runner", m.action.word()),
    })
}

fn mimic(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &MIMIC)?;
    let root: PathBuf = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    let message = match message_of(&p) {
        Ok(m) => m,
        Err((e, n)) => return refused(&e, &n),
    };
    let plan = match mimic_plan_core::from_message(&message, &Disk { root }) {
        Ok(plan) => plan,
        Err(Refusal { error, next }) => return refused(&error, &next),
    };
    let out = mimic_json(plan.as_ref());
    if p.flag("github-output") {
        let get = |k: &str| out[k].as_str().map(str::to_string).unwrap_or_else(|| out[k].to_string().trim_matches('"').to_string());
        let pairs = [
            ("go", get("go")),
            ("action", plan.as_ref().map_or(String::new(), |p| p.marker.action.word().to_string())),
            ("chunk", plan.as_ref().map_or(String::new(), |p| p.marker.chunk.to_string())),
            ("rows", get("rows").replace("null", "")),
            ("fresh", plan.as_ref().map_or("false".to_string(), |p| p.marker.fresh.to_string())),
            ("matrix", out["matrix"].to_string()),
        ];
        if let Err(e) = github_output(&pairs) {
            return refused(&e, "run it in a workflow step, where GITHUB_OUTPUT is set");
        }
    }
    say(&out, 0)
}
