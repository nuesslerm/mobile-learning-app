//! Path dependencies that cross a Cargo workspace boundary (`wf check --workspace-crossings`). Every cli, library, SDK and service is meant to be
//! its own workspace, and anything outside it takes a published version, so the crossings that exist today are listed in
//! `data/workspace-crossings.json` and that list only shrinks: a crossing not on it fails, and an entry that no longer crosses fails too, so the
//! file stays an honest worklist. A crossing is named `<manifest dir> -> <dependency dir>`, the manifest that declares the path (the workspace
//! root for an inherited `[workspace.dependencies]` entry). Read from the manifests with no toolchain, like `library_paths`.
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use toml::Value;

pub const ALLOWLIST_FILE: &str = "clis/pipeline/data/workspace-crossings.json";

pub struct Report {
    pub json: Json,
    pub findings: Vec<String>,
}

struct Crossing {
    manifest: String,
    dependency: String,
    target: String,
    tables: BTreeSet<String>,
    consumers: usize,
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn show(path: &Path) -> String {
    let text = path.to_string_lossy();
    if text.is_empty() { ".".to_string() } else { text.to_string() }
}

pub(crate) fn read(root: &Path, dir: &Path) -> Option<Value> {
    std::fs::read_to_string(root.join(dir).join("Cargo.toml")).ok()?.parse().ok()
}

pub(crate) fn workspace_of(dir: &Path, roots: &BTreeSet<PathBuf>) -> PathBuf {
    dir.ancestors().find(|a| roots.contains(*a)).unwrap_or(dir).to_path_buf()
}

pub(crate) fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if path.is_dir() {
            if !name.starts_with('.') && !["target", "node_modules", "references"].contains(&name.as_str()) {
                walk(&path, out);
            }
        } else if name == "Cargo.toml" {
            out.push(path);
        }
    }
}

fn path_specs(manifest: &Value) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    let named: [(&str, Option<&Value>); 3] = [("dependencies", manifest.get("dependencies")), ("dev-dependencies", manifest.get("dev-dependencies")), ("build-dependencies", manifest.get("build-dependencies"))];
    let mut collect = |table: &str, value: Option<&toml::Table>| {
        for (key, spec) in value.into_iter().flatten() {
            if let Some(path) = spec.get("path").and_then(Value::as_str) {
                let real = spec.get("package").and_then(Value::as_str).unwrap_or(key);
                found.push((table.to_string(), real.to_string(), path.to_string()));
            }
        }
    };
    for (name, value) in named {
        collect(name, value.and_then(Value::as_table));
    }
    collect("workspace.dependencies", manifest.get("workspace").and_then(|w| w.get("dependencies")).and_then(Value::as_table));
    for target in manifest.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
        for name in ["dependencies", "dev-dependencies", "build-dependencies"] {
            collect(name, target.get(name).and_then(Value::as_table));
        }
    }
    for registry in manifest.get("patch").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
        collect("patch", registry.as_table());
    }
    found
}

fn inheriting(manifest: &Value, key: &str) -> bool {
    crate::library_paths::tables(manifest).into_iter().any(|t| t.get(key).and_then(|s| s.get("workspace")).and_then(Value::as_bool) == Some(true)) && manifest.get("package").is_some()
}

fn crossings(root: &Path) -> Vec<Crossing> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let manifests: BTreeMap<PathBuf, Value> = files.iter().filter_map(|f| f.parent()?.strip_prefix(root).ok().map(Path::to_path_buf)).filter_map(|d| read(root, &d).map(|m| (d, m))).collect();
    let roots: BTreeSet<PathBuf> = manifests.iter().filter(|(_, m)| m.get("workspace").is_some()).map(|(d, _)| d.clone()).collect();
    let mut found: BTreeMap<(String, String), Crossing> = BTreeMap::new();
    for (dir, manifest) in &manifests {
        let own = workspace_of(dir, &roots);
        for (table, dependency, path) in path_specs(manifest) {
            let target = normalize(&dir.join(&path));
            if workspace_of(&target, &roots) == own {
                continue;
            }
            let name = read(root, &target).and_then(|m| m.get("package")?.get("name")?.as_str().map(str::to_string)).unwrap_or(dependency.clone());
            let consumers = if table == "workspace.dependencies" {
                manifests.iter().filter(|(d, m)| d.starts_with(dir) && workspace_of(d, &roots) == own && inheriting(m, &dependency)).count().max(1)
            } else {
                1
            };
            let entry = found.entry((show(dir), show(&target))).or_insert(Crossing { manifest: show(dir), dependency: name, target: show(&target), tables: BTreeSet::new(), consumers });
            entry.tables.insert(table);
            entry.consumers = entry.consumers.max(consumers);
        }
    }
    found.into_values().collect()
}

fn allowlist(root: &Path) -> Result<BTreeSet<String>, String> {
    let text = match std::fs::read_to_string(root.join(ALLOWLIST_FILE)) {
        Ok(text) => text,
        Err(_) => return Ok(BTreeSet::new()),
    };
    let json: Json = serde_json::from_str(&text).map_err(|e| format!("{ALLOWLIST_FILE} is not JSON: {e}"))?;
    Ok(json["allowed"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).collect())
}

pub fn check(root: &Path) -> Report {
    let found = crossings(root);
    let allowed = match allowlist(root) {
        Ok(allowed) => allowed,
        Err(e) => return Report { json: json!({ "error": e }), findings: vec![e] },
    };
    let name = |c: &Crossing| format!("{} -> {}", c.manifest, c.target);
    let mut findings = Vec::new();
    for c in found.iter().filter(|c| !allowed.contains(&name(c))) {
        findings.push(format!("{}: path dependency on {} crosses a workspace boundary; take a published version from a registry, or make the dependency part of this workspace", name(c), c.dependency));
    }
    let live: BTreeSet<String> = found.iter().map(name).collect();
    for stale in allowed.iter().filter(|a| !live.contains(*a)) {
        findings.push(format!("{ALLOWLIST_FILE}: `{stale}` no longer crosses a workspace boundary; remove the entry"));
    }
    let list: Vec<Json> = found.iter().map(|c| json!({ "manifest": c.manifest, "dependency": c.dependency, "target": c.target, "tables": c.tables, "consumers": c.consumers, "allowed": allowed.contains(&name(c)) })).collect();
    Report { json: json!({ "crossings": list, "allowlist": allowed.len(), "findings": findings }), findings }
}

pub fn count(root: &Path) -> usize {
    check(root).findings.len()
}
