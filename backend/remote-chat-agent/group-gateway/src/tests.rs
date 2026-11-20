use crate::domain::{provider_for_driver, Facts, Inbound, Reroute, Rule};
use crate::egress::{Destination, EgressPolicy};
use crate::gateway::{Gateway, Reply};
use crate::ports::TokenStore;
use crate::testing::{reply, CountingIds, FakeSidecar, FixedKeys, MemoryTokens, ScriptedUpstream};
use futures_util::StreamExt;
use rca_agent_core::bearers::token_digest;
use std::sync::Arc;

const REAL: &str = "sk-real-anthropic-key";

fn gateway(upstream: Arc<ScriptedUpstream>) -> (Gateway, Arc<MemoryTokens>) {
    let store = Arc::new(MemoryTokens::default());
    let keys = Arc::new(FixedKeys(vec![("GATEWAY_ANTHROPIC_API_KEY", REAL), ("GATEWAY_OPENAI_API_KEY", "sk-real-openai-key")]));
    (Gateway::new(store.clone(), keys, upstream, Arc::new(CountingIds::default())), store)
}

fn messages(body: &str) -> Inbound {
    Inbound { method: "POST".into(), path: "/v1/messages".into(), query: None, headers: vec![("anthropic-version".into(), "2023-06-01".into()), ("content-type".into(), "application/json".into())], body: body.as_bytes().to_vec() }
}

async fn drain(reply: Reply) -> (u16, Vec<Result<Vec<u8>, String>>) {
    match reply {
        Reply::Answer(answer) => (answer.status, answer.body.collect().await),
        Reply::Refused(refusal) => panic!("refused: {refusal:?}"),
    }
}

fn refused(reply: Reply) -> (u16, &'static str) {
    match reply {
        Reply::Refused(r) => (r.status, r.code),
        Reply::Answer(a) => panic!("answered {}", a.status),
    }
}

#[test]
fn given_rules_with_each_field_when_matched_against_a_request_then_every_set_field_must_match_and_a_model_may_end_in_a_star() {
    // Given a request for model claude-opus on /v1/messages of thread t1 at anthropic
    let facts = Facts { provider: "anthropic", route: "/v1/messages", model: Some("claude-opus-4"), thread: "t1" };
    // When rules are matched
    let hit = |rule: Rule| rule.matches(&facts);
    // Then a rule with no field matches, each field narrows it, and a star matches a model prefix
    assert!(hit(Rule::any()));
    assert!(hit(Rule::any().provider("anthropic").route("/v1/").model("claude-*").thread("t1")));
    assert!(!hit(Rule::any().provider("openai")));
    assert!(!hit(Rule::any().route("/v1/responses")));
    assert!(!hit(Rule::any().model("claude-sonnet*")));
    assert!(!hit(Rule::any().thread("t2")));
    // And a rule on a model never matches a request that names none
    assert!(!Rule::any().model("*").matches(&Facts { model: None, ..facts }));
}

#[test]
fn given_a_turn_of_each_driver_when_its_provider_is_looked_up_then_claude_codex_and_the_opencode_model_prefix_decide() {
    // Given/When the providers of the drivers are asked for
    let id = |driver, model| provider_for_driver(driver, model).map(|p| p.id);
    // Then Claude Code calls Anthropic, Codex OpenAI and opencode the provider its model is prefixed with; an unknown or unprefixed model is none
    assert_eq!(id("claudeAgent", Some("opus")), Some("anthropic"));
    assert_eq!(id("codex", None), Some("openai"));
    assert_eq!(id("opencode", Some("opencode-go/deepseek-v4.1-flash")), Some("opencode-go"));
    assert_eq!(id("opencode", Some("kimi-for-coding/k3")), Some("kimi"));
    assert_eq!((id("opencode", Some("unknown/x")), id("opencode", None), id("other", None)), (None, None, None));
}

