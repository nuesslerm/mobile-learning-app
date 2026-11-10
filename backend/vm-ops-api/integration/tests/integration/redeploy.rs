//! The redeploy and retire cases of plan section 3 ("Redeploy and retire") and `/redeploy/status` ("Reads"). Each sibling
//! is a `docker run` the `docker` double records and never runs; its stdout, stderr and exit code come from the case's
//! rules. Every recorded argv is compared with its golden, captured from Node, after the normalisations of "Comparing
//! recorded calls" (`siblings`), so a Rust argv that names Node's image or helper commands fails.
use crate::{expect_json, get, post, world};
use serde_json::{json, Value};
use std::time::Duration;
use vm_ops_api_integration::{
    assert_golden, case, sibling_image, siblings_json, Reply, Service, ToolRule, World, IMAGE_TAG,
};

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
const HINT: &str = "HOST_LABEL is unrecognized (this VM's .env should set it to main), or the app is not in main.tf's \
    local.vm_apps. Add it to local.vm_apps in main.tf and to HOST_ALLOWED_APPS in src/domain/apps.ts together.";
/// The arguments after the prefix that tell each sibling apart, as the double's `rest` regex matches them.
pub const HISTORY_APPEND: &str = r">> redeploy-history\.jsonl";
pub const APP_SIBLING: &str = r"^--rm -v /var/run/docker\.sock:/var/run/docker\.sock -v /home/ubuntu/";
const RETIRE_SIBLING: &str = r"docker compose -p \S+ down";
const PAIR_SIBLING: &str = r"docker exec orca-serve orca-serve pair";
const STATUS_LOG: &str = r"cat last-redeploy\.log";
const STATUS_HISTORY: &str = r"cat redeploy-history\.jsonl";

fn redeploy(app: &str, query: &str) -> String {
    format!("/services/{app}/redeploy{query}")
}

fn retire(app: &str) -> String {
    format!("/services/{app}/retire")
}

/// The `docker` double's answers for a redeploy: each history append exits 0, the app sibling prints `stdout` and
/// `stderr` and exits `exit_code`.
fn redeploy_world(stdout: &str, stderr: &str, exit_code: i32) -> World {
    let mut w = world();
    w.rule(ToolRule::docker(&["run", "--rm"]).rest(HISTORY_APPEND).answer("", "", 0))
        .rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(APP_SIBLING).answer(stdout, stderr, exit_code));
    w
}

/// The recorded sibling argv, normalised as the goldens hold them, for the running image's tag.
fn siblings(svc: &Service) -> Vec<Vec<String>> {
    serde_json::from_str(&siblings_json(svc.world(), IMAGE_TAG)).expect("JSON")
}

fn assert_siblings_golden(name: &str, siblings: &[Vec<String>]) {
    assert_golden(name, &(serde_json::to_string_pretty(siblings).expect("JSON") + "\n"));
}

/// The JSON line of one history append, as its sibling's script writes it, with the `ts` normalised.
fn history_line(sibling: &[String]) -> Value {
    let script = sibling.last().map(String::as_str).unwrap_or("");
    let line = script.strip_prefix("printf '%s\\n' '").and_then(|s| s.split_once("' >> ")).map(|(l, _)| l);
    serde_json::from_str(line.unwrap_or_else(|| panic!("no history line in {script:?}"))).expect("a JSON history line")
}

fn health_sha(svc: &Service) -> Value {
    svc.signed("GET", "/health", None).json()["serverScriptSha"].clone()
}

/// A leetcode-backend redeploy's stdout event: level, message, tag and requester, then the exit code once there is one.
fn redeploy_event(level: &str, message: &str, tag: &str, requested_by: &str, exit_code: Option<&str>) -> Value {
    let mut e = json!({ "level": level, "message": message, "app": "leetcode-backend", "tag": tag, "requestedBy": requested_by });
    if let Some(code) = exit_code {
        e["exitCode"] = json!(code);
    }
    e
}

fn event(svc: &Service, line: &Value) {
    svc.wait_line(&line.to_string(), Duration::from_secs(5));
}

fn post_empty(svc: &Service, target: &str) -> Reply {
    post(svc, target, b"")
}

/// Redeploys leetcode-backend with `query`, expecting 200 and three siblings, and returns the reply's JSON and the script
/// of its app sibling, the middle one.
fn redeployed(svc: &Service, query: &str) -> (Value, String) {
    let before = svc.world().docker_calls().len();
    let r = post_empty(svc, &redeploy("leetcode-backend", query));
    assert_eq!(r.status, 200, "{query}: {}", r.body);
    let ran = siblings(svc);
    assert_eq!(ran.len(), before + 3, "{query}: {ran:?}");
    (r.json(), ran[before + 1].last().cloned().unwrap_or_default())
}

