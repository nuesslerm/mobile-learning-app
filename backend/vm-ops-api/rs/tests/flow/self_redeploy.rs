//! `POST /redeploy` and its smoke test through the router `main` serves: the image `wf ship` built is smoke-tested as a throwaway
//! container (a sibling double answers for it) as its own step, and the answer carries the test's own output when it fails. The swap
//! is a separate request. Given/When/Then.
use super::harness::{world_with, Doubles};
use vm_ops_api_core::application::ports::SiblingResult;
use vm_ops_api_core::testing::{FakeSiblings, SiblingCall};

const COMMIT: &str = "123456789abcdef0123456789abcdef012345678";
const TAG: &str = "sha-1234567";

fn smoke_answers(result: SiblingResult) -> Doubles {
    Doubles { siblings: FakeSiblings::new().with_run("--healthcheck", vec![result]), ..Doubles::default() }
}

fn ran(code: i32, stderr: &str) -> SiblingResult {
    SiblingResult { code: Some(code), stdout: String::new(), stderr: stderr.into() }
}

fn path(commit: &str, tag: &str) -> String {
    format!("/redeploy?ref={commit}&tag={tag}")
}

#[tokio::test]
async fn given_a_commit_when_it_is_redeployed_then_the_answer_is_202_with_the_swap_spawned_and_no_smoke_test_ran() {
    // Given the service, whose smoke test the caller already ran as its own step
    let world = world_with(false, Doubles::default());

    // When the commit is redeployed at its tag
    let reply = world.signed("POST", &path(COMMIT, TAG), b"").await;

    // Then it is answered 202 with the commit being swapped in, no foreground script ran, and one detached sibling carries the tag
    assert_eq!((reply.status, reply.body.as_str()), (202, format!(r#"{{"started":true,"swapping":"{COMMIT}","tag":"{TAG}"}}"#).as_str()));
    let calls = world.siblings.calls();
    assert!(!calls.iter().any(|call| matches!(call, SiblingCall::Run(_))), "{calls:?}");
    let swaps: Vec<&Vec<String>> = calls.iter().filter_map(|call| if let SiblingCall::Detached(args) = call { Some(args) } else { None }).collect();
    assert_eq!(swaps.len(), 1);
    assert!(swaps[0].windows(2).any(|pair| pair == ["-e", &format!("IMAGE_TAG={TAG}")]), "{:?}", swaps[0]);
}

#[tokio::test]
async fn given_a_ref_that_is_no_commit_or_a_tag_that_is_missing_when_redeployed_then_it_is_refused_before_anything_runs() {
    // Given the service
    let world = world_with(false, Doubles::default());

    // When a branch name, a short sha and a missing tag are asked for
    let branch = world.signed("POST", &path("dev", TAG), b"").await;
    let short = world.signed("POST", &path("1234567", TAG), b"").await;
    let untagged = world.signed("POST", &format!("/redeploy?ref={COMMIT}"), b"").await;

    // Then each is a 400 and no sibling ran
    assert_eq!([branch.status, short.status, untagged.status], [400, 400, 400]);
    assert!(world.siblings.calls().is_empty());
}

#[tokio::test]
async fn given_an_image_that_passes_its_smoke_test_when_only_the_smoke_route_is_posted_then_the_container_is_left_up_nothing_is_swapped_and_stop_removes_it() {
    // Given a smoke test that passes, and a stop that removes the container
    let siblings = FakeSiblings::new().with_run("--healthcheck", vec![ran(0, "")]).with_run("docker rm -f vm-ops-api-smoke >/dev/null 2>&1 || true; echo", vec![SiblingResult { code: Some(0), stdout: "[smoke] vm-ops-api stopped\n".into(), stderr: String::new() }]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When only the smoke route is posted, and then the stop
    let started = world.signed("POST", &format!("/redeploy/smoke?ref={COMMIT}&tag={TAG}"), b"").await;
    let stopped = world.signed("POST", "/redeploy/smoke/stop", b"").await;

    // Then the start is 200 naming the spare port, its script keeps the container, and no swap was spawned
    assert_eq!((started.status, started.body.as_str()), (200, format!(r#"{{"smoke":"up","port":18999,"ref":"{COMMIT}","tag":"{TAG}"}}"#).as_str()));
    let calls = world.siblings.calls();
    assert!(matches!(&calls[0], SiblingCall::Run(args) if args.last().is_some_and(|script| script.contains("--name vm-ops-api-smoke -e PORT=18999 -e CI_PORT=18998 ops-api") && !script.contains("docker rm -f vm-ops-api-smoke >/dev/null 2>&1\necho"))), "{calls:?}");
    assert!(!calls.iter().any(|call| matches!(call, SiblingCall::Detached(_))), "{calls:?}");
    // And the stop is 200 and removed that container
    assert_eq!(stopped.status, 200, "{}", stopped.body);
    assert!(matches!(&calls[1], SiblingCall::Run(args) if args.last().is_some_and(|script| script.contains("docker rm -f vm-ops-api-smoke"))), "{calls:?}");
}

#[tokio::test]
async fn given_an_image_that_fails_its_smoke_test_when_only_the_smoke_route_is_posted_then_it_is_502_with_the_log_and_the_failure_is_recorded() {
    // Given a smoke test that fails with the container's log
    let world = world_with(false, smoke_answers(ran(1, "[smoke] vm-ops-api did not answer on port 18999; nothing was swapped. Its log:\nboom\n")));

    // When only the smoke route is posted
    let reply = world.signed("POST", &format!("/redeploy/smoke?ref={COMMIT}&tag={TAG}"), b"").await;

    // Then it is 502 with the test's output, and the history append records redeploy.failed
    assert_eq!(reply.status, 502, "{}", reply.body);
    assert!(reply.body.contains("boom"), "{}", reply.body);
    let calls = world.siblings.calls();
    assert!(calls.iter().any(|call| matches!(call, SiblingCall::Run(args) if args.last().is_some_and(|script| script.contains(">> redeploy-history.jsonl") && script.contains("redeploy.failed")))), "{calls:?}");
}