#[tokio::test]
async fn given_a_minted_token_when_the_cli_calls_the_gateway_then_the_provider_sees_the_key_and_never_the_token() {
    // Given a gateway with a key and a token minted for the thread
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, store) = gateway(upstream.clone());
    let minted = gateway.mint("t1", "anthropic").unwrap();
    // When the CLI calls with the token in x-api-key, as Claude Code does with ANTHROPIC_API_KEY
    let mut call = messages(r#"{"model":"opus"}"#);
    call.headers.push(("x-api-key".into(), minted.token.clone()));
    let (status, body) = drain(gateway.handle("anthropic", Some(&minted.token), call).await).await;
    // Then the provider was called at its own address with the key only, and the CLI's other headers went on
    assert_eq!((status, body), (200, vec![Ok(b"ok".to_vec())]));
    let sent = &upstream.seen()[0];
    assert_eq!(sent.url(), "https://api.anthropic.com/v1/messages");
    assert_eq!(sent.headers.iter().filter(|(n, _)| n == "x-api-key").map(|(_, v)| v.as_str()).collect::<Vec<_>>(), [REAL]);
    assert!(sent.headers.iter().any(|(n, v)| n == "anthropic-version" && v == "2023-06-01"));
    assert!(sent.headers.iter().all(|(_, v)| !v.contains(&minted.token)));
    // And the store holds the digest of the token and not the token
    assert_eq!(store.digests(), [token_digest(&minted.token)]);
    assert!(!store.digests().contains(&minted.token));
}

#[tokio::test]
async fn given_a_bearer_provider_when_a_request_goes_on_then_the_key_is_an_authorization_header_and_the_query_is_kept() {
    // Given a token for OpenAI
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, _) = gateway(upstream.clone());
    let minted = gateway.mint("t1", "openai").unwrap();
    // When the CLI calls /responses with its token as a bearer and a query
    let call = Inbound { method: "POST".into(), path: "/responses".into(), query: Some("a=1".into()), headers: vec![("Authorization".into(), format!("Bearer {}", minted.token))], body: b"{}".to_vec() };
    drain(gateway.handle("openai", Some(&minted.token), call).await).await;
    // Then the provider got the key as the only authorization, at its address with the query
    let sent = &upstream.seen()[0];
    assert_eq!(sent.url(), "https://api.openai.com/v1/responses?a=1");
    assert_eq!(sent.headers.iter().filter(|(n, _)| n.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.as_str()).collect::<Vec<_>>(), ["Bearer sk-real-openai-key"]);
}

#[tokio::test]
async fn given_a_token_for_another_provider_or_none_or_a_dead_one_when_called_then_it_is_a_401_and_nothing_reaches_the_provider() {
    // Given a token minted for Anthropic
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, _) = gateway(upstream.clone());
    let minted = gateway.mint("t1", "anthropic").unwrap();
    // When it is presented at OpenAI, when none is presented, when a made-up one is, and after the turn released it
    let wrong = gateway.handle("openai", Some(&minted.token), messages("{}")).await;
    let none = gateway.handle("anthropic", None, messages("{}")).await;
    let made_up = gateway.handle("anthropic", Some("rcag_nope"), messages("{}")).await;
    gateway.release("t1");
    let released = gateway.handle("anthropic", Some(&minted.token), messages("{}")).await;
    let unknown = gateway.handle("nowhere", Some(&minted.token), messages("{}")).await;
    // Then each is refused with its own code, and the provider was never called
    assert_eq!(refused(wrong), (401, "GATEWAY_WRONG_SCOPE"));
    assert_eq!(refused(none), (401, "GATEWAY_TOKEN_MISSING"));
    assert_eq!(refused(made_up), (401, "GATEWAY_TOKEN_DEAD"));
    assert_eq!(refused(released), (401, "GATEWAY_TOKEN_DEAD"));
    assert_eq!(refused(unknown), (404, "GATEWAY_UNKNOWN_PROVIDER"));
    assert!(upstream.seen().is_empty());
}

