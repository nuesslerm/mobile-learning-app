//! Against the deployed service (REMOTE_CHAT_AGENT_BASE_URL, default http://remote-chat-agent-prod:8787): what it logs reaches BetterStack,
//! and `POST /runs` does what the opencode options it mirrors do, with a real model behind it: a session continues, a read-only agent cannot
//! edit, a run's changes come back, an agent file and a variant are honoured, and token usage is reported. With `WF_DEPLOYED=1` it also runs one
//! real turn per provider over the agent API (`/agent/v1/socket`, a device bearer minted for the run and revoked after it), in the workspace whose executor
//! `RCA_VERIFY_WORKSPACE` names: a real model answers, which costs a turn of each provider's own credential.

use agent_api_proto::Event;
use futures_util::{SinkExt, StreamExt};
use rca_cli_driver::domain::{command_for_remote, Driver, TurnRequest, DRIVERS};
use serde_json::{json, Value};
use std::collections::VecDeque;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

fn base() -> String {
    std::env::var("REMOTE_CHAT_AGENT_BASE_URL").unwrap_or_else(|_| "http://remote-chat-agent-prod:8787".to_string())
}

/// `POST /runs` with the bot secret; the answer is its JSON, the run's ladder having picked the model.
async fn run(body: Value) -> Value {
    let secret = service_kit::e2e::setting("CHAT_AGENT_API_SECRET");
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build().unwrap();
    let reply = client.post(format!("{}/runs", base())).header("x-bot-secret", secret).json(&body).send().await.expect("the deployed service answers");
    let status = reply.status();
    let answer: Value = reply.json().await.expect("a JSON answer");
    assert!(status.is_success(), "POST /runs answered {status}: {answer}");
    answer
}

fn text(answer: &Value) -> String {
    answer["output"]["text"].as_str().unwrap_or_default().to_string()
}