/// Whether an app sibling's script provisions leetcode-backend's files from the repo at `at`.
fn provisions(script: &str, at: &str) -> bool {
    script.contains(&format!("/mobile-learning-app/{at}/backend/leetcode-backend/"))
}

/// `body` as JSON.stringify prints it, with `@` at the start of the string `key` printed as the lone low surrogate
/// U+DE00, which JSON.stringify escapes and serde_json refuses to hold.
fn with_lone_surrogate(body: &Value, key: &str) -> String {
    body.to_string().replacen(&format!(r#""{key}":"@"#), &format!(r#""{key}":"\ude00"#), 1)
}

/// 5000 UTF-16 units of compose-like output whose last `keep` start with U+1F600 (two units), and 5000 whose cut falls
/// between that character's two units; each is followed by what the cut keeps of it.
fn past_the_cap(keep: usize) -> [(String, String); 2] {
    let tick = |n: usize| "\u{2714}".repeat(n);
    let whole = (format!("{}\u{1f600}{}", "o".repeat(5000 - keep), tick(keep - 2)), format!("\u{1f600}{}", tick(keep - 2)));
    let split = (format!("{}\u{1f600}{}", "o".repeat(4999 - keep), tick(keep - 1)), format!("@{}", tick(keep - 1)));
    [whole, split]
}

case!(redeploy_checks_the_app_then_the_host_then_the_tag_ref_and_recreate, ["POST /services/:app/redeploy"], {
    // Given the service on host main
    let svc = world().start();
    // When an unknown app, vm-ops-api, which is no managed app, or metro-runner, which is no app any more, is redeployed
    // with a bad tag, Then it is 404 with the seven known apps: the app is checked first
    for app in ["nope", "vm-ops-api", "metro-runner"] {
        let r = post_empty(&svc, &redeploy(app, "?tag=-bad"));
        expect_json(&r, 404, &json!({ "error": "unknown app", "knownApps": KNOWN_APPS }));
    }
    // When a bad tag, bad refs or a recreate other than all are sent, Then each is 400 with its exact message, the tag
    // checked before the ref and the ref before recreate
    let tag = json!({ "error": "invalid tag format" });
    let bad_ref = json!({ "error": "invalid ref format" });
    let recreate = json!({ "error": "invalid recreate mode -- accepted value: 'all'" });
    let long_tag = format!("?tag={}", "a".repeat(129));
    let long_ref = format!("?ref={}", "a".repeat(256));
    let refused = [
        ("?tag=-bad&ref=a..b&recreate=x", &tag),
        ("?tag=a:b", &tag),
        (long_tag.as_str(), &tag),
        ("?ref=a..b&recreate=x", &bad_ref),
        ("?ref=/x", &bad_ref),
        ("?ref=x/", &bad_ref),
        ("?ref=a//b", &bad_ref),
        ("?ref=-x", &bad_ref),
        ("?ref=a%27b", &bad_ref),
        (long_ref.as_str(), &bad_ref),
        ("?recreate=x", &recreate),
        ("?recreate=ALL", &recreate),
        ("?recreate=", &recreate),
    ];
    for (query, error) in refused {
        expect_json(&post_empty(&svc, &redeploy("leetcode-backend", query)), 400, error);
    }
    // And no sibling ran
    assert_eq!(svc.world().docker_calls(), Vec::<Vec<String>>::new());
    // Given the service on a host label no app is declared for, and on labels that are names every object has
    for host in ["other", "toString", "constructor", "__proto__"] {
        let mut w = world();
        w.set("HOST_LABEL", host);
        let svc = w.start();
        // When a known app is redeployed with a bad tag, Then it is 409 with the hint: the host is checked before the tag
        let r = post_empty(&svc, &redeploy("leetcode-backend", "?tag=-bad"));
        let undeclared = json!({
            "error": "app not declared for this host", "host": host, "app": "leetcode-backend", "allowedApps": [], "hint": HINT,
        });
        expect_json(&r, 409, &undeclared);
        assert_eq!(svc.world().docker_calls(), Vec::<Vec<String>>::new());
    }
});

case!(redeploy_runs_history_the_app_sibling_and_history_and_answers_eight_fields, ["POST /services/:app/redeploy"], {
    // Given an app sibling that exits 0 with 5000 UTF-16 units of compose-like output on each stream: on stdout the last
    // 4000 start with a non-BMP character, on stderr the cut falls between that character's two units
    let [(out, out_kept), (err, err_kept)] = past_the_cap(4000);
    let svc = redeploy_world(&out, &err, 0).start();
    // When leetcode-backend is redeployed at a sha tag with an explicit ref and a requester, as wf ship sends it
    let r = post_empty(&svc, &redeploy("leetcode-backend", "?tag=sha-89abcde&ref=feature/x&requestedBy=wf%20ship"));
    // Then it is 200 with the eight fields: the explicit ref wins over the tag's commit, the script hash is
    // the one /health reports, and stdout and stderr are each their last 4000 UTF-16 units, the cut pair's half escaped
    let expected = json!({
        "app": "leetcode-backend", "tag": "sha-89abcde", "ref": "feature/x", "wipeState": false,
        "exitCode": 0, "serverScriptSha": health_sha(&svc), "stdout": out_kept, "stderr": err_kept,
    });
    let expected = with_lone_surrogate(&expected, "stderr");
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (200, expected.as_str(), Some("application/json")));
    // And three siblings ran in order: the history append of redeploy.triggered, the app sibling, the history append of
    // redeploy.succeeded, each the argv Node runs
    let ran = siblings(&svc);
    assert_siblings_golden("redeploy-leetcode-backend.json", &ran);
    let outcome = |sibling: &[String]| json!([history_line(sibling)["outcome"], history_line(sibling)["exitCode"]]);
    let outcomes = [outcome(&ran[0]), outcome(&ran[2])];
    assert_eq!(outcomes, [json!(["redeploy.triggered", null]), json!(["redeploy.succeeded", 0])]);
    // And stdout has the started and succeeded events
    event(&svc, &redeploy_event("info", "redeploy.started", "sha-89abcde", "wf ship", None));
    event(&svc, &redeploy_event("info", "redeploy.succeeded", "sha-89abcde", "wf ship", Some("0")));
});

case!(redeploy_takes_the_explicit_ref_then_the_tags_commit_then_dev_and_remembers_none, ["POST /services/:app/redeploy"], {
    // Given an app sibling that exits 0
    let svc = redeploy_world("", "", 0).start();
    // When it is redeployed with an explicit ref, Then that ref is used
    let (body, script) = redeployed(&svc, "?ref=feature/x");
    assert_eq!(body["ref"], json!("feature/x"), "{body}");
    assert!(provisions(&script, "feature/x"), "{script}");
    // When it is redeployed with no ref, Then the ref is not remembered: dev
    let (body, script) = redeployed(&svc, "");
    assert_eq!(body["ref"], json!("dev"), "{body}");
    assert!(provisions(&script, "dev"), "{script}");
    // When it is redeployed at a sha tag with no ref, Then the tag's commit is the ref
    let (body, script) = redeployed(&svc, "?tag=sha-89abcde");
    assert_eq!(body["ref"], json!("89abcde"), "{body}");
    assert!(provisions(&script, "89abcde"), "{script}");
    // When it is redeployed with an empty ref, Then it is dev
    let (body, script) = redeployed(&svc, "?ref=");
    assert_eq!(body["ref"], json!("dev"), "{body}");
    assert!(provisions(&script, "dev"), "{script}");
});

case!(redeploy_accepts_each_value_at_the_edge_of_its_limit, ["POST /services/:app/redeploy"], {
    // Given an app sibling that exits 0
    let svc = redeploy_world("", "", 0).start();
    // When the tag is sent empty, Then it is latest, at dev
    let (body, _) = redeployed(&svc, "?tag=");
    assert_eq!((&body["tag"], &body["ref"]), (&json!("latest"), &json!("dev")), "{body}");
    // When a tag of 128 characters is sent, the most the tag grammar allows, Then it is used as sent
    let tag = "a".repeat(128);
    assert_eq!(redeployed(&svc, &format!("?tag={tag}")).0["tag"], json!(tag));
    // When a requester of 80 characters is sent, the most a label may have, Then both history lines keep it
    let requester = "r".repeat(80);
    redeployed(&svc, &format!("?requestedBy={requester}"));
    let ran = siblings(&svc);
    let kept = [&ran[ran.len() - 3], &ran[ran.len() - 1]].map(|s| history_line(s)["requestedBy"].clone());
    assert_eq!(kept, [json!(requester), json!(requester)]);
    // When a sha tag names a commit of 40 hex, Then that commit is the ref
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let (body, script) = redeployed(&svc, &format!("?tag=sha-{commit}"));
    assert_eq!(body["ref"], json!(commit));
    assert!(provisions(&script, commit), "{script}");
    // When a ref of 255 characters is sent, the most a ref may have, Then it is used
    let long_ref = "a".repeat(255);
    let (body, script) = redeployed(&svc, &format!("?ref={long_ref}"));
    assert_eq!(body["ref"], json!(long_ref), "{body}");
    assert!(provisions(&script, &long_ref), "{script}");
    // When a sha tag carries 41 hex, which names no commit, Then the ref is dev, not the hex
    let (body, script) = redeployed(&svc, &format!("?tag=sha-{commit}8"));
    assert_eq!(body["ref"], json!("dev"), "{body}");
    assert!(provisions(&script, "dev"), "{script}");
});

case!(redeploy_records_an_invalid_requester_as_unknown, ["POST /services/:app/redeploy"], {
    // Given an app sibling that exits 0
    let svc = redeploy_world("", "", 0).start();
    // When a requester with a character outside the label's set, an empty one, one of 81 characters and none are sent
    let long = format!("?requestedBy={}", "a".repeat(81));
    for query in ["?requestedBy=bad%3Blabel", "?requestedBy=", long.as_str(), ""] {
        assert_eq!(post_empty(&svc, &redeploy("leetcode-backend", query)).status, 200, "{query}");
    }
    // Then both history lines of each redeploy, and its started event, name the requester unknown
    let ran = siblings(&svc);
    assert_eq!(ran.len(), 12, "{ran:?}");
    for i in [0, 2, 3, 5, 6, 8, 9, 11] {
        assert_eq!(history_line(&ran[i])["requestedBy"], json!("unknown"), "sibling {i}");
    }
    event(&svc, &redeploy_event("info", "redeploy.started", "latest", "unknown", None));
    // When a valid label with every kind of character it allows is sent, Then it is kept as sent
    assert_eq!(post_empty(&svc, &redeploy("leetcode-backend", "?requestedBy=ci%3A%20deploy.yml/run-1_a")).status, 200);
    assert_eq!(history_line(&siblings(&svc)[12])["requestedBy"], json!("ci: deploy.yml/run-1_a"));
});

case!(redeploy_wipes_state_recreates_all_or_takes_the_narrow_path_by_app, ["POST /services/:app/redeploy"], {
    // Given an app sibling that exits 0
    let svc = redeploy_world("", "", 0).start();
    // When leetcode-backend is redeployed with wipeState=TRUE, agent-shell with wipeState=1 (as wf ship --wipe-state
    // sends it), with recreate=all and with wipeState=yes, and remote-chat-agent plainly
    let variants = [
        ("leetcode-backend", "?wipeState=TRUE", true, "redeploy-leetcode-backend-wipe-state.json"),
        ("agent-shell", "?wipeState=1", true, "redeploy-agent-shell-wipe-state.json"),
        ("agent-shell", "?recreate=all", false, "redeploy-agent-shell-recreate-all.json"),
        ("agent-shell", "?wipeState=yes", false, "redeploy-agent-shell.json"),
        ("remote-chat-agent", "", false, "redeploy-remote-chat-agent.json"),
    ];
    for (i, (app, query, wipe, golden)) in variants.into_iter().enumerate() {
        let r = post_empty(&svc, &redeploy(app, query));
        // Then each is 200 with wipeState as the flag reads it (1 or true, in any case), and its app sibling is the one
        // Node runs: the full down and up with the volume removal for wipeState, even for an app with a narrow path, the
        // full down and up with recreate=all, and otherwise only the app's own service for agent-shell and remote-chat-agent
        assert_eq!((r.status, &r.json()["wipeState"]), (200, &json!(wipe)), "{app}{query}: {}", r.body);
        assert_siblings_golden(golden, &siblings(&svc)[3 * i + 1..3 * i + 2]);
    }
    let scripts: Vec<String> = siblings(&svc).iter().skip(1).step_by(3).map(|s| s.last().cloned().unwrap_or_default()).collect();
    assert!(scripts[0].contains("grep -E '^leetcode-backend_.*tailscale-state$' | xargs -r docker volume rm"), "{}", scripts[0]);
    let full_down = "docker compose -f docker-compose.prod.yml down ;";
    let wiped = "grep -E '^agent-shell_.*tailscale-state$' | xargs -r docker volume rm";
    assert!(scripts[1].contains(full_down) && scripts[1].contains(wiped), "{}", scripts[1]);
    assert!(scripts[2].contains(full_down) && !scripts[2].contains(wiped), "{}", scripts[2]);
    assert!(scripts[3].contains("up -d --no-deps app") && !scripts[3].contains(" down "), "{}", scripts[3]);
    assert!(scripts[4].contains("up -d --no-deps agent") && !scripts[4].contains(" down "), "{}", scripts[4]);
});

case!(redeploy_of_agent_shell_touches_nothing_of_ci_runner_and_ci_runner_redeploys_alone_in_its_own_project, ["POST /services/:app/redeploy"], {
    // Given an app sibling that exits 0
    let svc = redeploy_world("", "", 0).start();
    // When agent-shell is redeployed with the full down and up, and then ci-runner is redeployed
    for app in ["agent-shell", "ci-runner"] {
        let r = post_empty(&svc, &redeploy(app, "?recreate=all"));
        assert_eq!(r.status, 200, "{app}: {}", r.body);
    }
    let scripts: Vec<String> = siblings(&svc).iter().skip(1).step_by(3).map(|s| s.last().cloned().unwrap_or_default()).collect();
    let (shell, runner) = (&scripts[0], &scripts[1]);
    // Then agent-shell's `down` and `up` run in its own directory, so compose's project is agent-shell and never ci-runner's
    assert!(shell.contains("cd /home/ubuntu/agent-shell && docker compose -f docker-compose.prod.yml down"), "{shell}");
    assert!(!shell.contains("ci-runner"), "{shell}");
    // And ci-runner provisions its own compose file and env group and runs in its own directory, touching nothing of agent-shell
    assert!(runner.contains("infra/oracle-cloud/ci-runner/docker-compose.prod.yml"), "{runner}");
    assert!(runner.contains("wf secret sync-env --for ci-runner"), "{runner}");
    assert!(runner.contains("cd /home/ubuntu/ci-runner && docker compose -f docker-compose.prod.yml down"), "{runner}");
    assert!(!runner.contains("agent-shell"), "{runner}");
    // And both create the shared `clis` volume first, so the external declaration in each project resolves
    assert!(shell.contains("docker volume create clis") && runner.contains("docker volume create clis"));
});

case!(redeploy_whose_app_sibling_exits_1_is_502_with_history_failed, ["POST /services/:app/redeploy"], {
    // Given an app sibling that prints its progress, fails to pull and exits 1
    let err = "Error response from daemon: manifest for ns/leetcode-backend:sha-89abcde not found\n";
    let svc = redeploy_world("[redeploy] start 1760000000\n", err, 1).start();
    // When leetcode-backend is redeployed at a sha tag with an explicit ref
    let r = post_empty(&svc, &redeploy("leetcode-backend", "?tag=sha-89abcde&ref=feature/x"));
    // Then it is 502 with the eight fields, the exit code and the sibling's output
    let expected = json!({
        "app": "leetcode-backend", "tag": "sha-89abcde", "ref": "feature/x", "wipeState": false,
        "exitCode": 1, "serverScriptSha": health_sha(&svc), "stdout": "[redeploy] start 1760000000\n", "stderr": err,
    });
    expect_json(&r, 502, &expected);
    // And the second history append records redeploy.failed with exit code 1, and stdout has the failed event
    let ran = siblings(&svc);
    assert_siblings_golden("redeploy-leetcode-backend-failed.json", &ran);
    let last = history_line(&ran[2]);
    assert_eq!((&last["outcome"], &last["exitCode"]), (&json!("redeploy.failed"), &json!(1)));
    event(&svc, &redeploy_event("error", "redeploy.failed", "sha-89abcde", "unknown", Some("1")));
    // When it is redeployed again with no ref and no sha tag, Then it provisions from dev: the failed redeploy's ref was
    // not remembered
    let r = post_empty(&svc, &redeploy("leetcode-backend", ""));
    assert_eq!((r.status, &r.json()["ref"]), (502, &json!("dev")), "{}", r.body);
    let script = siblings(&svc)[4].last().cloned().unwrap_or_default();
    assert!(provisions(&script, "dev"), "{script}");
});

case!(retire_refuses_a_known_app_the_control_plane_and_a_name_outside_the_pattern, ["POST /services/:app/retire"], {
    // Given the service
    let svc = world().start();
    // When each known app, vm-ops-api and the watchdog (the control plane, whose compose project holds the tailnet state),
    // and names outside ^[a-z][a-z0-9-]{1,40}$ (one letter, a capital, a leading digit or dash, an underscore, 42
    // characters, an escaped dash, which stays escaped) are retired
    let long = format!("a{}", "b".repeat(41));
    let names = ["a", "Old-app", "1old", "-old", "old_app", long.as_str(), "old%2Dapp"];
    // Then each is 400 not a retired app, naming the app as the path gave it
    for app in KNOWN_APPS.iter().chain(&["vm-ops-api", "watchdog"]).chain(&names) {
        expect_json(&post_empty(&svc, &retire(app)), 400, &json!({ "error": "not a retired app", "app": app }));
    }
    // And no sibling ran
    assert_eq!(svc.world().docker_calls(), Vec::<Vec<String>>::new());
});

case!(retire_accepts_a_name_of_2_or_of_41_characters_and_one_that_only_contains_a_control_plane_name, ["POST /services/:app/retire"], {
    // Given a retire sibling that exits 0
    let mut w = world();
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(RETIRE_SIBLING).answer("", "", 0));
    let svc = w.start();
    // When names of 2 and of 41 characters are retired, the shortest and the longest the pattern allows, and names that
    // contain vm-ops-api or watchdog without being them
    let long = format!("a{}", "b".repeat(40));
    for (i, app) in ["ab", long.as_str(), "vm-ops-api-old", "watchdog2", "old-watchdog"].into_iter().enumerate() {
        // Then each is 200, and its sibling takes that compose project down
        expect_json(&post_empty(&svc, &retire(app)), 200, &json!({ "app": app, "exitCode": 0, "stdout": "", "stderr": "" }));
        let script = svc.world().docker_calls()[i].last().cloned().unwrap_or_default();
        assert!(script.starts_with(&format!("docker compose -p {app} down ")), "{script}");
    }
});

