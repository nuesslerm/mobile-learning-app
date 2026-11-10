//! `wf test goldens <runId|bundle URL> [--goldens-dir DIR]`: a published run's assert_screenshot captures (its
//! `goldens/<name>.actual.png` and sidecar `.json`; an older run's `shots/<name>.png`) become the goldens of its platform, the
//! `--update-goldens` of a run that happened on CI.
use serde_json::{json, Value};
use std::path::PathBuf;

fn get(url: &str) -> Result<Vec<u8>, String> {
    let r = cli_http::Request::get(url).timeout(std::time::Duration::from_secs(60)).send()?;
    if !r.ok() || r.body.is_empty() {
        return Err(format!("GET {url} returned HTTP {}", r.status));
    }
    Ok(r.body)
}

fn accept(target: &str, dir: Option<&str>) -> Result<Value, String> {
    let url = if target.starts_with("http") { target.to_string() } else { format!("{}/verification/test-runs/{target}/bundle.json", wf_bucket::artifacts::public_base()) };
    let bundle: Value = serde_json::from_slice(&get(&url)?).map_err(|e| format!("{url} is not a bundle: {e}"))?;
    let platform = bundle["harness"].as_str().ok_or_else(|| format!("{url} names no harness"))?;
    let dir = dir.map(PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join("docs/runbooks/goldens").join(platform));
    let shots: Vec<(String, &str)> = bundle["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| Some((golden_name(f["path"].as_str()?)?, f["url"].as_str()?)))
        .collect();
    if !shots.iter().any(|(name, _)| name.ends_with(".png")) {
        return Err(format!("{url} has no named screenshots (goldens/*.actual.png): the run made no device:assert_screenshot(name)"));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut written = Vec::new();
    for (name, file_url) in &shots {
        let path = dir.join(name);
        std::fs::write(&path, get(file_url)?).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path.display().to_string());
    }
    let names: Vec<&str> = shots.iter().filter_map(|(n, _)| n.strip_suffix(".png")).collect();
    Ok(json!({ "ok": true, "run": bundle["runId"], "platform": platform, "dir": dir.display().to_string(), "goldens": names, "written": written }))
}

/// The golden file a bundle path becomes: `goldens/<name>.actual.png` is `<name>.png` and `goldens/<name>.json` its sidecar
/// (a mismatch's expected and diff images are not); an older run's `shots/<name>.png|json` keep their names.
fn golden_name(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix("goldens/") {
        return match rest.strip_suffix(".actual.png") {
            Some(name) => Some(format!("{name}.png")),
            None => rest.strip_suffix(".json").map(|n| format!("{n}.json")),
        };
    }
    path.strip_prefix("shots/").filter(|n| n.ends_with(".png") || n.ends_with(".json")).map(str::to_string)
}

pub fn run(target: Option<&str>, dir: Option<&str>) -> i32 {
    let Some(target) = target else {
        println!("{}", json!({ "error": "wf test goldens wants a run", "next": "wf test goldens <runId|bundle URL> [--goldens-dir DIR] (runIds: wf test results --json)" }));
        return 1;
    };
    match accept(target, dir) {
        Ok(v) => {
            println!("{v}");
            0
        }
        Err(e) => {
            println!("{}", json!({ "error": e, "next": "wf test results --query \"test=<name> newest\" --json, then wf test goldens <runId>" }));
            1
        }
    }
}
