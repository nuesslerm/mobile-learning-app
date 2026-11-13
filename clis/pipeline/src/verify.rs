//! `wf check --budget [--strict]` (the simplicity budget) and `wf check --staged [--files ...]` (the pre-commit gate), and the
//! pnpm workspace graph and the app paths that tier 3 (`wf check <sha>`, in `wf`) plans by.
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::root::repo_root;
use wf_kit::time::now_ms;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use yaml_rust2::{Yaml, YamlLoader};

struct Budget {
    name: &'static str,
    value: usize,
    limit: usize,
    gated: bool,
}

/// JavaScript's `text.split('\n').length`: a trailing newline counts as one more line.
fn byte_count(path: &Path) -> Option<usize> {
    match std::fs::metadata(path) {
        Ok(meta) => Some(meta.len() as usize),
        Err(_) if !path.exists() => Some(0),
        Err(_) => None,
    }
}

fn line_count(path: &Path) -> Option<usize> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes.iter().filter(|b| **b == b'\n').count() + 1),
        Err(_) if !path.exists() => Some(0),
        Err(_) => None,
    }
}

/// The newlines of every `.rs` file under `dir`: what a crate's size is measured by (0 when it is not there).
fn rust_lines(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                rust_lines(&path)
            } else if path.extension().is_some_and(|x| x == "rs") {
                std::fs::read(&path).map_or(0, |b| b.iter().filter(|c| **c == b'\n').count())
            } else {
                0
            }
        })
        .sum()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn pre_commit_jobs(root: &Path) -> Option<usize> {
    let docs = YamlLoader::load_from_str(&std::fs::read_to_string(root.join("lefthook.yml")).ok()?).ok()?;
    let jobs = &docs.first()?["pre-commit"]["jobs"];
    Some(match jobs {
        Yaml::Array(a) => a.len(),
        _ => 0,
    })
}

fn agent_hooks(root: &Path) -> Option<usize> {
    let config = read_json(&root.join(".agents/config/hooks.json"))?;
    let mut commands = HashSet::new();
    if let Some(events) = config["claude"]["hooks"].as_object() {
        for groups in events.values() {
            // `Object.values(...).flat()`: an array contributes its elements, anything else itself.
            let items: Vec<&Value> = match groups {
                Value::Array(a) => a.iter().collect(),
                other => vec![other],
            };
            for group in items {
                for hook in group["hooks"].as_array()? {
                    commands.insert(hook["command"].as_str().unwrap_or("undefined").to_string());
                }
            }
        }
    }
    Some(commands.len())
}

