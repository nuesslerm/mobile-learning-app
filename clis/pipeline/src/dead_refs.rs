//! `wf check --dead-refs` (and part of `--staged`): every repo path, markdown link, workspace package, `wf` command (with
//! the verb of `wf browser|android <verb>`), Lua `device:<method>` and `pnpm run` script named in the live instructions,
//! docs, help, scripts, workflows and manifests must exist. History (docs/archive, docs/audits, plans, goals) and the
//! generated agent config are not read, except that a goal's verifier is checked for its device verbs and methods; a rename
//! that leaves a caller behind fails the commit instead of a CI run.
//! The pre-commit gate reads the index, so it judges the tree being committed, not what is left unstaged.
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

const TOP: [&str; 9] = ["clis", "libraries", "backend", "tooling", "frontend", "infra", "e2e", "docs", "scripts"];
// a path through one of these is a build output: fine while what builds it exists
const OUTPUT_DIRS: [&str; 6] = ["target", "target-musl", "dist", "build", "node_modules", ".flow-report"];
const SKIP: [&str; 13] = [
    "docs/archive/", "docs/audits/", "docs/plans/", "docs/goals/", "references/", ".claude/", ".codex/", ".opencode/", "node_modules/",
    "pnpm-lock.yaml", "docs/runbooks/.results/", "docs/gemini-context/", "clis/wf/tests/",
];
// prose that follows "wf" in a sentence, not a command
const NOT_COMMANDS: [&str; 24] = [
    "is", "can", "binary", "with", "the", "command", "commands", "itself", "and", "a", "an", "to", "that", "which", "it", "has", "does",
    "will", "on", "in", "of", "for", "as", "now",
];
const JS: [&str; 4] = [".mjs", ".js", ".ts", ".tsx"];
const TEXT: [&str; 14] = [".md", ".txt", ".sh", ".yml", ".yaml", ".lua", ".toml", ".mjs", ".js", ".ts", ".tsx", ".rs", "Dockerfile", ".json"];

pub struct Finding {
    pub file: String,
    pub line: usize,
    pub kind: &'static str,
    pub reference: String,
}

fn generated(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    matches!(name, "CLAUDE.md" | "AGENTS.md" | "opencode.json" | ".mcp.json")
}

fn tracked(root: &Path) -> Vec<String> {
    let out = Command::new("git").args(["ls-files", "-z"]).current_dir(root).stdin(Stdio::null()).output();
    out.map(|o| String::from_utf8_lossy(&o.stdout).split('\0').filter(|f| !f.is_empty()).map(str::to_string).collect()).unwrap_or_default()
}

fn scanned(file: &str) -> bool {
    !SKIP.iter().any(|s| file.starts_with(s) || file.contains(&format!("/{s}"))) && !generated(file) && TEXT.iter().any(|e| file.ends_with(e))
        && !["/__tests__/", "/tests/", "/fixtures/", "/evals/", "/dist/"].iter().any(|d| file.contains(d))
}

/// A goal, read for its device verbs and methods only: its verifiers run them, while its paths are history.
fn goal(file: &str) -> bool {
    file.starts_with("docs/goals/") && file.ends_with(".md")
}

/// Whether `wf <noun> <verb>` routes: one of the noun's own verbs or a device command of the table.
fn noun_verb(noun: &str, verb: &str) -> bool {
    let own = match noun {
        "browser" => wf_harness::browser::VERBS,
        _ => wf_harness::android::VERBS,
    };
    own.contains(&verb) || wf_harness::device::api::by_verb(verb).is_some()
}

/// `device:<name>` when the binder does not serve it (it is no command of the table), else None.
fn device_method(name: &str) -> Option<String> {
    use wf_harness::device::api;
    (!api::COMMANDS.iter().any(|c| c.surface != api::Surface::CliOnly && c.name == name)).then(|| format!("device:{name}"))
}

