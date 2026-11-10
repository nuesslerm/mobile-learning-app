//! The reads of plan section 3 ("Reads", all but `/redeploy/status`): health, ps, stats, logs (snapshot
//! and follow), the agent-shell reads (exec-script status, vm-ledger, artifact), the state files and the opencode-log
//! dashboard and runs. Every Engine answer comes from the fake Engine's script, every host file from the case's dir.
use crate::{
    engine_calls, exec_creates, expect_json, expect_not_running, get, public, sent, state_file, with_engine, world, AGENT_SHELL,
    EXITED_AGENT_SHELL, JOB, NO_AGENT_SHELL,
};
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;
use vm_ops_api_integration::{assert_golden, auth, case, frame, now_ms, Exec, Logs, GIT_COMMIT, HOST_LABEL, IMAGE_TAG};

const KNOWN_APPS: [&str; 10] = [
    "leetcode-backend",
    "remote-chat-agent",
    "leetcode-auth-cookie-capture",
    "agent-shell",
    "ci-runner",
    "vnc-auth-session",
    "reporting-service",
    "libs-proxy",
    "t3-serve",
    "orca-serve",
];
/// leetcode-backend's one container.
const LEETCODE_BACKEND: &str =
    r#"[{"Id":"l1","Names":["/leetcode-backend"],"State":"running","Status":"Up 1 hour","Image":"ns/leetcode-backend:sha-1"}]"#;
/// The exec-script status script, with `\n` as the two characters the shell's printf reads.
const STATUS_SCRIPT: &str = r#"cat "$1/output.log" 2>/dev/null; printf "\n---EXITCODE---\n"; cat "$1/exit-code" 2>/dev/null"#;
const MARKER: &str = "\n---EXITCODE---\n";
const BAD_ARTIFACT_PATH: &str = r#"path must be a relative path (no "..", only [A-Za-z0-9_-./]) under ~/e2e-artifacts"#;

case!(a_signed_health_read_reports_the_build_from_env, ["GET /health"], {
    // Given the service started with the test seed and public key, a commit, a host label and an image tag
    let svc = world().start();
    // When /health is read with today's rotating token and a signature, as wf reads it
    let r = svc.signed("GET", "/health", None);
    // Then it answers 200 with exactly the eight keys in order, commit, host and imageTag from env, ok and debugMode as
    // JSON booleans, both times as JS ISO strings and the script hash as 12 hex characters
    assert_eq!(r.status, 200, "{}", r.body);
    let body = r.json();
    let keys: Vec<&str> = body.as_object().expect("a JSON object").keys().map(String::as_str).collect();
    assert_eq!(keys, ["ok", "commit", "startedAt", "host", "debugMode", "time", "serverScriptSha", "imageTag"]);
    assert_eq!(
        (&body["ok"], &body["commit"], &body["host"], &body["debugMode"], &body["imageTag"]),
        (&json!(true), &json!(GIT_COMMIT), &json!(HOST_LABEL), &json!(false), &json!(IMAGE_TAG)),
        "{body}"
    );
    let iso = Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$").unwrap();
    for key in ["startedAt", "time"] {
        assert!(body[key].as_str().is_some_and(|t| iso.is_match(t)), "{key} is not a JS ISO time: {body}");
    }
    let sha = Regex::new(r"^[0-9a-f]{12}$").unwrap();
    assert!(body["serverScriptSha"].as_str().is_some_and(|s| sha.is_match(s)), "serverScriptSha: {body}");
});

case!(ps_lists_the_apps_containers_by_exact_name_in_any_state, ["GET /services/:app/ps"], {
    // Given an Engine listing remote-chat-agent's app container (running), its tailscale sidecar (exited), its opencode
    // container (created, with no Status), a decoy whose name only starts with the app's, and another app's container
    let w = with_engine(
        r#"[
        {"Id":"r1","Names":["/remote-chat-agent"],"State":"running","Status":"Up 2 hours","Image":"ns/remote-chat-agent:sha-1"},
        {"Id":"r2","Names":["/remote-chat-agent-old"],"State":"running","Status":"Up 9 days","Image":"ns/remote-chat-agent:old"},
        {"Id":"r3","Names":["/remote-chat-agent-tailscale"],"State":"exited","Status":"Exited (0) 3 hours ago","Image":"tailscale/tailscale:v1"},
        {"Id":"r4","Names":["/remote-chat-agent-opencode"],"State":"created","Image":"ns/opencode:sha-1"},
        {"Id":"l1","Names":["/leetcode-backend"],"State":"running","Status":"Up 1 hour","Image":"ns/leetcode-backend:sha-1"}
    ]"#,
    );
    let svc = w.start();
    // When remote-chat-agent's ps is read
    let r = get(&svc, "/services/remote-chat-agent/ps");
    // Then it lists the three containers of its exact names in the Engine's order, whatever their state, with names
    // without the leading slash and no status key for the one with no Status; the decoy is filtered out
    let expected = json!({ "app": "remote-chat-agent", "containers": [
        { "name": "remote-chat-agent", "state": "running", "status": "Up 2 hours", "image": "ns/remote-chat-agent:sha-1" },
        { "name": "remote-chat-agent-tailscale", "state": "exited", "status": "Exited (0) 3 hours ago", "image": "tailscale/tailscale:v1" },
        { "name": "remote-chat-agent-opencode", "state": "created", "image": "ns/opencode:sha-1" },
    ]});
    expect_json(&r, 200, &expected);
    // And the service listed every container, stopped ones included
    assert_eq!(engine_calls(&svc), ["GET /containers/json?all=true"]);
});

