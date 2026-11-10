//! The e2e tests (`e2e/*.lua`, each a scripted Lua test with a metadata header) and how a change picks them: `wf test plan` lists
//! them with every other test a push needs, marking the focused ones a change reaches (or all focused ones, or named focused ones), and
//! `wf test run --stage` runs one stage of such a plan (`test_plan.rs`).
use wf_kit::args::Parsed;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What an e2e test declares in its header.
#[derive(Debug, PartialEq, Clone)]
pub struct E2eTest {
    pub name: String,
    pub script: Option<String>,
    pub platform: String,
    /// What the test runs against: `update` (the EAS Update CI published, Android) or `export` (a web export served on the
    /// runner, browser). The header's raw value until `per_platform` settles it for one harness; `wf test run` refuses any other.
    pub against: String,
    /// Seconds one run of this test may take (`timeout:` in the header); the default is 1200.
    pub timeout_s: Option<u64>,
    /// `units: a=1; a=2 b=3`: one short job per unit, each with its `--arg` values; none means one job for the whole script.
    pub units: Vec<Vec<String>>,
    /// `focus: true`: the only runbooks that run anywhere; to run another one, focus it.
    pub focus: bool,
}

/// An e2e test from its file: the `--[[--- ... ---]]` header at the top of `e2e/<name>.lua` (one `key: value` a line); a
/// Lua file without one is a helper, not a test.
fn parse_test(path: &str, text: &str) -> Option<E2eTest> {
    let body = text.strip_prefix("--[[---\n")?;
    let front = &body[..body.find("\n---]]")?];
    let name = Path::new(path).file_stem()?.to_string_lossy().to_string();
    let get = |key: &str| front.lines().find_map(|l| l.strip_prefix(&format!("{key}:"))).map(|v| v.trim().to_string());
    Some(E2eTest {
        name,
        script: Some(path.to_string()),
        platform: get("platform").unwrap_or_else(|| "android".to_string()),
        against: get("against").unwrap_or_default(),
        timeout_s: get("timeout").and_then(|v| v.parse().ok()),
        focus: get("focus").is_some_and(|v| v == "true"),
        units: get("units").map(|v| v.split(';').map(|u| u.split_whitespace().map(str::to_string).collect::<Vec<_>>()).filter(|u| !u.is_empty()).collect()).unwrap_or_default(),
    })
}

/// The focused tests with a script: focus alone picks what CI runs (owner, 18 Oct), not the files a change touched.
fn select(runbooks: &[E2eTest]) -> Vec<&E2eTest> {
    runbooks.iter().filter(|r| r.script.is_some() && r.focus).collect()
}

/// The repository the queue reads: `WF_TEST_REPO`, the worker's own clone on the VM, or the checkout we run in.
pub fn repo() -> PathBuf {
    match std::env::var_os("WF_TEST_REPO") {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ if std::env::var("WF_SITE").as_deref() == Ok("vm") => PathBuf::from("/workspace/repos/wf-test"),
        _ => wf_kit::root::repo_root(),
    }
}

pub fn e2e_tests() -> Vec<E2eTest> {
    let dir = std::env::var_os("WF_TEST_E2E_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("e2e"));
    let mut out: Vec<E2eTest> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "lua"))
        .filter_map(|e| parse_test(&format!("e2e/{}", e.file_name().to_string_lossy()), &std::fs::read_to_string(e.path()).ok()?))
        .flat_map(per_platform)
        .collect();
    out.sort_by(|a, b| (&a.name, &a.platform).cmp(&(&b.name, &b.platform)));
    out
}

/// `platform: android, browser`: the same script is a test on each harness, each in its own CI stage.
fn per_platform(t: E2eTest) -> Vec<E2eTest> {
    t.platform.split(',').map(str::trim).filter(|p| !p.is_empty()).map(|p| E2eTest { platform: p.to_string(), against: against_for(&t.against, p), ..t.clone() }).collect()
}

/// `against:` for one harness: `android=update, browser=export` names one per harness, a bare value is every harness's, and
/// none is the harness's own (Android the update CI published, the browser a web export).
fn against_for(raw: &str, platform: &str) -> String {
    let named: Vec<(&str, &str)> = raw.split(',').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (k.trim(), v.trim())).collect();
    let bare = raw.trim();
    match named.iter().find(|(k, _)| *k == platform) {
        Some((_, v)) => v.to_string(),
        None if named.is_empty() && !bare.is_empty() => bare.to_string(),
        None if platform == "browser" => "export".to_string(),
        None => "update".to_string(),
    }
}

