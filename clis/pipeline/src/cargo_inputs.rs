//! A package whose cached scripts only run cargo must name no npm package in its Nx hash (`wf check --cargo-inputs`, also a gated budget row).
//! Nx adds every installed npm package to a task's hash unless the task lists `{ "externalDependencies": [] }`, and the installed layout differs by
//! CPU: the sandbox's gate and the CI runner then hash the same crate task differently and CI never replays what the gate seeded (run 1208).
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const CACHED: [&str; 2] = ["test:flow", "test:integration"];

pub struct Report {
    pub json: Value,
    pub findings: Vec<String>,
}

fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if !matches!(e.file_name().to_str(), Some("node_modules" | "target" | ".git" | ".nx" | "references")) {
                manifests(&path, out);
            }
        } else if e.file_name() == "package.json" {
            out.push(path);
        }
    }
}

fn runs_only_cargo(script: &str) -> bool {
    script.split("&&").all(|part| {
        let mut words = part.split_whitespace().skip_while(|w| w.contains('=') && !w.starts_with('-'));
        words.next() == Some("cargo")
    })
}

fn pins_no_npm(package: &Value, target: &str) -> bool {
    package["nx"]["targets"][target]["inputs"].as_array().is_some_and(|inputs| inputs.iter().any(|i| i["externalDependencies"].as_array().is_some_and(Vec::is_empty)))
}

pub fn check(root: &Path) -> Report {
    let mut files = Vec::new();
    manifests(root, &mut files);
    files.sort();
    let mut findings = Vec::new();
    let mut checked = 0;
    for file in files {
        let Some(package) = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        for target in CACHED {
            let Some(script) = package["scripts"][target].as_str().filter(|s| runs_only_cargo(s)) else { continue };
            checked += 1;
            if !pins_no_npm(&package, target) {
                let dir = file.parent().and_then(|d| d.strip_prefix(root).ok()).map_or(String::new(), |d| d.display().to_string());
                findings.push(format!("{dir} {target} runs only cargo ({}) but its Nx inputs do not list {{ \"externalDependencies\": [] }}", script.split_whitespace().take(2).collect::<Vec<_>>().join(" ")));
            }
        }
    }
    Report { json: json!({ "checked": checked, "violations": findings }), findings }
}