/// The device findings of one line: `wf browser|android <verb>` at a command position (in prose: in backticks, or a Rust
/// string a message prints) and every `device:<method>`.
fn device_refs(line: &str, file: &str, fenced: bool, res: &DeviceRes) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if !code_comment(line, file, fenced) {
        let res_for: Vec<&Regex> = match (prose(file) && !fenced, file.ends_with(".rs")) {
            (true, true) => vec![&res.ticked, &res.quoted],
            (true, false) => vec![&res.ticked],
            _ => vec![&res.cmd],
        };
        for c in res_for.into_iter().flat_map(|re| re.captures_iter(line)) {
            if !noun_verb(&c[1], &c[2]) {
                out.push(("verb", format!("wf {} {}", &c[1], &c[2])));
            }
        }
    }
    for c in res.method.captures_iter(line) {
        if let Some(why) = device_method(&c[1]) {
            out.push(("method", why));
        }
    }
    out
}

struct DeviceRes {
    cmd: Regex,
    ticked: Regex,
    quoted: Regex,
    method: Regex,
}

/// Each package.json's directory ("" for the root) with its scripts, and each package name with its directory.
struct Scripts {
    by_dir: HashMap<String, HashSet<String>>,
    by_name: HashMap<String, String>,
}

impl Scripts {
    fn read(files: &[String], text: &dyn Fn(&str) -> Option<String>) -> Scripts {
        let mut s = Scripts { by_dir: HashMap::new(), by_name: HashMap::new() };
        for f in files.iter().filter(|f| (f.as_str() == "package.json" || f.ends_with("/package.json")) && !f.starts_with("references/")) {
            let Some(v) = text(f).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
            let dir = f.strip_suffix("package.json").unwrap_or("").trim_end_matches('/').to_string();
            let names = v["scripts"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
            if let Some(name) = v["name"].as_str() {
                s.by_name.insert(name.to_string(), dir.clone());
            }
            s.by_dir.insert(dir, names);
        }
        s
    }

    /// The package a `pnpm run` names through its flags, else the nearest package above the naming file; None when it cannot
    /// be told (a filter glob, a recursive run, a directory outside the tree).
    fn home(&self, flags: &str, file: &str) -> Option<&HashSet<String>> {
        let words: Vec<&str> = flags.split_whitespace().flat_map(|w| w.splitn(2, '=')).collect();
        if words.iter().any(|w| matches!(*w, "-r" | "--recursive" | "--if-present")) {
            return None;
        }
        if let Some(i) = words.iter().position(|w| matches!(*w, "--filter" | "-F")) {
            return self.by_name.get(*words.get(i + 1)?).and_then(|d| self.by_dir.get(d));
        }
        if let Some(i) = words.iter().position(|w| matches!(*w, "-C" | "--dir")) {
            return self.by_dir.get(words.get(i + 1)?.trim_start_matches("./").trim_end_matches('/'));
        }
        if words.iter().any(|w| matches!(*w, "-w" | "--workspace-root")) {
            return self.by_dir.get("");
        }
        let mut dir = file;
        while let Some((up, _)) = dir.rsplit_once('/') {
            if let Some(found) = self.by_dir.get(up) {
                return Some(found);
            }
            dir = up;
        }
        self.by_dir.get("")
    }
}

/// `pnpm [flags] run <script>`, or `pnpm [flags] <a:script>` (a builtin never has a colon), after the given prefix.
fn pnpm_re(prefix: &str) -> Regex {
    let flag = r"--filter[ =]\S+|-F\s*\S+|-C\s+\S+|--dir[ =]\S+|-[rws]|--recursive|--workspace-root|--if-present|--parallel|--silent|--stream|--aggregate-output|--reporter[ =]\S+|--config\.\S+";
    Regex::new(&format!(r"{prefix}pnpm((?:\s+(?:{flag}))*)\s+(run\s+)?([a-zA-Z][a-zA-Z0-9:_.-]*)")).unwrap()
}

/// A relative markdown link target from the naming file, normalised; None for a URL, an anchor or a template.
fn link_target(target: &str, file: &str) -> Option<String> {
    if target.contains(':') || target.starts_with('#') || target.contains(['$', '{', '%', '<']) || target.is_empty() {
        return None;
    }
    let (base, rest) = match target.strip_prefix('/') {
        Some(rest) => ("", rest),
        None => (file.rsplit_once('/').map_or("", |(d, _)| d), target),
    };
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rest.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// The names of every package.json and Nx project.json in the tree.
fn workspace_packages(files: &[String], text: &dyn Fn(&str) -> Option<String>) -> HashSet<String> {
    files
        .iter()
        .filter(|f| (f.ends_with("package.json") || f.ends_with("project.json")) && !f.starts_with("references/"))
        .filter_map(|f| text(f))
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter_map(|v| v["name"].as_str().map(str::to_string))
        .collect()
}

struct Tree {
    files: Vec<String>,
    dirs: HashSet<String>,
}

impl Tree {
    fn has(&self, path: &str) -> bool {
        self.dirs.contains(path) || self.files.binary_search_by(|f| f.as_str().cmp(path)).is_ok()
    }

    /// A tracked file or directory, from the repository root or from any directory above the naming file; a glob that matches
    /// one; or a build output below a directory that exists.
    fn resolves(&self, path: &str, file: &str) -> bool {
        let path = path.trim_end_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        if let Some(i) = parts.iter().position(|p| OUTPUT_DIRS.contains(p)) {
            return i == 0 || self.has(&parts[..i].join("/")) || self.resolves(&parts[..i].join("/"), file);
        }
        if let Some(star) = path.find('*') {
            let prefix = &path[..star];
            return self.files.iter().any(|f| f.starts_with(prefix));
        }
        let mut dir = file;
        while let Some((up, _)) = dir.rsplit_once('/') {
            if self.has(&format!("{up}/{path}")) {
                return true;
            }
            dir = up;
        }
        self.has(path)
    }

    /// Below a tracked directory deeper than the top level, so an ignored path there is a build output of live code.
    fn inside_live_dir(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        (2..parts.len()).rev().any(|n| self.dirs.contains(&parts[..n].join("/")))
    }
}

/// The paths git ignores, of those given (generated outputs such as dist/ or a report directory).
fn ignored(root: &Path, paths: &[String]) -> HashSet<String> {
    use std::io::Write;
    let Ok(mut child) = Command::new("git").args(["check-ignore", "--stdin", "--no-index"]).current_dir(root).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn() else {
        return HashSet::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(paths.join("\n").as_bytes());
    }
    child.wait_with_output().map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect()).unwrap_or_default()
}

fn prose(file: &str) -> bool {
    file.ends_with(".md") || file.ends_with(".txt") || file.ends_with(".rs") || file.ends_with(".json")
}

/// A comment line of code (a fenced block counts as code); prose has no comment lines.
fn code_comment(line: &str, file: &str, fenced: bool) -> bool {
    let trimmed = line.trim_start();
    (fenced || !prose(file)) && (trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with("--"))
}

fn command_spans(line: &str, file: &str, fenced: bool, cmd: &Regex, ticked: &Regex) -> Vec<String> {
    if code_comment(line, file, fenced) {
        return vec![];
    }
    let re = if prose(file) && !fenced { ticked } else { cmd };
    re.captures_iter(line).filter_map(|c| c.get(1).map(|m| m.as_str().to_string())).collect()
}

/// Each file's text as the index holds it, through one `git cat-file --batch`.
fn index_texts(root: &Path, files: &[&String]) -> HashMap<String, String> {
    use std::io::{BufRead, Read, Write};
    let mut out = HashMap::new();
    let Ok(mut child) = Command::new("git").args(["cat-file", "--batch"]).current_dir(root).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn() else {
        return out;
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return out };
    let names: Vec<String> = files.iter().map(|f| format!(":{f}\n")).collect();
    let writer = std::thread::spawn(move || {
        for n in names {
            if stdin.write_all(n.as_bytes()).is_err() {
                break;
            }
        }
    });
    let mut reader = std::io::BufReader::new(stdout);
    for f in files {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 {
            break;
        }
        let parts: Vec<&str> = header.split_whitespace().collect();
        let Some(size) = parts.get(2).and_then(|n| n.parse::<usize>().ok()).filter(|_| parts.len() == 3) else { continue };
        let mut body = vec![0; size + 1];
        if reader.read_exact(&mut body).is_err() {
            break;
        }
        body.pop();
        out.insert(f.to_string(), String::from_utf8_lossy(&body).into_owned());
    }
    let _ = writer.join();
    let _ = child.wait();
    out
}

/// The findings in the working tree, or with `staged` in the index.
pub fn scan(root: &Path, staged: bool) -> Vec<Finding> {
    let mut files = tracked(root);
    files.sort();
    let mut dirs = HashSet::new();
    for f in &files {
        let mut at = 0;
        while let Some(i) = f[at..].find('/') {
            dirs.insert(f[..at + i].to_string());
            at += i + 1;
        }
    }
    let tree = Tree { files, dirs };
    let wanted: Vec<&String> = tree.files.iter().filter(|f| scanned(f) || goal(f) || f.ends_with("package.json") || f.ends_with("project.json")).collect();
    let index = if staged { index_texts(root, &wanted) } else { HashMap::new() };
    let text = |f: &str| if staged { index.get(f).cloned() } else { std::fs::read_to_string(root.join(f)).ok() };
    let packages = workspace_packages(&tree.files, &text);
    let path_re = Regex::new(&format!(r"(?:^|[^A-Za-z0-9_./@-])((?:{})/[A-Za-z0-9_./@*+-]*[A-Za-z0-9_*/])([<{{$\\-]?)", TOP.join("|"))).unwrap();
    let pkg_re = Regex::new(r"(?:^|[^A-Za-z0-9_./-])(@(?:cli|lib|app|feature|tooling)/[a-z0-9][a-z0-9-]*)").unwrap();
    // a command position: line start, after a separator or quote, or after `run:`/`then`/`do`/`exec`; prose after a space is not
    let cmd_re = Regex::new(r#"(?:^\s*|[;|&(`'"]\s*|\$\(\s*|\b(?:run:|then|do|exec)\s+)wf\s+([a-z][a-z0-9-]*)"#).unwrap();
    // an argument vector: spawnSync('wf', ['secret', ...]), Command::new("wf").args(["secret", ...])
    let argv_re = Regex::new(r#"['"]wf['"]\s*(?:,\s*\[|\)\s*\.args?\(\s*&?\[?)\s*['"]([a-z][a-z0-9-]*)['"]"#).unwrap();
    let ticked_re = Regex::new(r"`wf\s+([a-z][a-z0-9-]*)").unwrap();
    let pnpm_code_re = pnpm_re(r#"(?:^\s*|[;|&(`'"]\s*|\$\(\s*|\b(?:run:|then|do|exec)\s+|"[a-z:-]+":\s*")"#);
    let pnpm_ticked_re = pnpm_re("`");
    let link_re = Regex::new(r"\]\(\s*<?([^)\s>#?]*)").unwrap();
    let device = DeviceRes {
        cmd: Regex::new(r#"(?:^\s*|[;|&(`'"]\s*|\$\(\s*|\b(?:run:|then|do|exec)\s+)wf\s+(browser|android)\s+([a-z][a-z0-9-]*)"#).unwrap(),
        ticked: Regex::new(r"`wf\s+(browser|android)\s+([a-z][a-z0-9-]*)").unwrap(),
        quoted: Regex::new(r#""wf\s+(browser|android)\s+([a-z][a-z0-9-]*)"#).unwrap(),
        method: Regex::new(r"\bdevice:([a-z][a-z0-9_]*)").unwrap(),
    };
    let scripts = Scripts::read(&tree.files, &text);
    let mut findings = Vec::new();
    let mut unresolved: Vec<Finding> = Vec::new();
    for file in tree.files.iter().filter(|f| scanned(f) || goal(f)) {
        let Some(text) = text(file) else { continue };
        let md = file.ends_with(".md");
        let mut fenced = false;
        for (n, line) in text.lines().enumerate() {
            // a Rust file's own test module names made-up paths on purpose
            if file.ends_with(".rs") && line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if md && line.trim_start().starts_with("```") {
                fenced = !fenced;
                continue;
            }
            for (kind, reference) in device_refs(line, file, fenced, &device) {
                findings.push(Finding { file: file.clone(), line: n + 1, kind, reference });
            }
            if goal(file) {
                continue;
            }
            if md && !fenced {
                for c in link_re.captures_iter(line) {
                    if let Some(target) = link_target(&c[1], file) {
                        if !tree.has(&target) {
                            findings.push(Finding { file: file.clone(), line: n + 1, kind: "link", reference: c[1].to_string() });
                        }
                    }
                }
            }
            for c in path_re.captures_iter(line) {
                let path = c[1].trim_end_matches('.');
                let second = path.split('/').nth(1).unwrap_or("");
                let placeholder = !c[2].is_empty() || path.contains("...") || path.ends_with('-') || path.contains("//") || TOP.contains(&second) || path.split('/').any(|p| p == "foo");
                if !placeholder && !tree.resolves(path, file) {
                    unresolved.push(Finding { file: file.clone(), line: n + 1, kind: "path", reference: path.to_string() });
                }
            }
            for c in pkg_re.captures_iter(line) {
                let example = c[1].contains("/my-") || c[1].ends_with('-');
                if !example && !packages.contains(&c[1]) {
                    findings.push(Finding { file: file.clone(), line: n + 1, kind: "package", reference: c[1].to_string() });
                }
            }
            let argv = argv_re.captures_iter(line).filter_map(|c| c.get(1).map(|m| m.as_str().to_string()));
            for word in command_spans(line, file, fenced, &cmd_re, &ticked_re).into_iter().chain(argv) {
                if word != "help" && !NOT_COMMANDS.contains(&word.as_str()) && !is_command(&word) {
                    findings.push(Finding { file: file.clone(), line: n + 1, kind: "command", reference: format!("wf {word}") });
                }
            }
            let pnpm = if prose(file) && !fenced && !file.ends_with("package.json") { &pnpm_ticked_re } else { &pnpm_code_re };
            if !code_comment(line, file, fenced) {
                for c in pnpm.captures_iter(line) {
                    let (script, explicit) = (&c[3], c.get(2).is_some());
                    // in a JS/TS string a bare pnpm call of a colon script is usually fixture text; prose names no working directory, so the root counts
                    let named = explicit || (script.contains(':') && !JS.iter().any(|e| file.ends_with(e)));
                    let known = |set: &HashSet<String>| set.contains(script) || (prose(file) && scripts.by_dir.get("").is_some_and(|r| r.contains(script)));
                    if named && script != "run" && scripts.home(&c[1], file).is_some_and(|set| !known(set)) {
                        findings.push(Finding { file: file.clone(), line: n + 1, kind: "script", reference: format!("pnpm run {script}") });
                    }
                }
            }
        }
    }
    let candidates: Vec<String> = unresolved.iter().filter(|f| tree.inside_live_dir(&f.reference)).map(|f| f.reference.trim_end_matches('/').to_string()).collect();
    let outputs = ignored(root, &candidates);
    findings.extend(unresolved.into_iter().filter(|f| !outputs.contains(f.reference.trim_end_matches('/'))));
    let rank = |k: &str| ["path", "link", "package", "command", "verb", "method", "script"].iter().position(|r| *r == k);
    findings.sort_by(|a, b| (&a.file, a.line, rank(a.kind)).cmp(&(&b.file, b.line, rank(b.kind))));
    findings
}

pub fn report(findings: &[Finding]) -> Value {
    let list: Vec<Value> = findings.iter().map(|f| json!({ "file": f.file, "line": f.line, "kind": f.kind, "reference": f.reference })).collect();
    json!({
        "ok": findings.is_empty(),
        "findings": list,
        "next": if findings.is_empty() { Value::Null } else { json!("point each reference at what replaced it, or delete the text with the code it described") },
    })
}

static COMMANDS: std::sync::OnceLock<fn(&str) -> bool> = std::sync::OnceLock::new();

/// The router registers its command lookup here, so the scan can tell a gone command from a live one without depending on it.
pub fn set_commands(is_command: fn(&str) -> bool) {
    let _ = COMMANDS.set(is_command);
}

fn is_command(word: &str) -> bool {
    COMMANDS.get().is_some_and(|f| f(word))
}

/// The staged gate's scan: how many references are gone and the report of them, none when the tree is clean.
pub fn gate(root: &Path) -> Option<(usize, String)> {
    let findings = scan(root, true);
    (!findings.is_empty()).then(|| (findings.len(), report(&findings).to_string()))
}

/// `wf check --dead-refs`: the JSON report on stdout, exit 1 when anything named is gone.
pub fn run(root: &Path, staged: bool) -> i32 {
    let findings = scan(root, staged);
    println!("{}", report(&findings));
    i32::from(!findings.is_empty())
}
