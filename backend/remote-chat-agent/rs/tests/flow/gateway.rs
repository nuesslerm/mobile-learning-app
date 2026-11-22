//! The model gateway end to end through the router `main` serves. The providers are an authored scripted upstream (no recording of a provider exists
//! here), and the CLI of a turn is the scripted CLI of `exec-testkit` started by the real executor, which logs the environment it was given: what a
//! routed CLI holds is the token of its turn, and the provider's key is only ever in the service.
use crate::agent_threads::{chain_with, create, said, start_turn};
use crate::harness::{eventually, post, service, Env, GatewayDoubles, Options};
use axum::http::StatusCode;
use exec_testkit::script::Turn;
use rca_group_gateway::testing::{reply, ScriptedUpstream};
use serde_json::{json, Value};
use std::sync::Arc;

const REAL: &str = "sk-ant-real-key-never-in-a-cli";
const REAL_OPENAI: &str = "sk-openai-real-key-never-in-a-cli";
const REAL_GO: &str = "go-real-key-never-in-a-cli";

fn keyed(upstream: &Arc<ScriptedUpstream>) -> Options {
    Options { gateway: Some(GatewayDoubles { upstream: upstream.clone(), keys: vec![("GATEWAY_ANTHROPIC_API_KEY", REAL), ("GATEWAY_OPENAI_API_KEY", REAL_OPENAI), ("GATEWAY_OPENCODE_GO_API_KEY", REAL_GO)] }), ..Options::default() }
}

const WATCHED: [&str; 7] = ["ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "GATEWAY_ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "CODEX_GATEWAY_TOKEN", "OPENCODE_CONFIG_CONTENT", "GATEWAY_OPENCODE_GO_API_KEY"];

fn held(turn: Turn) -> Turn {
    turn.then_sleeping(2500).logging_env(&WATCHED)
}

fn call(provider: &str, token: &str) -> crate::harness::Sent {
    post(&format!("/gateway/{provider}/v1/messages")).header("x-api-key", token).json(json!({"model": "opus", "max_tokens": 1}))
}

fn go_call(provider: &str, token: &str) -> crate::harness::Sent {
    post(&format!("/gateway/{provider}/chat/completions")).header("authorization", &format!("Bearer {token}")).json(json!({"model": "deepseek-v4.1-flash"}))
}

async fn start_opencode(agent: &mut crate::mapper::Agent, thread: &str) {
    agent.ask("turn.start", json!({"thread": thread, "workspace": "w1", "driver": "opencode", "model": "opencode-go/deepseek-v4.1-flash", "prompt": "go"})).await;
}

