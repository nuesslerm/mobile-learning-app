//! A TypeScript package lives in `frontend/` (`wf check --ts-packages`). The packages outside it today are listed in `data/ts-packages.json` and the
//! list only shrinks: a package.json that is not on it fails, and an entry that no longer has one fails too, so the file stays an honest worklist.
//! Read from the tree with no toolchain, like `workspace_crossings`.
use serde_json::{json, Value as Json};
use std::collections::BTreeSet;
use std::path::Path;

pub const ALLOWLIST_FILE: &str = "clis/pipeline/data/ts-packages.json";
const SKIP: [&str; 6] = ["node_modules", "target", "references", "dist", ".git", ".nx"];

pub struct Report {
    pub json: Json,
    pub findings: Vec<String>,
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if path.is_dir() {
            if !SKIP.contains(&name.as_str()) {
                walk(root, &path, out);
            }
        } else if name == "package.json" {
            let rel = path.parent().and_then(|p| p.strip_prefix(root).ok()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            out.insert(if rel.is_empty() { ".".to_string() } else { rel });
        }
    }
}

fn packages(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    walk(root, root, &mut found);
    found.retain(|dir| dir != "." && dir != "frontend" && !dir.starts_with("frontend/") && !dir.contains("/tests/fixtures/"));
    found
}

fn allowlist(root: &Path) -> Result<BTreeSet<String>, String> {
    let Ok(text) = std::fs::read_to_string(root.join(ALLOWLIST_FILE)) else { return Ok(BTreeSet::new()) };
    let json: Json = serde_json::from_str(&text).map_err(|e| format!("{ALLOWLIST_FILE} is not JSON: {e}"))?;
    Ok(json["allowed"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).collect())
}

pub fn check(root: &Path) -> Report {
    let allowed = match allowlist(root) {
        Ok(allowed) => allowed,
        Err(e) => return Report { json: json!({ "error": e }), findings: vec![e] },
    };
    let found = packages(root);
    let mut findings = Vec::new();
    for dir in found.iter().filter(|d| !allowed.contains(*d)) {
        findings.push(format!("{dir}: a TypeScript package outside frontend/; move it under frontend/ (frontend/libraries for a library)"));
    }
    for stale in allowed.iter().filter(|a| !found.contains(*a)) {
        findings.push(format!("{ALLOWLIST_FILE}: `{stale}` has no package.json any more; remove the entry"));
    }
    Report { json: json!({ "packages": found, "allowlist": allowed.len(), "findings": findings }), findings }
}

pub fn count(root: &Path) -> usize {
    check(root).findings.len()
}