/// `path.resolve(dir, target)` without touching the disk: `.` and `..` are folded lexically.
fn resolve(dir: &Path, target: &str) -> PathBuf {
    let joined = if target.starts_with('/') { PathBuf::from(target) } else { dir.join(target) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Targets of `](target` where the target has no `)`, `#` or whitespace, as `/\]\(([^)#\s]+)/g` finds them.
fn link_targets(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = 0;
    while let Some(i) = text[rest..].find("](") {
        let start = rest + i + 2;
        let end = text[start..]
            .find(|c: char| c == ')' || c == '#' || c.is_whitespace())
            .map_or(text.len(), |n| start + n);
        if end > start {
            out.push(&text[start..end]);
            rest = end;
        } else {
            rest = start;
        }
    }
    out
}

fn has_scheme(t: &str) -> bool {
    let letters = t.bytes().take_while(u8::is_ascii_alphabetic).count();
    letters > 0 && t.as_bytes().get(letters) == Some(&b':')
}

fn dangling_links(root: &Path, file: &str) -> Option<usize> {
    let path = root.join(file);
    if !path.exists() {
        return Some(0);
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let dir = path.parent()?;
    Some(link_targets(&text).into_iter().filter(|t| !has_scheme(t) && !resolve(dir, t).exists()).count())
}

fn measure(root: &Path) -> Option<Vec<Budget>> {
    let scripts = read_json(&root.join("package.json"))?["scripts"].as_object().map_or(0, |m| m.len());
    let library_paths = crate::library_paths::violations(root);
    for found in &library_paths {
        wf_kit::wf_err!("wf: path dependency on a versioned library ({found}): use `version = \"<x.y>\", registry = \"<alias>\"` as libraries.json says; to edit a library against a service, use cargo `--config patch.mla.<name>.path` (wf lib --help)");
    }
    let cargo_deps = crate::cargo_deps::violations(root);
    for found in &cargo_deps {
        wf_kit::wf_err!("wf: crate dependency rule broken ({found}): see docs/plans/rca-parity/plan.md section 3, Rules");
    }
    let blast_radius = crate::blast_radius::check(root).findings;
    for found in &blast_radius {
        wf_kit::wf_err!("wf: blast radius rule broken ({found})");
    }
    let crossings = crate::workspace_crossings::check(root).findings;
    for found in &crossings {
        wf_kit::wf_err!("wf: workspace crossing rule broken ({found})");
    }
    let cargo_inputs = crate::cargo_inputs::check(root).findings;
    for found in &cargo_inputs {
        wf_kit::wf_err!("wf: cargo task hashes npm packages ({found})");
    }
    let scope_check = secrets::scope_violations(&root.join(".github/workflows")).unwrap_or_else(|e| vec![e]);
    for found in &scope_check {
        wf_kit::wf_err!("wf: CI vault scope rule broken ({found}): wf secret scope-check lists every job");
    }
    let recorders = crate::recorders::violations(root);
    for found in &recorders {
        wf_kit::wf_err!("wf: hand-rolled recorder ({found}): see docs/plans/mimic/plan.md section 8.2");
    }
    let unpinned = crate::workflow_hygiene::unpinned_actions(root);
    for found in &unpinned {
        wf_kit::wf_err!("wf: third-party action not pinned by sha ({found}): use owner/repo@<40 hex sha> with the tag in a trailing comment");
    }
    let env_artifacts = crate::workflow_hygiene::env_file_in_artifacts(root);
    for found in &env_artifacts {
        wf_kit::wf_err!("wf: a cache or artifact step includes a .env file ({found}): exclude it with a `!` path");
    }
    let inherits = crate::workflow_hygiene::secrets_inherit(root);
    for found in &inherits {
        wf_kit::wf_err!("wf: `secrets: inherit` ({found}): pass each secret the called workflow reads by name");
    }
    Some(vec![
        Budget { name: "pre-commit-jobs", value: pre_commit_jobs(root)?, limit: 5, gated: true },
        Budget { name: "root-claude-md-bytes", value: byte_count(&root.join("CLAUDE.md"))?, limit: 9216, gated: true },
        Budget { name: "known-gaps-lines", value: line_count(&root.join("docs/known-gaps.md"))?, limit: 200, gated: true },
        Budget { name: "concepts-dangling-links", value: dangling_links(root, "CONCEPTS.md")?, limit: 0, gated: true },
        // A core does no I/O: no I/O crate, and no tokio I/O feature, in its manifest or in the workspace tokio it inherits.
        Budget { name: "backend-core-io-violations", value: crate::core_purity::count(root), limit: 0, gated: true },
        // The plan's dependency rules between crates: exec-testkit is a dev-dependency of its few users only (`TESTKIT_USERS` in `cargo_deps`), and an adapter library
        // depends on nothing that consumes it (Cargo allows that cycle through a dev-dependency).
        Budget { name: "cargo-dep-violations", value: cargo_deps.len(), limit: 0, gated: true },
        // A crate's blast radius stays within its checked-in budget, and no low-level crate depends on a heavy one (`blast_radius`).
        Budget { name: "blast-radius-violations", value: blast_radius.len(), limit: 0, gated: true },
        // A path dependency across a Cargo workspace boundary is on the checked-in list, and the list only shrinks (`workspace_crossings`).
        Budget { name: "workspace-crossing-violations", value: crossings.len(), limit: 0, gated: true },
        // No CI job asks for a key past its class: an open job reads ci-open and public keys, no job reads a vm key (`wf secret scope-check`).
        Budget { name: "ci-scope-violations", value: scope_check.len(), limit: 0, gated: true },
        // Every third-party action is pinned to a commit sha, so a moved tag cannot change what a job runs (`workflow_hygiene`).
        Budget { name: "unpinned-third-party-actions", value: unpinned.len(), limit: 0, gated: true },
        // A generated env file holds secrets: no cache or artifact step may carry it away.
        Budget { name: "env-file-in-artifacts", value: env_artifacts.len(), limit: 0, gated: true },
        // A called workflow gets the secrets it names, never all of them.
        Budget { name: "secrets-inherit", value: inherits.len(), limit: 0, gated: true },
        // A reference is recorded by `wf mimic`: no generate/record script into a fixtures path, no cassette without provenance, no RECORD_* test switch.
        Budget { name: "handrolled-recorder-violations", value: recorders.len(), limit: 0, gated: true },
        // Nx hashes the installed npm layout into a cargo-only task unless it pins `externalDependencies: []`, and the layout differs by CPU (`cargo_inputs`).
        Budget { name: "cargo-task-npm-inputs", value: cargo_inputs.len(), limit: 0, gated: true },
        // A library is taken from its registry at a pinned version: a path dependency would put its source back into every consumer's inputs.
        Budget { name: "versioned-library-path-deps", value: library_paths.len(), limit: 0, gated: true },
        // Every wf crate depends on the kit, so a line added there reruns every flow test: an addition needs three dependants.
        Budget { name: "wf-kit-lines", value: rust_lines(&root.join("clis/wf-kit/src")), limit: 1200, gated: true },
        Budget { name: "root-scripts", value: scripts, limit: 15, gated: false },
        Budget { name: "agent-hooks", value: agent_hooks(root)?, limit: 4, gated: false },
    ])
}

/// Why `measure` could not run: the first input it reads that is missing or unreadable, in Node's words.
fn unreadable_input(root: &Path) -> String {
    for rel in ["package.json", "lefthook.yml", ".agents/config/hooks.json", "CONCEPTS.md"] {
        if std::fs::read_to_string(root.join(rel)).is_err() {
            return format!("ENOENT: no such file or directory, open '{}'", root.join(rel).display());
        }
    }
    "the budget inputs could not be read".to_string()
}

fn logfmt(level: &str, msg: &str, b: &Budget) -> String {
    let msg = msg.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "ts={} level={level} source=verify.budget msg=\"{msg}\" value={} limit={} gated={}",
        now_ms() / 1000,
        b.value,
        b.limit,
        b.gated
    )
}

/// Prints each budget and returns the failure line for those over a gating limit.
fn report(budgets: &[Budget], strict: bool) -> i32 {
    let mut over = Vec::new();
    for b in budgets {
        if b.value > b.limit {
            eprintln!("{}", logfmt("warn", &format!("{} over budget", b.name), b));
            if b.gated || strict {
                over.push(format!("{} {}>{}", b.name, b.value, b.limit));
            }
        } else {
            println!("{}", logfmt("info", b.name, b));
        }
    }
    if over.is_empty() {
        return 0;
    }
    wf_kit::wf_err!("wf: simplicity budget exceeded: {}", over.join(", "));
    1
}

/* ---------- --staged ---------- */

fn is_lintable(f: &str) -> bool {
    [".js", ".jsx", ".ts", ".tsx", ".cjs", ".mjs"].iter().any(|e| f.ends_with(e))
}

pub fn is_code(f: &str) -> bool {
    [".ts", ".tsx", ".mts", ".cts", ".js", ".mjs", ".cjs", ".json"].iter().any(|e| f.ends_with(e))
}

fn is_cm6_input(f: &str) -> bool {
    f == "pnpm-lock.yaml"
        || f == "frontend/mla-tamagui/features/leetcode/code-editor/core/cm6Bundle.ts"
        || f == "frontend/mla-tamagui/features/leetcode/code-editor/core/cm6Bundle.src.ts"
        || f.starts_with("frontend/libraries/logging/")
}

pub struct Package {
    pub name: String,
    pub(crate) dir: String,
    pub(crate) deps: Vec<String>,
}

pub fn workspace_packages(root: &Path) -> Result<Vec<Package>, String> {
    let out = Command::new("pnpm")
        .args(["ls", "-r", "--depth", "-1", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run pnpm: {e}"))?;
    if !out.status.success() {
        return Err(format!("pnpm ls -r failed with exit {}", out.status.code().unwrap_or(1)));
    }
    let listed: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("pnpm ls printed unreadable JSON: {e}"))?;
    let listed = listed.as_array().cloned().unwrap_or_default();
    let names: HashSet<String> = listed.iter().filter_map(|p| p["name"].as_str().map(str::to_string)).collect();
    let mut packages = Vec::new();
    for p in &listed {
        let (Some(name), Some(path)) = (p["name"].as_str(), p["path"].as_str()) else { continue };
        let Ok(rel) = Path::new(path).strip_prefix(root) else { continue };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let manifest = read_json(&Path::new(path).join("package.json")).ok_or_else(|| format!("cannot read {path}/package.json"))?;
        let mut deps: Vec<String> = Vec::new();
        for key in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
            for dep in manifest[key].as_object().map(|m| m.keys()).into_iter().flatten() {
                if dep != name && names.contains(dep) && !deps.contains(dep) {
                    deps.push(dep.clone());
                }
            }
        }
        packages.push(Package { name: name.to_string(), dir: rel.to_string_lossy().into_owned(), deps });
    }
    Ok(packages)
}

pub(crate) fn owning_dir<'a>(file: &str, dirs: &[&'a str]) -> &'a str {
    let normalized = file.replace('\\', "/");
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
    dirs.iter().filter(|d| normalized.starts_with(&format!("{d}/"))).max_by_key(|d| d.len()).copied().unwrap_or("")
}

