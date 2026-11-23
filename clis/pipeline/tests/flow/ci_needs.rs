//! `wf ci plan needs`: the tools one core-tests machine installs, from `data/test-needs.json`, and the guard that keeps that data in step with what the
//! crates' and packages' tests spawn, so a new test that needs Chromium cannot silently lose it.
use crate::support::{repo_root, run_wf, RunOpts};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TOOLS: [&str; 9] = ["playwright", "ffmpeg", "restic-sqlite", "bun", "java", "rust", "references", "opencode", "browser-harness"];

fn mapping() -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo_root().join("clis/pipeline/data/test-needs.json")).unwrap()).unwrap()
}

fn flow(name: &str) -> Value {
    json!({"kind": "flow", "crate": name, "dir": format!("clis/{name}"), "manifest": "clis/Cargo.toml", "why": ["all"]})
}

fn unit(name: &str) -> Value {
    json!({"kind": "unit", "crate": name, "dir": format!("clis/{name}"), "manifest": "clis/Cargo.toml", "why": ["all"]})
}

struct Needs {
    exit: i32,
    json: Value,
    output: String,
}

/// `wf ci plan needs --stage core --shard <i> --count <n> --plan <plan> --github-output` on a plan with these flow and typecheck-lint-unit units.
fn needs(flow_units: &[Value], unit_units: &[Value], shard: usize, count: usize) -> Needs {
    let dir = tempfile::tempdir().unwrap();
    let plan = dir.path().join("plan.json");
    std::fs::write(&plan, json!({"sha": "abc", "stages": {"flow-tests": flow_units, "typecheck-lint-unit-tests": unit_units}}).to_string()).unwrap();
    let gh = dir.path().join("gh-output");
    let args = ["ci", "plan", "needs", "--stage", "core", "--shard", &shard.to_string(), "--count", &count.to_string(), "--plan", &plan.display().to_string(), "--github-output"];
    let run = run_wf(&args, &RunOpts::default().env("GITHUB_OUTPUT", &gh.display().to_string()));
    let json = serde_json::from_str(run.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", run.stdout, run.stderr));
    Needs { exit: run.exit_code, json, output: std::fs::read_to_string(gh).unwrap_or_default() }
}

fn on(n: &Needs) -> Vec<&str> {
    TOOLS.iter().copied().filter(|k| n.json[*k] == json!(true)).collect()
}

#[test]
fn a_shard_of_only_a_pure_rust_crate_needs_the_rust_toolchain_and_nothing_else() {
    // Given a machine whose flow and unit units are one crate that spawns no outside tool
    // When the verb is asked for its tools
    let n = needs(&[flow("wf-kit")], &[unit("wf-kit")], 1, 1);
    // Then only rust is on, every key is printed in the order the workflow reads, and the step outputs carry the same values
    assert_eq!((n.exit, on(&n)), (0, vec!["rust"]), "{}", n.json);
    assert_eq!(n.json.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), TOOLS);
    assert!(n.output.contains("rust=true\n") && n.output.contains("playwright=false\n") && n.output.contains("restic-sqlite=false\n"), "{}", n.output);
}

#[test]
fn a_shard_holding_a_crate_whose_tests_start_chromium_gets_playwright() {
    // Given a machine that holds rca-exec, whose browser flow test asserts a Chromium is installed
    // When the verb is asked
    let n = needs(&[flow("rca-exec")], &[], 1, 1);
    // Then playwright is on beside rust, and the tools it does not use stay off
    assert_eq!(on(&n), vec!["playwright", "rust"], "{}", n.json);
}

#[test]
fn a_unit_the_data_does_not_list_gets_every_tool() {
    // Given a machine holding a crate nobody mapped beside a pure one
    // When the verb is asked
    let n = needs(&[flow("wf-kit"), flow("a-crate-added-yesterday")], &[], 1, 1);
    // Then every tool is on, so the mistake can only cost time
    assert_eq!(on(&n), TOOLS.to_vec(), "{}", n.json);
}

