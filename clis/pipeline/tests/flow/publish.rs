//! `wf test publish` as the CI test pipeline serves it: a run becomes a local bundle folder (the workflow uploads it as an Actions
//! artifact) and a metrics row. The ORDS table is a mock HTTP service.
use crate::support::{last_json, MockReply, MockServer, SlotSandbox};
use serde_json::{json, Value};

fn run_report(dir: &std::path::Path) -> Value {
    json!({ "schema": 1, "verdict": "fail", "ok": false, "exitCode": 1, "message": "t.lua:2: boom", "script": "e2e/typing.lua", "platform": "android", "device": "rdl", "screen": "480x1040@213",
            "sha": "abc1234", "speed": 2.0, "startedMs": 1792000000000i64, "finishedMs": 1792000012000i64, "finished": "2026-10-16T10:00:12.000Z", "durationS": 12.0, "recordDir": dir.display().to_string(),
            "args": { "chunk": "2" }, "steps": [{}, {}, {}], "screenshots": [{}, {}], "recordings": [{}], "goldens": [{ "ok": true }, { "ok": false }], "warnings": ["a JS warning"], "debugHits": [], "notes": [],
            "timing": { "wallS": 12.0, "deviceS": 1.5, "phases": { "setup": 2.0, "script": 9.0 }, "byVerb": [{ "verb": "tap", "calls": 2, "s": 1.5, "p95Ms": 900 }], "timeline": [{ "kind": "call", "name": "tap" }] } })
}

/// The JSON of a bundle file under `out/<runId>/`.
fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