pub fn affected(files: &[String], packages: &[Package]) -> Vec<String> {
    let dirs: Vec<&str> = packages.iter().map(|p| p.dir.as_str()).collect();
    let owned: HashSet<&str> = files.iter().map(|f| owning_dir(f, &dirs)).collect();
    let mut set: HashSet<&str> = packages.iter().filter(|p| owned.contains(p.dir.as_str())).map(|p| p.name.as_str()).collect();
    loop {
        let grown: Vec<&str> =
            packages.iter().filter(|p| !set.contains(p.name.as_str()) && p.deps.iter().any(|d| set.contains(d.as_str()))).map(|p| p.name.as_str()).collect();
        if grown.is_empty() {
            break;
        }
        set.extend(grown);
    }
    packages.iter().filter(|p| set.contains(p.name.as_str())).map(|p| p.name.clone()).collect()
}

pub fn filters(packages: &[String]) -> String {
    packages.iter().map(|n| format!("--filter {n}")).collect::<Vec<_>>().join(" ")
}

/// `sh -c command`, its output shown only when it fails.
fn exec(command: &str) -> i32 {
    match Command::new("sh").args(["-c", command]).stdin(Stdio::null()).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(1);
            if code != 0 {
                let mut err = std::io::stderr().lock();
                let _ = err.write_all(&o.stdout);
                let _ = err.write_all(&o.stderr);
            }
            code
        }
        Err(_) => 1,
    }
}