#[tokio::test]
async fn given_a_thread_that_starts_another_turn_when_a_token_is_minted_again_then_the_old_token_is_dead_and_the_new_one_works() {
    // Given a thread with a token
    let (gateway, _) = gateway(ScriptedUpstream::streaming(&["ok"]));
    let first = gateway.mint("t1", "anthropic").unwrap();
    // When the next turn mints
    let second = gateway.mint("t1", "anthropic").unwrap();
    // Then only the new token opens the gateway, and another thread's token is untouched
    let other = gateway.mint("t2", "anthropic").unwrap();
    assert_eq!(refused(gateway.handle("anthropic", Some(&first.token), messages("{}")).await).1, "GATEWAY_TOKEN_DEAD");
    assert_eq!(drain(gateway.handle("anthropic", Some(&second.token), messages("{}")).await).await.0, 200);
    assert_eq!(drain(gateway.handle("anthropic", Some(&other.token), messages("{}")).await).await.0, 200);
}

#[tokio::test]
async fn given_a_provider_without_a_key_when_a_turn_is_minted_or_a_key_vanishes_then_nothing_is_routed_or_sent() {
    // Given a gateway with a key for Anthropic only, and one whose key is withdrawn after the mint
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, _) = gateway(upstream.clone());
    // When a turn for a provider without a key mints
    // Then it gets no token: the turn is not routed and keeps its own login
    assert!(gateway.mint("t1", "kimi").is_none());
    assert!(gateway.mint("t1", "nowhere").is_none());
    let store = Arc::new(MemoryTokens::default());
    let late = Gateway::new(store.clone(), Arc::new(FixedKeys(vec![])), upstream.clone(), Arc::new(CountingIds::default()));
    store.put(token_digest("rcag_x"), crate::domain::Grant { thread: "t".into(), provider: "anthropic".into(), reroutes: vec![] });
    // And a request whose key is gone is refused before anything is sent
    assert_eq!(refused(late.handle("anthropic", Some("rcag_x"), messages("{}")).await), (503, "GATEWAY_NO_KEY"));
    assert!(upstream.seen().is_empty());
}

#[tokio::test]
async fn given_a_streaming_answer_when_it_is_passed_on_then_the_chunks_arrive_in_order_before_the_provider_has_finished() {
    // Given a provider that answers 200 and then holds the stream open after two chunks
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Vec<u8>, String>>();
    let rx = std::sync::Mutex::new(Some(tokio_stream::wrappers::UnboundedReceiverStream::new(rx)));
    let upstream = ScriptedUpstream::answering(move |_| Ok(crate::ports::UpstreamReply { status: 200, headers: vec![], body: Box::pin(rx.lock().unwrap().take().unwrap()) }));
    let (gateway, _) = gateway(upstream);
    let minted = gateway.mint("t1", "anthropic").unwrap();
    let Reply::Answer(mut answer) = gateway.handle("anthropic", Some(&minted.token), messages("{}")).await else { panic!("refused") };
    // When the provider sends the chunks one at a time
    tx.send(Ok(b"event: a\n".to_vec())).unwrap();
    let first = answer.body.next().await;
    tx.send(Ok(b"event: b\n".to_vec())).unwrap();
    let second = answer.body.next().await;
    // Then each arrived as sent while the stream was still open, in order
    assert_eq!((first, second), (Some(Ok(b"event: a\n".to_vec())), Some(Ok(b"event: b\n".to_vec()))));
}