case!(ps_of_vm_ops_api_metro_runner_or_an_unknown_app_is_404_with_the_six_known_apps, ["GET /services/:app/ps"], {
    // Given an Engine listing agent-shell
    let svc = with_engine(AGENT_SHELL).start();
    // When ps is read for vm-ops-api, which is no managed app, for metro-runner, which was removed, and for an unknown app
    // Then each is 404 unknown app with the seven known apps in order, and the Engine is never asked
    for app in ["vm-ops-api", "metro-runner", "nope"] {
        let r = get(&svc, &format!("/services/{app}/ps"));
        expect_json(&r, 404, &json!({ "error": "unknown app", "knownApps": KNOWN_APPS }));
    }
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
});

case!(stats_reports_the_host_and_each_running_containers_working_set, ["GET /stats"], {
    // Given the fake /proc (12,000,000 kB total, 6,000,000 kB available, 2,097,148 kB swap, load 0.52 0.58 0.59) and an
    // Engine with four running containers and an exited one: leetcode-backend (usage minus inactive_file, a 512 MiB
    // limit, 25% of 4 CPUs), agent-shell (cgroup v1's total_inactive_file, a limit equal to the host's memory, 1.234% of
    // 2 CPUs), watchdog (only usage and the v1 cache), and remote-chat-agent, whose stats call answers 500
    let w = with_engine(
        r#"[
        {"Id":"s1","Names":["/leetcode-backend"],"State":"running","Status":"Up 1 hour","Image":"ns/leetcode-backend:sha-1"},
        {"Id":"s2","Names":["/agent-shell"],"State":"running","Status":"Up 1 hour","Image":"ns/agent-shell:sha-1"},
        {"Id":"s3","Names":["/remote-chat-agent"],"State":"running","Status":"Up 1 hour","Image":"ns/remote-chat-agent:sha-1"},
        {"Id":"s4","Names":["/reporting-service"],"State":"exited","Status":"Exited (1) 1 hour ago","Image":"ns/reporting-service:sha-1"},
        {"Id":"s5","Names":["/watchdog"],"State":"running","Status":"Up 1 hour","Image":"ns/watchdog:sha-1"}
    ]"#,
    );
    let engine = w.fake_engine();
    engine.stats(
        "s1",
        r#"{"memory_stats":{"usage":315172800,"limit":536870912,"stats":{"inactive_file":104857600}},
            "cpu_stats":{"cpu_usage":{"total_usage":125000000},"system_cpu_usage":2200000000,"online_cpus":4},
            "precpu_stats":{"cpu_usage":{"total_usage":100000000},"system_cpu_usage":2000000000}}"#,
    );
    engine.stats(
        "s2",
        r#"{"memory_stats":{"usage":1073741824,"limit":12288000000,"stats":{"total_inactive_file":73741824}},
            "cpu_stats":{"cpu_usage":{"total_usage":101234},"system_cpu_usage":300000,"online_cpus":2},
            "precpu_stats":{"cpu_usage":{"total_usage":100000},"system_cpu_usage":200000}}"#,
    );
    engine.fail("/containers/s3/stats", 500, r#"{"message":"stats failed"}"#);
    engine.stats("s5", r#"{"memory_stats":{"usage":62914560,"stats":{"cache":10485760}}}"#);
    let svc = w.start();
    // When /stats is read
    let r = get(&svc, "/stats");
    // Then the host numbers are rounded MB and the load's first three fields; the containers are sorted by memMb, the
    // limit is null at the host's memory, cpuPct has one decimal and prints as JS prints it (50, not 50.0), the failing
    // container is left out and containersMb is the sum
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(
        r.body,
        concat!(
            r#"{"host":{"memTotalMb":11719,"memAvailableMb":5859,"swapTotalMb":2048,"load":[0.52,0.58,0.59]},"containers":["#,
            r#"{"name":"agent-shell","memMb":954,"memLimitMb":null,"cpuPct":2.5},"#,
            r#"{"name":"leetcode-backend","memMb":201,"memLimitMb":512,"cpuPct":50},"#,
            r#"{"name":"watchdog","memMb":50,"memLimitMb":null,"cpuPct":0}],"containersMb":1205}"#
        )
    );
    // And the service listed only running containers, and asked each of them, never the exited one, for one sample
    let mut calls = engine_calls(&svc);
    assert_eq!(calls.remove(0), "GET /containers/json");
    calls.sort();
    let sampled = ["s1", "s2", "s3", "s5"].map(|id| format!("GET /containers/{id}/stats?stream=false"));
    assert_eq!(calls, sampled);
});

