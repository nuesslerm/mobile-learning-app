//! The blast radius of each crate under `clis`: every crate rebuilt and retested when it changes (its dependants through normal, build and
//! dev edges, since a dev edge reruns the tests), and the test stages that reach. Read from the manifests with no toolchain, like
//! `cargo_deps`, so the flow tests can drive it on a temp workspace. Three gates, all in `wf check --budget` and alone as `wf check --blast-radius`:
//!
//! - **A radius never grows past its budget** (`data/blast-radius.json`, today's measured radii): a crate gaining a dependant, directly or
//!   through a new edge below it, fails with the shortest dependency paths that pull it in (like `cargo tree -i`).
//! - **A low-level crate gains no edge to a heavy one**: low-level is a radius of at least a quarter of the crates, heavy a closure of at least
//!   a quarter, so both are read from the graph and nothing is listed by hand. An edge that already exists is allowed with a reason.
//!
//! - **No crate declares a workspace crate its sources never name**: such an edge adds the dependant to the radius for nothing. The ones that
//!   exist today are listed with a reason under `allowed_unused`, a worklist that only shrinks (an entry that is no longer unused fails).
//!
//! `unused_dependencies` is the declared-but-unused finder (`wf check --unused-deps`): a word scan of the crate's own sources, so a crate used only
//! through a macro or a feature shows up as a candidate to check, not as a fact.
//! The blast radius of each Rust crate of the repo, across every Cargo workspace: every crate rebuilt and retested when it changes (its
//! dependants through normal, build and dev edges, since a dev edge reruns the tests), and the test stages that reach. Read from the manifests
//! with no toolchain, like `cargo_deps`, so the flow tests can drive it on a temp repo. An edge is a path dependency (direct, or inherited from
//! the workspace root's `[workspace.dependencies]`), and also a name match inside one workspace; a path that crosses a workspace boundary is an
//! edge like any other (the crossings themselves are listed in `data/workspace-crossings.json`). A dependency taken from a registry is no edge:
//! a release, not a source edit, moves its consumers. Gates, all in `wf check --budget` and alone as `wf check --blast-radius`:
//!
//! - **A radius never grows past its budget** (`data/blast-radius.json`, the one budget file for every workspace, today's measured radii): a
//!   crate gaining a dependant, directly or through a new edge below it, fails with the shortest dependency paths that pull it in.
//! - **A low-level crate gains no edge to a heavy one**: low-level is a radius of at least a quarter of the crates of its workspace, heavy a
//!   closure of at least a quarter of the crates of its own, so both are read from the graph and nothing is listed by hand. An edge that already
//!   exists is allowed with a reason.
//! - **No crate declares a workspace crate its sources never name**: such an edge adds the dependant to the radius for nothing. The ones that
//!   exist today are listed with a reason under `allowed_unused`, a worklist that only shrinks (an entry that is no longer unused fails).
//!
//! A crate name two workspaces share is shown as `name (dir)`.
//!
//! `unused_dependencies` is the declared-but-unused finder (`wf check --unused-deps`), over every crate of the repo: a word scan of the crate's own
//! sources, so a crate used only through a macro or a feature shows up as a candidate to check, not as a fact.
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use toml::Value;

pub const BUDGET_FILE: &str = "clis/pipeline/data/blast-radius.json";
const STAGES: [(&str, &str); 4] = [("flow", "tests/flow.rs"), ("integration", "tests/integration.rs"), ("e2e", "tests/e2e.rs"), ("contract", "tests/nx_contract.rs")];
const SHOWN_PATHS: usize = 3;

struct Member {
    id: String,
    dir: PathBuf,
    workspace: String,
    deps: Vec<Dep>,
}

/// A declared dependency: its key in code, whether a production build holds it, and the workspace crate it names (when it names one).
struct Dep {
    key: String,
    production: bool,
    target: Option<String>,
}

/// The crates of the repo and the edges between them.
pub struct Graph {
    members: Vec<Member>,
    forward: BTreeMap<String, BTreeSet<String>>,
    sizes: BTreeMap<String, usize>,
    unreadable: Vec<String>,
}

fn rel(root: &Path, file: &Path) -> PathBuf {
    file.parent().and_then(|d| d.strip_prefix(root).ok()).map(Path::to_path_buf).unwrap_or_default()
}

fn shown(dir: &Path) -> String {
    let text = dir.to_string_lossy();
    if text.is_empty() { ".".to_string() } else { text.to_string() }
}

