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

/// The folder a `test:integration` script points `WF_TEST_OUTPUT_DIR` at, as a workspace-relative path (its `D="$PWD/<path>"`, resolved from the package dir).
fn records_folder(package_dir: &std::path::Path, script: &str) -> Option<String> {
    let rest = script.split("D=\"$PWD/").nth(1)?;
    let relative = rest.split('"').next()?;
    let mut parts: Vec<String> = package_dir.strip_prefix(repo_root()).ok()?.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    for part in relative.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            other => parts.push(other.to_string()),
        }
    }
    Some(parts.join("/"))
}

fn package_dirs() -> Vec<std::path::PathBuf> {
    ["backend", "clis", "libraries", "frontend"]
        .iter()
        .filter_map(|top| std::fs::read_dir(repo_root().join(top)).ok())
        .flat_map(|entries| entries.flatten().map(|e| e.path()))
        .filter(|dir| dir.join("package.json").is_file())
        .collect()
}

#[test]
fn a_package_integration_script_that_sets_its_records_folder_declares_that_folder_as_an_output() {
    // Given every package whose test:integration script points WF_TEST_OUTPUT_DIR at a folder of its own
    let mut setting: Vec<String> = Vec::new();
    let mut wrong: Vec<String> = Vec::new();
    for dir in package_dirs() {
        let package: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).unwrap()).unwrap();
        let script = package["scripts"]["test:integration"].as_str().unwrap_or_default();
        let outputs: Vec<&str> = package["nx"]["targets"]["test:integration"]["outputs"].as_array().map(|o| o.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let name = package["name"].as_str().unwrap_or_default().to_string();

        // When the folder the script sets is compared with the outputs the target declares
        let Some(folder) = records_folder(&dir, script) else {
            if outputs.iter().any(|o| o.contains(".agents/tmp/test-output")) {
                wrong.push(format!("{name}: declares a records output but its script sets no folder"));
            }
            continue;
        };
        setting.push(name.clone());
        let declared = format!("{{workspaceRoot}}/{folder}");

        // Then the folder is declared, is under the shared records root, and the script removes it before the run
        if !outputs.contains(&declared.as_str()) {
            wrong.push(format!("{name}: the script writes {folder} but outputs are {outputs:?}"));
        }
        if !folder.starts_with(".agents/tmp/test-output/") {
            wrong.push(format!("{name}: {folder} is outside .agents/tmp/test-output, where the integration shard looks for records"));
        }
        if !script.contains("rm -rf \"$D\"") || !script.contains("WF_TEST_OUTPUT_DIR=\"$D\"") {
            wrong.push(format!("{name}: the script must remove the folder first and pass it as WF_TEST_OUTPUT_DIR"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
    for package in ["@backend/vm-ops-api", "@app/remote-chat-agent"] {
        assert!(setting.iter().any(|n| n == package), "{package} must keep its integration records in the cache: {setting:?}");
    }
}
