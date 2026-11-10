//! The no-handrolled-recorder rule (docs/plans/mimic/plan.md 8.2): a reference's behaviour is recorded by `wf mimic`, never by a script of
//! its own. A script that generates or records into a fixtures, goldens, vectors or cassettes path, a cassette without its provenance and
//! a `RECORD_*` switch in a test are each a finding, unless `clis/mimic/recorder-allowlist.json` names the file and why it stays.
use regex::Regex;
use std::path::{Path, PathBuf};

const SKIP: [&str; 8] = ["target", "node_modules", "references", ".git", ".nx", ".agents", "dist", ".expo"];
const SCRIPTS: [&str; 5] = ["mjs", "ts", "js", "py", "cjs"];
const FIXTURE_DIRS: [&str; 5] = ["fixtures", "goldens", "golden", "vectors", "cassettes"];
/// The harness's own sources record through its machinery: they are not hand-rolled recorders.
const OWN: [&str; 2] = ["clis/mimic/", "tooling/mimic-drivers/"];
const ALLOWLIST: &str = "clis/mimic/recorder-allowlist.json";

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !SKIP.contains(&entry.file_name().to_string_lossy().as_ref()) {
                walk(&path, out);
            }
        } else {
            out.push(path);
        }
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn in_fixture_dir(rel: &str) -> bool {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    parts.iter().any(|p| FIXTURE_DIRS.contains(p))
}

fn is_script(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| SCRIPTS.contains(&ext))
}

fn generator_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("");
    stem.starts_with("generate") || (stem.starts_with("record") && !name.ends_with(".py"))
}

fn allowed(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(ALLOWLIST)).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    json["allow"].as_array().into_iter().flatten().filter_map(|e| e["path"].as_str().map(String::from)).collect()
}

/// `<path>: <what is wrong>` for each finding; an allowlist entry whose file is gone is a finding too, so the list only shrinks.
pub fn violations(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    let allow = allowed(root);
    let writes = Regex::new(r#"(writeFileSync|writeFile|write_text|write_bytes|open\([^)]*['"]w)[^;\n]*(fixtures|goldens?|vectors|cassettes)/"#).unwrap();
    let env_switch = Regex::new(r#"(env::var(_os)?\(\s*"RECORD_[A-Z0-9_]+"|process\.env\.RECORD_|environ(\.get)?[\[(]\s*['"]RECORD_)"#).unwrap();
    let mut found = Vec::new();
    for file in &files {
        let path = rel(root, file);
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let own = OWN.iter().any(|o| path.starts_with(o));
        if name == "interactions.jsonl" && !file.with_file_name("cassette.json").exists() {
            found.push(format!("{path}: recorded interactions with no cassette.json beside them (provenance: pin, rules, toolchain)"));
        }
        if own || allow.contains(&path) || !is_script(&name) && !path.ends_with(".rs") {
            continue;
        }
        if is_script(&name) && in_fixture_dir(&path) && generator_name(&name) {
            found.push(format!("{path}: a script that generates or records into a fixtures path: record the reference with `wf mimic record` (docs/plans/mimic/plan.md)"));
            continue;
        }
        let text = match std::fs::metadata(file) {
            Ok(m) if m.len() <= 512 * 1024 => std::fs::read_to_string(file).unwrap_or_default(),
            _ => continue,
        };
        if is_script(&name) && writes.is_match(&text) {
            found.push(format!("{path}: a script that writes into a fixtures, goldens, vectors or cassettes path: record with `wf mimic record`"));
        } else if path.contains("/tests/") && env_switch.is_match(&text) {
            found.push(format!("{path}: a RECORD_* switch in a test rewrites its own fixtures: record with `wf mimic record`"));
        }
    }
    for entry in &allow {
        if !root.join(entry).exists() {
            found.push(format!("{ALLOWLIST}: {entry} no longer exists, delete its entry"));
        }
    }
    found
}

pub fn count(root: &Path) -> usize {
    violations(root).len()
}