case!(retire_takes_the_retired_apps_compose_project_down, ["POST /services/:app/retire"], {
    // Given a retire sibling that exits 0 with 5000 UTF-16 units on each stream: compose's progress on stdout, whose last
    // 2000 start with a non-BMP character, and on stderr, whose cut falls between that character's two units
    let mut w = world();
    let [(out, out_kept), (err, err_kept)] = past_the_cap(2000);
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(RETIRE_SIBLING).answer(&out, &err, 0));
    let svc = w.start();
    // When old-app, whose compose file is gone from the repo, is retired
    let r = post_empty(&svc, &retire("old-app"));
    // Then it is 200 with the exit code and the last 2000 UTF-16 units of each stream, the cut pair's half escaped
    let expected = json!({ "app": "old-app", "exitCode": 0, "stdout": out_kept, "stderr": err_kept });
    let expected = with_lone_surrogate(&expected, "stderr");
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (200, expected.as_str(), Some("application/json")));
    // And one sibling ran, the argv Node runs, and stdout has the started and succeeded events
    assert_siblings_golden("retire-old-app.json", &siblings(&svc));
    event(&svc, &json!({ "level": "info", "message": "retire.started", "app": "old-app" }));
    event(&svc, &json!({ "level": "info", "message": "retire.succeeded", "app": "old-app", "exitCode": "0" }));
});