pub fn load(root: &Path) -> Graph {
    use crate::workspace_crossings::{normalize, workspace_of};
    let mut files = Vec::new();
    crate::workspace_crossings::walk(root, &mut files);
    files.sort();
    let (mut manifests, mut unreadable) = (BTreeMap::new(), Vec::new());
    for file in files {
        match std::fs::read_to_string(&file).ok().and_then(|t| t.parse::<Value>().ok()) {
            Some(manifest) => {
                manifests.insert(rel(root, &file), manifest);
            }
            None => unreadable.push(format!("{} cannot be read", file.strip_prefix(root).unwrap_or(&file).display())),
        }
    }
    let roots: BTreeSet<PathBuf> = manifests.iter().filter(|(_, m)| m.get("workspace").is_some()).map(|(d, _)| d.clone()).collect();
    let name_of = |m: &Value| m.get("package").and_then(|p| p.get("name")).and_then(Value::as_str).map(str::to_string);
    let mut taken: BTreeMap<String, usize> = BTreeMap::new();
    for name in manifests.values().filter_map(name_of) {
        *taken.entry(name).or_insert(0) += 1;
    }
    let packages: BTreeMap<&PathBuf, (String, String)> = manifests
        .iter()
        .filter_map(|(dir, m)| {
            let name = name_of(m)?;
            let id = if taken[&name] > 1 { format!("{name} ({})", shown(dir)) } else { name.clone() };
            Some((dir, (id, name)))
        })
        .collect();
    let by_dir: BTreeMap<&PathBuf, &str> = packages.iter().map(|(d, (id, _))| (*d, id.as_str())).collect();
    let by_name: BTreeMap<(PathBuf, &str), &str> = packages.iter().map(|(d, (id, name))| ((workspace_of(d, &roots), name.as_str()), id.as_str())).collect();
    let (mut members, mut sizes) = (Vec::new(), BTreeMap::new());
    for (dir, manifest) in &manifests {
        let Some((id, _)) = packages.get(dir) else { continue };
        let workspace = workspace_of(dir, &roots);
        let inherited = manifests.get(&workspace).and_then(|m| m.get("workspace")).and_then(|w| w.get("dependencies")).and_then(Value::as_table);
        let mut deps = Vec::new();
        let mut collect = |holder: &Value| {
            for (table, production) in [("dependencies", true), ("build-dependencies", true), ("dev-dependencies", false)] {
                for (key, spec) in holder.get(table).and_then(Value::as_table).into_iter().flatten() {
                    let (base, from) = match inherited.and_then(|t| t.get(key)).filter(|_| spec.get("workspace").and_then(Value::as_bool) == Some(true)) {
                        Some(root_spec) => (root_spec, &workspace),
                        None => (spec, dir),
                    };
                    let package = base.get("package").and_then(Value::as_str).unwrap_or(key);
                    let by_path = base.get("path").and_then(Value::as_str).and_then(|p| by_dir.get(&normalize(&from.join(p))));
                    let target = by_path.or_else(|| by_name.get(&(workspace.clone(), package))).filter(|t| **t != id.as_str()).map(|t| t.to_string());
                    deps.push(Dep { key: key.clone(), production, target });
                }
            }
        };
        collect(manifest);
        for target in manifest.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
            collect(target);
        }
        *sizes.entry(shown(&workspace)).or_insert(0) += 1;
        members.push(Member { id: id.clone(), dir: root.join(dir), workspace: shown(&workspace), deps });
    }
    let forward = members.iter().map(|m| (m.id.clone(), m.deps.iter().filter_map(|d| d.target.clone()).collect())).collect();
    Graph { members, forward, sizes, unreadable }
}

impl Graph {
    /// Each crate that reaches `target`, with the shortest path from it down to `target`.
    fn dependants(&self, target: &str) -> BTreeMap<String, Vec<String>> {
        let mut next: BTreeMap<String, String> = BTreeMap::new();
        let mut queue = VecDeque::from([target.to_string()]);
        while let Some(at) = queue.pop_front() {
            for (name, deps) in &self.forward {
                if deps.contains(&at) && name != target && !next.contains_key(name) {
                    next.insert(name.clone(), at.clone());
                    queue.push_back(name.clone());
                }
            }
        }
        next.keys()
            .map(|start| {
                let mut path = vec![start.clone()];
                while let Some(hop) = next.get(path.last().unwrap()) {
                    path.push(hop.clone());
                }
                (start.clone(), path)
            })
            .collect()
    }