#[test]
fn a_run_is_published_as_a_local_bundle_and_a_metrics_row() {
    // Given a failed android run dir with evidence three folders deep and files that are not evidence
    let s = SlotSandbox::new("wf-flow-publish-");
    let dir = s.repo.join("runs/t-1");
    s.file("runs/t-1/report.json", run_report(&dir).to_string());
    for (f, body) in [("report.html", "<html>r</html>"), ("run.mp4", "mp4"), ("out/step.png", "png"), ("out/a/b/deep.png", "png"), ("out/a/b/c/too-deep.png", "png"), ("scratch.bin", "no")] {
        s.file(&format!("runs/t-1/{f}"), body);
    }
    // and an output folder whose index already lists an older run, and an ORDS table that takes a row
    s.file("bundles/index.json", json!({ "schema": 1, "runs": [{ "runId": "run-older" }] }).to_string());
    let out_dir = s.repo.join("bundles");
    let ords = MockServer::start(None);
    ords.queue(vec![MockReply::json(201, json!({}))], false);
    let table = format!("http://127.0.0.1:{}/ords/health_rpt/test_runs/", ords.port);
    let env = [("TEST_RUNS_ORDS_BASE_URL", table.as_str()), ("HEALTH_REPORTS_ORDS_PASSWORD", "pw"), ("GITHUB_SERVER_URL", "https://github.com"), ("GITHUB_REPOSITORY", "o/r"), ("GITHUB_RUN_ID", "77")];
    let path = dir.display().to_string();
    let out_arg = out_dir.display().to_string();

    // When it is published
    let r = s.run(&["test", "publish", &path, "--ran-on", "sandbox", "--out", &out_arg], &env);
    // Then the summary names the bundle folder and the run page, and the folder holds the evidence, bundle.json and nothing else
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    let run_id = out["runId"].as_str().expect("a run id").to_string();
    assert!(run_id.starts_with("run-"), "{out}");
    let folder = out_dir.join(&run_id);
    assert_eq!((out["files"].clone(), out["base"].clone(), out["bundle"].clone(), out["report"].clone()), (json!(5), json!("https://github.com/o/r/actions/runs/77"), json!(format!("{}/bundle.json", folder.display())), json!("report.html")), "{out}");
    let mut written: Vec<String> = Vec::new();
    let mut stack = vec![folder.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            if e.path().is_dir() { stack.push(e.path()) } else { written.push(e.path().strip_prefix(&folder).unwrap().to_string_lossy().into_owned()) }
        }
    }
    written.sort();
    assert_eq!(written, ["bundle.json", "out/a/b/deep.png", "out/step.png", "report.html", "report.json", "run.mp4"]);
    // and the bundle carries the media, the run's facts and the check that the recording and the report were written
    let bundle = read_json(&folder.join("bundle.json"));
    assert_eq!((bundle["schema"].as_u64(), bundle["kind"].as_str(), bundle["verdict"].as_str()), (Some(1), Some("run"), Some("fail")));
    assert_eq!(bundle["recordings"], json!(["run.mp4"]));
    assert_eq!(bundle["screenshots"].as_array().unwrap().len(), 2);
    assert_eq!(bundle["files"].as_array().unwrap().iter().find(|f| f["path"] == "out/step.png").unwrap()["type"], "image/png");
    assert_eq!((bundle["screen"].as_str(), bundle["warnings"].as_u64(), bundle["timingSummary"]["wallS"].as_f64(), bundle["args"]["chunk"].as_str()), (Some("480x1040@213"), Some(1), Some(12.0), Some("2")));
    assert_eq!(bundle["speed"].as_f64(), Some(2.0));
    assert_eq!((bundle["readBack"]["ok"].as_bool(), bundle["readBack"]["checked"].as_u64()), (Some(true), Some(2)), "{}", bundle["readBack"]);
    // and the metrics row in the bundle is filled from the report, naming the run page and not the local one
    let m = &bundle["metrics"];
    assert_eq!(m.as_object().unwrap().len(), 27, "{m}");
    assert_eq!((m["harness"].as_str(), m["verdict"].as_str(), m["exit_code"].as_i64(), m["ran_on"].as_str(), m["sha"].as_str()), (Some("android"), Some("fail"), Some(1), Some("sandbox"), Some("abc1234")));
    assert_eq!((m["steps"].as_u64(), m["screenshots"].as_u64(), m["recordings"].as_u64(), m["goldens_total"].as_u64(), m["goldens_failed"].as_u64(), m["warnings"].as_u64(), m["debug_hits"].as_u64()), (Some(3), Some(2), Some(1), Some(2), Some(1), Some(1), Some(0)));
    assert_eq!((m["device_or_screen"].as_str(), m["ts"].as_str(), m["attempt"].as_u64(), m["note"].as_str()), (Some("rdl 480x1040@213"), Some("2026-10-16T10:00:12.000Z"), Some(1), Some("t.lua:2: boom")));
    assert_eq!(m["report_path"], format!("https://github.com/o/r/actions/runs/77#{run_id}/report.html"));
    let timing: Value = serde_json::from_str(m["timing_json"].as_str().unwrap()).unwrap();
    assert!(timing["byVerb"][0]["verb"] == "tap" && timing.get("timeline").is_none(), "{timing}");
    // and the index lists the run first, keeps the older one, and keeps the timing a chart reads without the timeline
    let index = read_json(&out_dir.join("index.json"));
    assert_eq!((index["runs"][0]["runId"].as_str(), index["runs"][1]["runId"].as_str()), (Some(run_id.as_str()), Some("run-older")));
    let t = &index["runs"][0]["timing"];
    assert_eq!((t["phases"]["setup"].as_f64(), t["verbs"][0]["verb"].as_str(), t["verbs"][0]["p95Ms"].as_u64()), (Some(2.0), Some("tap"), Some(900)), "{t}");
    assert!(t.get("timeline").is_none() && index["runs"][0]["recordings"] == 1, "{t}");
    // and the row was posted to the table with the stored credentials, the bundle named in its note
    assert_eq!(out["recorded"]["posted"], true, "{out}");
    let posted = ords.requests();
    assert_eq!((posted.len(), posted[0].method.as_str()), (1, "POST"));
    let row: Value = serde_json::from_str(&posted[0].body).unwrap();
    assert!(row["run_id"] == run_id.as_str() && row["note"].as_str().unwrap().starts_with(&format!("bundle {run_id}/bundle.json")), "{row}");
    assert!(posted[0].auth.as_deref().is_some_and(|a| a.starts_with("Basic ")));

    // When the same run is published again and the table already holds its run_id
    ords.clear();
    ords.queue(vec![MockReply::text(400, "ORA-00001: unique constraint (HEALTH_RPT.TEST_RUNS_RUN_ID) violated RUN_ID"), MockReply::json(200, json!({ "items": [{ "id": 41 }] })), MockReply::json(200, json!({})), MockReply::json(201, json!({}))], false);
    let again = s.run(&["test", "publish", &path, "--ran-on", "sandbox", "--out", &out_arg], &env);
    // Then the run is listed once and first, and its row is replaced: looked up by run_id, deleted by id, inserted again
    assert_eq!(again.exit_code, 0, "{}{}", again.stdout, again.stderr);
    let index = read_json(&out_dir.join("index.json"));
    assert_eq!(index["runs"].as_array().unwrap().iter().map(|r| r["runId"].as_str().unwrap()).collect::<Vec<_>>(), [run_id.as_str(), "run-older"]);
    let calls: Vec<String> = ords.requests().iter().map(|q| format!("{} {}", q.method, q.url.split('?').next().unwrap())).collect();
    assert_eq!(calls, ["POST /ords/health_rpt/test_runs/", "GET /ords/health_rpt/test_runs/", "DELETE /ords/health_rpt/test_runs/41", "POST /ords/health_rpt/test_runs/"]);
    assert_eq!(last_json(&again)["recorded"]["posted"], true);

    // When a flow report dir, a queue job dir and an integration suite's run record are published without a row, into the folder RUNNER_TEMP names
    s.file("integration/trace.json", json!({ "schema": "method-trace/1", "app": "t3", "suite": "t3-serve-integration", "complete": true, "tests": [] }).to_string());
    s.file("integration/report.json", json!({ "schema": 1, "verdict": "pass", "ok": true, "exitCode": 0, "script": "backend/vm-ops-api/integration", "platform": "integration",
        "sha": "abc1234", "startedMs": 1792000000000i64, "finishedMs": 1792000003000i64, "durationS": 3.0, "recordDir": "/out/vm-ops-api-integration",
        "cases": [{ "name": "integration::reads::health", "verdict": "pass", "ms": 40 }], "warnings": [] }).to_string());
    let flow = s.repo.join("flow");
    s.file("flow/report.json", json!({ "schema": "flow-report/1", "harness": "flow-test-harness", "verdict": "failed", "startedAt": "2026-10-14T10:00:00.000Z", "finishedAt": "2026-10-14T10:00:42.500Z", "durationMs": 42_500, "root": "/repo/frontend/mla-tamagui",
        "paths": { "html": ".flow-report/index.html" }, "summary": { "total": 3, "passed": 2, "failed": 1, "files": 1 }, "errors": [], "warnings": ["run warn"],
        "tests": [{ "file": "app/a.flow.test.tsx", "verdict": "passed", "errors": [], "goldens": [{}, {}], "stops": [{}], "warnings": [] }, { "file": "app/a.flow.test.tsx", "verdict": "failed", "errors": [{ "message": "expected Done" }], "goldens": [], "stops": [], "warnings": ["w1", "w2"] }] }).to_string());
    s.file("flow/index.html", "<html>flow</html>");
    s.file("job/result.json", json!({ "id": "abc-1", "sha": "def", "runbook": "device-lab-typing", "platform": "android", "verdict": "error", "exitCode": 1, "ranOn": "vm", "finishedMs": 61_000, "durationS": 61.5, "note": "the app did not start cold", "attempt": 2, "retryReason": "fail in abc-0: boom", "recording": ["/a.mp4"], "timing": { "wallS": 61.5, "byVerb": [] } }).to_string());
    s.file("job/rec/report.json", run_report(&s.repo.join("job/rec")).to_string());
    s.file("job/rec/report.html", "<html>job</html>");
    let runner_temp = s.repo.join("runner-temp").display().to_string();
    let env_ci = [("RUNNER_TEMP", runner_temp.as_str())];
    let published = |dir: &str| {
        let r = s.run(&["test", "publish", dir, "--no-record", "--ran-on", "x"], &env_ci);
        assert_eq!(r.exit_code, 0, "{dir}: {}{}", r.stdout, r.stderr);
        let out = last_json(&r);
        let bundle = read_json(&std::path::Path::new(out["dir"].as_str().unwrap()).join("bundle.json"));
        (out, bundle)
    };
    let (flow_out, flow_bundle) = published(&flow.display().to_string());
    let (job_out, job_bundle) = published(&s.repo.join("job").display().to_string());
    let (_, it_bundle) = published(&s.repo.join("integration").display().to_string());
    // Then the integration record is a run of the integration harness, and its bundle lists the chain's method-hit trace and the run's other bundles none
    assert_eq!(it_bundle["traces"], json!(["trace.json"]), "{it_bundle}");
    assert!(flow_bundle["traces"] == json!([]) && job_bundle["traces"] == json!([]), "{flow_bundle} {job_bundle}");
    let m = &it_bundle["metrics"];
    assert_eq!((it_bundle["kind"].as_str(), m["harness"].as_str(), m["verdict"].as_str(), m["suite_or_runbook"].as_str()), (Some("run"), Some("integration"), Some("pass"), Some("backend/vm-ops-api/integration")), "{it_bundle}");
    // and all three bundles sit under RUNNER_TEMP/test-runs, listed in one index
    let ci_index = read_json(&s.repo.join("runner-temp/test-runs/index.json"));
    assert_eq!(ci_index["runs"].as_array().unwrap().len(), 3, "{ci_index}");
    // and the flow row counts tests, goldens, stops and warnings, and the job row lets the job decide identity, verdict and note
    assert_eq!((flow_out["recorded"]["posted"].clone(), flow_bundle["kind"].as_str()), (json!(false), Some("flow")));
    let m = &flow_bundle["metrics"];
    assert_eq!((m["harness"].as_str(), m["verdict"].as_str(), m["duration_s"].as_f64(), m["tests_total"].as_u64(), m["tests_failed"].as_u64()), (Some("flow"), Some("fail"), Some(42.5), Some(3), Some(1)));
    assert_eq!((m["goldens_total"].as_u64(), m["debug_hits"].as_u64(), m["warnings"].as_u64(), m["suite_or_runbook"].as_str(), m["note"].as_str()), (Some(2), Some(1), Some(3), Some("app/a.flow.test.tsx"), Some("expected Done")));
    assert!(flow_bundle["runId"].as_str().unwrap().starts_with("flow-") && flow_bundle["report"]["html"].as_str().unwrap() == "index.html", "{flow_bundle}");
    assert_eq!((job_out["runId"].as_str(), job_bundle["kind"].as_str()), (Some("job-abc-1"), Some("job")));
    let m = &job_bundle["metrics"];
    assert_eq!((m["job_id"].as_str(), m["attempt"].as_u64(), m["verdict"].as_str(), m["metro_commit"].as_str(), m["duration_s"].as_f64()), (Some("abc-1"), Some(2), Some("error"), None, Some(61.5)));
    assert_eq!((m["suite_or_runbook"].as_str(), m["note"].as_str(), m["retry_reason"].as_str(), m["steps"].as_u64()), (Some("device-lab-typing"), Some("the app did not start cold"), Some("fail in abc-0: boom"), Some(3)));
    assert_eq!(job_bundle["report"]["html"], "rec/report.html", "{job_bundle}");

    // When the dir holds no run at all
    s.file("empty/notes.txt", "x");
    let not_a_run = s.run(&["test", "publish", &s.repo.join("empty").display().to_string()], &env_ci);
    // Then it fails with the cause and the next step
    assert_eq!(not_a_run.exit_code, 1, "{}{}", not_a_run.stdout, not_a_run.stderr);
    let v: Value = serde_json::from_str(not_a_run.stdout.trim()).unwrap();
    assert!(v["error"].as_str().unwrap_or("").contains("neither") && v["next"].as_str().is_some(), "{v}");
}