case!(retire_takes_the_removed_metro_runner_down_with_its_volumes, ["POST /services/:app/retire"], {
    // Given a retire sibling that exits 0
    let mut w = world();
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(RETIRE_SIBLING).answer("", "", 0));
    let svc = w.start();
    // When metro-runner, which is no app of the service any more, is retired
    let r = post_empty(&svc, &retire("metro-runner"));
    // Then it is 200, and the one sibling that ran takes that compose project down with its volumes and orphans
    expect_json(&r, 200, &json!({ "app": "metro-runner", "exitCode": 0, "stdout": "", "stderr": "" }));
    let calls = svc.world().docker_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].last().map(String::as_str), Some("docker compose -p metro-runner down --volumes --remove-orphans"));
});

case!(retire_whose_sibling_exits_1_is_502, ["POST /services/:app/retire"], {
    // Given a retire sibling that exits 1
    let mut w = world();
    let err = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock\n";
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(RETIRE_SIBLING).answer("", err, 1));
    let svc = w.start();
    // When old-app is retired
    let r = post_empty(&svc, &retire("old-app"));
    // Then it is 502 with the exit code and the output, and stdout has the failed event
    expect_json(&r, 502, &json!({ "app": "old-app", "exitCode": 1, "stdout": "", "stderr": err }));
    event(&svc, &json!({ "level": "error", "message": "retire.failed", "app": "old-app", "exitCode": "1" }));
});