/// `nx run-many -t <targets> -p <projects>`: a task whose inputs hash like an earlier pass (this checkout's, a sandbox's, CI's)
/// is replayed from the cache, the rest run; the first failure stops the call and its output is shown.
fn run_nx(nx: &Path, root: &Path, targets: &str, projects: &[String], env: &[(&str, String)]) -> i32 {
    let ran = crate::nx::run_gate(nx, root, targets, projects, 4, env);
    if ran.code == 0 {
        println!("verify: nx {targets} over {} projects: {} of {} tasks replayed from the cache", projects.len(), ran.replayed, ran.tasks);
    }
    ran.code
}

/// Typecheck, unit and flow tests of every package a staged code file reaches (the file's package and what depends on it).
fn run_gate(nx: &Path, root: &Path, files: &[String], packages: &[Package], env: &[(&str, String)]) -> i32 {
    let affected = affected(files, packages);
    if affected.is_empty() {
        println!("verify: no workspace package affected");
        return 0;
    }
    run_nx(nx, root, "typecheck,test:unit,test:flow", &affected, env)
}

/// The projects that own a staged file: a file's lint is its own project's, not its dependents'. A file no package owns belongs
/// to the workspace root's project.
fn lint_projects(root: &Path, files: &[String], packages: &[Package]) -> Vec<String> {
    let dirs: Vec<&str> = packages.iter().map(|p| p.dir.as_str()).collect();
    let mut names: Vec<String> = Vec::new();
    for f in files {
        let owner = owning_dir(f, &dirs);
        let name = if owner.is_empty() { read_json(&root.join("package.json")).and_then(|m| m["name"].as_str().map(str::to_string)) } else { packages.iter().find(|p| p.dir == owner).map(|p| p.name.clone()) };
        if let Some(name) = name.filter(|n| !names.contains(n)) {
            names.push(name);
        }
    }
    names
}

