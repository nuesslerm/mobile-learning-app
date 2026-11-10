//! `wf test drift` (sandbox-first CI, S5): the Nx task hashes of this workspace, and where two environments' hashes differ and why.
//! A sandbox pass replays on CI only when both compute the same hash for the task, so a hash that moves with the checkout's path,
//! HOME, PATH or an unpinned tool silently turns every replay into a rerun. `--save` writes this environment's table, `--against`
//! compares it with another's (CI writes its own into the run bundle), `--pins` checks the local tools against `mise.toml`.
use wf_bucket::pins::{mise_pins, pins, Pin};
use wf_kit::args::{parse_spec, Positional, Spec};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The cached targets: the package scripts the stages run, the crates' flow and integration tests and the focused e2e tests.
const TARGETS: &str = "lint,typecheck,test:unit,test:flow,test:integration,cargo-flow,cargo-integration,cargo-e2e,cargo-e2e-ask,cargo-e2e-parity,cargo-e2e-parity_features,cargo-e2e-parity_capabilities,cargo-e2e-smoke,cargo-unit,cargo-clippy,e2e-browser,e2e-android,export:web,export:eas,cargo-contract,cargo-verify-deployed,check:editor-bundle";

const SPEC: Spec = Spec { values: &["save", "against", "targets", "projects"], flags: &["pins", "blocking"], positional: Positional::None, ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let root = crate::e2e_tests::repo();
    if p.flag("pins") {
        return Some(pins_report(&root));
    }
    let mine = match hash_table(&root, p.val("targets").unwrap_or(TARGETS), p.val("projects")) {
        Ok(table) => table,
        Err(error) => {
            println!("{}", json!({ "ok": false, "error": error, "next": "pnpm install --frozen-lockfile, then wf test drift again" }));
            return Some(1);
        }
    };
    let tasks = mine["tasks"].as_object().map_or(0, Map::len);
    if let Some(file) = p.val("save") {
        if let Err(e) = std::fs::write(file, mine.to_string()) {
            println!("{}", json!({ "ok": false, "error": format!("cannot write {file}: {e}") }));
            return Some(1);
        }
        println!("{}", json!({ "ok": true, "saved": file, "tasks": tasks, "runtime": mine["runtime"] }));
        return Some(0);
    }
    let Some(file) = p.val("against") else {
        let hashes: Map<String, Value> = mine["tasks"].as_object().into_iter().flatten().map(|(id, t)| (id.clone(), t["hash"].clone())).collect();
        println!("{}", json!({ "ok": true, "tasks": tasks, "runtime": mine["runtime"], "hashes": hashes }));
        return Some(0);
    };
    let Some(theirs) = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(|t| t["tasks"].is_object()) else {
        println!("{}", json!({ "ok": false, "error": format!("{file} is not a table `wf test drift --save` wrote"), "next": "wf test drift --save <file> on the other side" }));
        return Some(1);
    };
    let mut report = compare(&mine, &theirs);
    let blocking = blocking_causes(&report);
    report["blocking"] = json!(blocking);
    println!("{report}");
    report_drift(&report);
    Some(i32::from(if p.flag("blocking") { !blocking.is_empty() } else { report["ok"] != true }))
}

/// `{schema, runtime: {<command>: <output>}, tasks: {<project>:<target>: {hash, inputs: {<input>: <hash>}}}}` from Nx's own hasher.
fn hash_table(root: &Path, targets: &str, projects: Option<&str>) -> Result<Value, String> {
    wf_bucket::nx_why::hash_table(root, targets, projects)
}

fn kind(input: &str) -> &'static str {
    match input {
        "command" => "command",
        "AllExternalDependencies" => "dependencies",
        i if i.starts_with("runtime:") && i.contains("process.platform") => "platform",
        i if i.starts_with("runtime:") => "runtime",
        i if i.starts_with("env:") => "env",
        i if i.ends_with(":ProjectConfiguration") || i.ends_with(":TsConfig") => "config",
        _ => "files",
    }
}

