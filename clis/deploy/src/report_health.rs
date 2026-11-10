//! `wf check --health --list-unit-targets`: the workspace packages with a `test:unit` script, as JSON. The report itself
//! (live service checks, test tiers, publishing) stays in the TypeScript.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut dir = cwd.clone();
    for _ in 0..10 {
        if dir.join("pnpm-workspace.yaml").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }
    cwd
}

pub(crate) fn targets(root: &Path, scan_dirs: &[&str]) -> Option<Vec<Value>> {
    let mut out = vec![];
    for scan in scan_dirs {
        let scan_path = root.join(scan);
        if scan_path.exists() && !scan_path.is_dir() {
            // The TS throws on this; leave the crash and its message to it.
            return None;
        }
        let Ok(entries) = std::fs::read_dir(&scan_path) else { continue };
        let mut dirs: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .collect();
        dirs.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, dir) in dirs {
            let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else { continue };
            let Ok(pkg) = serde_json::from_str::<Value>(&text) else { continue };
            if !pkg["scripts"]["test:unit"].as_str().is_some_and(|s| !s.is_empty()) {
                continue;
            }
            let pkg_name = pkg["name"].as_str().map_or_else(|| format!("{scan}/{name}"), str::to_string);
            out.push(json!({ "pkgName": pkg_name, "dir": dir.to_string_lossy(), "script": "test:unit" }));
        }
    }
    Some(out)
}

const LIST_UNIT_TARGETS: wf_kit::args::Spec = wf_kit::args::Spec { flags: &["list-unit-targets"], positional: wf_kit::args::Positional::None, ..wf_kit::args::Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    if !wf_kit::args::probe(args, &[&LIST_UNIT_TARGETS, &crate::report_run::SPEC])?.flag("list-unit-targets") {
        return crate::report_run::run(args);
    }
    wf_kit::args::parse_spec(args, &LIST_UNIT_TARGETS)?;
    let list = targets(&repo_root(), &["backend", "clis", "features", "libraries", "tooling"])?;
    println!("{}", rca_core::stats::js_json_pretty(&Value::Array(list)));
    Some(0)
}