/// What the gate runs Nx with: the workspace's `nx`, the packages (pnpm's graph), and the shim that serves the shared build cache
/// (none when its links do not resolve), kept up until the gate is done.
struct NxGate {
    nx: PathBuf,
    packages: Vec<Package>,
    shim: Option<wf_bucket::cache::Shim>,
}

impl NxGate {
    fn env(&self) -> Vec<(&'static str, String)> {
        wf_bucket::cache::env_of(&self.shim)
    }
}

fn nx_gate(root: &Path) -> Result<NxGate, String> {
    let (nx, packages) = (crate::nx::bin(root)?, workspace_packages(root)?);
    Ok(NxGate { nx, packages, shim: wf_bucket::cache::Shim::start(root, wf_bucket::cache::Scope::Staged) })
}

fn staged_names() -> Vec<String> {
    let cwd = std::env::var_os("REPO_ROOT").map_or_else(|| std::env::current_dir().unwrap_or_default(), PathBuf::from);
    let out = Command::new("git").args(["diff", "--cached", "--name-only", "--diff-filter=ACMR"]).current_dir(cwd).stdin(Stdio::null()).output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if text.is_empty() {
                vec![]
            } else {
                text.split('\n').map(str::to_string).collect()
            }
        }
        _ => vec![],
    }
}

pub const STAGED: Spec = Spec { variadic: &["files"], flags: &["staged"], positional: Positional::None, ..Spec::NONE };

/// The staged gate's scan for references to code that is gone (`wf check --dead-refs`): how many and their report, none when the tree
/// is clean. It reads the whole command surface, so `wf` hands it in.
type DeadRefs = fn(&Path) -> Option<(usize, String)>;

/// What a cassette set depends on beyond its own directory: the harness, its drivers and the pin of the reference they run.
const MIMIC_INPUTS: [&str; 3] = ["clis/mimic/", "tooling/mimic-drivers/", "tooling/orca-conformance/orca-pin.json"];

fn in_a_set(root: &Path, file: &str) -> bool {
    root.join(file).ancestors().skip(1).take_while(|d| d.starts_with(root) && *d != root).any(|d| d.join("mimic.toml").exists())
}

fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn in_a_crawl(root: &Path, file: &str) -> bool {
    root.join(file).ancestors().skip(1).take_while(|d| d.starts_with(root) && *d != root).any(|d| d.join("crawl.toml").exists())
}

/// `wf mimic crawl --all --check`: the committed endpoint inventories and corpora still cover every row of the catalogs of the pinned
/// sources (and, when the reference trees are checked out here and the drivers changed, `--fresh`: the same crawl again, byte for byte).
fn crawl_gate(root: &Path, files: &[String]) -> i32 {
    let moved = files.iter().any(|f| MIMIC_INPUTS.iter().any(|p| f.starts_with(p)) || f.starts_with("docs/plans/rca-parity/coverage/") || f == "tooling/t3-conformance/t3-pin.json" || in_a_crawl(root, f));
    if !moved {
        return 0;
    }
    let base = format!("wf mimic crawl --all --check --root '{}'", root.display());
    let code = exec(&base);
    let trees = root.join("tooling/mimic-drivers/node_modules").is_dir() && root.join("references/orca/src").is_dir() && root.join("references/t3code/packages/contracts").is_dir();
    if code == 0 && trees && files.iter().any(|f| f.starts_with("tooling/mimic-drivers/")) {
        return exec(&format!("{base} --fresh"));
    }
    code
}

/// Each set that pins a directory of the reference's own code (`pin_tree` in its mimic.toml), with that directory: an edit under it is
/// an edit to what the set was recorded from.
fn pinned_trees(root: &Path) -> Vec<(PathBuf, PathBuf)> {
    let listed = Command::new("git").args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "mimic.toml", "**/mimic.toml"]).current_dir(root).stdin(Stdio::null()).output();
    let Ok(listed) = listed else { return vec![] };
    String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .filter_map(|f| {
            let dir = root.join(f).parent()?.to_path_buf();
            let config: toml::Value = std::fs::read_to_string(root.join(f)).ok()?.parse().ok()?;
            let tree = config.get("reference")?.get("pin_tree")?.as_str()?.to_string();
            Some((dir.clone(), lexical(&dir.join(tree))))
        })
        .collect()
}

/// `wf mimic verify --all` when a staged file belongs to a cassette set, what records one or the code a set is pinned to; `--live` too
/// (re-record the reference and diff) when the reference's own code or pin changed and its toolchain is installed here.
fn mimic_gate(root: &Path, files: &[String]) -> i32 {
    let trees = pinned_trees(root);
    let under = |f: &String, tree: &PathBuf| lexical(&root.join(f)).starts_with(tree);
    if !files.iter().any(|f| MIMIC_INPUTS.iter().any(|p| f.starts_with(p)) || in_a_set(root, f) || trees.iter().any(|(_, t)| under(f, t))) {
        return 0;
    }
    let base = format!("wf mimic verify --all --root '{}'", root.display());
    let code = exec(&base);
    let reference_moved = files.iter().any(|f| f.starts_with("tooling/mimic-drivers/") || f == "tooling/orca-conformance/orca-pin.json");
    let toolchain = root.join("tooling/mimic-drivers/node_modules").is_dir() && root.join("references/orca/src").is_dir();
    if code != 0 {
        return code;
    }
    if reference_moved && toolchain {
        return exec(&format!("{base} --live"));
    }
    // a set pinned to a tree of this repository is re-recorded in memory when that tree or its driver changed: it needs only the drivers' toolchain
    let drivers_moved = files.iter().any(|f| f.starts_with("tooling/mimic-drivers/"));
    let moved: Vec<String> = trees.iter().filter(|(_, t)| drivers_moved || files.iter().any(|f| under(f, t))).map(|(d, _)| format!("'{}'", d.display())).collect();
    if moved.is_empty() || !root.join("tooling/mimic-drivers/node_modules").is_dir() {
        return 0;
    }
    exec(&format!("wf mimic verify {} --live", moved.join(" ")))
}