#[test]
fn a_bundle_carries_what_the_build_cache_did_since_the_last_publish_and_only_once() {
    // Given a run dir, and the stats of two shim runs that just stopped beside one an hour old, as `wf test run --stage` leaves them
    let s = SlotSandbox::new("wf-flow-publish-cache-");
    let dir = s.repo.join("runs/t-1");
    s.file("runs/t-1/report.json", run_report(&dir).to_string());
    s.file("runs/t-1/report.html", "<html>r</html>");
    s.file("runs/t-1/run.mp4", "mp4");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let timing = |n: u64, each_ms: f64| json!({ "n": n, "sumMs": n as f64 * each_ms, "maxMs": if n > 0 { each_ms } else { 0.0 } });
    let stats = |hits: u64, misses: u64, puts: u64, requests: u64, degraded: Value| {
        json!({ "hits": hits, "misses": misses, "puts": puts, "skipped": 0, "requests": requests, "bytes": hits * 100, "degraded": degraded,
                "latencyMs": { "get": timing(hits + misses, 10.0), "put": timing(puts, 30.0), "exists": timing(0, 0.0) } })
    };
    let run = |ago_ms: i64, stats: Value| json!({ "finishedMs": now - ago_ms, "stats": stats }).to_string();
    let lines = [run(3_600_000, stats(9, 9, 9, 9, json!(null))), run(120_000, stats(7, 1, 1, 9, json!(null))), run(30_000, stats(3, 0, 0, 3, json!("budget")))];
    s.file("cache/build-cache-runs.jsonl", lines.join("\n") + "\n");
    let cache = s.repo.join("cache").display().to_string();
    let bundles = s.repo.join("bundles").display().to_string();
    let env = [("WF_TEST_RUNS_DIR", bundles.as_str()), ("WF_CACHE_DIR", cache.as_str())];
    let publish = || {
        let r = s.run(&["test", "publish", &dir.display().to_string(), "--no-record", "--ran-on", "x"], &env);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        let out = last_json(&r);
        let bundle = read_json(&std::path::Path::new(out["dir"].as_str().unwrap()).join("bundle.json"));
        (out, bundle)
    };

    // When the run is published
    let (out, bundle) = publish();
    // Then the bundle and the summary add up the two recent runs (counters and store latency), the first degraded reason wins, and the old run is left out
    let latency = json!({
        "get": { "n": 11, "sumMs": 110.0, "avgMs": 10.0, "maxMs": 10.0 },
        "put": { "n": 1, "sumMs": 30.0, "avgMs": 30.0, "maxMs": 30.0 },
        "exists": { "n": 0, "sumMs": 0.0, "avgMs": 0.0, "maxMs": 0.0 },
    });
    let sum = json!({ "runs": 2, "hits": 10, "misses": 1, "puts": 1, "skipped": 0, "quotaExceeded": 0, "requests": 12, "bytes": 1000, "degraded": "budget", "latencyMs": latency });
    assert_eq!((bundle["cache"].clone(), out["cache"].clone()), (sum.clone(), sum));
    // And a second publish is not credited with the same runs, nor with the old one
    let (_, again) = publish();
    assert_eq!(again["cache"], Value::Null, "{again}");
    // And only the two runs it carried left the file
    assert_eq!(std::fs::read_to_string(s.repo.join("cache/build-cache-runs.jsonl")).unwrap().lines().count(), 1);
}