/// Which tasks hash differently in the two tables, and on which input each differs: the report is the same for any pair of
/// environments (sandbox against CI, or CI against its last run).
fn compare(mine: &Value, theirs: &Value) -> Value {
    let empty = Map::new();
    let (a, b) = (mine["tasks"].as_object().unwrap_or(&empty), theirs["tasks"].as_object().unwrap_or(&empty));
    let (mut same, mut drifted, mut only_here, mut only_there) = (0, Vec::new(), Vec::new(), Vec::new());
    let mut causes: BTreeMap<String, Value> = BTreeMap::new();
    for id in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        let (Some(x), Some(y)) = (a.get(id), b.get(id)) else {
            if a.contains_key(id) {
                only_here.push(id);
            } else {
                only_there.push(id);
            }
            continue;
        };
        if x["hash"] == y["hash"] {
            same += 1;
            continue;
        }
        let (xi, yi) = (x["inputs"].as_object().unwrap_or(&empty), y["inputs"].as_object().unwrap_or(&empty));
        let differing: Vec<&String> = xi.keys().chain(yi.keys()).collect::<BTreeSet<_>>().into_iter().filter(|k| xi.get(*k) != yi.get(*k)).collect();
        for input in &differing {
            let value = |side: &Value, table: &Map<String, Value>| match input.strip_prefix("runtime:") {
                Some(command) => side["runtime"][command].clone(),
                None => table.get(*input).cloned().unwrap_or(Value::Null),
            };
            let cause = causes.entry((*input).clone()).or_insert_with(|| json!({ "kind": kind(input), "mine": value(mine, xi), "theirs": value(theirs, yi), "tasks": 0 }));
            cause["tasks"] = json!(cause["tasks"].as_u64().unwrap_or(0) + 1);
        }
        drifted.push(json!({ "task": id, "inputs": differing }));
    }
    let ok = drifted.is_empty() && only_here.is_empty() && only_there.is_empty();
    let mut report = json!({ "ok": ok, "compared": same + drifted.len(), "same": same, "drifted": drifted, "causes": causes, "onlyHere": only_here, "onlyThere": only_there });
    if !ok {
        report["next"] = json!("a runtime cause: install the tools mise.toml pins on the side that differs (wf test drift --pins); a platform cause: only a crate in the platform tier hashes on the platform, check its `[package.metadata.flow] platform` flag; a files cause: the sides hashed different checkouts, compare `nx show target <task> inputs` on both");
    }
    report
}

/// The causes that make `--blocking` fail: any input that differs on tasks both sides hashed, except the platform (only a crate in the
/// platform tier hashes on it, and a cause the sides cannot share), and a task only one side has, which is reported and does not block.
fn blocking_causes(report: &Value) -> Vec<String> {
    report["causes"].as_object().into_iter().flatten().filter(|(_, c)| c["kind"] != "platform").map(|(input, _)| input.clone()).collect()
}

/// `ci.drift.tasks_drifted` and the event that names the causes: how many tasks hash differently from the other side's.
fn report_drift(report: &Value) {
    use wf_bucket::ci_metrics::{event, Gauges, ALL};
    let drifted = report["drifted"].as_array().map_or(0, Vec::len);
    let causes: Vec<&str> = report["causes"].as_object().into_iter().flat_map(|c| c.keys().map(String::as_str)).take(10).collect();
    event(if drifted == 0 { "info" } else { "warn" }, "drift.report", &[("drifted", drifted.to_string()), ("compared", report["compared"].to_string()), ("causes", causes.join(";"))]);
    Gauges::new().set("ci.drift.tasks_drifted", ALL, ALL, drifted as f64).send();
}

/// A version the repository itself declares for a tool `mise.toml` pins: a CI job that sets up another node hashes every task on it,
/// so its run never replays the sandbox's.
struct Declared {
    file: String,
    line: usize,
    tool: &'static str,
    version: String,
}