/// `wf mimic verify --fixtures <dir>` for each fixture tree (a directory with a `fixture-provenance.json`) a staged file lies under, or
/// all of them when the harness changed: every fixture file must be classified by provenance.
fn fixture_gate(root: &Path, files: &[String]) -> i32 {
    let listed = Command::new("git").args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "fixture-provenance.json", "**/fixture-provenance.json"]).current_dir(root).stdin(Stdio::null()).output();
    let listed = match listed {
        Ok(l) if l.status.success() => l,
        Ok(l) => {
            eprintln!("fixture provenance gate: git ls-files failed: {}", String::from_utf8_lossy(&l.stderr).trim());
            return 1;
        }
        Err(e) => {
            eprintln!("fixture provenance gate: git ls-files could not start: {e}");
            return 1;
        }
    };
    let harness = files.iter().any(|f| f.starts_with("clis/mimic/"));
    for rules in String::from_utf8_lossy(&listed.stdout).split('\0').filter(|f| !f.is_empty()) {
        let dir = Path::new(rules).parent().unwrap_or(Path::new(""));
        if harness || files.iter().any(|f| Path::new(f).starts_with(dir)) {
            let code = exec(&format!("wf mimic verify --fixtures '{}'", root.join(dir).display()));
            if code != 0 {
                return code;
            }
        }
    }
    0
}

/// `check --staged [--files ...]`: lint, the budget, `dead_refs`, the editor bundle and the tests of what is staged (or named).
pub fn staged(args: &[String], dead_refs: DeadRefs) -> Option<i32> {
    let p = parse_spec(args, &STAGED)?;
    let files_arg: Option<Vec<String>> = p.flag("files").then(|| p.values("files").into_iter().map(str::to_string).collect());
    let root = repo_root();
    let Some(budgets) = measure(&root) else {
        wf_kit::wf_err!("wf: {}", unreadable_input(&root));
        return Some(1);
    };
    let files = files_arg.unwrap_or_else(staged_names);
    let lintable: Vec<String> = files.iter().filter(|f| is_lintable(f)).cloned().collect();
    let code_files: Vec<String> = files.iter().filter(|f| is_code(f)).cloned().collect();
    let gate = if lintable.is_empty() && code_files.is_empty() {
        None
    } else {
        match nx_gate(&root) {
            Ok(g) => Some(g),
            Err(message) => {
                wf_kit::wf_err!("wf: {message}");
                return Some(1);
            }
        }
    };
    let lint_code = match &gate {
        Some(g) if !lintable.is_empty() => {
            let projects = lint_projects(&root, &lintable, &g.packages);
            if projects.is_empty() { 0 } else { run_nx(&g.nx, &root, "lint", &projects, &g.env()) }
        }
        _ => 0,
    };
    let budget_code = report(&budgets, false);
    if budget_code != 0 {
        return Some(budget_code);
    }
    if let Some((count, report)) = dead_refs(&root) {
        wf_kit::wf_err!("wf: {count} references to code that is gone (wf check --dead-refs lists them)");
        println!("{report}");
        return Some(1);
    }
    let mimic = mimic_gate(&root, &files).max(crawl_gate(&root, &files));
    if mimic != 0 {
        return Some(mimic);
    }
    let fixtures = fixture_gate(&root, &files);
    if fixtures != 0 {
        return Some(fixtures);
    }
    if files.iter().any(|f| is_cm6_input(f)) {
        let code = match &gate {
            Some(g) => run_nx(&g.nx, &root, "check:editor-bundle", &["mla-tamagui".to_string()], &g.env()),
            None => exec("pnpm --filter mla-tamagui run check:editor-bundle"),
        };
        if code != 0 {
            return Some(if lint_code != 0 { lint_code } else { code });
        }
    }
    let tested = match &gate {
        Some(g) if !code_files.is_empty() => run_gate(&g.nx, &root, &code_files, &g.packages, &g.env()),
        _ => 0,
    };
    Some(if lint_code != 0 { lint_code } else { tested })
}

