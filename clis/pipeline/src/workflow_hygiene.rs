//! Supply-chain rules over the workflow files (docs/plans/ci-scoped-vault-access/plan.md, Phase 5): a third-party action is pinned by commit
//! sha, no cache or artifact step uploads a generated `.env`, and no workflow hands every secret to a called workflow.
use regex::Regex;
use std::path::{Path, PathBuf};
use yaml_rust2::{Yaml, YamlLoader};

/// `owner/repo@<reason>` entries that may stay unpinned; an entry needs the reason its sha could not be resolved.
const UNPINNED_ALLOWED: [(&str, &str); 0] = [];

fn files(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(root.join(".github/workflows")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml")).collect();
    out.extend(std::fs::read_dir(root.join(".github/actions")).into_iter().flatten().flatten().map(|e| e.path().join("action.yml")).filter(|p| p.is_file()));
    out.sort();
    out
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn content_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines().enumerate().filter(|(_, l)| !l.trim_start().starts_with('#')).map(|(n, l)| (n + 1, l))
}

/// `<file>:<line>: <uses>` for each `uses:` that is neither a local path nor `owner/repo@<40 hex>`.
pub fn unpinned_actions(root: &Path) -> Vec<String> {
    let uses = Regex::new(r##"^\s*(?:-\s+)?uses:\s*['"]?([^\s'"#]+)"##).unwrap();
    let pinned = Regex::new(r"^[\w.-]+/[\w.-]+(?:/[\w./-]+)?@[0-9a-f]{40}$").unwrap();
    let mut found = Vec::new();
    for file in files(root) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        for (n, line) in content_lines(&text) {
            let Some(target) = uses.captures(line).map(|c| c[1].to_string()) else { continue };
            if target.starts_with("./") || pinned.is_match(&target) || UNPINNED_ALLOWED.iter().any(|(name, _)| target.starts_with(&format!("{name}@"))) {
                continue;
            }
            found.push(format!("{}:{n}: {target}", rel(root, &file)));
        }
    }
    found
}

/// `<file>:<step>` for each `actions/upload-artifact` or `actions/cache` step whose `path` names a `.env` file with no `!` exclusion of it.
pub fn env_file_in_artifacts(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for file in files(root) {
        let Some(docs) = std::fs::read_to_string(&file).ok().and_then(|t| YamlLoader::load_from_str(&t).ok()) else { continue };
        let Some(doc) = docs.first() else { continue };
        let mut steps: Vec<&Yaml> = Vec::new();
        if let Yaml::Hash(jobs) = &doc["jobs"] {
            steps.extend(jobs.values().filter_map(|j| j["steps"].as_vec()).flatten());
        }
        steps.extend(doc["runs"]["steps"].as_vec().into_iter().flatten());
        for step in steps {
            let uses = step["uses"].as_str().unwrap_or("");
            if !(uses.starts_with("actions/upload-artifact@") || uses.starts_with("actions/cache@") || uses.starts_with("actions/cache/")) {
                continue;
            }
            let path = step["with"]["path"].as_str().unwrap_or("");
            let entries: Vec<&str> = path.lines().map(str::trim).filter(|e| !e.is_empty()).collect();
            let includes = entries.iter().any(|e| !e.starts_with('!') && e.contains(".env"));
            let excludes = entries.iter().any(|e| e.starts_with('!') && e.contains(".env"));
            if includes && !excludes {
                let name = step["name"].as_str().unwrap_or(uses);
                found.push(format!("{}: {name}", rel(root, &file)));
            }
        }
    }
    found
}

/// `<file>:<line>` for each `secrets: inherit`.
pub fn secrets_inherit(root: &Path) -> Vec<String> {
    let inherit = Regex::new(r"^\s*secrets:\s*inherit\s*(?:#.*)?$").unwrap();
    let mut found = Vec::new();
    for file in files(root) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        found.extend(content_lines(&text).filter(|(_, l)| inherit.is_match(l)).map(|(n, _)| format!("{}:{n}", rel(root, &file))));
    }
    found
}
