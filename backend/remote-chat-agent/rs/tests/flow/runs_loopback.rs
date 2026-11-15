//! The one-shot run over a real HTTP opencode: the service's production transport (`Http`) against `Loopback`, an opencode server on a loopback port,
//! so the request paths, the event stream and the body the service sends are what a real opencode would see.
use crate::doubles::{idle, says};
use crate::harness::{post, service_over_loopback, Env, Options};
use axum::http::StatusCode;
use remote_chat_agent_opencode::testing::Loopback;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_opencode_server_on_a_loopback_port_when_a_run_names_a_model_then_the_session_prompt_and_event_stream_go_over_http_and_the_text_comes_back() {
    // Given a loopback opencode that creates the session "s1" and streams one answer, and the service over its URL
    let env = Env::new(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    let repo = env.dir("repo");
    std::fs::create_dir(repo.join(".git")).unwrap();
    env.set("OPENCODE_DISPATCH_RUN_LOG", &env.home.join("runs.jsonl").to_string_lossy());
    env.set("OPENCODE_POOL_LEDGER", &env.home.join("pools.json").to_string_lossy());
    let server = Loopback::start().await;
    server.reply("POST", "/session", 200, r#"{"id":"s1"}"#);
    server.events(vec![says("s1", "hello over http"), idle("s1")]);
    let svc = service_over_loopback(&env, Options::default(), &server.url());
    svc.commands.answer(|r: &crate::doubles::Run| (r.program == "git").then(|| r.ok("")).flatten());

    // When a run pins a model
    let cwd = repo.to_string_lossy().into_owned();
    let served = svc.send(post("/runs").secret().json(json!({ "prompt": "p1", "shape": "document", "models": ["pp/m1"], "cwd": cwd, "auto": true }))).await;

    // Then the text came back, and opencode saw the session made in the directory and a prompt carrying the model and the text
    assert_eq!(served.status, StatusCode::OK, "{}", served.text());
    assert_eq!(served.json()["output"]["text"], json!("hello over http"));
    let seen = server.seen();
    let prompt = seen.iter().find(|r| r.method == "POST" && r.target.contains("/prompt_async")).expect("a prompt reached opencode");
    let body: serde_json::Value = serde_json::from_str(&prompt.body).unwrap();
    assert_eq!((body["model"].clone(), body["parts"].clone()), (json!({ "providerID": "pp", "modelID": "m1" }), json!([{ "type": "text", "text": "p1" }])));
    assert!(seen.iter().any(|r| r.method == "POST" && r.target.starts_with("/session?directory=")), "{:?}", seen.iter().map(|r| r.target.clone()).collect::<Vec<_>>());
}
