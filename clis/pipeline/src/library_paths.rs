//! A service or CLI takes a versioned library (libraries.json) from its registry, so a library edit with no release cannot move it: a
//! `path` dependency on one defeats that. Only a library's own manifests may name a sibling library by path, beside its version and
//! registry (that is how it is packaged and developed). Read from the manifests with no toolchain, like `core_purity`.
use std::path::{Path, PathBuf};
use toml::Value;

const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
const SKIP: [&str; 5] = ["target", "node_modules", "references", ".git", ".nx"];

pub(crate) fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !SKIP.contains(&entry.file_name().to_string_lossy().as_ref()) {
                manifests(&path, out);
            }
        } else if entry.file_name() == "Cargo.toml" {
            out.push(path);
        }
    }
}

pub(crate) fn tables(manifest: &Value) -> Vec<&toml::Table> {
    let mut found: Vec<&toml::Table> = DEP_TABLES.iter().filter_map(|k| manifest.get(*k).and_then(Value::as_table)).collect();
    if let Some(workspace) = manifest.get("workspace").and_then(|w| w.get("dependencies")).and_then(Value::as_table) {
        found.push(workspace);
    }
    for target in manifest.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
        found.extend(DEP_TABLES.iter().filter_map(|k| target.get(*k).and_then(Value::as_table)));
    }
    found
}

/// `<manifest>: <library>` for each path dependency on a library, outside the libraries' own manifests.
pub fn violations(root: &Path) -> Vec<String> {
    let Some(catalog) = std::fs::read_to_string(root.join("libraries.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return Vec::new() };
    let libraries: Vec<(String, String)> = catalog["libraries"].as_array().into_iter().flatten().filter_map(|l| Some((l["name"].as_str()?.to_string(), l["path"].as_str()?.to_string()))).collect();
    let mut files = Vec::new();
    for top in ["backend", "clis", "libraries", "infra"] {
        manifests(&root.join(top), &mut files);
    }
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(&file).parent().unwrap_or(Path::new("")).to_path_buf();
        if libraries.iter().any(|(_, path)| relative.starts_with(path)) {
            continue;
        }
        let Some(manifest) = std::fs::read_to_string(&file).ok().and_then(|t| t.parse::<Value>().ok()) else { continue };
        for table in tables(&manifest) {
            for (key, spec) in table {
                let real = spec.get("package").and_then(Value::as_str).unwrap_or(key);
                if spec.get("path").is_some() && libraries.iter().any(|(name, _)| name == real) {
                    found.push(format!("{}: {real}", file.strip_prefix(root).unwrap_or(&file).display()));
                }
            }
        }
    }
    found
}

pub fn count(root: &Path) -> usize {
    violations(root).len()
}
