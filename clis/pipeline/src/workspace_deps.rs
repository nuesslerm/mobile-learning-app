//! The workspace crates of the repo and the dependencies each declares: read from the manifests with no toolchain, like `cargo_deps`, so the
//! flow tests can drive it on a temp repo. A dependency is a path dependency (direct, or inherited from the workspace root's
//! `[workspace.dependencies]`), also a name match inside one workspace; a dependency taken from a registry names no workspace crate.
//!
//! - **No crate declares a workspace crate its sources never name** (`wf check --budget`, alone as `wf check --unused-workspace-deps`): such an
//!   edge reruns the dependant's builds and tests for nothing. There is no allowance list.
//! - `unused_dependencies` is the declared-but-unused finder (`wf check --unused-deps`), over every crate of the repo: a word scan of the
//!   crate's own sources, so a crate used only through a macro or a feature shows up as a candidate to check, not as a fact.
//!
//! A crate name two workspaces share is shown as `name (dir)`.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use toml::Value;

struct Member {
    id: String,
    dir: PathBuf,
    deps: Vec<Dep>,
}

/// A declared dependency: its key in code and the workspace crate it names (when it names one).
struct Dep {
    key: String,
    target: Option<String>,
    // the target crate's `[lib] name`, which its dependants use in code when it differs from the package name
    lib: Option<String>,
}

/// The crates of the repo and what they declare.
pub struct Graph {
    members: Vec<Member>,
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
    let lib_of: BTreeMap<&str, String> = packages
        .iter()
        .filter_map(|(d, (id, _))| manifests.get(*d).and_then(|m| m.get("lib")).and_then(|l| l.get("name")).and_then(Value::as_str).map(|n| (id.as_str(), n.to_string())))
        .collect();
    let by_dir: BTreeMap<&PathBuf, &str> = packages.iter().map(|(d, (id, _))| (*d, id.as_str())).collect();
    let by_name: BTreeMap<(PathBuf, &str), &str> = packages.iter().map(|(d, (id, name))| ((workspace_of(d, &roots), name.as_str()), id.as_str())).collect();
    let mut members = Vec::new();
    for (dir, manifest) in &manifests {
        let Some((id, _)) = packages.get(dir) else { continue };
        let workspace = workspace_of(dir, &roots);
        let inherited = manifests.get(&workspace).and_then(|m| m.get("workspace")).and_then(|w| w.get("dependencies")).and_then(Value::as_table);
        let mut deps = Vec::new();
        let mut collect = |holder: &Value| {
            for table in ["dependencies", "build-dependencies", "dev-dependencies"] {
                for (key, spec) in holder.get(table).and_then(Value::as_table).into_iter().flatten() {
                    let (base, from) = match inherited.and_then(|t| t.get(key)).filter(|_| spec.get("workspace").and_then(Value::as_bool) == Some(true)) {
                        Some(root_spec) => (root_spec, &workspace),
                        None => (spec, dir),
                    };
                    let package = base.get("package").and_then(Value::as_str).unwrap_or(key);
                    let by_path = base.get("path").and_then(Value::as_str).and_then(|p| by_dir.get(&normalize(&from.join(p))));
                    let target = by_path.or_else(|| by_name.get(&(workspace.clone(), package))).filter(|t| **t != id.as_str()).map(|t| t.to_string());
                    let lib = target.as_deref().and_then(|t| lib_of.get(t)).cloned();
                    deps.push(Dep { key: key.clone(), target, lib });
                }
            }
        };
        collect(manifest);
        for target in manifest.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
            collect(target);
        }
        members.push(Member { id: id.clone(), dir: root.join(dir), deps });
    }
    Graph { members, unreadable }
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
        let keys: BTreeSet<&str> = member
            .deps
            .iter()
            .filter(|d| !local_only || d.target.is_some())
            .filter(|d| !words.contains(&d.key.replace('-', "_")) && !d.lib.as_ref().is_some_and(|l| words.contains(l)))
            .map(|d| d.key.as_str())
            .collect();
        found.extend(keys.into_iter().map(|dep| (member.id.clone(), dep.to_string())));
    }
    found
}

/// `<crate>: <dependency>` for each dependency a crate declares and its sources never name.
pub fn unused_dependencies(root: &Path) -> Vec<String> {
    unused(&load(root), false).into_iter().map(|(krate, dep)| format!("{krate}: {dep}")).collect()
}

/// The findings of the workspace-dependency rule: manifests that cannot be read, and each declared workspace crate no source names.
pub fn check(root: &Path) -> Vec<String> {
    let graph = load(root);
    let mut findings = graph.unreadable.clone();
    findings.extend(unused(&graph, true).into_iter().map(|(krate, dep)| format!("{krate} declares the workspace crate {dep} but no source of it names it: the edge reruns {krate} whenever {dep} changes, remove it")));
    findings
}

pub fn count(root: &Path) -> usize {
    check(root).len()
}
