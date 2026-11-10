//! The self-redeploy of `POST /redeploy?ref=&tag=`: `wf ship` has built and pushed the images, the service proves the image as a
//! throwaway container (a foreground sibling the `docker` double records and never runs) and only then swaps itself through a
//! detached sibling. A failed proof answers 502 with its output, records `redeploy.failed` and swaps nothing.
use crate::{expect_json, post, world};
use serde_json::json;
use std::time::{Duration, Instant};
use vm_ops_api_integration::{case, Service, ToolRule, World};

/// The commit and tag `wf ship` built.
pub const BUILT_COMMIT: &str = "123456789abcdef0123456789abcdef012345678";
pub const BUILT_TAG: &str = "sha-1234567";
/// The tag of the image running now, which every sibling runs (plan section 4, row 7), unlike the tag that was built.
pub const RUNNING_TAG: &str = "sha-7654321";
const SMOKE_PATH: &str = "/redeploy/smoke?ref=123456789abcdef0123456789abcdef012345678&tag=sha-1234567";
const SWAP_PATH: &str = "/redeploy?ref=123456789abcdef0123456789abcdef012345678&tag=sha-1234567";

/// A service running another tag, whose docker answers nothing yet.
fn running() -> World {
    let mut w = world();
    w.set("VM_OPS_API_IMAGE_TAG", RUNNING_TAG);
    w
}

/// A service whose smoke test and swap siblings both exit 0.
pub fn swap_world() -> World {
    let mut w = running();
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).answer("", "", 0));
    w
}

/// The detached swap the `docker` double recorded, waited for up to `within`.
pub fn wait_swap(svc: &Service, within: Duration) -> Vec<String> {
    let deadline = Instant::now() + within;
    loop {
        let calls = svc.world().docker_calls();
        if let Some(swap) = calls.into_iter().find(|argv| argv.get(..3).is_some_and(|head| head == ["run", "--pull", "always"])) {
            return swap;
        }
        assert!(Instant::now() < deadline, "no swap within {within:?}\nstdout: {}", svc.stdout());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn is_swap(argv: &[String]) -> bool {
    argv.get(..3).is_some_and(|head| head == ["run", "--pull", "always"])
}

case!(self_redeploy_smoke_tests_the_image_and_then_swaps_at_its_commit_and_tag, ["POST /redeploy"], {
    // Given the service running another tag, whose smoke test and swap siblings exit 0
    let svc = swap_world().start();
    // When the commit is smoke-tested and then redeployed at its tag
    expect_json(&post(&svc, SMOKE_PATH, b""), 200, &json!({ "smoke": "up", "port": 18999, "ref": BUILT_COMMIT, "tag": BUILT_TAG }));
    let reply = post(&svc, SWAP_PATH, b"");
    // Then it is 202 with the commit swapped in, the smoke test ran first, on the spare port, against that commit, and no build ran
    expect_json(&reply, 202, &json!({ "started": true, "swapping": BUILT_COMMIT, "tag": BUILT_TAG }));
    let swap = wait_swap(&svc, Duration::from_secs(20));
    let calls = svc.world().docker_calls();
    let smoke = calls.first().expect("the smoke test ran first");
    assert_eq!(smoke[..4], ["run", "--pull", "missing", "--rm"], "{smoke:?}");
    let script = smoke.last().expect("a script");
    assert!(script.contains("--name vm-ops-api-smoke -e PORT=18999 -e CI_PORT=18998 ops-api") && script.contains(BUILT_COMMIT), "{script}");
    assert!(calls.iter().all(|argv| !argv.iter().any(|arg| arg.contains("buildx"))), "a build ran: {calls:?}");
    // And the swap carries the built tag and runs the image that runs now
    assert!(swap.windows(2).any(|pair| pair == ["-e", &format!("IMAGE_TAG={BUILT_TAG}")]), "{swap:?}");
    assert!(swap.iter().any(|arg| arg.ends_with(&format!("/vm-ops-api-rs:{RUNNING_TAG}"))), "{swap:?}");
});

case!(a_failed_smoke_test_answers_502_with_its_output_records_the_failure_and_swaps_nothing, ["POST /redeploy"], {
    // Given a smoke test that fails with the container's log
    let mut w = running();
    w.rule(ToolRule::docker(&["run", "--pull", "missing"]).rest("--healthcheck").answer("", "[smoke] vm-ops-api did not answer on port 18999; nothing was swapped. Its log:\nno such file\n", 1));
    let svc = w.start();
    // When the commit is redeployed
    let reply = post(&svc, SMOKE_PATH, b"");
    // Then it is 502 with that output, and no swap was spawned
    assert_eq!(reply.status, 502, "{}", reply.body);
    let body = reply.json();
    assert_eq!(body["error"], json!("smoke test failed, nothing was swapped"));
    assert!(body["reason"].as_str().is_some_and(|reason| reason.contains("no such file")), "{}", reply.body);
    let calls = svc.world().docker_calls();
    assert!(calls.iter().all(|argv| !is_swap(argv)), "{calls:?}");
    // And the failure is in the redeploy history, where /redeploy/status reads it, and in the event log
    let history = calls.iter().filter(|argv| argv.last().is_some_and(|script| script.contains(">> redeploy-history.jsonl"))).count();
    assert_eq!(history, 1, "{calls:?}");
    svc.wait_line(&json!({ "level": "error", "message": "self_redeploy.smoke_failed", "ref": BUILT_COMMIT, "tag": BUILT_TAG, "reason": body["reason"], "exitCode": "1" }).to_string(), Duration::from_secs(10));
});

case!(a_ref_that_is_no_commit_or_a_missing_tag_is_refused_before_anything_runs, ["POST /redeploy"], {
    // Given the service
    let svc = swap_world().start();
    // When a branch name, a short sha, and a commit without a tag are redeployed
    let branch = post(&svc, "/redeploy?ref=dev&tag=sha-1234567", b"");
    let short = post(&svc, "/redeploy?ref=1234567&tag=sha-1234567", b"");
    let untagged = post(&svc, &format!("/redeploy?ref={BUILT_COMMIT}"), b"");
    // Then each is a 400 and no sibling ran
    assert_eq!([branch.status, short.status, untagged.status], [400, 400, 400]);
    assert_eq!(svc.world().docker_calls(), Vec::<Vec<String>>::new());
});