    fn closure(&self, name: &str) -> BTreeSet<String> {
        let (mut seen, mut stack) = (BTreeSet::new(), vec![name.to_string()]);
        while let Some(at) = stack.pop() {
            for dep in self.forward.get(&at).into_iter().flatten() {
                if seen.insert(dep.clone()) {
                    stack.push(dep.clone());
                }
            }
        }
        seen
    }

    fn stages(&self, crates: impl Iterator<Item = String>) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for name in crates {
            let Some(member) = self.members.iter().find(|m| m.id == name) else { continue };
            for (stage, _) in STAGES.iter().filter(|(_, marker)| member.dir.join(marker).exists()) {
                *counts.entry(*stage).or_insert(0) += 1;
            }
        }
        counts
    }
}

pub struct Measured {
    pub name: String,
    pub workspace: String,
    pub radius: usize,
    pub closure: usize,
    pub stages: BTreeMap<&'static str, usize>,
}

pub fn measure(graph: &Graph) -> Vec<Measured> {
    graph
        .members
        .iter()
        .map(|m| {
            let dependants = graph.dependants(&m.id);
            let stages = graph.stages(dependants.keys().cloned().chain([m.id.clone()]));
            Measured { name: m.id.clone(), workspace: m.workspace.clone(), radius: dependants.len(), closure: graph.closure(&m.id).len(), stages }
        })
        .collect()
}

struct Budget {
    radius: BTreeMap<String, usize>,
    allowed_edges: BTreeMap<String, String>,
    allowed_unused: BTreeMap<String, String>,
}

fn read_budget(root: &Path) -> Result<Budget, String> {
    let path = root.join(BUDGET_FILE);
    if !path.exists() {
        return Ok(Budget { radius: BTreeMap::new(), allowed_edges: BTreeMap::new(), allowed_unused: BTreeMap::new() });
    }
    let parsed: Json = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or(format!("{BUDGET_FILE} cannot be read"))?;
    let map = |key: &str| parsed[key].as_object().cloned().unwrap_or_default();
    Ok(Budget {
        radius: map("radius").into_iter().filter_map(|(k, v)| Some((k, v.as_u64()? as usize))).collect(),
        allowed_edges: map("allowed_edges").into_iter().filter_map(|(k, v)| Some((k, v.as_str()?.to_string()))).collect(),
        allowed_unused: map("allowed_unused").into_iter().filter_map(|(k, v)| Some((k, v.as_str()?.to_string()))).collect(),
    })
}

fn show_stages(stages: &BTreeMap<&'static str, usize>) -> String {
    if stages.is_empty() {
        return "no test stage".to_string();
    }
    stages.iter().map(|(stage, n)| format!("{stage} x{n}")).collect::<Vec<_>>().join(", ")
}

pub struct Report {
    pub findings: Vec<String>,
    pub shrunk: Vec<String>,
    pub json: Json,
}