case!(stats_reads_a_missing_loadavg_as_0_and_a_failing_list_as_no_containers, ["GET /stats"], {
    // Given the fake /proc with no loadavg, and an Engine whose list answers 500
    let mut w = world();
    w.engine(500, r#"{"message":"engine down"}"#).proc_file("loadavg", None);
    let svc = w.start();
    // When /stats is read
    let r = get(&svc, "/stats");
    // Then the load is [0] and there are no containers: the list's failure is not the handler's
    let expected = json!({
        "host": { "memTotalMb": 11719, "memAvailableMb": 5859, "swapTotalMb": 2048, "load": [0] },
        "containers": [],
        "containersMb": 0,
    });
    expect_json(&r, 200, &expected);
});

case!(stats_without_an_engine_to_reach_is_502, ["GET /stats"], {
    // Given no Engine on the world's DOCKER_SOCKET
    let svc = world().start();
    // When /stats is read
    let r = get(&svc, "/stats");
    // Then the handler fails: 502 request failed, with the connection's error as the detail
    assert_eq!((r.status, &r.json()["error"]), (502, &json!("request failed")), "{}", r.body);
    assert!(r.json()["detail"].as_str().is_some_and(|d| !d.is_empty()), "{}", r.body);
});

case!(logs_snapshot_reads_each_container_and_names_one_that_fails, ["GET /services/:app/logs"], {
    // Given leetcode-backend's container with a stdout and a stderr frame, and its tailscale sidecar whose logs answer 500
    let w = with_engine(
        r#"[
        {"Id":"l1","Names":["/leetcode-backend"],"State":"running","Status":"Up 1 hour","Image":"ns/leetcode-backend:sha-1"},
        {"Id":"l2","Names":["/leetcode-backend-tailscale"],"State":"running","Status":"Up 1 hour","Image":"tailscale/tailscale:v1"}
    ]"#,
    );
    w.fake_engine()
        .logs("l1", Logs::chunks(vec![[frame(1, b"started\n"), frame(2, b"warn: slow\n")].concat()]))
        .fail("/containers/l2/logs", 500, r#"{"message":"logs failed"}"#);
    let svc = w.start();
    // When the app's logs are read
    let r = get(&svc, "/services/leetcode-backend/logs");
    // Then each container's text is its frames joined, the failing one reads <docker API returned 500>, the tail is 200
    let logs = json!({ "leetcode-backend": "started\nwarn: slow\n", "leetcode-backend-tailscale": "<docker API returned 500>" });
    expect_json(&r, 200, &json!({ "app": "leetcode-backend", "tail": 200, "logs": logs }));
    // And the containers were read one after the other with stdout, stderr and the tail
    let expected = [
        "GET /containers/json?all=true",
        "GET /containers/l1/logs?stdout=true&stderr=true&tail=200",
        "GET /containers/l2/logs?stdout=true&stderr=true&tail=200",
    ];
    assert_eq!(engine_calls(&svc), expected);
});

case!(logs_tail_is_read_as_parse_int_and_capped, ["GET /services/:app/logs"], {
    // Given leetcode-backend's one container, on a service without DEBUG_MODE and one with DEBUG_MODE=true
    let plain = with_engine(LEETCODE_BACKEND);
    plain.fake_engine().logs("l1", Logs::text("ok\n"));
    let plain = plain.start();
    let mut debug = with_engine(LEETCODE_BACKEND);
    debug.set("DEBUG_MODE", "true").fake_engine().logs("l1", Logs::text("ok\n"));
    let debug = debug.start();
    // When the logs are read with each tail
    // Then abc, 0 and -5 fall back to 200, 5000 is capped at 1000 (10,000 with DEBUG_MODE), 12abc is 12, and the Engine
    // is asked for exactly that tail
    let cases = [(&plain, "abc", 200), (&plain, "0", 200), (&plain, "-5", 200), (&plain, "5000", 1000), (&plain, "12abc", 12)];
    let debug_cases = [(&debug, "10000", 10_000), (&debug, "20000", 10_000)];
    for (svc, tail, expected) in cases.into_iter().chain(debug_cases) {
        let r = get(svc, &format!("/services/leetcode-backend/logs?tail={tail}"));
        expect_json(&r, 200, &json!({ "app": "leetcode-backend", "tail": expected, "logs": { "leetcode-backend": "ok\n" } }));
        let asked = format!("GET /containers/l1/logs?stdout=true&stderr=true&tail={expected}");
        assert_eq!(engine_calls(svc).last(), Some(&asked), "tail={tail}");
    }
});

case!(logs_take_vm_ops_api_and_an_app_with_no_containers_is_404, ["GET /services/:app/logs"], {
    // Given an Engine listing vm-ops-api's own container and nothing of leetcode-backend
    let w = with_engine(
        r#"[{"Id":"v1","Names":["/vm-ops-api-ops-api-1"],"State":"running","Status":"Up 1 hour","Image":"ns/vm-ops-api:sha-1"}]"#,
    );
    w.fake_engine().logs("v1", Logs::text("listening\n"));
    let svc = w.start();
    // When vm-ops-api's logs are read, Then they are served though vm-ops-api is no managed app
    let r = get(&svc, "/services/vm-ops-api/logs");
    expect_json(&r, 200, &json!({ "app": "vm-ops-api", "tail": 200, "logs": { "vm-ops-api-ops-api-1": "listening\n" } }));
    // When leetcode-backend's logs are read, Then it is 404: no containers
    let r = get(&svc, "/services/leetcode-backend/logs");
    expect_json(&r, 404, &json!({ "error": "no containers found for this app (not running?)" }));
    // When the logs of an unknown app and of the removed metro-runner are read, Then each is 404 with the seven known apps
    // and vm-ops-api
    let mut known = KNOWN_APPS.to_vec();
    known.push("vm-ops-api");
    for app in ["nope", "metro-runner"] {
        expect_json(&get(&svc, &format!("/services/{app}/logs")), 404, &json!({ "error": "unknown app", "knownApps": known }));
    }
});

case!(logs_follow_streams_text_with_frames_rejoined_across_chunks, ["GET /services/:app/logs"], {
    // Given leetcode-backend's one container, whose follow stream cuts a frame's header across two chunks and a
    // three-byte character inside one frame across the next two
    let w = with_engine(LEETCODE_BACKEND);
    let wire = [frame(1, b"line one\n"), frame(2, "line two \u{20ac}\n".as_bytes())].concat();
    let euro = wire.len() - 3;
    let chunks = vec![wire[..5].to_vec(), wire[5..euro].to_vec(), wire[euro..].to_vec()];
    w.fake_engine().logs("l1", Logs::chunks(chunks));
    let svc = w.start();
    // When the logs are followed with follow=TRUE, and again with follow=1 and a tail
    for (target, tail) in [("/services/leetcode-backend/logs?follow=TRUE", 200), ("/services/leetcode-backend/logs?follow=1&tail=5", 5)] {
        let r = get(&svc, target);
        // Then it is 200 plain text naming the container, uncached, the frames rejoined into whole text, ended when the
        // Engine's stream ends
        assert_eq!((r.status, r.body.as_str()), (200, "line one\nline two \u{20ac}\n"), "{target}");
        let headers = (r.header("content-type"), r.header("cache-control"), r.header("x-container-name"));
        assert_eq!(headers, (Some("text/plain; charset=utf-8"), Some("no-cache"), Some("leetcode-backend")), "{target}");
        // And the Engine was asked to follow stdout and stderr from the tail
        let asked = format!("GET /containers/l1/logs?follow=true&stdout=true&stderr=true&tail={tail}");
        assert_eq!(engine_calls(&svc).last(), Some(&asked));
    }
});

case!(logs_follow_ends_the_text_with_the_engines_failure, ["GET /services/:app/logs"], {
    // Given remote-chat-agent's app container, whose logs answer 500, and its opencode container, whose follow stream
    // sends one frame and then drops the connection with the body unfinished
    let w = with_engine(
        r#"[
        {"Id":"r1","Names":["/remote-chat-agent"],"State":"running","Status":"Up 2 hours","Image":"ns/remote-chat-agent:sha-1"},
        {"Id":"r4","Names":["/remote-chat-agent-opencode"],"State":"running","Status":"Up 2 hours","Image":"ns/opencode:sha-1"}
    ]"#,
    );
    w.fake_engine()
        .fail("/containers/r1/logs", 500, r#"{"message":"logs failed"}"#)
        .logs("r4", Logs::text("partial\n").then_abort());
    let svc = w.start();
    // When the app container's logs are followed
    let r = get(&svc, "/services/remote-chat-agent/logs?follow=true&container=remote-chat-agent");
    // Then the headers went out as 200 before the Engine answered, and the text is the Engine's failure
    assert_eq!(r.header("x-container-name"), Some("remote-chat-agent"));
    let ended = "\n<log stream ended: docker API returned 500: {\"message\":\"logs failed\"}>\n";
    assert_eq!((r.status, r.body.as_str()), (200, ended));
    // When the opencode container's logs are followed
    let r = get(&svc, "/services/remote-chat-agent/logs?follow=true&container=remote-chat-agent-opencode");
    // Then the text is the frame that arrived, then one line that says the stream ended and why
    assert_eq!(r.status, 200, "{}", r.body);
    let dropped = Regex::new(r"^partial\n\n<log stream ended: [^\n]+>\n$").unwrap();
    assert!(dropped.is_match(&r.body), "{:?}", r.body);
});

case!(logs_follow_needs_one_container_by_name, ["GET /services/:app/logs"], {
    // Given remote-chat-agent with two containers
    let w = with_engine(
        r#"[
        {"Id":"r1","Names":["/remote-chat-agent"],"State":"running","Status":"Up 2 hours","Image":"ns/remote-chat-agent:sha-1"},
        {"Id":"r4","Names":["/remote-chat-agent-opencode"],"State":"running","Status":"Up 2 hours","Image":"ns/opencode:sha-1"}
    ]"#,
    );
    let svc = w.start();
    let names = json!(["remote-chat-agent", "remote-chat-agent-opencode"]);
    // When the logs are followed with no container, Then it is 400 with the names
    let r = get(&svc, "/services/remote-chat-agent/logs?follow=true");
    let ambiguous = "multiple containers for this app -- pass &container=<name> to pick one";
    expect_json(&r, 400, &json!({ "error": ambiguous, "containers": names }));
    // When they are followed with a container the app does not have, Then it is 404 with the names
    let r = get(&svc, "/services/remote-chat-agent/logs?follow=true&container=nope");
    expect_json(&r, 404, &json!({ "error": "no container with that name for this app", "containers": names }));
    // And no logs were asked of the Engine
    assert_eq!(engine_calls(&svc), ["GET /containers/json?all=true", "GET /containers/json?all=true"]);
});

