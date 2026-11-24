//! The public performance options, with a deterministic OTLP workload standing in for Cargo.
use crate::support::{create_stub, run_wf, RunOpts};
use serde_json::Value;

#[test]
fn given_a_baseline_when_later_spans_slow_or_disappear_then_the_option_fails_and_history_merges_once() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let scenarios = root.join("clis/app-bench/scenarios");
    std::fs::create_dir_all(&scenarios).unwrap();
    std::fs::write(scenarios.join("rca-hosts-trace.json"), r#"{"suite":"fixture","min_samples":1,"p95_ms":{"read":100}}"#).unwrap();
    create_stub(&root.join("cargo"), r#"set -eu
case "$*" in *"--test integration performance::"*) ;; *) exit 2 ;; esac
[ "$MS" = absent ] && exit 0
exec python3 - <<'PY'
import json,os,urllib.request
span = dict(traceId="a"*32, spanId="b"*16, name="read", startTimeUnixNano="1", endTimeUnixNano=str(1+int(os.environ["MS"])*1000000))
body = {"resourceSpans":[{"scopeSpans":[{"spans":[span]}]}]}
urllib.request.urlopen(urllib.request.Request(os.environ["OTEL_EXPORTER_OTLP_TRACES_ENDPOINT"],json.dumps(body).encode(),{"Content-Type":"application/json"})).read()
PY"#);
    let db = root.join("samples.sqlite");
    let opts = RunOpts::default().bin_dir(root).cwd(root);
    let args = ["test", "run", "--performance", "--db", db.to_str().unwrap()];
    let measure = |ms| run_wf(&args, &opts.clone().env("MS", ms));
    let first = measure("10");
    assert_eq!(first.exit_code, 0, "{}", first.stderr);
    let slow = measure("16");
    assert_eq!(slow.exit_code, 1);
    assert_eq!(serde_json::from_str::<Value>(&slow.stdout).unwrap()["metrics"][0]["budget_ms"], 15.0);
    let absent = measure("absent");
    assert_eq!(absent.exit_code, 1);
    assert_eq!(serde_json::from_str::<Value>(&absent.stdout).unwrap()["metrics"][0]["samples"], 0);
    // A fixture can set a minimum measurable change for sub-millisecond operations.
    std::fs::write(scenarios.join("rca-hosts-trace.json"), r#"{"suite":"fixture","min_samples":1,"p95_ms":{"read":100},"min_change_ms":{"read":7}}"#).unwrap();
    assert_eq!(measure("16").exit_code, 0);
    assert_eq!(measure("18").exit_code, 1);
    let merged = root.join("merged.sqlite");
    let import = run_wf(&["test","run","--performance","--db",merged.to_str().unwrap(),"--merge",args[4],"--merge",args[4]], &opts);
    assert_eq!(import.exit_code, 0, "{}", import.stderr);
    assert_eq!(serde_json::from_str::<Value>(&import.stdout).unwrap()["runs"], 5);
}

#[test]
fn given_a_normal_command_when_traced_then_stdout_and_failure_status_are_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("history.sqlite");
    let opts = RunOpts::default().env("WF_PERFORMANCE_DB", db.to_str().unwrap());
    let plain = run_wf(&["--version"], &opts);
    let traced = run_wf(&["--performance", "--version"], &opts);
    assert_eq!((traced.exit_code, traced.stdout), (0, plain.stdout));
    assert!(traced.stderr.contains("cli.help"), "{}", traced.stderr);
    assert_eq!(run_wf(&["--performance", "unknown-command"], &opts).exit_code, 1);
}
