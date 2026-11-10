//! `Http` and `OpencodeBackend` against `Loopback`, an opencode server on a loopback port.

use futures_util::StreamExt;
use rca_service_core::application::opencode_client::Client;
use rca_service_core::application::settings::Settings;
use rca_service_core::testing::MapEnv;
use remote_chat_agent_core::application::ports::{Backend, Opencode, SendTurn};
use remote_chat_agent_core::domain::types::ModelSelection;
use remote_chat_agent_core::testing::FixedClock;
use crate::testing::Loopback;
use crate::{Http, OpencodeBackend};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn backend(server: &Loopback) -> OpencodeBackend {
    let env = MapEnv::new(PathBuf::from("/home"), None, PathBuf::from("/tmp"));
    let client = Client::new(Arc::new(Http::new(&server.url(), None)), Arc::new(FixedClock(0)), Arc::new(env));
    OpencodeBackend::new(client, Box::new(|thread| format!("/work/{thread}")), &Settings::from_lookup(&|_| None))
}

#[tokio::test]
async fn given_a_password_when_a_request_is_sent_then_the_server_sees_the_basic_credential_and_the_json_body() {
    // Given a server that answers 201 and a transport configured with a username and password
    let server = Loopback::start().await;
    server.reply("POST", "/thing", 201, r#"{"made":true}"#);
    let http = Http::new(&format!("{}/", server.url()), Some(("opencode".into(), "secret".into())));

    // When a request with a body is sent
    let reply = http.request("POST", "/thing?x=1", Some(&json!({"a": 1})), Duration::from_secs(5)).await.expect("a reply");

    // Then the reply carries the status and body, and the server saw the credential, the target and the body
    assert_eq!((reply.status, String::from_utf8_lossy(&reply.body).into_owned()), (201, r#"{"made":true}"#.to_string()));
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].method.as_str(), seen[0].target.as_str(), seen[0].body.as_str()), ("POST", "/thing?x=1", r#"{"a":1}"#));
    assert_eq!(seen[0].authorization.as_deref(), Some("Basic b3BlbmNvZGU6c2VjcmV0"));
}

#[tokio::test]
async fn given_scripted_events_when_the_stream_is_opened_then_each_data_line_arrives_in_order() {
    // Given a server with two scripted events
    let server = Loopback::start().await;
    server.events(vec![json!({"type": "a"}), json!({"type": "b"})]);
    let http = Http::new(&server.url(), None);

    // When the event stream is opened and read to its end
    let data: Vec<String> = http.open_stream("/event").await.expect("a stream").map(|d| d.expect("data")).collect().await;

    // Then the events came as their JSON text, in order
    let kinds: Vec<Value> = data.iter().map(|d| serde_json::from_str::<Value>(d).unwrap()["type"].clone()).collect();
    assert_eq!(kinds, vec![json!("a"), json!("b")]);
}

#[tokio::test]
async fn given_a_refused_subscription_when_the_stream_is_opened_then_the_status_is_the_error() {
    // Given a server that refuses the event path
    let server = Loopback::start().await;
    server.reply("GET", "/nope", 503, "{}");

    // When the stream is opened
    let opened = Http::new(&server.url(), None).open_stream("/nope").await;

    // Then it fails with the status in the message
    assert_eq!(opened.err().as_deref(), Some("opencode /nope responded with 503"));
}

#[tokio::test]
async fn given_a_thread_when_a_session_starts_and_a_turn_is_sent_then_opencode_gets_the_session_and_the_prompt_for_its_directory() {
    // Given a backend over the loopback server
    let server = Loopback::start().await;
    let backend = backend(&server);

    // When a session starts for a thread and a plan-mode turn with a chosen model is sent
    let session = backend.start_session("t1", "full-access").await.expect("a session");
    let model = ModelSelection { cli: "opencode".into(), provider_id: Some("p".into()), model_id: Some("m".into()) };
    backend.send_turn(&SendTurn { thread_id: "t1", text: "hi", model: &model, runtime_mode: "full-access", interaction_mode: "plan", files: &[] }).await.expect("sent");

    // Then the session was created in the thread's directory with the allow-all rules, and the prompt carries text, model and plan agent
    assert_eq!((session.as_str(), backend.session_for("t1").as_deref()), ("ses_loopback", Some("ses_loopback")));
    let seen = server.seen();
    let targets: Vec<(&str, &str)> = seen.iter().map(|s| (s.method.as_str(), s.target.as_str())).collect();
    assert_eq!(targets, vec![("POST", "/session?directory=%2Fwork%2Ft1"), ("PATCH", "/session/ses_loopback"), ("POST", "/session/ses_loopback/prompt_async?directory=%2Fwork%2Ft1")]);
    let created: Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(created["permission"], json!([{ "permission": "*", "pattern": "*", "action": "allow" }]));
    let prompt: Value = serde_json::from_str(&seen[2].body).unwrap();
    assert_eq!(prompt, json!({ "parts": [{ "type": "text", "text": "hi" }], "model": { "providerID": "p", "modelID": "m" }, "agent": "plan" }));
}

#[tokio::test]
async fn given_an_idle_event_when_events_are_opened_then_a_completed_turn_comes_before_the_idle() {
    // Given a server whose stream carries one session.idle
    let server = Loopback::start().await;
    server.events(vec![json!({ "type": "session.idle", "properties": { "sessionID": "s9" } })]);

    // When the backend's events are read
    let events: Vec<_> = backend(&server).open_events("/work").await.expect("a stream").map(|e| e.expect("an event")).collect().await;

    // Then the gateway's turn completion precedes the idle itself
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, vec!["session.turn.completed", "session.idle"]);
    assert_eq!(events[0].props, json!({ "sessionID": "s9", "state": "completed" }));
}
