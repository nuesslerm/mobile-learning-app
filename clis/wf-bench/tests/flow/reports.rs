use serde_json::{json, Value};
use std::fs;
use std::process::Command;

#[test]
fn given_a_record_when_building_then_the_catalog_is_reproducible_and_invalid_evidence_is_rejected()
{
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let archive = root.join("docs/benchmarks");
    let report = archive.join("reports/example");
    fs::create_dir_all(&report).unwrap();
    fs::create_dir_all(archive.join("schema")).unwrap();
    fs::write(
        archive.join("schema/report-v1.schema.json"),
        include_str!("../../../../docs/benchmarks/schema/report-v1.schema.json"),
    )
    .unwrap();
    fs::write(report.join("measurement.txt"), "2 ms").unwrap();
    let path = report.join("report.json");
    let mut record = json!({
        "schemaVersion":1,"id":"example","title":"Latency <measured>", "subject":"Drivers",
        "question":"How long?","date":null,"revision":null,"environment":null,"workload":"One tap",
        "method":"Clock","finding":"2 ms","variants":[],"scenarios":[],"metrics":[],"observations":[],
        "tables":[],"evidence":[{"label":"Measurement","path":"measurement.txt"}]
    });
    fs::write(&path, record.to_string()).unwrap();
    let invoke = |arg| {
        let exe =
            std::env::var_os("WF_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_wf-bench").into());
        let output = Command::new(exe)
            .args(["bench", "reports", arg])
            .current_dir(root)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["ok"], output.status.success());
        result
    };
    assert_eq!(invoke("check")["ok"], false);
    assert_eq!(invoke("build")["reports"], 1);
    assert_eq!(invoke("build")["updated"], 0);
    assert_eq!(invoke("check")["ok"], true);
    assert!(fs::read_to_string(report.join("index.html"))
        .unwrap()
        .contains("Latency &lt;measured&gt;"));
    fs::write(report.join("index.html"), "obsolete").unwrap();
    assert_eq!(invoke("check")["ok"], false);
    record["evidence"][0]["path"] = json!("missing.txt");
    fs::write(&path, record.to_string()).unwrap();
    assert_eq!(invoke("build")["ok"], false);
    assert_eq!(
        fs::read_to_string(report.join("index.html")).unwrap(),
        "obsolete"
    );
}

#[test]
fn given_the_committed_benchmarks_when_checked_then_all_evidence_and_views_are_current() {
    let exe = std::env::var_os("WF_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_wf-bench").into());
    let output = Command::new(exe)
        .args(["bench", "reports", "check"])
        .current_dir(wf_kit::root::repo_root())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
}
