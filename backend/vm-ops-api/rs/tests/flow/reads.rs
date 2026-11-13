//! The read routes through the router: ps, stats, logs (snapshot and follow), and the opencode run log.
//! Given/When/Then.
use crate::harness::{signed_headers, today, world, world_with, Doubles, DASHBOARD, NOON};
use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use vm_ops_api_core::application::ports::{ContainersError, Event, Object, RunLog};
use vm_ops_api_core::domain::containers::DockerContainer;
use vm_ops_api_core::domain::container_stats::DockerStats;
use vm_ops_api_core::testing::{FakeContainers, FakeHostFiles, FollowEnd};

const UNAUTHORIZED: &str = r#"{"error":"unauthorized"}"#;

fn container(id: &str, name: &str, state: &str) -> DockerContainer {
    DockerContainer {
        id: id.to_string(),
        names: Some(vec![format!("/{name}")]),
        state: Some(state.to_string()),
        status: Some("Up 1 hour".to_string()),
        image: Some("ns/app:sha-1".to_string()),
    }
}

fn object(value: Value) -> Object {
    value.as_object().cloned().expect("an object")
}

fn stats(json: &str) -> DockerStats {
    serde_json::from_str(json).expect("an Engine stats body")
}

#[tokio::test]
async fn given_an_app_with_containers_when_ps_is_read_then_each_is_a_row_and_an_unknown_app_lists_the_eight_known_apps() {
    // Given an Engine listing remote-chat-agent's app container and an opencode container with no status
    let mut opencode = container("r4", "remote-chat-agent-opencode", "created");
    opencode.status = None;
    let containers = FakeContainers::new().with_app_containers("remote-chat-agent", vec![container("r1", "remote-chat-agent", "running"), opencode]);
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    // When ps is read for it, signed, and for vm-ops-api, which is no managed app
    let listed = world.signed("GET", "/services/remote-chat-agent/ps", b"").await;
    let unknown = world.signed("GET", "/services/vm-ops-api/ps", b"").await;
    // Then the rows keep the Engine's order and leave out the status that was not listed
    assert_eq!(listed.status, 200, "{}", listed.body);
    assert_eq!(
        listed.body,
        concat!(
            r#"{"app":"remote-chat-agent","containers":[{"name":"remote-chat-agent","state":"running","status":"Up 1 hour","image":"ns/app:sha-1"},"#,
            r#"{"name":"remote-chat-agent-opencode","state":"created","image":"ns/app:sha-1"}]}"#
        )
    );
    // And the unknown app is 404 with the eight known apps in order
    let known = ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture", "agent-shell", "vnc-auth-session", "reporting-service", "t3-serve", "orca-serve"];
    assert_eq!((unknown.status, unknown.body), (404, json!({ "error": "unknown app", "knownApps": known }).to_string()));
    // And without auth ps is 401, as every route but the public ones
    assert_eq!(world.send("GET", "/services/remote-chat-agent/ps", &[], b"").await.body, UNAUTHORIZED);
}