const FAKE_LINK: &str = "orca://pair?code=FAKE-CREDENTIAL-123";

case!(pair_mints_a_link_in_the_orca_serve_container_and_the_link_is_only_in_the_reply, ["POST /services/orca-serve/pair"], {
    // Given a pair sibling that exits 0 and prints a log line, then the verb's JSON line with a recognisable fake link
    let mut w = world();
    let stdout = format!("starting\n{{\"ok\":true,\"url\":\"{FAKE_LINK}\",\"deviceId\":\"d1\"}}\n");
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(PAIR_SIBLING).answer(&stdout, "", 0));
    let svc = w.start();
    // When orca-serve is paired, and then another app, which is refused
    let r = post_empty(&svc, "/services/orca-serve/pair");
    let other = post_empty(&svc, "/services/leetcode-backend/pair");
    // Then the reply is 200 with exactly the link, one fixed sibling ran, and the other app is a 400
    expect_json(&r, 200, &json!({ "ok": true, "url": FAKE_LINK }));
    expect_json(&other, 400, &json!({ "error": "not a pairable app", "app": "leetcode-backend" }));
    assert_siblings_golden("pair-orca-serve.json", &siblings(&svc));
    // And the events name the app and exit code, and the link is on no stdout line
    event(&svc, &json!({ "level": "info", "message": "pair.started", "app": "orca-serve" }));
    event(&svc, &json!({ "level": "info", "message": "pair.succeeded", "app": "orca-serve", "exitCode": "0" }));
    assert!(!svc.stdout().contains("FAKE-CREDENTIAL"), "{}", svc.stdout());
});