fn opencode_token(start: &exec_testkit::script::Invocation) -> String {
    let config: Value = serde_json::from_str(start.env["OPENCODE_CONFIG_CONTENT"].as_deref().expect("a routed CLI is given a config")).unwrap();
    config["provider"]["opencode-go"]["options"]["apiKey"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_gateway_with_a_key_when_an_opencode_turn_runs_then_the_cli_holds_a_token_and_the_gateway_address_never_the_key_and_the_provider_gets_the_key() {
    // Given a service whose gateway holds the opencode-go key over a scripted provider, and an opencode CLI whose turn is held open and logs its environment
    let env = Env::new(&[]);
    let upstream = ScriptedUpstream::streaming(&["data: one\n\n", "data: two\n\n"]);
    let chain = chain_with(&env, keyed(&upstream)).await;
    chain.cli.script("opencode", &exec_testkit::script::Script::of(vec![held(Turn::default()), held(Turn::default())]).printing_version("0.0.0-flow"));
    let mut agent = chain.agent().await;
    agent.ask("thread.create", json!({"thread": "t1", "title": "n", "workspace": "w1", "driver": "opencode"})).await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    // When a turn starts, and the CLI calls the gateway with what it was given
    start_opencode(&mut agent, "t1").await;
    eventually("the CLI started", || !chain.cli.invocations("opencode").iter().all(|i| !i.argv.contains(&"go".to_string()))).await;
    let start = chain.cli.invocations("opencode").into_iter().find(|i| i.argv.contains(&"go".to_string())).unwrap();
    let token = opencode_token(&start);
    let answer = chain.service().send(go_call("opencode-go", &token)).await;
    // Then the CLI was given the gateway's address and a token of its own, and nothing of the provider's key anywhere it could read
    let config: Value = serde_json::from_str(start.env["OPENCODE_CONFIG_CONTENT"].as_deref().unwrap()).unwrap();
    assert_eq!(config["provider"]["opencode-go"]["options"]["baseURL"], json!(format!("http://127.0.0.1:{}/gateway/opencode-go", chain.port)));
    assert!(token.starts_with("rcag_") && token != REAL_GO, "{token}");
    assert_eq!(start.env["GATEWAY_OPENCODE_GO_API_KEY"], None);
    assert!(!format!("{:?} {:?}", start.argv, start.env).contains(REAL_GO));
    // And the gateway streamed the provider's answer, which was called with the key and not with the token
    assert_eq!((answer.status, answer.text()), (StatusCode::OK, "data: one\n\ndata: two\n\n".to_string()));
    let sent = &upstream.seen()[0];
    assert_eq!(sent.url(), "https://opencode.ai/zen/go/v1/chat/completions");
    assert!(sent.headers.iter().any(|(n, v)| n == "authorization" && v == &format!("Bearer {REAL_GO}")) && sent.headers.iter().all(|(n, v)| n == "authorization" || !v.contains(&token)));
    // And the token is the turn's alone: another provider refuses it, the service secret opens nothing, and a restarted service knows no token
    assert_eq!(chain.service().send(go_call("openai", &token)).await.json()["error"]["code"], json!("GATEWAY_WRONG_SCOPE"));
    assert_eq!(chain.service().send(go_call("opencode-go", crate::harness::SECRET)).await.status, StatusCode::UNAUTHORIZED);
    let restarted = service(&env, keyed(&upstream));
    let dead = restarted.send(go_call("opencode-go", &token)).await;
    assert_eq!((dead.status, dead.json()["error"]["code"].clone()), (StatusCode::UNAUTHORIZED, json!("GATEWAY_TOKEN_DEAD")));
    assert!(dead.json()["error"]["message"].as_str().unwrap().contains("start the turn again"));
    // When the turn ends, its token is dead, and the next turn of the thread is given another that works
    agent.events_through("turn.finished").await;
    assert_eq!(chain.service().send(go_call("opencode-go", &token)).await.json()["error"]["code"], json!("GATEWAY_TOKEN_DEAD"));
    start_opencode(&mut agent, "t1").await;
    eventually("the second turn started", || chain.cli.invocations("opencode").iter().filter(|i| i.argv.contains(&"go".to_string())).count() == 2).await;
    let fresh = opencode_token(chain.cli.invocations("opencode").iter().filter(|i| i.argv.contains(&"go".to_string())).nth(1).unwrap());
    assert_ne!(fresh, token);
    assert_eq!(chain.service().send(go_call("opencode-go", &fresh)).await.status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_gateway_with_keys_when_an_opencode_and_a_codex_turn_run_then_opencode_is_pointed_at_its_provider_with_the_turn_token_and_codex_keeps_its_own_login() {
    // Given a gateway that holds the OpenAI and opencode-go keys, an opencode CLI whose turn is held open, and a Codex server that answers a turn
    let env = Env::new(&[]);
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let chain = chain_with(&env, keyed(&upstream)).await;
    chain.cli.script("opencode", &exec_testkit::script::Script::of(vec![held(Turn::default())]).printing_version("0.0.0-flow"));
    let codex_turn = exec_testkit::script::Run::new().handshake().thread_start("th-1").turn_saying("th-1", "u-1", "ok");
    chain.cli.script("codex", &exec_testkit::script::Script::default().printing_version("0.0.0-flow").playing_app_server(vec![codex_turn.into()]));
    let mut agent = chain.agent().await;
    for (thread, driver) in [("tc", "codex"), ("to", "opencode")] {
        let made = agent.ask("thread.create", json!({"thread": thread, "title": "n", "workspace": "w1", "driver": driver})).await;
        assert!(made.get("result").is_some(), "{made}");
    }
    // When a turn of each starts
    agent.ask("turn.start", json!({"thread": "tc", "workspace": "w1", "driver": "codex", "prompt": "go"})).await;
    agent.ask("turn.start", json!({"thread": "to", "workspace": "w1", "driver": "opencode", "model": "opencode-go/deepseek-v4.1-flash", "prompt": "go"})).await;
    // (the executor also asks each CLI for its version, which is no turn)
    let turn_of = |program: &str| chain.cli.invocations(program).into_iter().find(|i| i.argv.contains(&"go".to_string()));
    eventually("both turns started", || turn_of("opencode").is_some() && chain.cli.app_server_log("codex").iter().any(|r| r.message["method"] == "turn/start")).await;
    let opencode = turn_of("opencode").unwrap();
    // Then opencode's config names the baseURL and the token of its provider
    let base = format!("http://127.0.0.1:{}/gateway", chain.port);
    let config: Value = serde_json::from_str(opencode.env["OPENCODE_CONFIG_CONTENT"].as_deref().expect("opencode is given a config")).unwrap();
    assert_eq!(config["provider"]["opencode-go"]["options"]["baseURL"], json!(format!("{base}/opencode-go")));
    let opencode_token = config["provider"]["opencode-go"]["options"]["apiKey"].as_str().unwrap().to_string();
    // And Codex's server was sent no gateway address and no token, since it keeps the login of the host (`docs/gaps/codex-session-not-through-gateway.md`)
    let sent = chain.cli.app_server_log("codex").iter().map(|r| r.message.to_string()).collect::<Vec<_>>().join("\n");
    assert!(!sent.contains("/gateway") && !sent.contains("rcag_") && !sent.contains("model_provider"), "{sent}");
    // And neither holds a provider key, and the opencode token opens its own provider: the key is what that provider is called with
    for seen in [format!("{:?} {:?}", opencode.argv, opencode.env), sent] {
        assert!(!seen.contains(REAL_OPENAI) && !seen.contains(REAL_GO) && !seen.contains(REAL), "{seen}");
    }
    let chat = chain.service().send(post("/gateway/opencode-go/chat/completions").header("authorization", &format!("Bearer {opencode_token}")).json(json!({"model": "deepseek-v4.1-flash"}))).await;
    assert_eq!(chat.status, StatusCode::OK, "{}", chat.text());
    let seen = upstream.seen();
    assert_eq!(seen[0].url(), "https://opencode.ai/zen/go/v1/chat/completions");
    assert!(seen[0].headers.iter().any(|(n, v)| n == "authorization" && v == &format!("Bearer {REAL_GO}")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_gateway_that_holds_no_key_for_the_provider_when_a_turn_runs_then_it_is_not_routed_and_the_cli_keeps_its_own_login() {
    // Given a service whose gateway holds only an OpenAI key, and a Claude Code CLI (a subscription login lives at its host, and Claude Code is never routed: `docs/gaps/claude-session-not-through-gateway.md`)
    let env = Env::new(&[]);
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let options = Options { gateway: Some(GatewayDoubles { upstream: upstream.clone(), keys: vec![("GATEWAY_OPENAI_API_KEY", REAL_OPENAI)] }), ..Options::default() };
    let chain = chain_with(&env, options).await;
    chain.script(vec![Turn::replaying(&[said("hi")]).logging_env(&WATCHED)]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    // When a turn starts and ends
    start_turn(&mut agent, "t1").await;
    agent.events_through("turn.finished").await;
    // Then the CLI was started as it always is: no gateway address, no token, no blanked subscription token, and the provider was never called
    let start = chain.cli.invocations("claude").remove(0);
    assert!(start.env.values().all(Option::is_none), "{:?}", start.env);
    assert!(upstream.seen().is_empty());
}

#[tokio::test]
async fn given_a_minted_token_when_the_provider_answers_in_parts_then_the_parts_arrive_in_order_and_a_provider_that_fails_fails_the_call() {
    // Given a gateway over a provider that answers three chunks, then breaks a stream after one, then does not answer
    let env = Env::new(&[]);
    let script = std::sync::atomic::AtomicUsize::new(0);
    let upstream = ScriptedUpstream::answering(move |_| match script.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
        0 => Ok(reply(200, vec![Ok(b"a".to_vec()), Ok(b"b".to_vec()), Ok(b"c".to_vec())])),
        1 => Ok(reply(200, vec![Ok(b"a".to_vec()), Err("reset by peer".into())])),
        _ => Err("connection refused".into()),
    });
    let svc = service(&env, keyed(&upstream));
    let token = svc.gateway().mint("t1", "anthropic").expect("a key is held").token;
    // When the CLI calls three times
    let whole = svc.send_streamed(call("anthropic", &token)).await;
    let broken = svc.send_streamed(call("anthropic", &token)).await;
    let down = svc.send(call("anthropic", &token)).await;
    // Then the first is the chunks in order, the second ends with a failure after what came (never as a complete answer), and the third is a 502 that says why
    assert_eq!(whole, (StatusCode::OK, b"abc".to_vec(), None));
    assert_eq!((broken.0, broken.1), (StatusCode::OK, b"a".to_vec()));
    assert!(broken.2.is_some_and(|why| why.contains("reset by peer")), "the body ended as a complete one");
    assert_eq!((down.status, down.json()["error"]["code"].clone()), (StatusCode::BAD_GATEWAY, json!("GATEWAY_UPSTREAM_FAILED")));
    assert!(down.json()["error"]["message"].as_str().unwrap().contains("connection refused"));
}

#[tokio::test]
async fn given_a_reroute_table_when_a_turn_starts_then_the_provider_is_called_with_the_target_model_and_a_table_changed_later_does_not_move_it() {
    // Given a gateway that serves claude-opus by claude-sonnet, and a turn that starts under the table
    let env = Env::new(&[]);
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let svc = service(&env, keyed(&upstream));
    svc.gateway().set_reroutes(vec![rca_group_gateway::Reroute::new("anthropic", "claude-opus*", "anthropic", "claude-sonnet-4").unwrap()]);
    let token = svc.gateway().mint("t1", "anthropic").unwrap().token;
    // When the table is cleared and the CLI calls
    svc.gateway().set_reroutes(vec![]);
    svc.send(post("/gateway/anthropic/v1/messages").header("x-api-key", &token).json(json!({"model": "claude-opus-4", "max_tokens": 1}))).await;
    // Then the provider was asked for claude-sonnet-4, the model the turn began with
    assert_eq!(upstream.seen()[0].model().as_deref(), Some("claude-sonnet-4"));
}
