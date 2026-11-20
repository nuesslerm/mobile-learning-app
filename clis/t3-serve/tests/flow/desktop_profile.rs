//! `t3 serve` as the bearer connection profile a desktop stores (docs/solutions/workflow/t3-desktop-bearer-profile.md): the steps of the
//! desktop's onboarding in the order `client-runtime/src/connection/onboarding.ts` takes them, against the real router.
use super::support::*;

/// The desktop's compatibility check of the descriptor (`connection/compatibility.ts`): the protocol it speaks, and whether a refusal would
/// offer the server an update.
fn compatibility(descriptor: &Value) -> (bool, bool) {
    let version = descriptor["orchestrationProtocolVersion"].as_u64().unwrap_or(1);
    let can_self_update = descriptor["capabilities"].get("serverSelfUpdate").is_some_and(|c| c != "desktop-managed" || descriptor["capabilities"]["desktopAppUpdate"] == json!(true));
    (version == 2, version < 2 && can_self_update)
}

#[test]
fn a_desktop_walks_descriptor_compatibility_code_redemption_ticket_and_socket_to_a_first_call() {
    // Given a server and a pairing code minted for the desktop
    let h = Harness::start();
    let code = h.pair(PairingRequest { label: "desktop".into(), ..PairingRequest::default() }).code;
    // When the desktop fetches the descriptor it will store as the profile
    let descriptor = http(h.addr, "GET", "/.well-known/t3/environment", &[], "");
    // Then it is readable from any origin, the protocol is the one the desktop speaks, and no update is ever asked of this server
    assert_eq!(descriptor.status, 200);
    assert!(descriptor.head.contains("access-control-allow-origin: *"), "{}", descriptor.head);
    assert_eq!(compatibility(&descriptor.body), (true, false));
    let profile = json!({"environmentId": descriptor.body["environmentId"], "label": descriptor.body["label"], "httpBaseUrl": format!("http://{}", h.addr), "wsBaseUrl": format!("ws://{}", h.addr)});
    assert_eq!(profile["environmentId"], ENVIRONMENT_ID);
    // When it redeems the code, asks for the socket ticket the profile's bearer allows, and opens the socket
    let token = exchange(h.addr, &code);
    assert_eq!((token.status, token.body["token_type"].clone()), (200, json!("Bearer")));
    assert!(token.head.contains("access-control-allow-origin: *"), "{}", token.head);
    let bearer = token.body["access_token"].as_str().unwrap();
    let ticket = http(h.addr, "POST", "/api/auth/websocket-ticket/rpc", &[("Authorization", &format!("Bearer {bearer}"))], "");
    assert_eq!(ticket.status, 200, "{:?}", ticket.body);
    let mut rpc = open_socket(h.addr, &format!("orchestrationProtocol=2&wsTicket={}", ticket.body["ticket"].as_str().unwrap())).expect("the socket opens");
    // Then the desktop's connection probe answers, and the lifecycle stream welcomes it to the environment it stored
    assert_eq!(success(&rpc.call("server.probe", json!({}))), &json!({}));
    let id = rpc.send("subscribeServerLifecycle", json!({}));
    let welcome = rpc.chunk(&id);
    assert_eq!((welcome[0]["type"].clone(), welcome[0]["payload"]["environment"]["environmentId"].clone()), (json!("welcome"), json!(ENVIRONMENT_ID)));
}

#[test]
fn a_code_redeemed_by_the_desktop_cannot_be_redeemed_again_and_the_health_check_needs_no_credential() {
    // Given a code the desktop has redeemed
    let h = Harness::start();
    let code = h.pair(PairingRequest::default()).code;
    assert_eq!(exchange(h.addr, &code).status, 200);
    // When a second client tries the same code, and the desktop's reachability check reads /health
    let again = exchange(h.addr, &code);
    let health = http(h.addr, "GET", "/health", &[], "");
    // Then the code is refused, and the health answer is open
    assert_eq!(again.status, 401, "{:?}", again.body);
    assert_eq!((health.status, health.body["ok"].clone()), (200, json!(true)));
}

#[test]
fn a_descriptor_that_names_an_older_protocol_without_self_update_blocks_without_offering_an_update() {
    // Given the descriptor of a server that speaks protocol 1 and cannot update itself
    let mut descriptor = http(Harness::start().addr, "GET", "/.well-known/t3/environment", &[], "").body;
    descriptor["orchestrationProtocolVersion"] = json!(1);
    // When the desktop checks compatibility
    // Then the connection is blocked and `serverUpdateRequired` is not set
    assert_eq!(compatibility(&descriptor), (false, false));
    // And the same server announcing a self-update capability would be offered one
    descriptor["capabilities"]["serverSelfUpdate"] = json!("in-place");
    assert_eq!(compatibility(&descriptor), (false, true));
}