case!(pair_whose_sibling_fails_or_prints_no_link_is_502_and_echoes_nothing, ["POST /services/orca-serve/pair"], {
    // Given a pair sibling that exits 1 with the link on stdout and a secret on stderr
    let mut w = world();
    let stdout = format!("{{\"url\":\"{FAKE_LINK}\"}}\n");
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(PAIR_SIBLING).answer(&stdout, "FAKE-CREDENTIAL-STDERR", 1));
    let svc = w.start();
    // When orca-serve is paired
    let r = post_empty(&svc, "/services/orca-serve/pair");
    // Then it is 502 with a fixed error, nothing of the output is in the reply, and stdout has the failed event and no secret
    assert_eq!((r.status, r.body.contains("FAKE-CREDENTIAL")), (502, false), "{}", r.body);
    assert_eq!(r.json()["ok"], json!(false));
    event(&svc, &json!({ "level": "error", "message": "pair.failed", "app": "orca-serve", "exitCode": "1" }));
    assert!(!svc.stdout().contains("FAKE-CREDENTIAL"), "{}", svc.stdout());
});

case!(every_sibling_runs_the_running_image_or_latest, [
    "GET /redeploy/status",
    "POST /services/:app/redeploy",
    "POST /services/:app/retire"
], {
    for (tag, image_tag) in [(Some(IMAGE_TAG), IMAGE_TAG), (Some("unknown"), "latest"), (None, "latest")] {
        // Given the service with VM_OPS_API_IMAGE_TAG set, set to unknown, or unset, and siblings that exit 0
        let mut w = redeploy_world("", "", 0);
        w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest(RETIRE_SIBLING).answer("", "", 0))
            .rule(status_log().answer("", "", 0));
        match tag {
            Some(tag) => w.set("VM_OPS_API_IMAGE_TAG", tag),
            None => w.unset("VM_OPS_API_IMAGE_TAG"),
        };
        let svc = w.start();
        // When an app is redeployed, an old one retired, and the redeploy status read
        assert_eq!(post_empty(&svc, &redeploy("leetcode-backend", "")).status, 200);
        assert_eq!(post_empty(&svc, &retire("old-app")).status, 200);
        assert_eq!(get(&svc, "/redeploy/status").status, 200);
        // Then each of the six siblings names exactly one image, ns/vm-ops-api-rs at that tag (plan section 4, deliberate
        // difference 7)
        let ran = svc.world().docker_calls();
        assert_eq!(ran.len(), 6, "{ran:?}");
        let image = sibling_image(image_tag);
        for argv in &ran {
            let images: Vec<&String> = argv.iter().filter(|a| a.starts_with("ns/")).collect();
            assert_eq!(images, [&image], "{argv:?}");
            assert!(!argv.iter().any(|a| a.contains("ns/vm-ops-api:")), "a sibling names the old image: {argv:?}");
        }
    }
});