pub(crate) fn git(args: &[&str]) -> Option<String> {
    Command::new("git").args(args).current_dir(repo()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn head_sha() -> String {
    git(&["rev-parse", "HEAD"]).unwrap_or_default()
}


/// A unit's `k=v` pairs, sorted: the key a published run's `args` object is matched on.
pub fn unit_key(pairs: &[String]) -> String {
    let mut v = pairs.to_vec();
    v.sort();
    v.join(" ")
}

pub fn args_key(args: &Value) -> String {
    let pairs: Vec<String> = args.as_object().into_iter().flatten().filter(|(k, _)| !matches!(k.as_str(), "out" | "base" | "against")).map(|(k, v)| format!("{k}={}", v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect();
    unit_key(&pairs)
}

fn test_json(r: &E2eTest, selected: bool) -> Value {
    json!({ "name": r.name, "script": r.script, "platform": r.platform, "against": r.against, "focus": r.focus, "selected": selected })
}

/// Which runbooks a request names: explicit names, `--all`, or those a change touches (`--changed [--base <ref>]`).
pub fn pick(p: &Parsed, books: &[E2eTest]) -> Result<Vec<usize>, String> {
    let named: Vec<&str> = p.positional.iter().skip(1).map(String::as_str).collect();
    if !named.is_empty() {
        // a test on two harnesses is one entry per platform: a name picks them all, `--platform` keeps one
        let mut picked = Vec::new();
        for n in named {
            let all: Vec<usize> = (0..books.len()).filter(|i| books[*i].name == n).collect();
            if all.is_empty() {
                return Err(format!("no e2e test named {n} (e2e/{n}.lua with a header)"));
            }
            if !all.iter().any(|i| books[*i].focus && books[*i].script.is_some()) {
                return Err(format!("{n} is not focused: add `focus: true` to its front matter to run it"));
            }
            picked.extend(all.into_iter().filter(|i| books[*i].focus && books[*i].script.is_some()));
        }
        return Ok(picked);
    }
    if p.flag("all") {
        return Ok((0..books.len()).filter(|i| books[*i].script.is_some() && books[*i].focus).collect());
    }
    if p.flag("changed") {
        let picked: HashSet<String> = select(books).iter().map(|r| r.name.clone()).collect();
        return Ok((0..books.len()).filter(|i| picked.contains(&books[*i].name)).collect());
    }
    Ok(Vec::new())
}

/// `--base last-green`: the newest commit on dev whose CI run passed, so a run GitHub cancelled or a red one leaves its
/// tests to the next push; `<sha>~1` when gh cannot answer or none is an ancestor. Default HEAD~1.
pub(crate) fn resolve_base(raw: Option<&str>, sha: &str) -> String {
    match raw {
        None => "HEAD~1".to_string(),
        Some("last-green") => last_green(sha).unwrap_or_else(|| format!("{sha}~1")),
        Some(b) => b.to_string(),
    }
}

fn last_green(sha: &str) -> Option<String> {
    let repo_name = std::env::var("GITHUB_REPOSITORY").ok()?;
    let out = Command::new("gh")
        .args(["run", "list", "-R", &repo_name, "--workflow", "deploy.yml", "--branch", "dev", "--status", "success", "--limit", "30", "--json", "headSha", "--jq", ".[].headSha"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|green| !green.is_empty() && Command::new("git").args(["merge-base", "--is-ancestor", green, sha]).current_dir(repo()).status().is_ok_and(|s| s.success()))
        .map(str::to_string)
}

/// `wf test plan`: every stage's tests as JSON, or the e2e tests with the picked ones marked.
pub(crate) fn plan(p: &Parsed) -> i32 {
    let sha = p.val("sha").map(str::to_string).unwrap_or_else(head_sha);
    let books = e2e_tests();
    let mut picked = match pick(p, &books) {
        Ok(v) => v,
        Err(e) => {
            wf_kit::wf_err!("wf: {e}");
            return 1;
        }
    };
    if let Some(platform) = p.val("platform") {
        picked.retain(|i| books[*i].platform == platform);
    }
    let scripted: Vec<usize> = (0..books.len()).filter(|i| books[*i].script.is_some()).collect();
    let rows: Vec<Value> = scripted.iter().map(|i| test_json(&books[*i], picked.contains(i))).collect();
    // every stage's tests, package tests included (crate::test_plan)
    let named: Vec<String> = p.positional.iter().skip(1).cloned().collect();
    let base = resolve_base(p.val("base"), &sha);
    let scope = if !named.is_empty() {
        crate::test_plan::Scope::Named(named)
    } else if p.flag("all") {
        crate::test_plan::Scope::All
    } else if p.flag("changed") {
        crate::test_plan::Scope::Changed(git(&["diff", "--name-only", &base, &sha]).unwrap_or_default().lines().map(str::to_string).collect())
    } else {
        crate::test_plan::Scope::Named(Vec::new())
    };
    let packages = crate::verify::workspace_packages(&repo()).unwrap_or_default();
    let mut planned = match crate::test_plan::plan(&repo(), &sha, p.flag("changed").then_some(base.as_str()), &scope, &books, &packages) {
        Ok(v) => v,
        Err(e) => {
            println!("{}", json!({ "error": format!("cannot plan: {e}") }));
            return 1;
        }
    };
    if let Some(list) = p.val("force") {
        let names: Vec<String> = list.split(',').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect();
        match crate::test_plan::plan(&repo(), &sha, None, &crate::test_plan::Scope::All, &books, &packages) {
            Ok(everything) => crate::test_plan::force(&mut planned, &everything, &names),
            Err(e) => {
                println!("{}", json!({ "error": format!("cannot plan: {e}") }));
                return 1;
            }
        }
    }
    planned["tests"] = json!(rows);
    if p.flag("github-output") {
        let Some(path) = std::env::var_os("GITHUB_OUTPUT") else {
            wf_kit::wf_err!("wf: --github-output needs GITHUB_OUTPUT");
            return 1;
        };
        use std::io::Write;
        let written = std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| f.write_all(crate::test_plan::github_output(&planned).as_bytes()));
        if let Err(e) = written {
            wf_kit::wf_err!("wf: cannot write GITHUB_OUTPUT: {e}");
            return 1;
        }
    }
    if p.flag("json") || p.flag("github-output") {
        println!("{planned}");
    } else {
        for i in &scripted {
            println!("{} {} {} {}", if picked.contains(i) { "*" } else { " " }, books[*i].name, books[*i].platform, books[*i].script.as_deref().unwrap_or(""));
        }
    }
    0
}

/// `wf test run --stage <stage> [--plan <file>] [other run flags]`: the stage's tests from the plan CI wrote (or, without
/// `--plan`, from a plan of this commit's change against HEAD~1).
pub(crate) fn run_stage(args: &[String]) -> i32 {
    let mut stage = None;
    let mut plan_file = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stage" => {
                stage = args.get(i + 1).cloned();
                i += 2;
            }
            "--plan" => {
                plan_file = args.get(i + 1).cloned();
                i += 2;
            }
            _ => {
                rest.push(args[i].clone());
                i += 1;
            }
        }
    }
    let Some(stage) = stage.filter(|s| crate::test_plan::STAGES.contains(&s.as_str())) else {
        println!("{}", json!({ "error": "--stage wants one of the plan's stages", "next": crate::test_plan::STAGES.join(" | ") }));
        return 2;
    };
    let plan: Value = match plan_file {
        Some(f) => match std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str(&t).ok()) {
            Some(v) => v,
            None => {
                println!("{}", json!({ "error": format!("cannot read the plan {f}"), "next": "wf test plan --changed --base <ref> --json > plan.json" }));
                return 1;
            }
        },
        None => {
            let sha = head_sha();
            let files: Vec<String> = git(&["diff", "--name-only", "HEAD~1", &sha]).unwrap_or_default().lines().map(str::to_string).collect();
            let packages = crate::verify::workspace_packages(&repo()).unwrap_or_default();
            match crate::test_plan::plan(&repo(), &sha, Some("HEAD~1"), &crate::test_plan::Scope::Changed(files), &e2e_tests(), &packages) {
                Ok(v) => v,
                Err(e) => {
                    println!("{}", json!({ "error": format!("cannot plan: {e}") }));
                    return 1;
                }
            }
        }
    };
    crate::test_plan::run_stage(&stage, &plan, &rest)
}