/* ---------- the apps a path reaches ---------- */

pub const APP_PATHS: [(&str, &str); 13] = [
    ("frontend/mla-tamagui", "mla-tamagui"),
    ("infra/oracle-cloud/vm-ops-api", "vm-ops-api"),
    ("infra/oracle-cloud/watchdog", "vm-ops-api"),
    ("backend/watchdog", "vm-ops-api"),
    ("backend/vm-ops-api", "vm-ops-api"),
    ("infra/oracle-cloud/agent-shell", "agent-shell"),
    ("clis/vnc-auth-capture", "vnc-auth-session"),
    ("backend/leetcode-backend", "leetcode-backend"),
    ("backend/remote-chat-agent", "remote-chat-agent"),
    ("backend/leetcode-auth-cookie-capture", "leetcode-auth-cookie-capture"),
    ("backend/reporting-service", "reporting-service"),
    ("clis/t3-serve", "t3-serve"),
    ("clis/orca-serve", "orca-serve"),
];
pub const HTTP_HEALTH_APPS: [&str; 6] = ["leetcode-backend", "remote-chat-agent", "reporting-service", "t3-serve", "orca-serve", "vm-ops-api"];
pub const FRONTEND_APP: &str = "mla-tamagui";
pub const GATE_STEPS: [&str; 2] = ["typecheck", "test:unit + test:flow"];

/// The app a path belongs to, one per path: its own package directory, else a directory the desktop's image is built from.
fn app_of(path: &str) -> Option<&'static str> {
    let under = |dir: &str| path == dir || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'));
    let own = APP_PATHS.iter().find(|(dir, _)| under(dir)).map(|(_, app)| *app);
    own.or_else(|| crate::legs::DESKTOP_IMAGE_PATHS.iter().any(|dir| under(dir)).then_some("vnc-auth-session"))
}

pub fn apps_for_paths(paths: &[String]) -> Vec<&'static str> {
    let mut apps: Vec<&'static str> = Vec::new();
    for app in paths.iter().filter_map(|p| app_of(p)) {
        if !apps.contains(&app) {
            apps.push(app);
        }
    }
    apps
}

pub const BUDGET: Spec = Spec { flags: &["budget", "strict", "blast-radius", "unused-deps", "workspace-crossings", "cargo-inputs"], positional: Positional::None, ..Spec::NONE };

/// `check --budget [--strict]`.
pub fn budget(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &BUDGET)?;
    let strict = p.flag("strict");
    let root = repo_root();
    if p.flag("blast-radius") {
        let report = crate::blast_radius::check(&root);
        println!("{}", report.json);
        for found in &report.findings {
            wf_kit::wf_err!("wf: blast radius rule broken ({found})");
        }
        return Some(i32::from(!report.findings.is_empty()));
    }
    if p.flag("workspace-crossings") {
        let report = crate::workspace_crossings::check(&root);
        println!("{}", report.json);
        for found in &report.findings {
            wf_kit::wf_err!("wf: workspace crossing rule broken ({found})");
        }
        return Some(i32::from(!report.findings.is_empty()));
    }
    if p.flag("cargo-inputs") {
        let report = crate::cargo_inputs::check(&root);
        println!("{}", report.json);
        for found in &report.findings {
            wf_kit::wf_err!("wf: cargo task hashes npm packages ({found})");
        }
        return Some(i32::from(!report.findings.is_empty()));
    }
    if p.flag("unused-deps") {
        let found = crate::blast_radius::unused_dependencies(&root);
        println!("{}", serde_json::json!({ "unused": found, "note": "a word scan of each crate's sources: a dependency used only through a macro or a feature is a candidate to check" }));
        return Some(0);
    }
    match measure(&root) {
        Some(budgets) => Some(report(&budgets, strict)),
        None => {
            wf_kit::wf_err!("wf: {}", unreadable_input(&root));
            Some(1)
        }
    }
}