/// The `docker` double's answers for the two read-only status siblings.
fn status_world(log: ToolRule, history: &str) -> World {
    let mut w = world();
    w.rule(log).rule(ToolRule::docker(&["run", "--rm"]).rest(STATUS_HISTORY).answer(history, "", 0));
    w
}

fn status_log() -> ToolRule {
    ToolRule::docker(&["run", "--rm"]).rest(STATUS_LOG)
}

case!(redeploy_status_reads_the_log_and_the_history_through_two_read_only_siblings, ["GET /redeploy/status"], {
    // Given a last-redeploy.log of 5000 UTF-16 units whose last 4000 start with a non-BMP character, its stderr of 5000
    // whose cut falls between that character's two units, and a history whose lines include one that is no JSON, a blank
    // one, an indented one, the falsy JSON values null, 0, false and "", and a JSON number
    let [(log, log_kept), _] = past_the_cap(4000);
    let [_, (err, err_kept)] = past_the_cap(2000);
    let first = r#"{"ts":"2026-10-20T10:00:00.000Z","key":"service-leetcode-backend","requestedBy":"wf ship","tag":"sha-89abcde","ref":"89abcde","outcome":"redeploy.succeeded","exitCode":0}"#;
    let last = r#"{"ts":"2026-10-21T09:00:00Z","key":"vm-ops-api","requestedBy":"self-redeploy","tag":"sha-1234567","ref":"0123456","outcome":"redeploy.succeeded","exitCode":0}"#;
    let history = format!("{first}\n{{oops\n\n   {last}   \nnull\n0\nfalse\n\"\"\n42\n");
    let svc = status_world(status_log().answer(&log, &err, 0), &history).start();
    // When the redeploy status is read
    let r = get(&svc, "/redeploy/status");
    // Then it is 200 with the last 4000 UTF-16 units of the log, the last 2000 of its stderr (the cut pair's half
    // escaped), and, in order, the history lines that parse to a truthy value: a line that is no JSON or is falsy is dropped
    let parsed: Vec<Value> = [first, last].iter().map(|l| serde_json::from_str(l).expect("JSON")).collect();
    let expected = json!({ "log": log_kept, "stderr": err_kept, "history": [parsed[0], parsed[1], 42] });
    let expected = with_lone_surrogate(&expected, "stderr");
    assert_eq!((r.status, r.body.as_str(), r.header("content-type")), (200, expected.as_str(), Some("application/json")));
    // And the two siblings, the log's cat and the history's cat, are the read-only argv Node runs
    assert_siblings_golden("redeploy-status.json", &siblings(&svc));
});

