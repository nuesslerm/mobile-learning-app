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
    let html = root.join("report.html");
    let generated = run_wf(&["test","run","--performance","--db",merged.to_str().unwrap(),"--report",html.to_str().unwrap()], &RunOpts::default());
    assert_eq!(generated.exit_code, 0, "{}", generated.stderr);
    let counts: Value = serde_json::from_str(&generated.stdout).unwrap();
    assert_eq!((counts["runs"].as_u64(), counts["spans"].as_u64()), (Some(5), Some(4)));
    assert!(std::fs::read_to_string(html).unwrap().contains("Individual calls and spans"));
}

#[test]
fn given_a_normal_command_when_traced_then_stdout_and_failure_status_are_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("history.sqlite");
    let opts = RunOpts::default().cwd(tmp.path()).env("WF_PERFORMANCE_DB", db.to_str().unwrap());
    let plain = run_wf(&["--version"], &opts);
    let traced = run_wf(&["--performance", "--version"], &opts);
    assert_eq!((traced.exit_code, traced.stdout), (0, plain.stdout), "{}", traced.stderr);
    assert!(traced.stderr.contains("cli.help"), "{}", traced.stderr);
    assert_eq!(run_wf(&["--performance", "unknown-command"], &opts).exit_code, 1);
}

#[test]
fn given_a_catalog_baseline_when_calls_slow_change_outcome_or_disappear_then_the_gate_distinguishes_each_case() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wf-kit/src");
    let checked = std::process::Command::new("python3").arg("-c").arg(r#"
import sqlite3, sys
sys.path.insert(0, sys.argv[1])
from performance import SCHEMA
from performance_catalog import check
db = sqlite3.connect(':memory:')
db.executescript(SCHEMA)
entry = [('host.call','probe.call','host.call','')]
def measure(run, ms, outcomes, baseline=None):
    for i, outcome in enumerate(outcomes):
        key = run,'trace',str(i)
        db.execute('INSERT INTO spans VALUES(?,?,?,?,?,?,?)', (*key,None,'probe.call',0,int(ms*1e6)))
        db.execute('INSERT INTO span_results VALUES(?,?,?,?,?)', (*key,outcome,None))
    passed = check(db,run,(baseline,) if baseline else None,entry)
    return passed, db.execute('SELECT verdict FROM catalog WHERE run_id=?',(run,)).fetchone()[0]
assert measure('base',2,['ok']*5) == (True,'No comparable baseline')
assert measure('same',6,['ok']*5,'base') == (True,'Within limit')
assert measure('slow',8,['ok']*5,'base') == (False,'Exceeded')
assert measure('different',8,['refused']*5,'base') == (False,'Fewer successful calls')
assert measure('missing',2,['ok']*4,'base') == (False,'Incomplete')
"#).arg(source).output().unwrap();
    assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
}
