//! The redeploy, retire and redeploy-status routes: each request enters through the router `main` serves, and the `docker` siblings
//! are a double that answers by what their script says. Given/When/Then.
use super::harness::{world, world_with, Doubles};
use vm_ops_api_core::application::ports::SiblingResult;
use vm_ops_api_core::testing::{FakeSiblings, SiblingCall};

const HISTORY: &str = ">> redeploy-history.jsonl";

fn argvs(world: &super::harness::World) -> Vec<Vec<String>> {
    world.siblings.calls().into_iter().map(|call| match call {
        SiblingCall::Run(args) | SiblingCall::Detached(args) => args,
    }).collect()
}

#[tokio::test]
async fn given_a_signed_redeploy_when_posted_then_it_is_200_with_the_eight_fields_and_three_siblings_on_this_image_ran() {
    // Given leetcode-backend on host main, whose sibling exits 0
    let siblings = FakeSiblings::new().with_run(HISTORY, vec![SiblingResult::exited(0, "", "")]).with_run("[redeploy] start", vec![SiblingResult::exited(0, "compose done\n", "")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When it is redeployed at a sha tag with an explicit ref
    let reply = world.signed("POST", "/services/leetcode-backend/redeploy?tag=sha-89abcde&ref=feature/x&requestedBy=wf%20ship", b"").await;

    // Then the eight fields come in Node's order, the script hash is the one /health reports, and the content type is exactly JSON
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        reply.body,
        r#"{"app":"leetcode-backend","tag":"sha-89abcde","ref":"feature/x","wipeState":false,"exitCode":0,"serverScriptSha":"0123456789ab","stdout":"compose done\n","stderr":""}"#
    );
    assert_eq!(reply.header("content-type"), Some("application/json"));
    // And the history append, the app sibling and the closing append all ran on this service's own image
    let ran = argvs(&world);
    assert_eq!(ran.len(), 3);
    assert!(ran[1].iter().any(|arg| arg == "ns/vm-ops-api-rs:sha-1234567"), "{:?}", ran[1]);
}

#[tokio::test]
async fn given_bad_input_when_a_redeploy_or_a_retire_is_posted_then_each_is_refused_with_nodes_reply_and_no_sibling_runs() {
    // Given the service
    let world = world(false);

    // When an unknown app, a bad tag, a bad recreate, and a retire of a managed app and of the control plane are posted
    let unknown = world.signed("POST", "/services/nope/redeploy", b"").await;
    let bad_tag = world.signed("POST", "/services/leetcode-backend/redeploy?tag=bad!", b"").await;
    let bad_recreate = world.signed("POST", "/services/leetcode-backend/redeploy?recreate=x", b"").await;
    let managed = world.signed("POST", "/services/leetcode-backend/retire", b"").await;
    let control = world.signed("POST", "/services/vm-ops-api/retire", b"").await;

    // Then each is the status and body Node answers
    assert_eq!((unknown.status, unknown.body.as_str()), (404, r#"{"error":"unknown app","knownApps":["leetcode-backend","remote-chat-agent","leetcode-auth-cookie-capture","agent-shell","ci-runner","vnc-auth-session","reporting-service","t3-serve","orca-serve"]}"#));
    assert_eq!((bad_tag.status, bad_tag.body.as_str()), (400, r#"{"error":"invalid tag format"}"#));
    assert_eq!((bad_recreate.status, bad_recreate.body.as_str()), (400, r#"{"error":"invalid recreate mode -- accepted value: 'all'"}"#));
    assert_eq!((managed.status, managed.body.as_str()), (400, r#"{"error":"not a retired app","app":"leetcode-backend"}"#));
    assert_eq!((control.status, control.body.as_str()), (400, r#"{"error":"not a retired app","app":"vm-ops-api"}"#));
    assert!(argvs(&world).is_empty(), "{:?}", argvs(&world));
}

#[tokio::test]
async fn given_a_retire_sibling_that_fails_when_an_old_app_is_retired_then_it_is_502_with_the_same_four_keys() {
    // Given a retire sibling that exits 1
    let siblings = FakeSiblings::new().with_run("docker compose -p old-app down", vec![SiblingResult::exited(1, "", "Cannot connect to the Docker daemon\n")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When old-app is retired
    let reply = world.signed("POST", "/services/old-app/retire", b"").await;

    // Then it is 502 with the exit code and the output, and the sibling took the compose project down with its volumes
    assert_eq!((reply.status, reply.body.as_str()), (502, r#"{"app":"old-app","exitCode":1,"stdout":"","stderr":"Cannot connect to the Docker daemon\n"}"#));
    assert_eq!(argvs(&world)[0].last().map(String::as_str), Some("docker compose -p old-app down --volumes --remove-orphans"));
}

#[tokio::test]
async fn given_the_two_status_siblings_when_the_redeploy_status_is_read_then_the_log_and_the_parsed_history_answer_and_a_failed_log_is_502() {
    // Given a log and a history with one line that parses and one that does not
    let line = r#"{"ts":"2026-10-21T09:00:00Z","key":"vm-ops-api","outcome":"redeploy.succeeded","exitCode":0}"#;
    let siblings = FakeSiblings::new()
        .with_run("cat last-redeploy.log", vec![SiblingResult::exited(0, "exit=0 restored=none\n", ""), SiblingResult::exited(125, "", "Unable to find image\n")])
        .with_run("cat redeploy-history.jsonl", vec![SiblingResult::exited(0, &format!("{{oops\n{line}\n"), "")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When the status is read twice
    let ok = world.signed("GET", "/redeploy/status", b"").await;
    let failed = world.signed("GET", "/redeploy/status", b"").await;

    // Then the first is 200 with the log, an empty stderr and the one history line, and the second is 502 with the same keys
    assert_eq!((ok.status, ok.body.as_str()), (200, format!(r#"{{"log":"exit=0 restored=none\n","stderr":"","history":[{line}]}}"#).as_str()));
    assert_eq!((failed.status, failed.body.as_str()), (502, format!(r#"{{"log":"","stderr":"Unable to find image\n","history":[{line}]}}"#).as_str()));
}

#[tokio::test]
async fn given_an_app_with_a_smoke_test_when_the_smoke_route_is_posted_then_one_sibling_leaves_the_throwaway_container_running_and_stop_removes_it() {
    // Given the leetcode-backend smoke sibling passing, and one that removes the container
    let siblings = FakeSiblings::new().with_run("smoke start", vec![SiblingResult::exited(0, "[smoke] leetcode-backend healthy on port 18999\n", "")]).with_run("-smoke >/dev/null", vec![SiblingResult::exited(0, "[smoke] leetcode-backend stopped\n", "")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When its smoke test is started and then stopped
    let started = world.signed("POST", "/services/leetcode-backend/smoke?tag=sha-89abcde&ref=feature/x", b"").await;
    let stopped = world.signed("POST", "/services/leetcode-backend/smoke/stop", b"").await;

    // Then the start is 200 with the redeploy's own body and ran one sibling that keeps the container, with no history appended
    assert_eq!(started.status, 200, "{}", started.body);
    assert!(started.body.contains(r#""exitCode":0"#) && started.body.contains("healthy on port 18999"), "{}", started.body);
    let ran = argvs(&world);
    assert!(ran[0].last().is_some_and(|script| script.contains("--name leetcode-backend-smoke -e PORT=18999 app") && !script.contains("docker rm -f leetcode-backend-smoke >/dev/null 2>&1\necho")), "{:?}", ran[0]);
    // And the stop is 200 and removes exactly that container
    assert_eq!((stopped.status, stopped.body.as_str()), (200, r#"{"app":"leetcode-backend","exitCode":0,"stdout":"[smoke] leetcode-backend stopped\n","stderr":""}"#));
    assert!(ran.len() == 2 && ran[1].last().is_some_and(|script| script.contains("docker rm -f leetcode-backend-smoke")), "{ran:?}");
}

#[tokio::test]
async fn given_an_app_without_a_smoke_test_when_the_smoke_route_is_posted_then_it_says_so_and_runs_no_sibling() {
    // Given the service
    let world = world(false);

    // When agent-shell's smoke test is started
    let reply = world.signed("POST", "/services/agent-shell/smoke", b"").await;

    // Then it is 200 saying the app has none, and nothing ran
    assert!(reply.status == 200 && reply.body.contains("agent-shell has no smoke test"), "{}", reply.body);
    assert!(argvs(&world).is_empty(), "{:?}", argvs(&world));
}

#[tokio::test]
async fn given_vnc_auth_session_when_its_smoke_test_is_started_then_the_copy_runs_in_smoke_mode_on_spare_ports_and_is_probed_with_any_http_answer() {
    // Given vnc-auth-session, whose listener would otherwise repoint the live tailnet route and take the live gate port
    let siblings = FakeSiblings::new().with_run("smoke start", vec![SiblingResult::exited(0, "[smoke] vnc-auth-session healthy on port 18999\n", "")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When its smoke test is started
    let reply = world.signed("POST", "/services/vnc-auth-session/smoke?tag=sha-89abcde&ref=feature/x", b"").await;

    // Then the copy runs in smoke mode with its own gate and listener ports, and the probe accepts any HTTP answer from the listener
    assert_eq!(reply.status, 200, "{}", reply.body);
    let ran = argvs(&world);
    let script = ran[0].last().expect("a script");
    assert!(script.contains("--name vnc-auth-session-smoke -e VNC_AUTH_LISTENER_PORT=18999 -e VNC_AUTH_SMOKE=1 -e VNC_AUTH_GATE_PORT=18998 app"), "{script}");
    assert!(script.contains("curl -s -o /dev/null http://127.0.0.1:18999/"), "{script}");
}

const FAKE_LINK: &str = "orca://pair?code=FAKE-CREDENTIAL-123";

#[tokio::test]
async fn given_a_pair_sibling_that_prints_a_link_when_orca_serve_is_paired_then_the_link_is_only_in_the_reply_and_in_no_log_event() {
    // Given a pair sibling whose last line is the verb's JSON with a recognisable fake link
    let stdout = format!("starting\n{{\"ok\":true,\"url\":\"{FAKE_LINK}\",\"deviceId\":\"d1\"}}\n");
    let siblings = FakeSiblings::new().with_run("docker exec orca-serve", vec![SiblingResult::exited(0, &stdout, "")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When orca-serve is paired with a signed request, and with no auth
    let unsigned = world.send("POST", "/services/orca-serve/pair", &[], b"").await;
    assert_eq!((unsigned.status, argvs(&world).len()), (401, 0));
    let reply = world.signed("POST", "/services/orca-serve/pair", b"").await;

    // Then the reply is 200 with exactly the link, the one fixed script ran, and the link is in no event
    assert_eq!((reply.status, reply.body.as_str()), (200, format!(r#"{{"ok":true,"url":"{FAKE_LINK}"}}"#).as_str()));
    assert_eq!(argvs(&world).len(), 1);
    assert_eq!(argvs(&world)[0].last().map(String::as_str), Some("docker exec orca-serve orca-serve pair --endpoint ws://orca-serve:6768"));
    assert!(!format!("{:?}", world.log.events()).contains("FAKE-CREDENTIAL"));
}

#[tokio::test]
async fn given_any_other_app_or_a_failing_pair_sibling_when_paired_then_it_is_400_or_502_and_neither_the_output_nor_the_link_is_echoed() {
    // Given a pair sibling that exits 1 with the link on stdout and a secret on stderr
    let stdout = format!("{{\"url\":\"{FAKE_LINK}\"}}\n");
    let siblings = FakeSiblings::new().with_run("docker exec orca-serve", vec![SiblingResult::exited(1, &stdout, "FAKE-CREDENTIAL-STDERR")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When another app is paired, and then orca-serve
    let other = world.signed("POST", "/services/leetcode-backend/pair", b"").await;
    assert_eq!((other.status, other.body.as_str()), (400, r#"{"error":"not a pairable app","app":"leetcode-backend"}"#));
    assert!(argvs(&world).is_empty());
    let failed = world.signed("POST", "/services/orca-serve/pair", b"").await;

    // Then it is 502 with a fixed error, and nothing of the sibling's output is in the reply or the events
    assert_eq!((failed.status, failed.body.contains("FAKE-CREDENTIAL")), (502, false));
    assert!(failed.body.starts_with(r#"{"ok":false,"error":"#), "{}", failed.body);
    assert!(!format!("{:?}", world.log.events()).contains("FAKE-CREDENTIAL"));
}

const RUNNER_TOKEN: &str = "AAAA-fake_registration-token-9Z";

fn ci_runner_world() -> super::harness::World {
    let siblings = FakeSiblings::new().with_run(HISTORY, vec![SiblingResult::exited(0, "", "")]).with_run("[redeploy] start", vec![SiblingResult::exited(0, &format!("compose done\nconfig token {RUNNER_TOKEN}\n"), &format!("warn {RUNNER_TOKEN}"))]);
    world_with(false, Doubles { siblings, ..Doubles::default() })
}

#[tokio::test]
async fn given_a_runner_token_body_when_ci_runner_is_redeployed_then_it_is_written_to_the_env_after_the_local_overrides_and_is_in_no_log_script_or_reply() {
    // Given the ci-runner redeploy sibling, which echoes the token on both streams
    let world = ci_runner_world();

    // When ci-runner is redeployed with a RUNNER_TOKEN body
    let reply = world.signed("POST", "/services/ci-runner/redeploy?tag=sha-89abcde", format!("RUNNER_TOKEN={RUNNER_TOKEN}\n").as_bytes()).await;

    // Then it is 200 and the variable reaches the sibling's env, and the script writes it to .env after append-env-local and verify-env
    assert_eq!(reply.status, 200, "{}", reply.body);
    let ran = argvs(&world);
    let sibling = &ran[1];
    let at = sibling.iter().position(|arg| arg == &format!("RUNNER_TOKEN={RUNNER_TOKEN}")).expect("the variable in the sibling env");
    assert_eq!(sibling[at - 1], "-e");
    let script = sibling.last().expect("a script");
    let order = ["append-env-local", "verify-env", "printf '%s=%s\\n' RUNNER_TOKEN \"$RUNNER_TOKEN\" >> /home/ubuntu/ci-runner/.env", "docker compose"].map(|part| script.find(part).unwrap_or_else(|| panic!("{part} in {script}")));
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
    // And the value is in no stored script, reply, event or other sibling's argv
    assert!(!script.contains(RUNNER_TOKEN), "{script}");
    assert!(!reply.body.contains(RUNNER_TOKEN) && reply.body.contains("***"), "{}", reply.body);
    assert!(!format!("{:?}", world.log.events()).contains(RUNNER_TOKEN));
    assert!(ran.iter().enumerate().filter(|(i, _)| *i != 1).all(|(_, args)| !args.concat().contains(RUNNER_TOKEN)));
}

#[tokio::test]
async fn given_a_body_that_breaks_a_rule_when_ci_runner_is_redeployed_then_it_is_400_naming_the_rule_without_the_value_and_nothing_runs() {
    // Given the service
    let world = ci_runner_world();
    let long = format!("RUNNER_TOKEN={}", "a".repeat(257));

    // When an unlisted name, a value with a newline, a space, a shell metacharacter, a too long value, a second line and a too big body are posted
    let bodies: [(&str, Vec<u8>, &str); 7] = [
        ("unlisted", b"GITHUB_PAT=abc123".to_vec(), "env name is not on this app's allow-list"),
        ("newline", b"RUNNER_TOKEN=abc\nINJECT=1".to_vec(), "env name is not on this app's allow-list"),
        ("space", b"RUNNER_TOKEN=abc def".to_vec(), "env value must be 1 to 256 characters of [A-Za-z0-9_-] (a tskey-auth-* key for TS_CI_RUNNER_AUTH_KEY, at most 160)"),
        ("meta", b"RUNNER_TOKEN=abc$(touch${IFS}x)".to_vec(), "env value must be 1 to 256 characters of [A-Za-z0-9_-] (a tskey-auth-* key for TS_CI_RUNNER_AUTH_KEY, at most 160)"),
        ("long", long.into_bytes(), "env value must be 1 to 256 characters of [A-Za-z0-9_-] (a tskey-auth-* key for TS_CI_RUNNER_AUTH_KEY, at most 160)"),
        ("twice", b"RUNNER_TOKEN=abc\nRUNNER_TOKEN=def".to_vec(), "env name is set twice"),
        ("big", vec![b'a'; 1025], "env body exceeds 1024 bytes"),
    ];
    for (label, body, rule) in bodies {
        let reply = world.signed("POST", "/services/ci-runner/redeploy", &body).await;

        // Then each is 400 with the rule and none of the body
        assert_eq!((reply.status, reply.body.as_str()), (400, format!(r#"{{"error":"{rule}"}}"#).as_str()), "{label}");
    }
    assert!(argvs(&world).is_empty(), "{:?}", argvs(&world));
}

#[tokio::test]
async fn given_an_app_that_declares_no_env_names_when_redeployed_with_a_runner_token_body_then_it_is_400_and_no_sibling_runs() {
    // Given the service
    let world = ci_runner_world();

    // When the same RUNNER_TOKEN body is posted for leetcode-backend
    let reply = world.signed("POST", "/services/leetcode-backend/redeploy", format!("RUNNER_TOKEN={RUNNER_TOKEN}").as_bytes()).await;

    // Then it is 400 naming the rule, the value is not echoed, and nothing ran
    assert_eq!((reply.status, reply.body.as_str()), (400, r#"{"error":"this app accepts no env body"}"#));
    assert!(argvs(&world).is_empty());
}
