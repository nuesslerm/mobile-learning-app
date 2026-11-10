//! The phone's transport and the host's own status through the chain: pairing, the handshake, authentication and the first methods. Each test
//! names the coverage rows its calls exercise and keeps the trace of what each call did; a row is served only by a call that was answered.
use crate::rig::*;
use orca_serve::testing::PhoneClient;
use serde_json::json;

fn agent_methods(case: &exec_testkit::Case) -> Vec<String> {
    case.calls().last().unwrap()["agent"].as_array().unwrap().iter().filter(|h| h["ok"] == true).map(|h| h["method"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_paired_phone_handshakes_and_asks_the_host_what_it_is_and_the_status_comes_from_the_real_service() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a phone paired by `orca-serve pair`
        let chain = Chain::start(None);
        // When it handshakes, authenticates and asks for the status, the platform and to be told its client capabilities were taken
        let mut phone = Phone::connect(case, &chain);
        let status = phone.call(case, &chain, &["rpc:status.get"], "status.get", json!({}));
        let status_agent = agent_methods(case);
        let platform = phone.call(case, &chain, &["rpc:host.platform"], "host.platform", json!({}));
        let caps = phone.call(case, &chain, &["rpc:runtime.clientCapabilities.update"], "runtime.clientCapabilities.update", json!({"clientCapabilities": ["mobile.test.v1"]}));
        // Then the answers are the host's, the status asked the real service once, and nothing is degraded
        assert_eq!(status["result"]["machineName"], "chain host", "{}", chain.evidence());
        assert_eq!(status["result"]["hostPlatform"], platform["result"]["platform"]);
        assert!(status["result"].get("degradations").is_none(), "the real service answers service.status: {status}");
        assert_eq!(status_agent, ["service.status"], "status.get went to rca's agent API through the tap");
        assert_eq!(caps["result"], json!({"clientCapabilities": ["mobile.test.v1"]}));
    });
}

#[test]
fn a_phone_opens_the_client_events_stream_and_ends_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given a connected phone
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        // When it subscribes to client events and then unsubscribes by the id it was given
        let opened = case.call(&chain.tap, &rows(&["rpc:runtime.clientEvents.subscribe"]), || {
            let id = phone.raw().send_request("runtime.clientEvents.subscribe", json!({}));
            let ready = phone.raw().next_stream_frame(&id);
            (ready["ok"] == true && ready["result"]["type"] == "ready", (id, ready))
        });
        let (stream, ready) = opened;
        let subscription = ready["result"]["subscriptionId"].as_str().expect("the ready event names the subscription").to_string();
        let ended = phone.call(case, &chain, &["rpc:runtime.clientEvents.unsubscribe"], "runtime.clientEvents.unsubscribe", json!({"subscriptionId": subscription}));
        // Then the stream's first event reads rca's state (nativeChatQueueFollowUps comes from state.get), the unsubscribe is taken, and the stream ends
        assert_eq!(ready["streaming"], true);
        assert!(ready["result"]["snapshot"]["nativeChatQueueFollowUps"].is_boolean(), "{ready}");
        assert!(case.calls().iter().any(|c| c["agent"].as_array().unwrap().iter().any(|h| h["method"] == "state.get" && h["ok"] == true)), "the snapshot did not read rca's state");
        assert_eq!(ended["result"], json!({"unsubscribed": true}));
        let end = phone.raw().next_stream_frame(&stream);
        assert_eq!(end["result"], json!({"type": "end"}));
    });
}

#[test]
fn a_phone_with_a_wrong_token_gets_unauthorized_and_a_closed_socket_and_rca_is_never_asked() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a phone that knows the host key but not a token
        let chain = Chain::start(None);
        let offer = chain.pair();
        let mut client = PhoneClient::connect(chain.addr, offer.host_public().unwrap());
        // When it handshakes and sends a token that was never minted (the call is a refusal, which the trace records as not served)
        let answer = case.call(&chain.tap, &rows(&["transport:e2ee.handshake"]), || {
            client.hello_v2().expect("the handshake needs no token");
            let answer = client.authenticate("a-token-nobody-minted").expect("an answer");
            (false, answer)
        });
        let closed = client.wait_closed();
        // Then it is told unauthorized, sealed, the socket is closed with the auth code, and the service saw no call from the mapper for it
        assert_eq!(answer["type"], "e2ee_error", "{answer}");
        assert_eq!(answer["error"]["code"], "unauthorized");
        assert_eq!(closed.code, 4001, "{closed:?}");
        let window = case.calls().last().unwrap();
        assert!(window["agent"].as_array().unwrap().is_empty(), "rca was asked for a phone that never authenticated: {window}");
    });
}
