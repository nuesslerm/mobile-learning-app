//! The state writes of plan section 3 ("Writes"): the cooldown ledger's versioned write and the opencode-log run writes
//! (start, lines, finish), each read back through the service's own routes and, for the ledger, from its file.
use crate::{expect_json, get, post, public, world};
use regex::Regex;
use serde_json::{json, Value};
use std::time::Duration;
use vm_ops_api_integration::{case, iso, now_ms, Reply, Service};

const EXPECTED_SHAPE: &str = "expected { ledger, version }";
/// A ledger as wf's cooldown mirror posts it, compact: keys out of order, a nested array, a fraction, an empty object.
const LEDGER: &str = r#"{"minimax/MiniMax-M3":{"until":1700000000000,"reasons":["cap",429],"backoff":0.5},"empty":{},"none":[]}"#;
/// The same ledger as the service stores it: `JSON.stringify(ledger, null, 2)` and a newline.
const LEDGER_FILE: &str = r#"{
  "minimax/MiniMax-M3": {
    "until": 1700000000000,
    "reasons": [
      "cap",
      429
    ],
    "backoff": 0.5
  },
  "empty": {},
  "none": []
}
"#;

fn cooldowns(ledger: &str, version: &str) -> String {
    format!(r#"{{"ledger":{ledger},"version":{version}}}"#)
}

fn ledger_file(svc: &Service) -> Option<String> {
    std::fs::read_to_string(svc.world().dir().join("state/pool-cooldowns.json")).ok()
}

case!(cooldowns_write_bumps_the_version_and_pretty_prints_the_ledger, ["POST /cooldowns"], {
    // Given the service, with no cooldown file and the version at 0
    let svc = world().start();
    // When a ledger is posted at version 0
    let r = post(&svc, "/cooldowns", cooldowns(LEDGER, "0").as_bytes());
    // Then it is 200 at version 1, the file is the ledger pretty-printed with a trailing newline, and a read gives both
    expect_json(&r, 200, &json!({ "version": 1 }));
    assert_eq!(ledger_file(&svc).as_deref(), Some(LEDGER_FILE));
    let read = get(&svc, "/cooldowns");
    assert_eq!((read.status, read.body.as_str()), (200, cooldowns(LEDGER, "1").as_str()));
    // When another ledger is posted at the old version 0, Then it is 409 stale with the current version, and the file stays
    expect_json(&post(&svc, "/cooldowns", cooldowns("{}", "0").as_bytes()), 409, &json!({ "error": "stale", "version": 1 }));
    assert_eq!(ledger_file(&svc).as_deref(), Some(LEDGER_FILE));
    // When it is posted at version 1, Then it is 200 at version 2 and the file is the empty ledger
    expect_json(&post(&svc, "/cooldowns", cooldowns("{}", "1").as_bytes()), 200, &json!({ "version": 2 }));
    assert_eq!(ledger_file(&svc).as_deref(), Some("{}\n"));
});

case!(cooldowns_write_refuses_a_bad_shape_invalid_json_and_an_empty_body, ["POST /cooldowns"], {
    // Given the service, with no cooldown file
    let svc = world().start();
    // When the body is no JSON, Then it is 400 invalid JSON body
    expect_json(&post(&svc, "/cooldowns", b"{oops"), 400, &json!({ "error": "invalid JSON body" }));
    // When the body is empty, null, an array, or an object whose ledger is missing, an array, null or a string, or whose
    // version is missing, a string or a fraction, Then each is 400 expected { ledger, version }
    let bad = [
        "".to_string(),
        "null".into(),
        "[]".into(),
        r#"{"version":0}"#.into(),
        cooldowns("[]", "0"),
        cooldowns("null", "0"),
        cooldowns(r#""x""#, "0"),
        r#"{"ledger":{}}"#.into(),
        cooldowns("{}", r#""0""#),
        cooldowns("{}", "0.5"),
    ];
    for body in &bad {
        expect_json(&post(&svc, "/cooldowns", body.as_bytes()), 400, &json!({ "error": EXPECTED_SHAPE }));
    }
    // And nothing was written: no file, and the version is still 0
    assert_eq!(ledger_file(&svc), None);
    expect_json(&get(&svc, "/cooldowns"), 200, &json!({ "ledger": {}, "version": 0 }));
});

case!(cooldowns_write_takes_any_json_number_that_is_an_integer_as_the_version, ["POST /cooldowns"], {
    // Given the service, with no cooldown file and the version at 0
    let svc = world().start();
    // When the version is -1, an integer but not the current one, Then it is 409 stale at version 0 and nothing is written
    expect_json(&post(&svc, "/cooldowns", cooldowns("{}", "-1").as_bytes()), 409, &json!({ "error": "stale", "version": 0 }));
    assert_eq!(ledger_file(&svc), None);
    // When the version is 0.0, which JSON.parse reads as the integer 0, Then it is 200 at version 1 and the file is written
    expect_json(&post(&svc, "/cooldowns", cooldowns(LEDGER, "0.0").as_bytes()), 200, &json!({ "version": 1 }));
    assert_eq!(ledger_file(&svc).as_deref(), Some(LEDGER_FILE));
    // When the version is 1e0, the integer 1 in exponent form, Then it is 200 at version 2
    expect_json(&post(&svc, "/cooldowns", cooldowns("{}", "1e0").as_bytes()), 200, &json!({ "version": 2 }));
    assert_eq!(ledger_file(&svc).as_deref(), Some("{}\n"));
});

case!(cooldowns_write_that_cannot_persist_is_409_stale_and_logs_persist_failed, ["POST /cooldowns"], {
    // Given a cooldown file path that is a directory, so no write can land (the suite may run as root, so no chmod)
    let w = world();
    std::fs::create_dir_all(w.dir().join("state/pool-cooldowns.json")).expect("a dir where the ledger file goes");
    let svc = w.start();
    // When a ledger is posted at version 0
    let r = post(&svc, "/cooldowns", cooldowns(LEDGER, "0").as_bytes());
    // Then it is 409 stale at the unchanged version 0, and stdout has the persist_failed error with the cause
    expect_json(&r, 409, &json!({ "error": "stale", "version": 0 }));
    let event = svc.wait_event("cooldowns.persist_failed", Duration::from_secs(5));
    assert_eq!(event["level"], "error", "{event}");
    assert!(event["err"].as_str().is_some_and(|e| !e.is_empty()), "{event}");
    expect_json(&get(&svc, "/cooldowns"), 200, &json!({ "ledger": {}, "version": 0 }));
});

/// The run's detail, read with no auth as the app and the dashboard read it.
fn detail(svc: &Service, run: &str) -> Reply {
    public(svc, &format!("/opencode-log/api/runs/{run}"))
}

/// A time the service stamped: a JS ISO string no earlier than `from` and no later than now.
fn stamped(meta: &Value, key: &str, from: &str) -> String {
    let (t, to) = (meta[key].as_str().unwrap_or_default().to_string(), iso(now_ms()));
    let iso_time = Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$").unwrap();
    assert!(iso_time.is_match(&t) && from <= t.as_str() && t <= to, "{key} {t:?} is no JS ISO time in [{from}, {to}]: {meta}");
    t
}

/// Asserts a run write's reply: 200 `{ok:true}`.
fn written(r: &Reply) {
    expect_json(r, 200, &json!({ "ok": true }));
}

case!(opencode_log_writes_need_the_bearer, [
    "POST /opencode-log/<runId>/start",
    "POST /opencode-log/<runId>/line",
    "POST /opencode-log/<runId>/finish"
], {
    // Given the service
    let svc = world().start();
    // When a start, a line and a finish are posted with no auth
    let writes = [("start", r#"{"prompt":"p"}"#), ("line", r#"{"lines":["a"]}"#), ("finish", r#"{"ok":true}"#)];
    for (action, body) in writes {
        let target = format!("/opencode-log/run-1/{action}");
        let r = svc.raw("POST", &target, &[], Some(body.as_bytes())).unwrap_or_else(|e| panic!("{e}"));
        // Then each is 401
        expect_json(&r, 401, &json!({ "error": "unauthorized" }));
    }
    // And the run does not exist
    expect_json(&detail(&svc, "run-1"), 404, &json!({ "error": "run not found" }));
    assert!(!svc.world().dir().join("state/opencode-log/run-1").exists());
});

case!(opencode_log_start_lines_and_finish_read_back_through_the_public_routes, [
    "POST /opencode-log/<runId>/start",
    "POST /opencode-log/<runId>/line",
    "POST /opencode-log/<runId>/finish"
], {
    // Given the service
    let svc = world().start();
    let from = iso(now_ms());
    // When a run starts with wf's start shape (runId, prompt, caller, claudeSessionId, model) and two line posts follow,
    // one with non-strings among its lines
    let start = r#"{"runId":"run-1","prompt":"Fix the login bug","caller":"implementer","claudeSessionId":"session-abc","model":"opencode/minimax-m3"}"#;
    written(&post(&svc, "/opencode-log/run-1/start", start.as_bytes()));
    written(&post(&svc, "/opencode-log/run-1/line", br#"{"lines":["first","second"]}"#));
    written(&post(&svc, "/opencode-log/run-1/line", br#"{"lines":["third",5,null,{"a":1}]}"#));
    // And a line post whose lines are no list, and one with none
    written(&post(&svc, "/opencode-log/run-1/line", br#"{"lines":"fourth"}"#));
    written(&post(&svc, "/opencode-log/run-1/line", b"{}"));
    // Then the run reads back running: agent is the caller, both caller fields kept, only the string lines stored
    let r = detail(&svc, "run-1");
    let started = stamped(&r.json(), "startedAt", &from);
    let running = json!({
        "runId": "run-1", "prompt": "Fix the login bug", "agent": "implementer", "caller": "implementer",
        "claudeSessionId": "session-abc", "model": "opencode/minimax-m3", "startedAt": started, "finishedAt": null,
        "status": "running", "ok": null, "tokens": null, "cost": null, "durationMs": null,
    });
    let mut with_lines = running.clone();
    with_lines["lines"] = json!(["first", "second", "third"]);
    expect_json(&r, 200, &with_lines);
    // When it finishes with wf's finish shape and no model
    let finish = r#"{"ok":true,"tokens":{"total":1500,"input":1200,"output":300},"cost":0.0123,"durationMs":4567}"#;
    written(&post(&svc, "/opencode-log/run-1/finish", finish.as_bytes()));
    // Then the run reads back finished in the same key order, with its start's model and claudeSessionId kept
    let r = detail(&svc, "run-1");
    let finished_at = stamped(&r.json(), "finishedAt", &started);
    let mut finished = running.clone();
    for (key, value) in [
        ("finishedAt", json!(finished_at)),
        ("status", json!("finished")),
        ("ok", json!(true)),
        ("tokens", json!({ "total": 1500, "input": 1200, "output": 300 })),
        ("cost", json!(0.0123)),
        ("durationMs", json!(4567)),
    ] {
        finished[key] = value;
    }
    let mut with_lines = finished.clone();
    with_lines["lines"] = json!(["first", "second", "third"]);
    expect_json(&r, 200, &with_lines);
    // And the runs list has it without its lines
    expect_json(&public(&svc, "/opencode-log/api/runs"), 200, &json!({ "runs": [finished] }));
});

case!(opencode_log_start_takes_agent_from_caller_and_nulls_non_strings, ["POST /opencode-log/<runId>/start"], {
    // Given the service
    let svc = world().start();
    let from = iso(now_ms());
    // When runs start with only an agent, with a caller and an agent, with every field the wrong type, and with no body
    let starts = [
        ("run-a", r#"{"prompt":"Find the docs","agent":"researcher"}"#, ("Find the docs", json!("researcher"), json!(null))),
        ("run-b", r#"{"prompt":"p","caller":"implementer","agent":"researcher"}"#, ("p", json!("implementer"), json!("implementer"))),
        ("run-c", r#"{"prompt":5,"caller":7,"agent":true,"claudeSessionId":{},"model":["m"]}"#, ("", json!(null), json!(null))),
        ("run-d", "", ("", json!(null), json!(null))),
    ];
    for (run, body, (prompt, agent, caller)) in starts {
        written(&post(&svc, &format!("/opencode-log/{run}/start"), body.as_bytes()));
        // Then agent is the caller, else the agent; a non-string prompt is '' and every other non-string is null
        let r = detail(&svc, run);
        let meta = json!({
            "runId": run, "prompt": prompt, "agent": agent, "caller": caller, "claudeSessionId": null, "model": null,
            "startedAt": stamped(&r.json(), "startedAt", &from), "finishedAt": null, "status": "running", "ok": null,
            "tokens": null, "cost": null, "durationMs": null, "lines": [],
        });
        expect_json(&r, 200, &meta);
    }
});

case!(opencode_log_finish_without_a_start_writes_the_stub_and_coerces_its_fields, ["POST /opencode-log/<runId>/finish"], {
    // Given the service, with no run started
    let svc = world().start();
    let from = iso(now_ms());
    // When runs finish that never started: with a model, a non-boolean ok, token counts as text, a missing one and a
    // boolean, a cost as text and a fractional duration; with ok false and tokens as an array; with tokens as text; with
    // counts JS Number() reads leniently (padded, hex, a one-element array); and with counts it reads as Infinity or NaN
    // and a null
    let finishes = [
        (
            "run-x",
            r#"{"ok":"yes","model":"opencode/model-b","tokens":{"total":"120","output":true},"cost":"1","durationMs":12.5}"#,
            (json!("opencode/model-b"), json!(null), json!({ "total": 120, "input": 0, "output": 1 }), json!(12.5)),
        ),
        ("run-y", r#"{"ok":false,"tokens":[7]}"#, (json!(null), json!(false), json!({ "total": 0, "input": 0, "output": 0 }), json!(null))),
        ("run-z", r#"{"tokens":"5"}"#, (json!(null), json!(null), json!(null), json!(null))),
        (
            "run-u",
            r#"{"tokens":{"total":" 12 ","input":"0x10","output":[5]}}"#,
            (json!(null), json!(null), json!({ "total": 12, "input": 16, "output": 5 }), json!(null)),
        ),
        (
            "run-v",
            r#"{"tokens":{"total":"1e400","input":null,"output":"12abc"}}"#,
            (json!(null), json!(null), json!({ "total": null, "input": 0, "output": 0 }), json!(null)),
        ),
    ];
    for (run, body, (model, ok, tokens, duration)) in finishes {
        written(&post(&svc, &format!("/opencode-log/{run}/finish"), body.as_bytes()));
        // Then the stub has exactly runId, prompt '', agent and model, and startedAt, in that order (no caller, no
        // claudeSessionId), then the finish fields: ok only a boolean, tokens each `Number(x) || 0` (Infinity stored as
        // JSON.stringify prints it, null), cost and duration only numbers
        let r = detail(&svc, run);
        let started = stamped(&r.json(), "startedAt", &from);
        let meta = json!({
            "runId": run, "prompt": "", "agent": null, "model": model, "startedAt": started,
            "finishedAt": stamped(&r.json(), "finishedAt", &started), "status": "finished", "ok": ok, "tokens": tokens,
            "cost": null, "durationMs": duration, "lines": [],
        });
        expect_json(&r, 200, &meta);
    }
});

case!(opencode_log_writes_refuse_invalid_json_and_a_bad_run_id, [
    "POST /opencode-log/<runId>/start",
    "POST /opencode-log/<runId>/line",
    "POST /opencode-log/<runId>/finish"
], {
    // Given the service
    let svc = world().start();
    for action in ["start", "line", "finish"] {
        // When the body is no JSON, Then each write is 400 invalid JSON body
        let r = post(&svc, &format!("/opencode-log/run-1/{action}"), b"{oops");
        expect_json(&r, 400, &json!({ "error": "invalid JSON body" }));
        // When the run id is no run id, Then no write route matches: 404
        let r = post(&svc, &format!("/opencode-log/bad_id!/{action}"), b"{}");
        expect_json(&r, 404, &json!({ "error": "not found" }));
    }
    // And no run was written: neither run has a dir, whether or not the store made its own
    expect_json(&detail(&svc, "run-1"), 404, &json!({ "error": "run not found" }));
    for run in ["run-1", "bad_id!"] {
        assert!(!svc.world().dir().join("state/opencode-log").join(run).exists(), "{run} has a run dir");
    }
});
