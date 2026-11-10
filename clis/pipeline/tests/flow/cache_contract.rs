//! The cache layers' contract (docs/solutions/ci/cache-layers.md): a cached Nx target that writes files names them in `outputs`, so
//! the bucket holds what the task produced and a clean runner restores it. A target that only passes or fails (a verdict) lists none.
use crate::support::repo_root;
use serde_json::Value;

/// The cached targets whose run writes files a replay must put back.
const FILE_WRITING: &[&str] = &["typecheck", "export:web", "export:eas"];

/// The cached targets that produce a verdict and nothing else.
fn is_verdict_only(name: &str) -> bool {
    name == "lint" || name.starts_with("test:") || name.starts_with("check:") || name.starts_with("cargo-")
}

fn target_defaults() -> serde_json::Map<String, Value> {
    let text = std::fs::read_to_string(repo_root().join("nx.json")).expect("nx.json is readable");
    let nx: Value = serde_json::from_str(&text).expect("nx.json is JSON");
    nx["targetDefaults"].as_object().cloned().expect("nx.json has targetDefaults")
}

fn outputs_of(target: &Value) -> usize {
    target["outputs"].as_array().map_or(0, Vec::len)
}

#[test]
fn a_cached_nx_target_that_writes_files_declares_them_as_outputs() {
    // Given the target defaults of the workspace's nx.json
    let targets = target_defaults();

    // When the targets known to write files are read
    let missing: Vec<&str> = FILE_WRITING.iter().copied().filter(|name| targets.get(*name).is_none_or(|t| t["cache"] != true || outputs_of(t) == 0)).collect();

    // Then each is cached and lists at least one output
    assert!(missing.is_empty(), "cached, file-writing targets without `outputs` in nx.json: {missing:?}");
}

#[test]
fn every_cached_nx_target_is_either_a_known_verdict_or_a_declared_file_writer() {
    // Given the target defaults of the workspace's nx.json
    let targets = target_defaults();

    // When each cached target is sorted into verdict-only, file-writing, or neither
    let unknown: Vec<&str> = targets
        .iter()
        .filter(|(_, t)| t["cache"] == true)
        .map(|(name, _)| name.as_str())
        .filter(|name| !is_verdict_only(name) && !FILE_WRITING.contains(name))
        .collect();

    // Then none is left unclassified, so a new cached target must say what it writes
    assert!(unknown.is_empty(), "cached targets not classified as verdict-only or file-writing (add them to FILE_WRITING with `outputs`, or make them verdicts): {unknown:?}");
}