#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_a_request_in_a_fresh_trace_when_the_deployed_service_answers_then_its_log_line_reaches_betterstack() {
    let base = std::env::var("REMOTE_CHAT_AGENT_BASE_URL").unwrap_or_else(|_| "http://remote-chat-agent-prod:8787".to_string());
    let line = service_kit::e2e::logged_request(&format!("{base}/health")).await;
    service_kit::e2e::assert_inferred(&line, "remote-chat-agent");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_a_session_when_the_next_turn_names_it_then_the_model_remembers_and_the_session_and_usage_are_reported() {
    // Given a first turn that sets a word, read-only, with a title
    let first = run(json!({ "prompt": "Remember the secret word PELICAN. Reply with only OK.", "shape": "document", "auto": false, "title": "options e2e" })).await;
    let session = first["session"].as_str().expect("the answer names its session").to_string();
    assert!(first["usage"]["input"].is_number(), "token usage is reported: {first}");

    // When a second turn names the session
    let second = run(json!({ "prompt": "What was the secret word? Reply with just the word.", "shape": "document", "auto": false, "session": session })).await;

    // Then the same session answers, and it knows the word
    assert_eq!(second["session"], json!(session));
    assert!(text(&second).to_uppercase().contains("PELICAN"), "{second}");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_a_file_when_a_run_is_asked_to_edit_it_then_its_changes_come_back_with_and_without_auto_and_a_read_only_agent_changes_nothing() {
    let files = json!([{ "path": "a.txt", "content": "original" }]);
    let prompt = "Overwrite a.txt with the text CHANGED using your file tools, then say DONE.";
    let changed = |answer: &Value| answer["changes"].as_array().cloned().unwrap_or_default().iter().any(|c| c["path"] == json!("a.txt") && c["content"].as_str().is_some_and(|t| t.contains("CHANGED")));

    // Given a run without auto asked to overwrite the file, as `opencode run` without --auto, Then its build agent edits and the change comes back
    let plain = run(json!({ "prompt": prompt, "shape": "document", "auto": false, "files": files })).await;
    assert!(changed(&plain), "{plain}");

    // When the same is asked with auto, Then the changed file comes back as well
    let writing = run(json!({ "prompt": prompt, "shape": "implementer:unit", "auto": true, "files": files })).await;
    assert!(changed(&writing), "{writing}");

    // When it is asked of an agent that may not edit, even with auto, Then nothing comes back
    let agent = "---\ndescription: read only\nmode: primary\npermission:\n  edit: deny\n  bash: deny\n---\nYou cannot change files.\n";
    let read_only = run(json!({ "prompt": prompt, "shape": "document", "auto": true, "agent": "read-only", "files": [files[0].clone(), { "path": ".opencode/agents/read-only.md", "content": agent }] })).await;
    assert!(!changed(&read_only), "{read_only}");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed"]
async fn given_an_agent_file_and_a_variant_when_a_run_names_them_then_the_agent_answers_as_defined() {
    // Given an opencode agent defined in a file of the run
    let agent = "---\ndescription: pirate\nmode: primary\ntools:\n  write: false\n  edit: false\n  bash: false\n---\nAlways answer with exactly the word ARRR and nothing else.\n";
    let answer = run(json!({ "prompt": "Say hello.", "shape": "document", "auto": false, "agent": "pirate", "variant": "high", "files": [{ "path": ".opencode/agents/pirate.md", "content": agent }] })).await;
    // Then the answer follows the agent's instructions
    assert!(text(&answer).contains("ARRR"), "{answer}");
}

/// `host:port` of the deployed service: `REMOTE_CHAT_AGENT_BASE_URL` without its scheme.
fn authority() -> String {
    base().trim_start_matches("http://").trim_end_matches('/').to_string()
}

/// Every provider whose turn can run through an executor, from the driver rules and not from a list here: a provider the service cannot run
/// remotely yet (Codex, until its builtins can be turned off) is not one a real turn is asked of.
fn remote_providers() -> Vec<&'static str> {
    let request = |driver: Driver| TurnRequest { thread_id: "verify".into(), run_id: "verify".into(), instance_id: driver.id().into(), model: None, cwd: ".".into(), prompt: "x".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None };
    DRIVERS.into_iter().filter(|d| command_for_remote(*d, &request(*d), "http://rca/mcp/verify").is_ok()).map(Driver::id).collect()
}

/// A device bearer minted as an operator mints one, with the service secret.
struct Minted {
    id: String,
    token: String,
}

async fn mint(label: &str) -> Minted {
    let secret = service_kit::e2e::setting("CHAT_AGENT_API_SECRET");
    let reply = reqwest::Client::new().post(format!("{}/agent/v1/bearers", base())).header("x-bot-secret", secret).json(&json!({ "label": label })).send().await.expect("the deployed service answers the mint");
    assert_eq!(reply.status().as_u16(), 201, "the mint answered {}", reply.status());
    let answer = reply.json::<Value>().await.expect("a JSON answer");
    Minted { id: answer["id"].as_str().expect("the mint answers the id").to_string(), token: answer["token"].as_str().expect("the mint answers the token").to_string() }
}

/// Revokes a bearer this run minted, so no run leaves a live credential on the deployed service.
async fn revoke(id: &str) {
    let secret = service_kit::e2e::setting("CHAT_AGENT_API_SECRET");
    let reply = reqwest::Client::new().delete(format!("{}/agent/v1/bearers/{id}", base())).header("x-bot-secret", secret).send().await.expect("the deployed service answers the revoke");
    assert_eq!(reply.status().as_u16(), 200, "the revoke of {id} answered {}", reply.status());
}

/// The agent socket of the deployed service as a mapper uses it: an answer is matched to its request by id, and the events that arrive
/// meanwhile are kept in order.
struct Socket {
    ws: WebSocketStream<TcpStream>,
    next: u64,
    events: VecDeque<Event>,
}

impl Socket {
    async fn open(bearer: &str) -> Socket {
        let mut request = format!("ws://{}/agent/v1/socket", authority()).into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {bearer}").parse().unwrap());
        let tcp = TcpStream::connect(authority()).await.expect("the deployed service is reachable");
        let (ws, _) = tokio_tungstenite::client_async(request, tcp).await.expect("the agent socket accepts the device bearer");
        Socket { ws, next: 1, events: VecDeque::new() }
    }

    async fn frame(&mut self) -> Value {
        let read = async {
            loop {
                if let Message::Text(text) = self.ws.next().await.expect("the socket stays open").expect("a frame") {
                    return serde_json::from_str::<Value>(&text).expect("a JSON frame");
                }
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(300), read).await.expect("a frame arrives within five minutes")
    }

    async fn ask(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.ws.send(Message::Text(json!({ "id": id, "method": method, "params": params }).to_string().into())).await.unwrap();
        loop {
            let frame = self.frame().await;
            if frame.get("event").is_some() {
                self.events.push_back(Event::from_value(&frame).expect("an event frame"));
            } else if frame["id"] == json!(id) {
                assert!(frame.get("error").is_none(), "{method} was refused: {frame}");
                return frame["result"].clone();
            }
        }
    }

    /// The turn's events up to and including its `turn.finished`.
    async fn events_through_finish(&mut self) -> Vec<Event> {
        let mut seen = Vec::new();
        loop {
            let event = match self.events.pop_front() {
                Some(event) => event,
                None => Event::from_value(&self.frame().await).expect("an event frame"),
            };
            let done = event.kind == "turn.finished";
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
}

#[tokio::test]
#[ignore = "against the deployed service and a real model: run by test:verify:deployed, and only with WF_DEPLOYED=1"]
async fn given_each_provider_that_runs_through_an_executor_when_a_real_turn_is_started_on_the_deployed_service_then_it_answers_and_finishes_completed() {
    if std::env::var("WF_DEPLOYED").as_deref() != Ok("1") {
        eprintln!("skipped: a real turn per provider runs only with WF_DEPLOYED=1");
        return;
    }
    // Given a device bearer, the workspace whose executor the deployed service has attached, and the providers a turn can run through it
    let workspace = std::env::var("RCA_VERIFY_WORKSPACE").expect("RCA_VERIFY_WORKSPACE names a workspace whose rca-exec is attached to the deployed service");
    let providers = remote_providers();
    assert!(!providers.is_empty(), "no provider runs through an executor");
    let bearer = mint("deployed verification").await;
    // When a real turn of each provider runs, then the bearer is revoked whatever the turns did: it opens a production socket
    let turns = tokio::spawn(real_turns(bearer.token.clone(), workspace, providers));
    let outcome = turns.await;
    revoke(&bearer.id).await;
    if let Err(failed) = outcome {
        match failed.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(other) => panic!("the turns did not finish: {other}"),
        }
    }
}

/// One thread of each provider, created, followed and asked for one word and no tool: each turn finishes completed and its answer carries the word.
async fn real_turns(bearer: String, workspace: String, providers: Vec<&'static str>) {
    let mut socket = Socket::open(&bearer).await;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for provider in providers {
        let thread = format!("verify-{provider}-{stamp}");
        socket.ask("thread.create", json!({ "thread": thread, "workspace": workspace, "driver": provider })).await;
        socket.ask("thread.subscribe", json!({ "thread": thread })).await;
        socket.ask("turn.start", json!({ "thread": thread, "workspace": workspace, "driver": provider, "prompt": "Reply with exactly the word PONG and use no tool." })).await;
        let events = socket.events_through_finish().await;
        let finished = events.last().expect("the turn finished");
        assert_eq!(finished.data["reason"], json!("completed"), "{provider}: {events:?}");
        let answer: String = events.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == json!("assistant")).filter_map(|e| e.data["text"].as_str()).collect();
        assert!(answer.to_uppercase().contains("PONG"), "{provider} answered {answer:?}");
    }
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed, and only with WF_DEPLOYED=1"]
async fn given_the_deployed_service_when_its_providers_are_listed_then_each_provider_that_runs_a_turn_holds_a_login_in_its_own_credential_directory_and_no_reply_carries_a_token() {
    if std::env::var("WF_DEPLOYED").as_deref() != Ok("1") {
        eprintln!("skipped: the credential directories are read only with WF_DEPLOYED=1");
        return;
    }
    // Given a device bearer for the run (revoked whatever happens: it opens a production socket)
    let bearer = mint("deployed credential directories").await;
    let checked = tokio::spawn({
        let token = bearer.token.clone();
        async move {
            let mut socket = Socket::open(&token).await;
            // When the service's groups and providers are asked for
            let status = socket.ask("service.status", json!({})).await;
            let listed = socket.ask("provider.refresh", json!({})).await;
            let usage = socket.ask("usage.summary", json!({})).await;
            (status, listed, usage)
        }
    })
    .await;
    revoke(&bearer.id).await;
    let (status, listed, usage) = match checked {
        Ok(answers) => answers,
        Err(failed) => match failed.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(other) => panic!("the check did not finish: {other}"),
        },
    };
    // Then the accounts group is served, every driver has a row, and each provider a turn can run through holds a login (its CLI wrote it to
    // /data/providers/<provider>, the directory the service points that CLI at)
    assert!(status["groups"].as_array().unwrap().iter().any(|g| g == "accounts"), "the deployed service does not serve accounts: {status}");
    let rows = listed["providers"].as_array().expect("a list of providers");
    assert_eq!(rows.iter().map(|r| r["provider"].as_str().unwrap()).collect::<Vec<_>>(), DRIVERS.iter().map(|d| d.id()).collect::<Vec<_>>());
    for provider in remote_providers() {
        let row = rows.iter().find(|r| r["provider"] == provider).unwrap();
        assert_eq!(row["status"], json!("signed_in"), "{provider} holds no login in its credential directory: sign it in with provider.auth.start ({row})");
    }
    // And no reply carries a token or a key: a provider is a status, never a credential
    for reply in [&listed, &usage] {
        let text = reply.to_string().to_lowercase();
        for forbidden in ["token", "secret", "password", "api_key", "sk-"] {
            assert!(!text.contains(forbidden), "a reply mentions {forbidden}: {reply}");
        }
    }
}

/// The hosted source control of the deployed service through the agent socket, against one real test repository with the sign-in the executor's
/// machine already holds: the sign-in status (never a token), the repository and its pull requests, read only.
async fn read_hosted(bearer: String, workspace: String, provider: String, repo: String) {
    let mut socket = Socket::open(&bearer).await;
    let status = socket.ask("sourceControl.discover", json!({ "workspace": workspace, "provider": provider })).await;
    let signed = &status["providers"][0];
    assert_eq!((signed["installed"].clone(), signed["signed_in"].clone()), (json!(true), json!(true)), "the executor's machine is not signed in to {provider}: {status}");
    assert!(signed.get("token").is_none(), "{status}");
    let view = socket.ask("sourceControl.lookupRepository", json!({ "workspace": workspace, "provider": provider, "repo": repo })).await;
    assert_eq!(view["repo"]["slug"].as_str().map(str::to_lowercase), Some(repo.to_lowercase()), "{view}");
    let listed = socket.ask("pr.list", json!({ "workspace": workspace, "provider": provider, "repo": repo, "state": "all", "limit": 5 })).await;
    let first = listed["prs"].as_array().and_then(|prs| prs.first()).and_then(|pr| pr["number"].as_u64());
    if let Some(number) = first {
        let one = socket.ask("pr.view", json!({ "workspace": workspace, "provider": provider, "repo": repo, "number": number })).await;
        assert_eq!(one["pr"]["number"], json!(number), "{one}");
    }
}

#[tokio::test]
#[ignore = "against the deployed service and a real test repository: run by test:verify:deployed, and only with WF_DEPLOYED=1 and WF_HOSTED_TEST_REPO"]
async fn given_a_real_test_repository_when_its_pull_requests_are_read_through_the_deployed_service_then_the_executors_own_sign_in_answers_and_no_token_is_shown() {
    let (Some(repo), true) = (std::env::var("WF_HOSTED_TEST_REPO").ok().filter(|r| !r.is_empty()), std::env::var("WF_DEPLOYED").as_deref() == Ok("1")) else {
        eprintln!("skipped: WF_DEPLOYED=1 and WF_HOSTED_TEST_REPO are not both set");
        return;
    };
    let workspace = std::env::var("RCA_VERIFY_WORKSPACE").expect("RCA_VERIFY_WORKSPACE names a workspace whose rca-exec is attached to the deployed service");
    let provider = std::env::var("WF_HOSTED_TEST_PROVIDER").unwrap_or_else(|_| "github".into());
    let bearer = mint("deployed hosted verification").await;
    let outcome = tokio::spawn(read_hosted(bearer.token.clone(), workspace, provider, repo)).await;
    revoke(&bearer.id).await;
    if let Err(failed) = outcome {
        match failed.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(other) => panic!("the reads did not finish: {other}"),
        }
    }
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed, and only with WF_DEPLOYED=1"]
async fn given_the_deployed_service_when_a_webhook_gets_a_wrong_token_and_the_schedules_are_listed_then_it_is_401_the_groups_are_served_and_no_reply_carries_a_token() {
    if std::env::var("WF_DEPLOYED").as_deref() != Ok("1") {
        eprintln!("skipped: the schedules are read only with WF_DEPLOYED=1");
        return;
    }
    // Given a request to a schedule's webhook with a token no schedule has, with no credential at all
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().unwrap();
    let reply = client.post(format!("{}/hooks/v1/schedules/none/not-a-token", base())).body("{}").send().await.expect("the deployed service answers");
    // Then it is the schedule's own 401 and starts nothing
    let status = reply.status().as_u16();
    let body: Value = reply.json().await.expect("a JSON answer");
    assert_eq!((status, body), (401, json!({"error": "unauthorized"})));
    // And the schedules groups are served, the list is read only, and no token is in it (a webhook is a path and whether it has one)
    let bearer = mint("deployed schedules verification").await;
    let outcome = tokio::spawn({
        let token = bearer.token.clone();
        async move {
            let mut socket = Socket::open(&token).await;
            (socket.ask("service.status", json!({})).await, socket.ask("schedule.list", json!({})).await)
        }
    })
    .await;
    revoke(&bearer.id).await;
    let (status, listed) = match outcome {
        Ok(answers) => answers,
        Err(failed) => match failed.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(other) => panic!("the reads did not finish: {other}"),
        },
    };
    let groups = status["groups"].as_array().unwrap();
    assert!(groups.iter().any(|g| g == "schedules") && groups.iter().any(|g| g == "schedules.watch"), "the deployed service does not serve the schedules groups: {status}");
    let text = listed.to_string().replace("has_token", "");
    assert!(listed["tasks"].is_array() && !text.contains("token"), "{listed}");
}

#[tokio::test]
#[ignore = "against the deployed service: run by test:verify:deployed, and only with WF_DEPLOYED=1"]
async fn given_the_deployed_service_when_an_attachment_is_kept_read_and_deleted_then_the_bytes_round_trip_and_nothing_is_left_behind() {
    if std::env::var("WF_DEPLOYED").as_deref() != Ok("1") {
        eprintln!("skipped: the attachments are written only with WF_DEPLOYED=1");
        return;
    }
    // Given a bearer and an attachment id made for this run
    let bearer = mint("deployed attachments verification").await;
    let id = format!("verify-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()));
    let outcome = tokio::spawn({
        let (token, id) = (bearer.token.clone(), id.clone());
        async move {
            let mut socket = Socket::open(&token).await;
            let status = socket.ask("service.status", json!({})).await;
            // When the bytes are kept, read back, and deleted twice
            let kept = socket.ask("upload.put", json!({"id": id, "name": "verify.txt", "mime_type": "text/plain", "kind": "file", "data": "dmVyaWZ5"})).await;
            let read = socket.ask("attachment.get", json!({"id": id})).await;
            let first = socket.ask("attachment.delete", json!({"id": id})).await;
            let second = socket.ask("attachment.delete", json!({"id": id})).await;
            (status, kept, read, first, second)
        }
    })
    .await;
    revoke(&bearer.id).await;
    let (status, kept, read, first, second) = match outcome {
        Ok(answers) => answers,
        Err(failed) => match failed.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(other) => panic!("the calls did not finish: {other}"),
        },
    };
    // Then the group is served, the bytes came back as they went, and the first delete removed them while the second found nothing
    assert!(status["groups"].as_array().unwrap().iter().any(|g| g == "attachments"), "the deployed service does not serve attachments: {status}");
    assert_eq!((kept["attachment"]["size_bytes"].clone(), read["data"].clone(), read["eof"].clone()), (json!(6), json!("dmVyaWZ5"), json!(true)), "{kept} {read}");
    assert_eq!((first["deleted"].clone(), second["deleted"].clone()), (json!(true), json!(false)));
}

/// What a `/ws` thread subscription shows of a thread's session: how many times it went `ready` (each `session.idle` of opencode settles the
/// running turn and sets the session ready) and how many of the thread's messages came from the assistant, with the last status seen.
#[derive(Default, Debug, PartialEq)]
struct IdleCount {
    ready: u64,
    assistant_messages: u64,
    user_messages: u64,
    last_status: String,
}

impl IdleCount {
    /// Folds one `/ws` thread-subscription frame (`{"kind":"event","item":{"kind":"event","event":{..}}}`) into the count.
    fn see(&mut self, frame: &Value) {
        let event = &frame["item"]["event"];
        match event["type"].as_str() {
            Some("thread.session-set") => {
                let status = event["payload"]["session"]["status"].as_str().unwrap_or_default();
                if status == "ready" {
                    self.ready += 1;
                }
                self.last_status = status.to_string();
            }
            Some("thread.message-sent") => match event["payload"]["role"].as_str() {
                Some("assistant") => self.assistant_messages += 1,
                Some("user") => self.user_messages += 1,
                _ => {}
            },
            _ => {}
        }
    }
}

fn ws_message(thread: &str, text: &str, cwd: &str, bootstrap: bool) -> Value {
    let model = json!({ "cli": "opencode" });
    let mut command = json!({ "type": "thread.turn.start", "commandId": format!("probe-{thread}-{text}"), "threadId": thread, "createdAt": "2026-01-01T00:00:00.000Z",
        "message": { "messageId": format!("msg-{thread}-{text}"), "role": "user", "text": format!("reply with the word {text}") }, "runtimeMode": "full-access", "interactionMode": "default" });
    if bootstrap {
        command["bootstrap"] = json!({ "createThread": { "title": "idle probe", "cwd": cwd, "modelSelection": model, "runtimeMode": "full-access", "interactionMode": "default" } });
    }
    command
}

/// Two `thread.turn.start` back to back on one thread of the opencode-backed thread view (`/ws`, the service secret as the token): how many
/// times does the thread's session go ready (one per opencode `session.idle`) before it stays quiet? One means opencode folds a queued prompt into the
/// running one and `Ingestions` never sees its second idle (plan `rca-open-issues`, Step 6); two means each prompt ends in its own idle.
/// Prints one JSON line `{"idle_events", "turns_settled", "subscribers_after"}`; `subscribers_after` is null because the opencode event
/// subscribers are not visible from outside the container (read `ingestion.started`/`ingestion.ended` in BetterStack for the thread id printed beside it).
#[tokio::test]
#[ignore = "against the deployed service and a real model: run by test:verify:deployed, and only with WF_DEPLOYED=1"]
async fn given_two_turns_started_back_to_back_on_one_thread_when_the_session_goes_quiet_then_both_turns_settled_and_the_idle_events_are_counted() {
    if std::env::var("WF_DEPLOYED").as_deref() != Ok("1") {
        eprintln!("skipped: the idle probe runs a real model and only with WF_DEPLOYED=1");
        return;
    }
    // Given a fresh thread id and a directory of the deployed container for opencode to work in
    let cwd = std::env::var("RCA_VERIFY_CWD").unwrap_or_else(|_| "/tmp".to_string());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let thread = format!("verify-idle-{stamp}");
    let secret = service_kit::e2e::setting("CHAT_AGENT_API_SECRET");
    let tcp = TcpStream::connect(authority()).await.expect("the deployed service is reachable");
    let request = format!("ws://{}/ws?token={secret}", authority()).into_client_request().unwrap();
    let (mut ws, _) = tokio_tungstenite::client_async(request, tcp).await.expect("the thread socket accepts the service secret");
    let sent = |value: Value| Message::Text(value.to_string().into());

    // When both turns are started without waiting for the first, and the thread is followed from its start
    ws.send(sent(json!({ "kind": "request", "id": "t1", "method": "orchestration.dispatchCommand", "params": ws_message(&thread, "one", &cwd, true) }))).await.unwrap();
    ws.send(sent(json!({ "kind": "request", "id": "t2", "method": "orchestration.dispatchCommand", "params": ws_message(&thread, "two", &cwd, false) }))).await.unwrap();
    ws.send(sent(json!({ "kind": "subscribe", "id": "follow", "method": "orchestration.subscribeThread", "params": { "threadId": thread } }))).await.unwrap();

    // Then the stream is read until the session has been ready and nothing has come for 45 s, bounded to 5 minutes
    let mut count = IdleCount::default();
    let mut answers = 0;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        let quiet = if count.ready > 0 && answers == 2 { 45 } else { 300 };
        let wait = std::time::Duration::from_secs(quiet).min(deadline.saturating_duration_since(tokio::time::Instant::now()));
        let Ok(Some(Ok(Message::Text(text)))) = tokio::time::timeout(wait, ws.next()).await else { break };
        let frame: Value = serde_json::from_str(&text).expect("a JSON frame");
        if frame["kind"] == "response" {
            answers += 1;
            assert_eq!(frame["ok"], json!(true), "a turn start was refused: {frame}");
        } else if frame["id"] == "follow" {
            // The snapshot carries what happened before the subscription opened; its messages and session count as the events they stand for.
            if frame["item"]["kind"] == "snapshot" {
                let snapshot = &frame["item"]["snapshot"];
                count.user_messages += snapshot["messages"].as_array().map_or(0, |m| m.iter().filter(|m| m["role"] == "user").count() as u64);
                count.assistant_messages += snapshot["messages"].as_array().map_or(0, |m| m.iter().filter(|m| m["role"] == "assistant").count() as u64);
                count.last_status = snapshot["session"]["status"].as_str().unwrap_or_default().to_string();
            }
            count.see(&frame);
        }
    }

    println!("{}", json!({ "idle_events": count.ready, "turns_settled": count.assistant_messages, "subscribers_after": null, "thread": thread, "user_messages": count.user_messages, "last_status": count.last_status }));
    assert_eq!(count.user_messages, 2, "both turns reached the thread: {count:?}");
    assert!(count.ready >= 1 && count.last_status == "ready", "the thread settled: {count:?}");
}