case!(utf8_is_decoded_per_frame_so_a_character_cut_across_two_frames_is_replaced, [
    "GET /services/:app/logs",
    "GET /services/agent-shell/vm-ledger/<name>"
], {
    // Given a euro sign cut after its second byte across two frames, in a container's logs and in a ledger's cat output
    let cut = [frame(1, b"a\xe2\x82"), frame(1, b"\xacb")].concat();
    let w = with_engine(AGENT_SHELL);
    w.fake_engine().logs("as1", Logs::chunks(vec![cut[..11].to_vec(), cut[11..].to_vec()])).exec(
        Exec::exact(&["cat", "/workspace/vm-ledger/pool-cooldowns.json"]).frames(cut.clone(), 0),
    );
    let svc = w.start();
    // When the logs are read and followed, and the ledger is read
    let snapshot = get(&svc, "/services/agent-shell/logs?container=agent-shell");
    let followed = get(&svc, "/services/agent-shell/logs?follow=true&container=agent-shell");
    let ledger = get(&svc, "/services/agent-shell/vm-ledger/pool-cooldowns.json");
    // Then each frame is decoded alone: the two halves come back as two replacement characters
    let replaced = "a\u{fffd}\u{fffd}b";
    assert_eq!(snapshot.json()["logs"]["agent-shell"], json!(replaced), "{}", snapshot.body);
    assert_eq!(followed.body, replaced);
    assert_eq!(ledger.body, replaced);
});

