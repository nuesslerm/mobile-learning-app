//! `wf check --arch` (also part of `--staged`): the architecture rule set guards itself. Baselines and ast-grep `ignores` only
//! shrink against `git merge-base HEAD origin/dev` and name no wildcard path, and every ast-grep rule has a test with a valid and
//! an invalid case, so a rule that checks nothing or a baseline that grows fails the commit.
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Finding {
    pub file: String,
    pub kind: &'static str,
    pub detail: String,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

fn tracked(root: &Path) -> Vec<String> {
    git(root, &["ls-files"]).unwrap_or_default().lines().map(str::to_string).collect()
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

// A whole named subtree (`**/x/src/**`, `x/*/src/testing.rs`) is a boundary; a wildcard inside a name (`src/*.rs`) hides files nobody listed.
fn wildcard(entry: &str) -> bool {
    entry.trim_start_matches("**/").trim_end_matches("/**").replace("/*/", "/").contains('*')
}

/// The `ignores:` entries of an ast-grep rule file.
fn ignores(text: &str) -> Vec<String> {
    let item = Regex::new(r#"^\s+-\s+["']?(.+?)["']?\s*$"#).unwrap();
    let mut in_list = false;
    let mut out = vec![];
    for line in text.lines() {
        if line.starts_with("ignores:") {
            in_list = true;
        } else if in_list && !line.starts_with(' ') && !line.starts_with('-') {
            in_list = false;
        } else if in_list {
            if let Some(c) = item.captures(line) {
                out.push(c[1].to_string());
            }
        }
    }
    out
}

fn baseline(text: &str) -> Vec<String> {
    serde_json::from_str::<Value>(text).ok().and_then(|v| v.as_array().map(|a| a.iter().map(Value::to_string).collect())).unwrap_or_default()
}

fn entries(rel: &str, text: &str) -> Vec<String> {
    if rel.ends_with(".json") { baseline(text) } else { ignores(text) }
}

fn ratchet(root: &Path, files: &[String], out: &mut Vec<Finding>) {
    let base = git(root, &["merge-base", "HEAD", "origin/dev"]).map(|s| s.trim().to_string());
    for rel in files {
        let now = entries(rel, &read(root, rel));
        for e in now.iter().filter(|e| wildcard(e)) {
            out.push(Finding { file: rel.clone(), kind: "wildcard", detail: e.clone() });
        }
        let Some(base) = &base else { continue };
        let Some(was) = git(root, &["show", &format!("{base}:{rel}")]) else { continue };
        let before = entries(rel, &was).len();
        if now.len() > before {
            out.push(Finding { file: rel.clone(), kind: "grew", detail: format!("{} entries, {before} at the merge-base", now.len()) });
        }
    }
}

fn cases(text: &str, key: &str) -> usize {
    let mut in_list = false;
    let mut n = 0;
    for line in text.lines() {
        if line.starts_with(&format!("{key}:")) {
            in_list = true;
        } else if in_list && line.trim_start().starts_with("- ") {
            n += 1;
        } else if in_list && !line.starts_with(' ') && !line.starts_with('-') {
            in_list = false;
        }
    }
    n
}

fn tested(root: &Path, all: &[String], config: &str, out: &mut Vec<Finding>) {
    let dir = config.trim_end_matches("sgconfig.yml");
    let text = read(root, config);
    let test_dirs: Vec<String> = Regex::new(r"testDir:\s*(\S+)").unwrap().captures_iter(&text).map(|c| format!("{dir}{}/", &c[1])).collect();
    let id = Regex::new(r"(?m)^id:\s*(\S+)").unwrap();
    let rule_dirs: Vec<String> = text.lines().skip_while(|l| !l.starts_with("ruleDirs:")).skip(1).take_while(|l| l.starts_with(' ')).map(|l| format!("{dir}{}/", l.trim().trim_start_matches("- "))).collect();
    for rel in all.iter().filter(|f| rule_dirs.iter().any(|d| f.starts_with(d.as_str())) && f.ends_with(".yml")) {
        let Some(name) = id.captures(&read(root, rel)).map(|c| c[1].to_string()) else { continue };
        let test = all.iter().find(|f| test_dirs.iter().any(|d| f.starts_with(d.as_str())) && f.ends_with(&format!("/{name}-test.yml")));
        match test {
            None => out.push(Finding { file: rel.clone(), kind: "untested", detail: format!("no {name}-test.yml in {}", test_dirs.join(", ")) }),
            Some(t) => {
                let body = read(root, t);
                if cases(&body, "valid") == 0 || cases(&body, "invalid") == 0 {
                    out.push(Finding { file: t.clone(), kind: "untested", detail: "needs at least one valid and one invalid case".into() });
                }
            }
        }
    }
}

pub fn scan(root: &Path) -> Vec<Finding> {
    let all = tracked(root);
    let mut out = vec![];
    let ratcheted: Vec<String> = all
        .iter()
        .filter(|f| f.ends_with(".dependency-cruiser-known-violations.json") || ((f.starts_with("arch-rules/") || f.contains("/arch-rules/")) && f.ends_with(".yml")))
        .cloned()
        .collect();
    ratchet(root, &ratcheted, &mut out);
    for config in all.iter().filter(|f| f.ends_with("sgconfig.yml")) {
        tested(root, &all, config, &mut out);
    }
    out
}

pub fn report(findings: &[Finding]) -> Value {
    json!({ "findings": findings.iter().map(|f| json!({ "file": f.file, "kind": f.kind, "detail": f.detail })).collect::<Vec<_>>() })
}

pub fn gate(root: &Path) -> Option<(usize, String)> {
    let findings = scan(root);
    (!findings.is_empty()).then(|| (findings.len(), report(&findings).to_string()))
}

/// `wf check --arch`: the JSON report on stdout, exit 1 when a rule is untested or a baseline grew.
pub fn run(root: &Path) -> i32 {
    let findings = scan(root);
    println!("{}", report(&findings));
    i32::from(!findings.is_empty())
}
