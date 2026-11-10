//! `POST /drill`: the drills answer 202 before a detached sibling takes this service's container down, and take the signed
//! token alone. Given/When/Then, through the router `main` serves.
use super::harness::{legacy_headers, world, SEED};
use vm_ops_api_core::testing::SiblingCall;

#[tokio::test]
async fn given_a_signed_drill_when_each_action_is_posted_then_it_is_202_naming_the_container_and_one_detached_sibling_runs() {
    // Given the service
    let world = world(false);

    // When the stop drill and the freeze drill are posted, signed
    let stop = world.signed("POST", "/drill?action=stop", b"").await;
    let freeze = world.signed("POST", "/drill?action=freeze", b"").await;

    // Then each is 202 naming the action and the container, in that key order
    assert_eq!((stop.status, stop.body.as_str()), (202, r#"{"accepted":true,"action":"stop","container":"vm-ops-api-ops-api-1"}"#));
    assert_eq!((freeze.status, freeze.body.as_str()), (202, r#"{"accepted":true,"action":"freeze","container":"vm-ops-api-ops-api-1"}"#));
    assert_eq!(stop.header("content-type"), Some("application/json"));
    // And each started one detached sibling on this service's own image and waited for none
    let scripts: Vec<(String, String)> = world
        .siblings
        .calls()
        .into_iter()
        .map(|call| match call {
            SiblingCall::Detached(args) => (args[7].clone(), args[10].clone()),
            SiblingCall::Run(args) => panic!("a drill waited for {args:?}"),
        })
        .collect();
    assert_eq!(scripts[0], ("ns/vm-ops-api-rs:sha-1234567".to_string(), "sleep 5; docker stop vm-ops-api-ops-api-1".to_string()));
    assert!(scripts[1].1.starts_with("sleep 5; docker kill -s STOP vm-ops-api-ops-api-1; sleep 600; docker kill -s CONT"), "{}", scripts[1].1);
}

#[tokio::test]
async fn given_the_legacy_token_or_no_action_when_a_drill_is_posted_then_it_is_401_or_400_and_nothing_runs() {
    // Given the service in its migration posture, which still takes the raw token elsewhere
    let world = world(false);

    // When the drill is posted with the raw seed, and signed with no action or another
    let raw = world.send("POST", "/drill?action=stop", &legacy_headers(), b"").await;
    let rotating_only = world.send("POST", "/drill?action=stop", &[("authorization".to_string(), format!("Bearer {}", vm_ops_api_core::domain::auth::derive_rotating_token(SEED, &super::harness::today())))], b"").await;
    let none = world.signed("POST", "/drill", b"").await;
    let other = world.signed("POST", "/drill?action=reboot", b"").await;

    // Then the raw token and an unsigned rotating token are 401, the others 400, and no sibling started
    assert_eq!([(raw.status, raw.body.as_str()), (rotating_only.status, rotating_only.body.as_str())], [(401, r#"{"error":"unauthorized"}"#); 2]);
    assert_eq!((none.status, other.status), (400, 400));
    assert_eq!(none.body, r#"{"error":"action must be stop or freeze"}"#);
    assert!(world.siblings.calls().is_empty(), "{:?}", world.siblings.calls());
}
