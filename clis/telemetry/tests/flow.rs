//! The call-site contract, black box, for the Rust logger: the fixture's records are checked against its markers.

#[path = "fixtures/callsite.rs"]
mod fixture;

use serde_json::Value;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn contract() -> Value {
    serde_json::from_str(&std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contract/callsite.json")).unwrap()).unwrap()
}

fn records() -> Vec<Value> {
    fixture::console_lines().iter().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn verdict(source: &str, records: &[Value]) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/callsite.rs");
    let file = telemetry::callsite::file_of(&repo_root(), "clis/telemetry", &path).unwrap();
    telemetry::callsite::check(&contract(), "rust", source, &file, records).unwrap()
}

fn source() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/callsite.rs")).unwrap()
}

#[test]
fn given_the_marked_fixture_when_its_records_are_checked_then_every_case_names_its_file_line_and_function() {
    let report = verdict(&source(), &records());
    assert_eq!(report["ok"], true, "{report:#}");
    assert_eq!(report["cases"].as_array().unwrap().len(), 5);
    // and the file is the one relative to the cargo workspace the contract names for Rust
    assert_eq!(report["cases"][0]["actual"]["file"], "tests/fixtures/callsite.rs");
}

#[test]
fn given_a_marker_that_names_the_wrong_function_when_checked_then_the_case_fails_with_expected_and_actual() {
    let wrong = source().replace("// @site method fn=greet", "// @site method fn=not_greet");
    let report = verdict(&wrong, &records());
    let case = report["cases"].as_array().unwrap().iter().find(|c| c["case"] == "method").unwrap().clone();
    assert_eq!((report["ok"].clone(), case["ok"].clone(), case["expected"]["fn"].clone(), case["actual"]["fn"].clone()), (false.into(), false.into(), "not_greet".into(), "greet".into()));
}

#[test]
fn given_a_record_with_a_missing_function_or_a_case_without_a_marker_when_checked_then_the_problem_is_named() {
    let mut records = records();
    records.truncate(4);
    let report = verdict(&source(), &records);
    assert_eq!(report["ok"], false);
    assert!(report["problems"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().contains("`async-function` was emitted 0 times")), "{report:#}");
    let unmarked = source().replace("// @site async-function fn=async_function", "");
    assert!(verdict(&unmarked, &self::records())["problems"].to_string().contains("no marker for the contract case `async-function`"));
}