pub fn check(root: &Path) -> Report {
    let graph = load(root);
    let mut findings = graph.unreadable.clone();
    let budget = match read_budget(root) {
        Ok(budget) => budget,
        Err(e) => {
            findings.push(e);
            Budget { radius: BTreeMap::new(), allowed_edges: BTreeMap::new(), allowed_unused: BTreeMap::new() }
        }
    };
    let unused_edges = unused(&graph, true);
    let listed = |krate: &str, dep: &str| budget.allowed_unused.get(&format!("{krate} -> {dep}")).is_some_and(|reason| !reason.trim().is_empty());
    findings.extend(unused_edges.iter().filter(|(krate, dep)| !listed(krate, dep)).map(|(krate, dep)| format!("{krate} declares the workspace crate {dep} but no source of it names it: the edge only widens the blast radius of {dep}, remove it, or list `{krate} -> {dep}` with a reason under allowed_unused in {BUDGET_FILE}")));
    let live: BTreeSet<String> = unused_edges.iter().map(|(krate, dep)| format!("{krate} -> {dep}")).collect();
    findings.extend(budget.allowed_unused.keys().filter(|k| !live.contains(*k)).map(|k| format!("{BUDGET_FILE}: allowed_unused `{k}` is no longer an unused workspace dependency; remove the entry")));
    let measured = measure(&graph);
    let threshold = |workspace: &str| graph.sizes.get(workspace).copied().unwrap_or(0).div_ceil(4).max(2);
    let workspace_of: BTreeMap<&str, &str> = graph.members.iter().map(|m| (m.id.as_str(), m.workspace.as_str())).collect();
    let radius_of: BTreeMap<&str, usize> = measured.iter().map(|m| (m.name.as_str(), m.radius)).collect();
    let closure_of: BTreeMap<&str, usize> = measured.iter().map(|m| (m.name.as_str(), m.closure)).collect();
    let mut shrunk = Vec::new();
    for m in &measured {
        let limit = budget.radius.get(&m.name).copied().unwrap_or(0);
        if m.radius > limit {
            let mut paths: Vec<Vec<String>> = graph.dependants(&m.name).into_values().collect();
            paths.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
            let shown: Vec<String> = paths.iter().take(SHOWN_PATHS).map(|p| p.join(" -> ")).collect();
            let more = paths.len().saturating_sub(SHOWN_PATHS);
            let tail = if more > 0 { format!("; and {more} more (cargo tree -i {})", m.name) } else { String::new() };
            let missing = if budget.radius.contains_key(&m.name) { "" } else { " (no budget entry: add the crate at its radius to the budget file)" };
            findings.push(format!(
                "the blast radius of {} is {}, over its budget {limit}{missing}: that many crates rebuild and retest when it changes, reaching {}; pulled in by {}{tail}",
                m.name, m.radius, show_stages(&m.stages), shown.join("; ")
            ));
        } else if m.radius < limit {
            shrunk.push(format!("the blast radius of {} is {}, under its budget {limit}: lower the budget", m.name, m.radius));
        }
    }
    for member in &graph.members {
        if radius_of[member.id.as_str()] < threshold(&member.workspace) {
            continue;
        }
        let mut seen = BTreeSet::new();
        for package in member.deps.iter().filter(|d| d.production).filter_map(|d| d.target.as_ref()) {
            let Some(&closure) = closure_of.get(package.as_str()) else { continue };
            let edge = format!("{} -> {package}", member.id);
            if closure >= threshold(workspace_of[package.as_str()]) && seen.insert(edge.clone()) && !budget.allowed_edges.get(&edge).is_some_and(|reason| !reason.trim().is_empty()) {
                let brings: Vec<String> = graph.closure(package).into_iter().take(5).collect();
                findings.push(format!(
                    "the low-level crate {} (radius {}) depends on the heavy crate {package} (it pulls in {closure} crates, among them {}): {edge}; give {package} no such dependant, or allow the edge with a reason in {BUDGET_FILE}",
                    member.id, radius_of[member.id.as_str()], brings.join(", ")
                ));
            }
        }
    }
    let crates: Vec<Json> = measured.iter().map(|m| json!({ "name": m.name, "workspace": m.workspace, "radius": m.radius, "budget": budget.radius.get(&m.name), "closure": m.closure, "stages": m.stages })).collect();
    let json = json!({ "thresholds": graph.sizes.keys().map(|w| (w.clone(), threshold(w))).collect::<BTreeMap<_, _>>(), "crates": crates, "findings": findings, "shrunk": shrunk });
    Report { findings, shrunk, json }
}

fn rust_words(dir: &Path, own: &Path, words: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !["target", "node_modules"].contains(&entry.file_name().to_string_lossy().as_ref()) && !(path != own && path.join("Cargo.toml").exists()) {
                rust_words(&path, own, words);
            }
        } else if path.extension().is_some_and(|x| x == "rs") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            words.extend(text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).map(str::to_string));
        }
    }
}

/// `(crate, dependency key)` for each dependency a crate declares and its sources never name; `local_only` keeps those that are workspace crates.
fn unused(graph: &Graph, local_only: bool) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for member in &graph.members {
        let mut words = BTreeSet::new();
        rust_words(&member.dir, &member.dir, &mut words);
        let keys: BTreeSet<&str> = member.deps.iter().filter(|d| !local_only || d.target.is_some()).map(|d| d.key.as_str()).filter(|key| !words.contains(&key.replace('-', "_"))).collect();
        found.extend(keys.into_iter().map(|dep| (member.id.clone(), dep.to_string())));
    }
    found
}

/// `<crate>: <dependency>` for each dependency a crate declares and its sources never name.
pub fn unused_dependencies(root: &Path) -> Vec<String> {
    unused(&load(root), false).into_iter().map(|(krate, dep)| format!("{krate}: {dep}")).collect()
}

pub fn count(root: &Path) -> usize {
    check(root).findings.len()
}