case!(a_failing_engine_list_is_502_on_every_read_that_lists_containers, [
    "GET /services/:app/logs",
    "GET /services/agent-shell/exec-script/<jobId>",
    "GET /services/agent-shell/vm-ledger/<name>",
    "GET /services/agent-shell/artifact"
], {
    // Given an Engine whose list answers 500
    let mut w = world();
    w.engine(500, r#"{"message":"engine down"}"#);
    let svc = w.start();
    let failed = json!({ "error": "request failed", "detail": r#"docker API returned 500: {"message":"engine down"}"# });
    // When the logs, an exec-script status, a cancel, a ledger and an artifact are read
    // Then each is 502 request failed with the Engine's answer as the detail
    for target in [
        "/services/leetcode-backend/logs".to_string(),
        format!("/services/agent-shell/exec-script/{JOB}"),
        "/services/agent-shell/vm-ledger/pool-cooldowns.json".to_string(),
        "/services/agent-shell/artifact?path=a.txt".to_string(),
    ] {
        expect_json(&get(&svc, &target), 502, &failed);
    }
    let cancel = format!("/services/agent-shell/exec-script/{JOB}/cancel");
    let r = sent(&svc, "POST", &cancel, &auth("POST", &cancel, &now_ms().to_string(), b""));
    expect_json(&r, 502, &failed);
});

case!(vm_ledger_serves_both_ledgers_with_their_content_types, ["GET /services/agent-shell/vm-ledger/<name>"], {
    // Given agent-shell, where cat prints each ledger
    let runs = "{\"runId\":\"r1\"}\n{\"runId\":\"r2\"}\n";
    let w = with_engine(AGENT_SHELL);
    w.fake_engine()
        .exec(Exec::exact(&["cat", "/workspace/vm-ledger/opencode-dispatch-runs.jsonl"]).answer(runs, 0))
        .exec(Exec::exact(&["cat", "/workspace/vm-ledger/pool-cooldowns.json"]).answer("{\"pools\":{}}\n", 0));
    let svc = w.start();
    // When the dispatch-runs ledger is read
    let r = get(&svc, "/services/agent-shell/vm-ledger/opencode-dispatch-runs.jsonl");
    // Then it is 200 plain text with cat's output
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (200, runs, Some("text/plain; charset=utf-8")));
    // And the cat ran in agent-shell's app container, attached, with no Env: list, create, start, inspect
    let expected = [
        "GET /containers/json?all=true",
        "POST /containers/as1/exec",
        "POST /exec/exec-1/start",
        "GET /exec/exec-1/json",
    ];
    assert_eq!(engine_calls(&svc), expected);
    let cmd = json!(["cat", "/workspace/vm-ledger/opencode-dispatch-runs.jsonl"]);
    assert_eq!(exec_creates(&svc), [json!({ "AttachStdout": true, "AttachStderr": true, "Cmd": cmd })]);
    let start: Value = serde_json::from_slice(&svc.world().fake_engine().request_body(2)).expect("JSON");
    assert_eq!(start, json!({ "Detach": false }));
    // When the cooldowns ledger is read, Then it is 200 JSON with cat's output
    let r = get(&svc, "/services/agent-shell/vm-ledger/pool-cooldowns.json");
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (200, "{\"pools\":{}}\n", Some("application/json; charset=utf-8")));
});

case!(vm_ledger_refuses_another_name_before_any_exec_and_a_failing_cat_is_404, ["GET /services/agent-shell/vm-ledger/<name>"], {
    // Given agent-shell, where cat of the cooldowns ledger exits 1
    let w = with_engine(AGENT_SHELL);
    let missing = Exec::exact(&["cat", "/workspace/vm-ledger/pool-cooldowns.json"]).answer("cat: no such file\n", 1);
    w.fake_engine().exec(missing);
    let svc = w.start();
    // When a name that is no ledger is read, Then it is 404 unknown ledger and the Engine is never asked
    expect_json(&get(&svc, "/services/agent-shell/vm-ledger/secrets.env"), 404, &json!({ "error": "unknown ledger" }));
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
    // When the cooldowns ledger is read, Then it is 404 ledger not found
    expect_json(&get(&svc, "/services/agent-shell/vm-ledger/pool-cooldowns.json"), 404, &json!({ "error": "ledger not found" }));
});

case!(agent_shell_reads_are_503_without_a_running_agent_shell, [
    "GET /services/agent-shell/exec-script/<jobId>",
    "GET /services/agent-shell/vm-ledger/<name>",
    "GET /services/agent-shell/artifact"
], {
    let reads = [
        format!("/services/agent-shell/exec-script/{JOB}"),
        "/services/agent-shell/vm-ledger/pool-cooldowns.json".to_string(),
        "/services/agent-shell/artifact?path=a.txt".to_string(),
    ];
    // Given an Engine listing only agent-shell's tailscale sidecar and another app
    let svc = with_engine(NO_AGENT_SHELL).start();
    // When each agent-shell read is made, Then each is 503: the sidecar is never the app container
    for target in &reads {
        expect_not_running(&get(&svc, target));
    }
    // Given an Engine listing only an exited agent-shell
    let svc = with_engine(EXITED_AGENT_SHELL).start();
    // When each read is made, Then each is 503 too: an exited container is not the app container
    for target in &reads {
        expect_not_running(&get(&svc, target));
    }
});

case!(artifact_reads_the_file_as_base64_through_the_guard_script, ["GET /services/agent-shell/artifact"], {
    // Given agent-shell, where the guard script for runs/r1/shot.png prints its base64 with a newline
    let w = with_engine(AGENT_SHELL);
    let rule = Exec::prefix(&["bash", "-c"], r#"^F="/home/agent/e2e-artifacts/runs/r1/shot\.png"\n"#).answer("aGVsbG8=\n", 0);
    w.fake_engine().exec(rule);
    let svc = w.start();
    // When the artifact is read
    let r = get(&svc, "/services/agent-shell/artifact?path=runs/r1/shot.png");
    // Then it is 200 with the path and the trimmed base64
    expect_json(&r, 200, &json!({ "path": "runs/r1/shot.png", "base64": "aGVsbG8=" }));
    // And the exec ran the guard script Node runs, in agent-shell's app container
    let creates = exec_creates(&svc);
    assert_eq!(engine_calls(&svc)[1], "POST /containers/as1/exec");
    assert_golden("artifact-guard-cmd.json", &(serde_json::to_string_pretty(&creates[0]["Cmd"]).expect("JSON") + "\n"));
});

case!(artifact_refuses_a_bad_path_before_any_exec_and_a_missing_file_is_404, ["GET /services/agent-shell/artifact"], {
    // Given agent-shell, where the guard script exits 1 for missing.png, prints NOT_FOUND for gone.png, and prints
    // NOT_ALLOWED with exit 1 for link.png
    let w = with_engine(AGENT_SHELL);
    let guard = |file: &str| Exec::prefix(&["bash", "-c"], &format!(r#"^F="/home/agent/e2e-artifacts/{}"\n"#, regex::escape(file)));
    w.fake_engine()
        .exec(guard("missing.png").answer("", 1))
        .exec(guard("gone.png").answer("NOT_FOUND\n", 0))
        .exec(guard("link.png").answer("NOT_ALLOWED\n", 1));
    let svc = w.start();
    // When the path is missing, empty, holds .., or holds a character outside [A-Za-z0-9_-./]
    // Then each is 400 with the same message and the Engine is never asked
    let refused = json!({ "error": BAD_ARTIFACT_PATH });
    for query in ["", "?path=", "?path=a/../b", "?path=a%20b", "?path=a;b"] {
        expect_json(&get(&svc, &format!("/services/agent-shell/artifact{query}")), 400, &refused);
    }
    assert_eq!(engine_calls(&svc), Vec::<String>::new());
    // When the guard exits 1, prints NOT_FOUND, or prints NOT_ALLOWED and exits 1
    // Then each is 404 artifact not found with the path: the exit code is checked before NOT_ALLOWED
    for file in ["missing.png", "gone.png", "link.png"] {
        let r = get(&svc, &format!("/services/agent-shell/artifact?path={file}"));
        expect_json(&r, 404, &json!({ "error": "artifact not found", "path": file }));
    }
});

case!(exec_script_status_splits_at_the_last_marker_and_polls_to_the_exit_code, [
    "GET /services/agent-shell/exec-script/<jobId>"
], {
    // Given agent-shell, where the job's status prints its output and no exit code yet, then the output (which holds the
    // marker itself) and exit code 0
    let w = with_engine(AGENT_SHELL);
    let job_dir = format!("/tmp/exec-jobs/{JOB}");
    let status = Exec::exact(&["sh", "-c", STATUS_SCRIPT, "sh", &job_dir])
        .answer(&format!("step 1{MARKER}"), 0)
        .answer(&format!("step 1{MARKER}step 2{MARKER}0\n"), 0);
    w.fake_engine().exec(status);
    let svc = w.start();
    let target = format!("/services/agent-shell/exec-script/{JOB}");
    // When the status is read, Then the job is running with no exit code
    expect_json(&get(&svc, &target), 200, &json!({ "jobId": JOB, "running": true, "exitCode": null, "output": "step 1" }));
    // When it is read again, Then the output is everything before the last marker and the exit code is 0
    let done = json!({ "jobId": JOB, "running": false, "exitCode": 0, "output": format!("step 1{MARKER}step 2") });
    expect_json(&get(&svc, &target), 200, &done);
    // And each read listed, created, started and inspected one exec in agent-shell's app container
    assert_eq!(engine_calls(&svc).len(), 8);
    assert!(engine_calls(&svc).iter().filter(|c| c.ends_with("/exec")).all(|c| c == "POST /containers/as1/exec"));
});

case!(exec_script_status_keeps_the_last_4000_utf16_units_and_reads_any_exit_code_text, [
    "GET /services/agent-shell/exec-script/<jobId>"
], {
    // Given agent-shell with six jobs: 5000 UTF-16 units of output whose last 4000 start with a non-BMP character
    // (2 units), an exit code of abc, output with no marker, an unknown job (no files, so only the marker), an id in
    // upper case, and 5000 units whose cut falls between the two units of a non-BMP character
    let jobs = [
        JOB,
        "11111111-2222-4333-8444-555555555555",
        "22222222-3333-4444-8555-666666666666",
        "33333333-4444-4555-8666-777777777777",
        "ABCDEF01-2345-4678-89AB-CDEF01234567",
        "44444444-5555-4666-8777-888888888888",
    ];
    let long = format!("{}\u{1f600}{}", "x".repeat(1000), "y".repeat(3998));
    let split = format!("{}\u{1f600}{}", "x".repeat(999), "y".repeat(3999));
    let outputs = [
        format!("{long}{MARKER}0"),
        format!("out{MARKER}abc"),
        "garbage".to_string(),
        MARKER.to_string(),
        MARKER.to_string(),
        format!("{split}{MARKER}0"),
    ];
    let w = with_engine(AGENT_SHELL);
    for (job, output) in jobs.iter().zip(&outputs) {
        let job_dir = format!("/tmp/exec-jobs/{job}");
        w.fake_engine().exec(Exec::exact(&["sh", "-c", STATUS_SCRIPT, "sh", &job_dir]).answer(output, 0));
    }
    let svc = w.start();
    let status = |job: &str| get(&svc, &format!("/services/agent-shell/exec-script/{job}"));
    // When each status is read
    // Then the output keeps the last 4000 UTF-16 units, which are 3999 characters here
    let kept = format!("\u{1f600}{}", "y".repeat(3998));
    expect_json(&status(jobs[0]), 200, &json!({ "jobId": jobs[0], "running": false, "exitCode": 0, "output": kept }));
    // And an exit code that is no number is null though the job is done
    expect_json(&status(jobs[1]), 200, &json!({ "jobId": jobs[1], "running": false, "exitCode": null, "output": "out" }));
    // And output with no marker is all output, still running; an unknown job is running with no output
    expect_json(&status(jobs[2]), 200, &json!({ "jobId": jobs[2], "running": true, "exitCode": null, "output": "garbage" }));
    expect_json(&status(jobs[3]), 200, &json!({ "jobId": jobs[3], "running": true, "exitCode": null, "output": "" }));
    // And an upper-case UUID is a job id, kept as sent
    expect_json(&status(jobs[4]), 200, &json!({ "jobId": jobs[4], "running": true, "exitCode": null, "output": "" }));
    // And a cut inside a surrogate pair keeps the lone low surrogate, which JSON.stringify escapes (compared as raw
    // text, since serde_json refuses a lone surrogate)
    let r = status(jobs[5]);
    let lone = format!(r#"{{"jobId":"{}","running":false,"exitCode":0,"output":"\ude00{}"}}"#, jobs[5], "y".repeat(3999));
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (200, lone.as_str(), Some("application/json")));
    // When the id is no UUID, Then it is 400 before the Engine is asked anything
    let before = engine_calls(&svc).len();
    expect_json(&status("not-a-uuid"), 400, &json!({ "error": "jobId must be a UUID" }));
    assert_eq!(engine_calls(&svc).len(), before);
});

case!(cooldowns_read_is_the_ledger_file_at_version_0_after_a_start, ["GET /cooldowns"], {
    // Given the service, with no cooldown file yet
    let svc = world().start();
    // When the cooldowns are read, Then the ledger is empty at version 0
    expect_json(&get(&svc, "/cooldowns"), 200, &json!({ "ledger": {}, "version": 0 }));
    // When the file holds an object, Then it is the ledger, still at version 0 (the version lives in memory)
    state_file(&svc, "pool-cooldowns.json", "{\n  \"minimax\": {\n    \"until\": 1700000000000\n  }\n}\n");
    expect_json(&get(&svc, "/cooldowns"), 200, &json!({ "ledger": { "minimax": { "until": 1_700_000_000_000_i64 } }, "version": 0 }));
    // When it holds an array, Then the ledger is empty
    state_file(&svc, "pool-cooldowns.json", "[1]\n");
    expect_json(&get(&svc, "/cooldowns"), 200, &json!({ "ledger": {}, "version": 0 }));
});

case!(opencode_log_dashboard_is_public_on_its_paths, ["GET /opencode-log, /opencode-log/, /opencode-log/<runId>"], {
    // Given the service and the dashboard file it serves
    let svc = world().start();
    let dashboard = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rs/static/opencode-log/index.html");
    let dashboard = std::fs::read_to_string(&dashboard).unwrap_or_else(|e| panic!("{}: {e}", dashboard.display()));
    // When the dashboard is read with no auth on its three paths, and on /opencode-log/api, which matches the run id
    // pattern
    for target in ["/opencode-log", "/opencode-log/", "/opencode-log/run-1", "/opencode-log/api"] {
        let r = public(&svc, target);
        // Then each is 200 HTML with its content-length, no cache-control, and the file's bytes
        assert_eq!(r.status, 200, "{target}: {}", r.body);
        assert_eq!(r.header("content-type"), Some("text/html; charset=utf-8"), "{target}");
        assert_eq!(r.header("content-length"), Some(dashboard.len().to_string().as_str()), "{target}");
        assert_eq!(r.header("cache-control"), None, "{target}");
        assert!(r.body == dashboard, "{target} is not the dashboard file");
    }
    // When a path whose last segment is no run id is read, Then it is not public: 401, then 404 when signed
    expect_json(&public(&svc, "/opencode-log/bad_id!"), 401, &json!({ "error": "unauthorized" }));
    expect_json(&get(&svc, "/opencode-log/bad_id!"), 404, &json!({ "error": "not found" }));
});

/// A run's meta.json as the start route writes it (JSON.stringify of the RunMeta, in its key order).
fn meta(run_id: &str, prompt: &str, agent: &str, started_at: &str) -> String {
    json!({
        "runId": run_id, "prompt": prompt, "agent": agent, "caller": agent, "claudeSessionId": null, "model": "minimax/MiniMax-M3",
        "startedAt": started_at, "finishedAt": null, "status": "running", "ok": null, "tokens": null, "cost": null, "durationMs": null,
    })
    .to_string()
}

case!(opencode_log_runs_are_listed_newest_first_and_filtered, ["GET /opencode-log/api/runs"], {
    // Given the service, with no run log yet
    let svc = world().start();
    // When the runs are listed, Then there are none
    expect_json(&public(&svc, "/opencode-log/api/runs"), 200, &json!({ "runs": [] }));
    // Given three runs started at different times, a run dir with no meta.json and a stray file
    let a = meta("run-a", "Fix the Login bug", "implementer", "2026-10-20T10:00:00.000Z");
    let b = meta("run-b", "Find the docs", "researcher", "2026-10-21T09:00:00.000Z");
    let c = meta("run-c", "add a LOGIN test", "implementer", "2026-10-21T11:00:00.000Z");
    for (id, text) in [("run-b", &b), ("run-a", &a), ("run-c", &c)] {
        state_file(&svc, &format!("opencode-log/{id}/meta.json"), text);
    }
    state_file(&svc, "opencode-log/run-d/lines.jsonl", "only lines\n");
    state_file(&svc, "opencode-log/stray.txt", "not a run\n");
    let runs = |metas: &[&String]| format!("{{\"runs\":[{}]}}", metas.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(","));
    // When the runs are listed with no auth, by agent, by a prompt substring in lower and in mixed case, and by both
    // Then each list holds the stored metas, newest startedAt first; agent is exact, q is any case
    for (query, expected) in [
        ("", runs(&[&c, &b, &a])),
        ("?agent=implementer", runs(&[&c, &a])),
        ("?agent=Implementer", runs(&[])),
        ("?q=login", runs(&[&c, &a])),
        ("?q=LoGiN", runs(&[&c, &a])),
        ("?agent=researcher&q=login", runs(&[])),
    ] {
        let r = public(&svc, &format!("/opencode-log/api/runs{query}"));
        assert_eq!((r.status, r.body.as_str()), (200, expected.as_str()), "{query}");
    }
});

case!(opencode_log_run_detail_has_the_last_5000_non_empty_lines, ["GET /opencode-log/api/runs/<runId>"], {
    // Given a run with 5003 lines and empty lines among them, and a run with no lines file
    let svc = world().start();
    let a = meta("run-a", "Fix the Login bug", "implementer", "2026-10-20T10:00:00.000Z");
    let lines: Vec<String> = (0..5003).map(|i| format!("{{\"n\":{i}}}")).collect();
    let file = format!("\n{}\n\n{}\n", lines[..5001].join("\n"), lines[5001..].join("\n\n"));
    state_file(&svc, "opencode-log/run-a/meta.json", &a);
    state_file(&svc, "opencode-log/run-a/lines.jsonl", &file);
    state_file(&svc, "opencode-log/run-b/meta.json", &meta("run-b", "Find the docs", "researcher", "2026-10-21T09:00:00.000Z"));
    // When the run's detail is read with no auth
    let r = public(&svc, "/opencode-log/api/runs/run-a");
    // Then it is the meta with the last 5000 non-empty lines
    let detail = format!("{},\"lines\":{}}}", &a[..a.len() - 1], json!(lines[3..]));
    assert_eq!((r.status, r.body.as_str()), (200, detail.as_str()));
    // When the run with no lines is read, Then its lines are empty
    let r = public(&svc, "/opencode-log/api/runs/run-b");
    assert_eq!((r.status, r.json()["lines"].clone()), (200, json!([])), "{}", r.body);
    // When an unknown run and an invalid id are read, Then each is 404 run not found
    for id in ["run-z", "bad_id!"] {
        expect_json(&public(&svc, &format!("/opencode-log/api/runs/{id}")), 404, &json!({ "error": "run not found" }));
    }
});