#[test]
fn a_machine_needs_the_union_of_its_flow_shard_and_the_mirrored_typecheck_shard_and_a_pure_node_package_needs_nothing() {
    // Given two machines, flow units dealt over them, and typecheck units dealt the mirrored way
    let flows = [flow("rca-exec"), flow("deploy"), flow("wf-kit"), flow("secrets")];
    let units = [unit("wf-kit"), unit("exec-testkit"), unit("secrets"), json!({"kind": "unit", "package": "@lib/logging", "dir": "frontend/libraries/logging", "script": "test:unit", "why": ["all"]})];
    let map = mapping();
    let names = |stage: &str, i: usize| -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let plan = dir.path().join("plan.json");
        std::fs::write(&plan, json!({"sha": "abc", "stages": {"flow-tests": flows, "typecheck-lint-unit-tests": units}}).to_string()).unwrap();
        let r = run_wf(&["ci", "plan", "shard-keys", "--stage", stage, "--count", "2", "--plan", &plan.display().to_string(), "--units"], &RunOpts::default());
        serde_json::from_str::<Vec<Vec<String>>>(r.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr))[i - 1].clone()
    };
    for shard in [1usize, 2] {
        // When the verb is asked for one machine
        let n = needs(&flows, &units, shard, 2);
        // Then each tool is on exactly when a unit of flow shard i or typecheck shard 3 - i is listed with it
        let held: Vec<String> = names("flow-tests", shard).into_iter().chain(names("typecheck-lint-unit-tests", 3 - shard)).map(|k| k.trim_start_matches("crate-").to_string()).collect();
        let want: Vec<&str> = TOOLS.iter().copied().filter(|t| held.iter().any(|h| map["units"][h.as_str()].as_array().is_some_and(|l| l.iter().any(|x| x == t)))).collect();
        assert_eq!(on(&n), want, "shard {shard} holds {held:?}");
    }
    // And the two machines together cover every tool the units use
    let all: Vec<&str> = TOOLS.iter().copied().filter(|t| (1..=2).any(|s| on(&needs(&flows, &units, s, 2)).contains(t))).collect();
    assert_eq!(all, vec!["playwright", "restic-sqlite", "rust", "opencode"]);
    // And a package of plain TypeScript tests alone needs no tool
    let ts = needs(&[], &[units[3].clone()], 1, 1);
    assert_eq!((ts.exit, on(&ts)), (0, vec![]), "{}", ts.json);
}

#[test]
fn a_plan_that_cannot_be_read_is_refused_so_the_workflow_can_fall_back_to_every_tool() {
    // Given a plan path that is not a file
    // When the verb is asked
    let r = run_wf(&["ci", "plan", "needs", "--stage", "core", "--shard", "1", "--count", "1", "--plan", "/nonexistent/plan.json"], &RunOpts::default());
    // Then it exits 2 with the ci-plan refusal
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!((r.exit_code, v["ok"].clone()), (2, json!(false)), "{}", r.stdout);
}

