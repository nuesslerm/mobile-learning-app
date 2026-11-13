//! `POST /clis/install`: the CI port installs the wf build CI published through a sibling that mounts the `clis` volume, only when the caller's
//! sha256 is well formed, and the admin port does not serve it. Given/When/Then, through the router `main` serves.
use super::harness::{world, world_with, Doubles};
use vm_ops_api_core::application::ports::SiblingResult;
use vm_ops_api_core::testing::{FakeSiblings, SiblingCall};

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn given_a_sibling_that_installs_when_the_ci_port_posts_a_sha_then_it_is_200_with_the_tree_and_one_waited_sibling_mounts_the_clis_volume() {
    // Given the service, whose sibling installs the build and prints its tree hash
    let siblings = FakeSiblings::new().with_run("", vec![SiblingResult::exited(0, "tree-abc123\n", "")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When the CI port is posted the sha256 of the build CI published
    let reply = world.ci_signed("POST", &format!("/clis/install?sha256={SHA}"), b"").await;

    // Then it is 200 naming the tree, and one sibling ran to the end on this service's image with only the clis volume mounted
    assert_eq!((reply.status, reply.body.as_str()), (200, r#"{"installed":true,"tree":"tree-abc123"}"#));
    let calls = world.siblings.calls();
    let [SiblingCall::Run(args)] = calls.as_slice() else { panic!("expected one waited sibling, got {calls:?}") };
    assert_eq!(args[..8], ["run", "--pull", "missing", "--rm", "-v", "clis:/opt/clis", "ns/vm-ops-api-rs:sha-1234567", "sh"]);
    assert!(args[9].contains(SHA) && !args.iter().any(|a| a.contains("docker.sock")), "{args:?}");
}

#[tokio::test]
async fn given_no_sha_or_a_bad_one_when_posted_then_it_is_400_and_no_sibling_runs() {
    // Given the service
    let world = world(false);

    // When the CI port is posted no sha256, a short one and one with shell text
    for target in ["/clis/install", "/clis/install?sha256=abc", "/clis/install?sha256=0123456789abcdef0123456789abcdef0123456789abcdef01234567%3Bid"] {
        let reply = world.ci_signed("POST", target, b"").await;

        // Then each is 400
        assert_eq!((reply.status, reply.body.as_str()), (400, r#"{"error":"sha256 must be 64 lowercase hex digits"}"#), "{target}");
    }
    assert_eq!(world.siblings.calls(), []);
}

#[tokio::test]
async fn given_a_sibling_that_refuses_the_build_when_posted_then_it_is_502_with_its_last_stderr_line() {
    // Given a sibling that finds the release asset is not the named build
    let siblings = FakeSiblings::new().with_run("", vec![SiblingResult::exited(1, "", "the release asset is not the build the caller named (sha256 differs)\n")]);
    let world = world_with(false, Doubles { siblings, ..Doubles::default() });

    // When the CI port is posted a well formed sha256
    let reply = world.ci_signed("POST", &format!("/clis/install?sha256={SHA}"), b"").await;

    // Then it is 502 with that line
    assert_eq!((reply.status, reply.body.as_str()), (502, r#"{"error":"the release asset is not the build the caller named (sha256 differs)"}"#));
}

#[tokio::test]
async fn given_the_admin_port_when_posted_the_install_then_it_is_not_served_and_nothing_runs() {
    // Given the service
    let world = world(false);

    // When the admin port is posted the install, signed with either seed
    let admin = world.signed("POST", &format!("/clis/install?sha256={SHA}"), b"").await;
    let ci_on_admin = world.ci_signed_on_admin("POST", &format!("/clis/install?sha256={SHA}"), b"").await;

    // Then neither runs a sibling
    assert!(admin.status >= 400 && ci_on_admin.status >= 400, "{} {}", admin.status, ci_on_admin.status);
    assert_eq!(world.siblings.calls(), []);
}
