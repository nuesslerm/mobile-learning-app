//! The black-box check of the call-site contract (`contract/callsite.json`): given an implementation's source file, whose logging
//! calls carry `@site <case> fn=<name>` markers, and the records that implementation emitted, it says whether each record names
//! the file, line and function the markers promise. Rust's tests and the TypeScript logger's tests both run it.

use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// The expected call site of one marked logging call.
struct Marker {
    case: String,
    line: u64,
    function: Option<String>,
}

/// The verdict: `ok`, one entry per case, and the problems that are not about one case.
pub fn check(contract: &Value, implementation: &str, source: &str, file: &str, records: &[Value]) -> Result<Value, String> {
    let spec = &contract["implementations"][implementation];
    if spec.is_null() {
        return Err(format!("the contract has no implementation `{implementation}`"));
    }
    let key = |name: &str| spec["record"][name].as_str().map(str::to_string).ok_or_else(|| format!("the contract names no `{name}` key for `{implementation}`"));
    let (case_key, file_key, line_key, fn_key) = (key("case")?, key("file")?, key("line")?, key("fn")?);

    let pattern = Regex::new(contract["markerPattern"].as_str().ok_or("the contract has no markerPattern")?).map_err(|e| e.to_string())?;
    let markers: Vec<Marker> = source
        .lines()
        .enumerate()
        .filter_map(|(at, text)| {
            let found = pattern.captures(text)?;
            Some(Marker { case: found[1].to_string(), line: at as u64 + 1, function: (&found[2] != "-").then(|| found[2].to_string()) })
        })
        .collect();

    let required: Vec<&str> = contract["cases"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, case)| case["in"].as_array().is_some_and(|names| names.iter().any(|n| n == implementation)))
        .map(|(id, _)| id.as_str())
        .collect();

    let mut problems: Vec<String> = Vec::new();
    for id in &required {
        if !markers.iter().any(|m| m.case == *id) {
            problems.push(format!("the source has no marker for the contract case `{id}`"));
        }
    }
    for marker in &markers {
        if !required.contains(&marker.case.as_str()) {
            problems.push(format!("the marker `{}` is not a contract case for `{implementation}`", marker.case));
        }
    }

    let mut by_case: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for record in records {
        if let Some(case) = record[&case_key].as_str() {
            by_case.entry(case.to_string()).or_default().push(record);
        }
    }

    let mut cases = Vec::new();
    for marker in &markers {
        let expected = json!({ "file": file, "line": marker.line, "fn": marker.function });
        let found = by_case.get(&marker.case).map_or(&[][..], Vec::as_slice);
        let (actual, ok) = match found {
            [one] => {
                let actual = json!({ "file": one[&file_key], "line": one[&line_key], "fn": one.get(&fn_key).cloned().unwrap_or(Value::Null) });
                let ok = actual == expected;
                (actual, ok)
            }
            other => {
                problems.push(format!("the case `{}` was emitted {} times, not once", marker.case, other.len()));
                (Value::Null, false)
            }
        };
        cases.push(json!({ "case": marker.case, "ok": ok, "expected": expected, "actual": actual }));
    }
    for case in by_case.keys() {
        if !markers.iter().any(|m| &m.case == case) {
            problems.push(format!("a record names the case `{case}`, which no marker in the source declares"));
        }
    }

    let ok = problems.is_empty() && cases.iter().all(|c| c["ok"] == true);
    Ok(json!({ "ok": ok, "implementation": implementation, "source": file, "cases": cases, "problems": problems }))
}

/// `source` relative to `<repo_root>/<root>`, the `file` the contract promises for it.
pub fn file_of(repo_root: &Path, root: &str, source: &Path) -> Result<String, String> {
    let base = repo_root.join(root).canonicalize().map_err(|e| format!("{}: {e}", repo_root.join(root).display()))?;
    let source = source.canonicalize().map_err(|e| format!("{}: {e}", source.display()))?;
    source
        .strip_prefix(&base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| format!("{} is not under {}", source.display(), base.display()))
}