fn guards() -> Vec<(&'static str, Regex)> {
    [
        ("playwright", r"find_chromium|WF_CHROMIUM|PLAYWRIGHT_BROWSERS_PATH|ms-playwright|chromium\.launch|from .playwright.|require\(.playwright"),
        ("browser-harness", r"browser-harness/dist"),
        ("ffmpeg", r#"FFMPEG_PATH|"ffmpeg"|Command::new\("ffmpeg"#),
        ("restic-sqlite", r#"Command::new\("(restic|sqlite3)"\)"#),
        ("java", r#"which\("java"\)|Command::new\("java"\)|gradlew|JAVA_HOME"#),
        ("bun", r#"Command::new\("bun"\)|setup-bun"#),
        ("opencode", r#"Command::new\("opencode"\)|OPENCODE_BIN_DIR|which\("opencode"\)"#),
        ("references", r"references/(orca|t3code)"),
    ]
    .into_iter()
    .map(|(k, p)| (k, Regex::new(p).unwrap()))
    .collect()
}

fn unit_dirs(root: &Path, names: &[&str]) -> BTreeMap<String, PathBuf> {
    let mut found = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let file = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !["node_modules", "target", ".git", "references", "dist", ".expo", ".nx", "build"].contains(&file.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            let name = match file.as_str() {
                "Cargo.toml" => std::fs::read_to_string(&path).ok().and_then(|t| t.parse::<toml::Table>().ok()).and_then(|t| t.get("package")?.get("name")?.as_str().map(str::to_string)),
                "package.json" => std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["name"].as_str().map(str::to_string)),
                _ => None,
            };
            if let Some(n) = name.filter(|n| names.contains(&n.as_str())) {
                found.insert(n, path.parent().unwrap().to_path_buf());
            }
        }
    }
    found
}

/// Test-reading files of a unit: its tests and sources, not the e2e, integration, bench and verify suites a core-tests machine does not run.
fn scanned(dir: &Path, others: &[PathBuf], out: &mut Vec<PathBuf>) {
    let skip = Regex::new(r"^(e2e|integration|bench|blackbox|verify|deployed)[^/]*$|^(node_modules|target|dist|\.expo|build)$").unwrap();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let (path, file) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
        let stem = file.strip_suffix(".rs").or_else(|| file.strip_suffix(".ts")).unwrap_or(&file).to_string();
        if skip.is_match(&stem) || file.contains(".e2e.") || file.contains(".integration.") || file.contains(".bench.") || others.contains(&path) {
            continue;
        }
        if path.is_dir() {
            scanned(&path, others, out);
        } else if ["rs", "ts", "tsx", "js", "mjs", "cjs"].iter().any(|e| path.extension().is_some_and(|x| x == *e)) {
            out.push(path);
        }
    }
}

/// A source file counts only when it holds unit tests: the flow and e2e binaries are what run the rest.
fn spawns(path: &Path, re: &Regex) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    if path.components().any(|c| c.as_os_str() == "src") && path.extension().is_some_and(|e| e == "rs") && !text.contains("#[test]") {
        return None;
    }
    let line = text.lines().position(|l| !l.trim_start().starts_with(['/', '*', '#']) && re.is_match(l))?;
    Some(line + 1)
}

#[test]
fn a_crate_or_package_whose_tests_spawn_a_tool_is_listed_with_it_so_a_new_chromium_test_cannot_lose_its_browser() {
    // Given the data of what each unit installs, with the units it exempts and why
    let map = mapping();
    let listed = map["units"].as_object().unwrap();
    let names: Vec<&str> = listed.keys().map(String::as_str).collect();
    let dirs = unit_dirs(&repo_root(), &names);
    let all_dirs: Vec<PathBuf> = dirs.values().cloned().collect();
    let mut missing = Vec::new();
    // When the tests and sources of every listed unit are scanned for the strings that mean a tool is used
    for (name, tools) in listed {
        let dir = dirs.get(name).unwrap_or_else(|| panic!("{name} is listed in test-needs.json but no Cargo.toml or package.json names it"));
        if dir.join("pnpm-workspace.yaml").is_file() {
            continue;
        }
        let has = |t: &str| tools.as_array().unwrap().iter().any(|x| x == t);
        let exempt = |t: &str| map["exempt"][name.as_str()][t].is_string();
        let nested: Vec<PathBuf> = all_dirs.iter().filter(|d| *d != dir && d.starts_with(dir)).cloned().collect();
        let mut files = Vec::new();
        scanned(dir, &nested, &mut files);
        for (tool, re) in guards() {
            if has(tool) || exempt(tool) {
                continue;
            }
            if let Some(hit) = files.iter().find_map(|f| spawns(f, &re).map(|l| format!("{}:{l}", f.strip_prefix(repo_root()).unwrap_or(f).display()))) {
                missing.push(format!("{name} needs {tool} ({hit}): add it to its list in clis/pipeline/data/test-needs.json, or exempt it there with the reason"));
            }
        }
        // Then a crate lists rust, as does a package whose test script runs cargo
        let cargo = dir.join("Cargo.toml").is_file() || std::fs::read_to_string(dir.join("package.json")).is_ok_and(|t| t.contains("cargo test"));
        if cargo && !has("rust") {
            missing.push(format!("{name} runs cargo but is listed without rust"));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