case!(redeploy_status_is_502_with_the_same_body_when_the_log_sibling_exits_non_zero, ["GET /redeploy/status"], {
    // Given a log sibling that cannot pull the image and exits 125, and one history line
    let err = "Unable to find image 'ns/vm-ops-api:sha-1234567' locally\n";
    let line = r#"{"key":"service-agent-shell","outcome":"redeploy.failed","exitCode":1}"#;
    let svc = status_world(status_log().answer("", err, 125), &format!("{line}\n")).start();
    // When the redeploy status is read
    let r = get(&svc, "/redeploy/status");
    // Then it is 502 with the same three keys: the empty log, the stderr and the history the second sibling read
    let history: Value = serde_json::from_str(line).expect("JSON");
    expect_json(&r, 502, &json!({ "log": "", "stderr": err, "history": [history] }));
    // And the history sibling still ran
    assert_eq!(svc.world().docker_calls().len(), 2);
});

case!(redeploy_status_decodes_each_sibling_write_alone, ["GET /redeploy/status"], {
    // Given a log sibling whose stdout and stderr each carry a euro sign cut after its second byte across two writes, and
    // a history sibling with no answer scripted, which exits 97 with nothing on stdout
    let mut w = world();
    w.rule(status_log().answer("a\u{20ac}b", "c\u{20ac}d", 0).cut(3, 3));
    let svc = w.start();
    // When the redeploy status is read
    let r = get(&svc, "/redeploy/status");
    // Then each write was decoded alone: the two halves came back as two replacement characters, and the history is empty
    expect_json(&r, 200, &json!({ "log": "a\u{fffd}\u{fffd}b", "stderr": "c\u{fffd}\u{fffd}d", "history": [] }));
});
