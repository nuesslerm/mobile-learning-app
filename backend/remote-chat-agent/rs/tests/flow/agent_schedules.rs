//! `schedules` and `schedules.watch` end to end: the flow's clock is moved by hand and the service's schedules ticked, and a due schedule starts a
//! turn through the agent group: the real `rca-exec` serving a repository and the scripted CLI the service starts, as in `agent_threads`. The
//! webhook is the HTTP route outside the bearers, with its own token.
use crate::agent_threads::{chain, said, Chain};
use crate::harness::{post, service, Env, Options, SCHEDULE_EPOCH_MS};
use crate::mapper::Agent;
use agent_api_proto::Event;
use axum::http::StatusCode;
use exec_testkit::script::Turn;
use serde_json::{json, Value};

const MINUTE: u64 = 60_000;

fn schedule(id: &str, kind: Value, prompt: &str) -> Value {
    json!({"id": id, "title": "Nightly", "prompt": prompt, "enabled": true, "schedule": kind, "workspace": "w1", "driver": "claudeAgent"})
}

fn every_minute() -> Value {
    json!({"type": "interval", "every_ms": MINUTE})
}

/// Reads the stream until an event the test is waiting for, keeping what came before it.
async fn until(agent: &mut Agent, what: &str, done: impl Fn(&Event) -> bool) -> Vec<Event> {
    let mut seen = Vec::new();
    loop {
        let event = agent.event().await;
        let end = done(&event);
        seen.push(event);
        assert!(seen.len() < 500, "{what}: {} events and not there", seen.len());
        if end {
            return seen;
        }
    }
}

fn run_state(state: &'static str) -> impl Fn(&Event) -> bool {
    move |e| e.kind == "schedule.run" && e.data["run"]["state"] == json!(state)
}

fn summary(events: &[Event]) -> Vec<(String, Value)> {
    events
        .iter()
        .map(|e| {
            let what = match e.kind.as_str() {
                "schedule.run" => e.data["run"]["state"].clone(),
                "schedule.webhook" => e.data["outcome"].clone(),
                _ => e.data["change"].clone(),
            };
            (e.kind.clone(), what)
        })
        .collect()
}

/// A mapper that follows the `schedules` stream on a socket of its own, and one that asks.
async fn watching(chain: &Chain) -> (Agent, Agent) {
    let mut watcher = chain.agent().await;
    let followed = watcher.ask("schedule.subscribe", json!({})).await;
    assert_eq!((followed["result"]["topic"].clone(), followed["result"]["reset"].clone()), (json!("schedules"), json!(false)), "{followed}");
    (watcher, chain.agent().await)
}