/// The `node-version:` lines of one workflow or composite action; a line ending in `# pins-exempt: <reason>` sets up a node no Nx task runs on (a reference server's toolchain) and is left out.
fn node_versions(file: &str, text: &str) -> Vec<Declared> {
    let versions = text.lines().enumerate().filter_map(|(n, line)| {
        let value = line.trim().trim_start_matches("- ").strip_prefix("node-version:")?;
        if value.split_once("# pins-exempt:").is_some_and(|(_, why)| !why.trim().is_empty()) {
            return None;
        }
        Some(Declared { file: file.to_string(), line: n + 1, tool: "node", version: value.split('#').next().unwrap_or_default().trim().trim_matches(['"', '\'']).to_string() })
    });
    versions.collect()
}

/// Every `node-version:` of `.github/workflows` and `.github/actions`, and the root `packageManager` (`pnpm@11.5.2+sha...`).
fn declared(root: &Path) -> Vec<Declared> {
    let ls = |dir: &Path| -> Vec<std::path::PathBuf> { std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect() };
    let mut files: Vec<std::path::PathBuf> = ls(&root.join(".github/workflows")).into_iter().filter(|f| f.extension().is_some_and(|x| x == "yml" || x == "yaml")).collect();
    files.extend(ls(&root.join(".github/actions")).into_iter().map(|d| d.join("action.yml")));
    files.sort();
    let mut found: Vec<Declared> = files
        .iter()
        .filter_map(|f| Some(node_versions(&f.strip_prefix(root).ok()?.display().to_string(), &std::fs::read_to_string(f).ok()?)))
        .flatten()
        .collect();
    let package = std::fs::read_to_string(root.join("package.json")).unwrap_or_default();
    let manager = serde_json::from_str::<Value>(&package).ok().and_then(|v| v["packageManager"].as_str().map(str::to_string));
    if let Some((_, version)) = manager.as_deref().and_then(|m| m.split_once('@')).filter(|(tool, _)| *tool == "pnpm") {
        let line = package.lines().position(|l| l.contains("\"packageManager\"")).map_or(0, |n| n + 1);
        found.push(Declared { file: "package.json".to_string(), line, tool: "pnpm", version: version.split('+').next().unwrap_or_default().to_string() });
    }
    found
}

fn pins_report(root: &Path) -> i32 {
    let pins = pins(root);
    let mise = mise_pins(&std::fs::read_to_string(root.join("mise.toml")).unwrap_or_default());
    let pinned = |tool: &str| mise.iter().find(|(k, v)| k == tool && v != "latest").map(|(_, v)| v.clone());
    let declared: Vec<(Declared, String)> = declared(root).into_iter().filter_map(|d| pinned(d.tool).map(|p| (d, p))).collect();
    let off: Vec<Value> = declared.iter().filter(|(d, p)| d.version != *p).map(|(d, p)| json!({ "file": d.file, "line": d.line, "tool": d.tool, "version": d.version, "pinned": p })).collect();
    let tools_ok = pins.iter().all(Pin::ok);
    let ok = tools_ok && off.is_empty();
    let listed: Vec<Value> = pins.iter().map(|p| json!({ "tool": p.tool, "pinned": p.pinned, "actual": p.actual, "ok": p.ok() })).collect();
    let mut report = json!({ "ok": ok, "pins": listed, "declared": { "checked": declared.len(), "off": off } });
    if !tools_ok {
        report["next"] = json!("mise install: a task hashed on other versions never shares a cache entry with the pinned ones, and this run may not seed the shared cache");
    } else if !off.is_empty() {
        report["next"] = json!("set each listed file to the version mise.toml pins (it is the one source), then wf test drift --pins again");
    }
    println!("{report}");
    i32::from(!ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLATFORM: &str = "runtime:node -p \"process.platform+'-'+process.arch\"";

    fn table(platform: &str) -> Value {
        json!({ "runtime": { PLATFORM.trim_start_matches("runtime:"): platform }, "tasks": { "crate-a:cargo-flow": { "hash": platform, "inputs": { PLATFORM: platform } } } })
    }

    #[test]
    fn given_two_tables_that_differ_by_platform_when_compared_then_the_cause_is_a_platform() {
        let report = compare(&table("linux-arm64"), &table("linux-x64"));
        assert_eq!(report["causes"][PLATFORM], json!({ "kind": "platform", "mine": "linux-arm64", "theirs": "linux-x64", "tasks": 1 }), "{report}");
    }
}