#[tokio::test]
async fn given_a_provider_that_fails_when_a_request_goes_on_then_it_fails_closed_at_the_head_and_in_the_body() {
    // Given a provider that is unreachable for one request and breaks its stream for the next
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let upstream = ScriptedUpstream::answering(move |_| match calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
        0 => Err("connection refused".into()),
        _ => Ok(reply(200, vec![Ok(b"partial".to_vec()), Err("connection reset".into())])),
    });
    let (gateway, _) = gateway(upstream);
    let minted = gateway.mint("t1", "anthropic").unwrap();
    // When the CLI calls twice
    let unreachable = gateway.handle("anthropic", Some(&minted.token), messages("{}")).await;
    let (status, body) = drain(gateway.handle("anthropic", Some(&minted.token), messages("{}")).await).await;
    // Then the first is a 502 with a clear code, and the second ends with the error after what came, never as a complete body
    assert_eq!(refused(unreachable), (502, "GATEWAY_UPSTREAM_FAILED"));
    assert_eq!((status, body), (200, vec![Ok(b"partial".to_vec()), Err("connection reset".into())]));
}

#[tokio::test]
async fn given_a_reroute_table_when_a_turn_starts_then_its_model_is_sent_on_to_the_target_and_a_later_table_does_not_move_the_turn() {
    // Given a table that serves claude-opus by claude-sonnet, and a turn that starts under it
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, _) = gateway(upstream.clone());
    gateway.set_reroutes(vec![Reroute::new("anthropic", "claude-opus*", "anthropic", "claude-sonnet-4").unwrap()]);
    let pinned = gateway.mint("t1", "anthropic").unwrap();
    let plain = gateway.mint("t2", "anthropic").unwrap();
    // When the table is emptied and both turns call
    gateway.set_reroutes(vec![]);
    drain(gateway.handle("anthropic", Some(&pinned.token), messages(r#"{"model":"claude-opus-4","max_tokens":1}"#)).await).await;
    drain(gateway.handle("anthropic", Some(&plain.token), messages(r#"{"model":"claude-haiku"}"#)).await).await;
    // Then the first still went to claude-sonnet-4 (the other fields kept), and the second to its own model
    let models: Vec<String> = upstream.seen().iter().map(|r| r.model().unwrap()).collect();
    assert_eq!(models, ["claude-sonnet-4", "claude-haiku"]);
    assert!(String::from_utf8_lossy(&upstream.seen()[0].body).contains(r#""max_tokens":1"#));
    // And a turn that starts after the change is not rerouted
    let after = gateway.mint("t3", "anthropic").unwrap();
    drain(gateway.handle("anthropic", Some(&after.token), messages(r#"{"model":"claude-opus-4"}"#)).await).await;
    assert_eq!(upstream.seen()[2].model().as_deref(), Some("claude-opus-4"));
}

#[tokio::test]
async fn given_a_reroute_when_one_turn_sends_requests_for_two_models_then_only_the_matching_model_is_rerouted() {
    // Given a table that serves claude-opus by claude-sonnet, and a turn started under an alias (the CLI's own name for its model)
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, _) = gateway(upstream.clone());
    gateway.set_reroutes(vec![Reroute::new("anthropic", "claude-opus*", "anthropic", "claude-sonnet-4").unwrap()]);
    let minted = gateway.mint("t1", "anthropic").unwrap();
    // When the CLI sends a request for the full opus id and a background one for its small model
    drain(gateway.handle("anthropic", Some(&minted.token), messages(r#"{"model":"claude-opus-4-1"}"#)).await).await;
    drain(gateway.handle("anthropic", Some(&minted.token), messages(r#"{"model":"claude-haiku-4-5"}"#)).await).await;
    // Then the first went to claude-sonnet-4 and the small model's request kept its model
    let models: Vec<String> = upstream.seen().iter().map(|r| r.model().unwrap()).collect();
    assert_eq!(models, ["claude-sonnet-4", "claude-haiku-4-5"]);
}

#[test]
fn given_two_protocols_when_a_reroute_is_made_then_only_the_same_protocol_is_one() {
    // Given Messages, Responses and a second chat-completions provider
    // When reroutes are made
    // Then Messages to Messages and chat to chat are served, and Messages to Responses is not a reroute
    assert!(Reroute::new("anthropic", "a", "anthropic", "b").is_ok());
    assert!(Reroute::new("opencode-go", "a", "openrouter", "b").is_ok());
    assert!(Reroute::new("anthropic", "a", "openai", "b").unwrap_err().contains("keeps the protocol"));
    assert!(Reroute::new("anthropic", "a", "nowhere", "b").is_err());
}

#[tokio::test]
async fn given_a_reroute_to_another_provider_of_the_same_protocol_when_the_turn_calls_then_that_provider_gets_it_with_its_own_key() {
    // Given a chat-completions turn at opencode-go rerouted to openrouter, and a gateway that holds both keys
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let keys = Arc::new(FixedKeys(vec![("GATEWAY_OPENCODE_GO_API_KEY", "go-key"), ("GATEWAY_OPENROUTER_API_KEY", "or-key")]));
    let gateway = Gateway::new(Arc::new(MemoryTokens::default()), keys, upstream.clone(), Arc::new(CountingIds::default()));
    gateway.set_reroutes(vec![Reroute::new("opencode-go", "deepseek*", "openrouter", "deepseek/deepseek-chat").unwrap()]);
    let minted = gateway.mint("t1", "opencode-go").unwrap();
    // When the CLI calls
    let call = Inbound { method: "POST".into(), path: "/chat/completions".into(), query: None, headers: vec![], body: br#"{"model":"deepseek-v4.1-flash"}"#.to_vec() };
    drain(gateway.handle("opencode-go", Some(&minted.token), call).await).await;
    // Then OpenRouter was called with its key and the target model
    let sent = &upstream.seen()[0];
    assert_eq!(sent.url(), "https://openrouter.ai/api/v1/chat/completions");
    assert_eq!(sent.model().as_deref(), Some("deepseek/deepseek-chat"));
    assert!(sent.headers.iter().any(|(n, v)| n == "authorization" && v == "Bearer or-key"));
}

fn headroom_gateway(sidecar: Arc<FakeSidecar>, rule: Rule) -> (Gateway, Arc<ScriptedUpstream>) {
    let upstream = ScriptedUpstream::streaming(&["ok"]);
    let (gateway, _) = gateway(upstream.clone());
    (gateway.with_headroom(rule, sidecar), upstream)
}

const LONG: &str = "line one of a long tool output line two of a long tool output line three of a long tool output";

fn body_with_tail(tail: &str) -> String {
    format!(r#"{{"model":"m","system":[{{"type":"text","text":"You are   an agent","cache_control":{{"type":"ephemeral"}}}}],"tools":[{{"name":"bash","input_schema":{{"type":"object"}}}}],"messages":[{{"role":"user","content":"first   question"}},{{"role":"assistant","content":[{{"type":"text","text":"{LONG}"}}]}},{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":"{tail}"}}]}}],"stream":true}}"#)
}

#[tokio::test]
async fn given_headroom_on_when_a_request_goes_on_then_only_the_tail_is_shortened_and_every_byte_before_it_is_the_one_sent() {
    // Given the compression stage on for every request, with a sidecar that keeps 10 characters
    let sidecar = FakeSidecar::keeping(10);
    let (gateway, upstream) = headroom_gateway(sidecar.clone(), Rule::any());
    let minted = gateway.mint("t1", "anthropic").unwrap();
    let sent_body = body_with_tail(LONG);
    // When a request whose last message holds a long tool result goes through
    drain(gateway.handle("anthropic", Some(&minted.token), messages(&sent_body)).await).await;
    // Then the provider got a body whose bytes up to the last message are the ones the CLI sent, the tail is shortened, and what follows is kept
    let got = String::from_utf8(upstream.seen()[0].body.clone()).unwrap();
    let cut = sent_body.rfind(r#"{"role":"user","content":[{"type":"tool_result""#).unwrap();
    assert_eq!(&got.as_bytes()[..cut], &sent_body.as_bytes()[..cut]);
    assert!(got.ends_with(r#""content":"line one o"}]}],"stream":true}"#), "{got}");
    // And the sidecar was asked about the tail only
    assert_eq!(*sidecar.calls.lock().unwrap(), [LONG]);
}

#[tokio::test]
async fn given_headroom_off_or_a_rule_that_does_not_match_or_a_broken_sidecar_when_a_request_goes_on_then_the_body_is_untouched() {
    let sent = body_with_tail(LONG);
    // Given the stage left off, the stage on for another model, and the stage on with a sidecar that fails
    let off = ScriptedUpstream::streaming(&["ok"]);
    let (plain, _) = gateway(off.clone());
    let (other_model, other) = headroom_gateway(FakeSidecar::keeping(3), Rule::any().model("claude-*"));
    let (broken, down) = headroom_gateway(FakeSidecar::broken(), Rule::any());
    // When the same request goes through each
    for (gw, name) in [(&plain, "plain"), (&other_model, "other"), (&broken, "broken")] {
        let minted = gw.mint(name, "anthropic").unwrap();
        drain(gw.handle("anthropic", Some(&minted.token), messages(&sent)).await).await;
    }
    // Then each sent the body as it came
    for upstream in [off, other, down] {
        assert_eq!(String::from_utf8(upstream.seen()[0].body.clone()).unwrap(), sent);
    }
}

#[tokio::test]
async fn given_headroom_on_for_a_responses_request_and_one_that_ends_with_an_assistant_turn_then_the_input_tail_is_shortened_and_the_assistant_turn_is_not() {
    // Given the stage on
    let (gateway, upstream) = headroom_gateway(FakeSidecar::keeping(4), Rule::any());
    let minted = gateway.mint("t1", "openai").unwrap();
    // When a Responses request ends with a tool output, and a Messages request ends with an assistant message
    let responses = format!(r#"{{"model":"m","instructions":"keep   me","input":[{{"type":"message","role":"user","content":"hello"}},{{"type":"function_call_output","call_id":"c","output":"{LONG}"}}]}}"#);
    let call = Inbound { method: "POST".into(), path: "/responses".into(), query: None, headers: vec![], body: responses.clone().into_bytes() };
    drain(gateway.handle("openai", Some(&minted.token), call).await).await;
    let anthropic = gateway.mint("t2", "anthropic").unwrap();
    let assistant = format!(r#"{{"messages":[{{"role":"assistant","content":"{LONG}"}}]}}"#);
    drain(gateway.handle("anthropic", Some(&anthropic.token), messages(&assistant)).await).await;
    // Then the tool output is cut to the sidecar's four characters with the instructions and the first item intact, and the assistant turn is as sent
    let first = String::from_utf8(upstream.seen()[0].body.clone()).unwrap();
    assert!(first.starts_with(&responses[..responses.find(r#"{"type":"function_call_output""#).unwrap()]), "{first}");
    assert!(first.ends_with(r#""output":"line"}]}"#), "{first}");
    assert_eq!(String::from_utf8(upstream.seen()[1].body.clone()).unwrap(), assistant);
}

#[test]
fn given_the_egress_policy_of_a_workspace_host_when_destinations_are_asked_then_only_the_service_is_allowed_and_the_policy_is_plain_data() {
    // Given the policy that lets a workspace host reach the service on 8787
    let policy = EgressPolicy::workspace_host(Destination { host: "agent.example.ts.net".into(), port: Some(8787) });
    // When destinations are asked
    // Then the service on its port is allowed, the model providers, GitHub, the same host on another port and a lookalike host are not
    assert!(policy.allows("agent.example.ts.net", 8787) && policy.allows("AGENT.example.ts.net", 8787));
    for (host, port) in [("api.anthropic.com", 443), ("api.openai.com", 443), ("github.com", 443), ("agent.example.ts.net", 22), ("agent.example.ts.net.evil.test", 8787)] {
        assert!(!policy.allows(host, port), "{host}:{port}");
    }
    // And it is data: it survives a round trip through JSON
    assert_eq!(serde_json::from_str::<EgressPolicy>(&serde_json::to_string(&policy).unwrap()).unwrap(), policy);
}