#[tokio::test]
async fn given_an_engine_that_refuses_the_list_when_ps_and_logs_are_read_then_each_is_502_with_the_engines_answer_and_the_event_names_it() {
    // Given an Engine whose list answers 500
    let refusal = ContainersError::refused("docker API", 500, r#"{"message":"engine down"}"#, 200);
    let world = world_with(false, Doubles { containers: FakeContainers::new().with_listing_error(refusal), ..Doubles::default() });
    // When ps and logs are read
    let ps = world.signed("GET", "/services/leetcode-backend/ps", b"").await;
    let logs = world.signed("GET", "/services/leetcode-backend/logs", b"").await;
    // Then each is 502 request failed with the Engine's answer as the detail
    let failed = json!({ "error": "request failed", "detail": r#"docker API returned 500: {"message":"engine down"}"# }).to_string();
    assert_eq!((ps.status, ps.body.as_str(), logs.status, logs.body.as_str()), (502, failed.as_str(), 502, failed.as_str()));
    // And each was logged as a failed handler with its path
    let expected = |path: &str| Event::error("http.handler_failed").with("method", "GET").with("path", path).with("err", r#"docker API returned 500: {"message":"engine down"}"#);
    assert_eq!(world.log.events(), [expected("/services/leetcode-backend/ps"), expected("/services/leetcode-backend/logs")]);
}

#[tokio::test]
async fn given_the_host_files_and_running_containers_when_stats_is_read_then_the_numbers_are_the_working_sets_sorted_and_summed() {
    // Given a host with 12,000,000 kB of memory and load 0.52, and two running containers (the smaller listed first), one of
    // which has a limit and a CPU sample, while an exited one is not listed as running
    let containers = FakeContainers::new()
        .with_running(vec![container("s1", "leetcode-backend", "running"), container("s2", "agent-shell", "running")])
        .with_stats(
            "s1",
            Ok(stats(
                r#"{"memory_stats":{"usage":315172800,"limit":536870912,"stats":{"inactive_file":104857600}},
                    "cpu_stats":{"cpu_usage":{"total_usage":125000000},"system_cpu_usage":2200000000,"online_cpus":4},
                    "precpu_stats":{"cpu_usage":{"total_usage":100000000},"system_cpu_usage":2000000000}}"#,
            )),
        )
        .with_stats("s2", Ok(stats(r#"{"memory_stats":{"usage":1073741824,"limit":12288000000,"stats":{"total_inactive_file":73741824}}}"#)));
    let meminfo = "MemTotal:       12000000 kB\nMemAvailable:    6000000 kB\nSwapTotal:       2097148 kB\n";
    let host_files = FakeHostFiles::default().with("meminfo", meminfo).with("loadavg", "0.52 0.58 0.59 1/234 5678\n");
    let world = world_with(false, Doubles { containers, host_files, ..Doubles::default() });
    // When /stats is read
    let reply = world.signed("GET", "/stats", b"").await;
    // Then the host is in rounded megabytes, the containers are biggest first with a whole cpuPct printed as 50, and the total is the sum
    assert_eq!(
        reply.body,
        concat!(
            r#"{"host":{"memTotalMb":11719,"memAvailableMb":5859,"swapTotalMb":2048,"load":[0.52,0.58,0.59]},"containers":["#,
            r#"{"name":"agent-shell","memMb":954,"memLimitMb":null,"cpuPct":0},"#,
            r#"{"name":"leetcode-backend","memMb":201,"memLimitMb":512,"cpuPct":50}],"containersMb":1155}"#
        )
    );
}

#[tokio::test]
async fn given_an_app_with_containers_when_logs_are_read_then_each_container_is_text_and_a_refused_one_is_named_by_its_status() {
    // Given leetcode-backend's container and its sidecar, whose logs the Engine refuses
    let containers = FakeContainers::new()
        .with_app_containers("leetcode-backend", vec![container("l1", "leetcode-backend", "running"), container("l2", "leetcode-backend-tailscale", "running")])
        .with_logs("l1", Ok("started\nwarn: slow\n".to_string()))
        .with_logs("l2", Err(ContainersError::refused("docker API", 500, "logs failed", 300)));
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    // When the logs are read with a tail of 5000 and for an app with no containers, and for an unknown app
    let snapshot = world.signed("GET", "/services/leetcode-backend/logs?tail=5000", b"").await;
    let none = world.signed("GET", "/services/agent-shell/logs", b"").await;
    let unknown = world.signed("GET", "/services/metro-runner/logs", b"").await;
    // Then the tail is capped at 1000 and the refused container reads <docker API returned 500>
    let logs = json!({ "leetcode-backend": "started\nwarn: slow\n", "leetcode-backend-tailscale": "<docker API returned 500>" });
    assert_eq!(snapshot.body, json!({ "app": "leetcode-backend", "tail": 1000, "logs": logs }).to_string());
    // And an app with no containers is 404, and an unknown one lists the known apps and vm-ops-api
    assert_eq!((none.status, none.body), (404, r#"{"error":"no containers found for this app (not running?)"}"#.to_string()));
    assert_eq!(unknown.status, 404);
    let known = serde_json::from_str::<Value>(&unknown.body).expect("json")["knownApps"].clone();
    assert_eq!(known.as_array().and_then(|apps| apps.last()), Some(&json!("vm-ops-api")), "{known}");
}

#[tokio::test]
async fn given_an_app_with_two_containers_when_logs_are_followed_then_one_must_be_named_and_its_text_streams_until_the_engine_ends_it() {
    // Given remote-chat-agent with an app container and an opencode container whose follow sends two texts
    let containers = FakeContainers::new()
        .with_app_containers("remote-chat-agent", vec![container("r1", "remote-chat-agent", "running"), container("r4", "remote-chat-agent-opencode", "running")])
        .with_follow("r4", vec![Ok("line one\n".to_string()), Ok(String::new()), Ok("line two \u{20ac}\n".to_string())], FollowEnd::Ends);
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    // When it is followed with no container, with an unknown one, and with the opencode container named
    let ambiguous = world.signed("GET", "/services/remote-chat-agent/logs?follow=true", b"").await;
    let missing = world.signed("GET", "/services/remote-chat-agent/logs?follow=1&container=nope", b"").await;
    let followed = world.signed("GET", "/services/remote-chat-agent/logs?follow=TRUE&container=remote-chat-agent-opencode&tail=5", b"").await;
    // Then the first two are 400 and 404 with the container names
    let names = json!(["remote-chat-agent", "remote-chat-agent-opencode"]);
    let message = "multiple containers for this app -- pass &container=<name> to pick one";
    assert_eq!((ambiguous.status, ambiguous.body), (400, json!({ "error": message, "containers": names }).to_string()));
    assert_eq!((missing.status, missing.body), (404, json!({ "error": "no container with that name for this app", "containers": names }).to_string()));
    // And the third is plain text naming the container, uncached, with no length, that ends when the Engine's stream ends
    assert_eq!((followed.status, followed.body.as_str()), (200, "line one\nline two \u{20ac}\n"));
    let headers = (followed.header("content-type"), followed.header("cache-control"), followed.header("x-container-name"));
    assert_eq!(headers, (Some("text/plain; charset=utf-8"), Some("no-cache"), Some("remote-chat-agent-opencode")));
    assert_eq!(followed.header("content-length"), None);
}

#[tokio::test]
async fn given_a_follow_that_fails_when_read_then_the_reply_is_200_and_the_text_ends_with_the_failure() {
    // Given an app container whose follow the Engine refuses, and an opencode container whose follow sends a text and then breaks
    let refused = ContainersError::refused("docker API", 500, r#"{"message":"logs failed"}"#, 300);
    let containers = FakeContainers::new()
        .with_app_containers("remote-chat-agent", vec![container("r1", "remote-chat-agent", "running"), container("r4", "remote-chat-agent-opencode", "running")])
        .with_follow_error("r1", refused)
        .with_follow("r4", vec![Ok("partial\n".to_string()), Err(ContainersError::failed("aborted"))], FollowEnd::Hangs);
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    // When each is followed
    let first = world.signed("GET", "/services/remote-chat-agent/logs?follow=true&container=remote-chat-agent", b"").await;
    let second = world.signed("GET", "/services/remote-chat-agent/logs?follow=true&container=remote-chat-agent-opencode", b"").await;
    // Then both are 200, as the headers went out first, and each text ends with one line that says why
    assert_eq!((first.status, first.body.as_str()), (200, "\n<log stream ended: docker API returned 500: {\"message\":\"logs failed\"}>\n"));
    assert_eq!((second.status, second.body.as_str()), (200, "partial\n\n<log stream ended: aborted>\n"));
}

#[tokio::test]
async fn given_a_follow_that_is_idle_when_the_caller_goes_away_then_the_engines_stream_is_dropped() {
    // Given a container whose follow sent a text and then waits for more
    let containers = FakeContainers::new()
        .with_app_containers("leetcode-backend", vec![container("l1", "leetcode-backend", "running")])
        .with_follow("l1", vec![Ok("only\n".to_string())], FollowEnd::Hangs);
    let world = world_with(false, Doubles { containers, ..Doubles::default() });
    let headers = signed_headers("GET", "/services/leetcode-backend/logs?follow=true", b"", NOON, &today());
    let mut request = Request::builder().method("GET").uri("/services/leetcode-backend/logs?follow=true");
    for (name, value) in &headers {
        request = request.header(name, value);
    }
    // When the caller reads the first text and then drops the reply
    let response = world.router.clone().oneshot(request.body(Body::empty()).expect("a request")).await.expect("the router answers");
    let mut body = response.into_body();
    let first = body.frame().await.expect("a frame").expect("readable").into_data().expect("data");
    assert_eq!((first.as_ref(), world.containers.followers_dropped()), (b"only\n".as_slice(), 0));
    drop(body);
    // Then the Engine's stream was dropped with it
    assert_eq!(world.containers.followers_dropped(), 1);
}

#[tokio::test]
async fn given_the_dashboard_when_read_without_auth_then_it_is_served_on_its_paths_and_a_path_that_is_no_run_id_is_not_public() {
    // Given the service
    let world = world(false);
    // When the dashboard is read with no auth on its three paths and on /opencode-log/api, which is a valid run id
    for target in ["/opencode-log", "/opencode-log/", "/opencode-log/run-1", "/opencode-log/api"] {
        let page = world.send("GET", target, &[], b"").await;
        // Then each is 200 HTML with the embedded page and its length, and no cache-control
        assert_eq!((page.status, page.body.as_bytes()), (200, DASHBOARD), "{target}");
        assert_eq!((page.header("content-type"), page.header("cache-control")), (Some("text/html; charset=utf-8"), None), "{target}");
        assert_eq!(page.header("content-length"), Some(DASHBOARD.len().to_string().as_str()), "{target}");
    }
    // When a path whose last segment is no run id is read, Then it is 401 without auth and 404 signed
    assert_eq!(world.send("GET", "/opencode-log/bad_id!", &[], b"").await.body, UNAUTHORIZED);
    assert_eq!(world.signed("GET", "/opencode-log/bad_id!", b"").await.status, 404);
}

#[tokio::test]
async fn given_runs_when_listed_and_read_without_auth_then_the_list_is_newest_first_and_filtered_and_a_run_has_its_lines() {
    // Given two runs by different agents, and the lines of the first
    let world = world(false);
    let meta = |id: &str, prompt: &str, agent: &str, at: &str| object(json!({ "runId": id, "prompt": prompt, "agent": agent, "startedAt": at }));
    world.run_log.write_meta("run-a", meta("run-a", "Fix the Login bug", "implementer", "2026-10-20T10:00:00.000Z")).await.expect("stored");
    world.run_log.write_meta("run-b", meta("run-b", "Find the docs", "researcher", "2026-10-21T09:00:00.000Z")).await.expect("stored");
    world.run_log.append_lines("run-a", &["{\"n\":1}".to_string(), "{\"n\":2}".to_string()]).await.expect("stored");
    // When the runs are listed, filtered by agent and by a prompt text in another case, and a run and an unknown one are read
    let all = world.send("GET", "/opencode-log/api/runs", &[], b"").await;
    let by_agent = world.send("GET", "/opencode-log/api/runs?agent=implementer", &[], b"").await;
    let by_text = world.send("GET", "/opencode-log/api/runs?q=LOGIN", &[], b"").await;
    let run = world.send("GET", "/opencode-log/api/runs/run-a", &[], b"").await;
    let unknown = world.send("GET", "/opencode-log/api/runs/bad_id!", &[], b"").await;
    // Then the list is newest first, the filters keep the matching run, and the run is its meta with its lines last
    let ids = |reply: &crate::harness::Reply| -> Vec<String> {
        let body: Value = serde_json::from_str(&reply.body).expect("json");
        body["runs"].as_array().expect("runs").iter().map(|run| run["runId"].as_str().expect("an id").to_string()).collect()
    };
    assert_eq!(ids(&all), ["run-b", "run-a"]);
    assert_eq!((ids(&by_agent), ids(&by_text)), (["run-a"].map(String::from).to_vec(), ["run-a"].map(String::from).to_vec()));
    let detail = r#"{"runId":"run-a","prompt":"Fix the Login bug","agent":"implementer","startedAt":"2026-10-20T10:00:00.000Z","lines":["{\"n\":1}","{\"n\":2}"]}"#;
    assert_eq!((run.status, run.body.as_str()), (200, detail));
    assert_eq!((unknown.status, unknown.body), (404, r#"{"error":"run not found"}"#.to_string()));
}