fn prompts(chain: &Chain) -> Vec<String> {
    chain.cli.invocations("claude").iter().filter_map(|start| start.argv.iter().find(|a| a.contains("on schedule")).cloned()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_due_schedule_when_the_clock_passes_it_then_the_agent_group_runs_its_turn_and_the_stream_records_the_run_from_start_to_success() {
    // Given the real chain, a CLI whose turn says one thing, a mapper following the schedules stream and a schedule that runs every minute
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("hello from the schedule")])]);
    let (mut watcher, mut agent) = watching(&chain).await;
    let saved = agent.ask("schedule.upsert", schedule("nightly", every_minute(), "say hello on schedule")).await;
    assert_eq!(saved["result"]["task"]["next_run_ms"], json!(SCHEDULE_EPOCH_MS + MINUTE), "{saved}");
    // When the clock stands short of it, nothing starts, and when it reaches it, one turn does
    chain.service().clock.advance(MINUTE - 1);
    assert_eq!(chain.service().schedules().tick().await, 0);
    assert!(prompts(&chain).is_empty());
    chain.service().clock.advance(1);
    assert_eq!(chain.service().schedules().tick().await, 1);
    // Then the stream has the save, the run that began and, when the turn is over, the run that succeeded, all of one thread and turn
    let events = until(&mut watcher, "the run succeeds", run_state("succeeded")).await;
    assert_eq!(summary(&events), [("schedule.changed".to_string(), json!("saved")), ("schedule.run".to_string(), json!("started")), ("schedule.run".to_string(), json!("succeeded"))]);
    let (started, ended) = (&events[1].data["run"], &events[2].data["run"]);
    assert_eq!((started["source"].clone(), started["thread"].clone(), started["turn"].clone()), (json!("timer"), ended["thread"].clone(), ended["turn"].clone()));
    assert!(events.windows(2).all(|pair| pair[0].seq + 1 == pair[1].seq), "{:?}", events.iter().map(|e| e.seq).collect::<Vec<_>>());
    // And the CLI was started once with the schedule's prompt, and the thread the run names holds the turn as any mapper reads it
    assert_eq!(prompts(&chain), ["say hello on schedule"]);
    let thread = started["thread"].as_str().unwrap();
    let history = agent.ask("thread.history", json!({"thread": thread})).await;
    let kinds: Vec<&str> = history["result"]["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!((kinds.first().copied(), kinds.last().copied()), (Some("turn.started"), Some("turn.finished")), "{history}");
    assert_eq!(agent.ask("thread.get", json!({"thread": thread})).await["result"]["thread"]["title"], json!("Nightly"));
    // And the schedule shows the run and its next due time a minute on
    let listed = agent.ask("schedule.list", json!({})).await;
    let task = &listed["result"]["tasks"][0];
    assert_eq!((task["run_count"].clone(), task["last_run_status"].clone(), task["next_run_ms"].clone()), (json!(1), json!("succeeded"), json!(SCHEDULE_EPOCH_MS + 2 * MINUTE)), "{listed}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_schedule_that_was_disabled_when_the_clock_passes_it_then_no_turn_starts_and_run_now_starts_one_anyway() {
    // Given an enabled schedule that is then disabled through the socket
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("hello from the schedule")])]);
    let (mut watcher, mut agent) = watching(&chain).await;
    agent.ask("schedule.upsert", schedule("nightly", every_minute(), "say hello on schedule")).await;
    let off = agent.ask("schedule.setEnabled", json!({"id": "nightly", "enabled": false})).await;
    assert_eq!((off["result"]["task"]["enabled"].clone(), off["result"]["task"]["next_run_ms"].clone()), (json!(false), Value::Null), "{off}");
    // When the clock passes where it would have run, Then no turn starts and no CLI is started
    chain.service().clock.advance(5 * MINUTE);
    assert_eq!(chain.service().schedules().tick().await, 0);
    assert!(chain.cli.invocations("claude").is_empty());
    // When it is run now, Then its turn runs and the run is a manual one
    let ran = agent.ask("schedule.runNow", json!({"id": "nightly"})).await;
    assert_eq!((ran["result"]["run"]["source"].clone(), ran["result"]["run"]["state"].clone()), (json!("manual"), json!("started")), "{ran}");
    let events = until(&mut watcher, "the manual run succeeds", run_state("succeeded")).await;
    assert_eq!(summary(&events), [("schedule.changed".to_string(), json!("saved")), ("schedule.changed".to_string(), json!("disabled")), ("schedule.run".to_string(), json!("started")), ("schedule.run".to_string(), json!("succeeded"))]);
    assert_eq!(prompts(&chain), ["say hello on schedule"]);
    // And a refused turn is the error and a failed run: the schedule names a thread the service does not have
    let mut lost = schedule("lost", every_minute(), "say hello on schedule");
    lost["thread"] = json!("t-none");
    agent.ask("schedule.upsert", lost).await;
    let refused = agent.ask("schedule.runNow", json!({"id": "lost"})).await;
    assert_eq!(refused["error"]["code"], json!("THREAD_NOT_FOUND"), "{refused}");
    let failed = until(&mut watcher, "the refused run is recorded", run_state("failed")).await;
    assert_eq!(failed.last().unwrap().data["run"]["error"], json!("no thread has the id t-none"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_webhook_schedule_when_requests_arrive_without_a_bearer_then_a_wrong_token_is_401_and_seen_by_the_schedule_and_the_right_token_runs_the_rendered_prompt() {
    // Given a webhook schedule whose prompt names a field of the body, its token, and a mapper following the stream
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("reviewed")])]);
    let (mut watcher, mut agent) = watching(&chain).await;
    let saved = agent.ask("schedule.upsert", schedule("hook", json!({"type": "webhook"}), "review pr {{body.pr}} on schedule")).await;
    let token = saved["result"]["webhook_token"].as_str().expect("the first save shows the token").to_string();
    let path = saved["result"]["task"]["webhook"]["path"].as_str().unwrap().to_string();
    assert_eq!(path, "/hooks/v1/schedules/hook");
    // When a request with a wrong token arrives, with no bearer and no service secret
    let wrong = chain.service().send(post(&format!("{path}/not-the-token")).raw_json(r#"{"pr": 7}"#)).await;
    // Then it is 401 and starts nothing, and the schedule's stream saw it arrive and be turned away (the 401 is the schedule's own, not a front door's)
    assert_eq!((wrong.status, wrong.json()), (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"})));
    let seen = until(&mut watcher, "the rejected request is seen", |e| e.kind == "schedule.webhook").await;
    assert_eq!((seen.last().unwrap().data["schedule"].clone(), seen.last().unwrap().data["outcome"].clone()), (json!("hook"), json!("rejected_token")));
    assert!(chain.cli.invocations("claude").is_empty());
    // And a request to a schedule that does not exist is the same 401, and so is the token of another schedule's path
    let unknown = chain.service().send(post(&format!("/hooks/v1/schedules/none/{token}")).raw_json("{}")).await;
    assert_eq!((unknown.status, unknown.json()), (StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"})));
    // When the right token arrives
    let accepted = chain.service().send(post(&format!("{path}/{token}")).raw_json(r#"{"pr": 42}"#)).await;
    // Then it is 202 with the run, the turn runs the prompt with the body's field in it, and the stream has the request let in and the run through
    assert_eq!(accepted.status, StatusCode::ACCEPTED, "{}", accepted.text());
    assert_eq!((accepted.json()["run"]["source"].clone(), accepted.json()["run"]["state"].clone()), (json!("webhook"), json!("started")));
    let events = until(&mut watcher, "the webhook run succeeds", run_state("succeeded")).await;
    assert!(summary(&events).contains(&("schedule.webhook".to_string(), json!("accepted"))));
    assert_eq!(prompts(&chain), ["review pr 42 on schedule"]);
    // When the token is rotated, Then the old one is 401 and the new one is let in
    let rotated = agent.ask("schedule.rotateWebhookToken", json!({"id": "hook"})).await;
    let fresh = rotated["result"]["webhook_token"].as_str().unwrap().to_string();
    chain.script(vec![Turn::replaying(&[said("reviewed again")])]);
    assert_eq!(chain.service().send(post(&format!("{path}/{token}")).raw_json("{}")).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(chain.service().send(post(&format!("{path}/{fresh}")).raw_json(r#"{"pr": 43}"#)).await.status, StatusCode::ACCEPTED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_webhook_schedule_when_the_service_restarts_then_the_schedule_and_its_token_survive_and_the_file_holds_a_digest_never_the_token() {
    // Given a webhook schedule saved on one service
    let env = Env::process(&[]);
    let first = service(&env, Options::default());
    let port = first.listen().await;
    let mut agent = Agent::open(port, &first.mint("flow mapper").await).await;
    let saved = agent.ask("schedule.upsert", schedule("hook", json!({"type": "webhook"}), "review on schedule")).await;
    let token = saved["result"]["webhook_token"].as_str().unwrap().to_string();
    // Then the file on the state volume holds the digest of the token and not the token, and only its owner can read it
    let file = env.home.join(".local/state/rca/schedules.json");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(&rca_agent_core::token_digest(&token)) && !text.contains(&token));
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    // When a second service starts on the same home (a restart)
    let second = service(&env, Options::default());
    let port = second.listen().await;
    let mut again = Agent::open(port, &second.mint("flow mapper").await).await;
    let listed = again.ask("schedule.list", json!({})).await;
    // Then it lists the schedule, with a webhook that still has its token, and the token still opens it (no executor is attached, so the turn is refused, not the request)
    assert_eq!((listed["result"]["tasks"][0]["id"].clone(), listed["result"]["tasks"][0]["webhook"].clone()), (json!("hook"), json!({"path": "/hooks/v1/schedules/hook", "has_token": true})));
    let let_in = second.send(post(&format!("/hooks/v1/schedules/hook/{token}")).raw_json("{}")).await;
    assert_eq!((let_in.status, let_in.json()["error"].clone()), (StatusCode::BAD_GATEWAY, json!("NO_EXECUTOR")), "{}", let_in.text());
    assert_eq!(second.send(post("/hooks/v1/schedules/hook/other")).await.status, StatusCode::UNAUTHORIZED);
}

use std::os::unix::fs::PermissionsExt;
