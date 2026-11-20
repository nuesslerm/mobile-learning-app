//! The accounts and git.ai groups over the doubles: the providers' status, a login by device code and by a pasted code, how one ends or is
//! cancelled, the events of a session and their resume, logout, usage, credit, and the commit message a model writes for a change.
use crate::testing::{Ending, LoginScript, MemoryDirs, ScriptedAccounts, ScriptedLogin, ScriptedModelApi};
use crate::{commit_message_prompt, pull_request_prompt, read_pull_request_fields, tidy_message, AccountError, AccountsGroup, GitAiGroup, HostAccounts, ModelError, ProviderAccounts, Sessions, GIT_AI_SERVED, PULL_REQUEST_SYSTEM, SERVED};
use agent_api_accounts::{self as api, Limits, ResetCredits, ResetOutcome, Updated, Window};
use agent_api_git::ai::{self, PullRequestParams, Style};
use agent_api_proto::Event;
use async_trait::async_trait;
use rca_agent_core::{AgentApi, Build, Connection, EventSink, Group};
use rca_cli_driver::domain::{Driver, LoginMethod, DRIVERS};
use rca_exec_channel::remote::{ActionOutput, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::Registry;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };
const DEVICE: [&str; 2] = ["Open https://login.example.test/device to sign in.", "Enter the code ABCD-1234 there."];

struct Queue(mpsc::UnboundedSender<Event>);

#[async_trait]
impl EventSink for Queue {
    fn send(&self, event: &Event) -> bool {
        self.0.send(event.clone()).is_ok()
    }

    async fn send_replay(&self, event: &Event) -> bool {
        self.send(event)
    }
}

struct Rig {
    api: AgentApi,
    dirs: Arc<MemoryDirs>,
    login: Arc<ScriptedLogin>,
    accounts: Arc<ScriptedAccounts>,
}

fn rig() -> Rig {
    let dirs = MemoryDirs::new();
    let login = ScriptedLogin::new(dirs.clone());
    let accounts = ScriptedAccounts::new();
    let sessions = Arc::new(Sessions::waiting(dirs.clone(), login.clone(), Duration::from_millis(500)));
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(AccountsGroup::new(sessions, accounts.clone()))];
    Rig { api: AgentApi::new(groups).unwrap(), dirs, login, accounts }
}

impl Rig {
    async fn ask(&self, method: &str, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method, "params": params}).to_string()).await
    }

    async fn auth(&self, method: &str, params: Value) -> Value {
        let answered = self.ask(method, params).await;
        answered["result"]["auth"].clone()
    }
}

fn error_code(answered: &Value) -> &str {
    answered["error"]["code"].as_str().unwrap_or_else(|| panic!("no error in {answered}"))
}

#[test]
fn given_the_served_rows_when_compared_with_the_contracts_then_each_method_is_there_in_order_and_only_git_ai_calls_an_executor_action() {
    // Given the rows of the two groups
    // Then accounts serves the contract's methods in order calling only cli_detect, the login actions and the account actions, and git.ai calls git_diff
    assert_eq!(SERVED.iter().map(|s| s.method).collect::<Vec<_>>(), api::METHODS);
    assert!(SERVED.iter().all(|s| s.actions.iter().all(|a| ["cli_detect", "login_start", "login_send", "login_status", "login_cancel", "logout", "cli_update", "codex_app_server_request", "claude_account_request"].contains(a))));
    for action in ["login_start", "login_send", "login_status", "login_cancel", "logout", "cli_update", "codex_app_server_request", "claude_account_request"] {
        assert!(SERVED.iter().any(|s| s.actions.contains(&action)), "no accounts row calls {action}");
    }
    assert_eq!(GIT_AI_SERVED.iter().map(|s| (s.method, s.actions.to_vec())).collect::<Vec<_>>(), [(ai::GENERATE_COMMIT_MESSAGE, vec![exec_git::GIT_DIFF]), (ai::GENERATE_PULL_REQUEST_FIELDS, vec![exec_git::GIT_COMPARE, exec_git::GIT_DIFF])]);
}

#[tokio::test]
async fn given_no_login_when_providers_are_listed_then_every_driver_is_signed_out_with_the_methods_its_cli_offers_and_none_is_selected() {
    // Given a service whose providers hold no login
    let rig = rig();
    // When the providers are listed and refreshed
    let listed = rig.ask("provider.list", json!({})).await;
    let refreshed = rig.ask("provider.refresh", json!({"provider": "codex"})).await;
    // Then each driver is there, signed out, with the methods of its CLI, and none is selected
    let rows = listed["result"]["providers"].as_array().unwrap();
    assert_eq!(rows.iter().map(|r| r["provider"].as_str().unwrap()).collect::<Vec<_>>(), DRIVERS.iter().map(|d| d.id()).collect::<Vec<_>>());
    assert!(rows.iter().all(|r| r["status"] == "signed_out" && r["selected"] == false));
    assert_eq!(rows.iter().find(|r| r["provider"] == "claudeAgent").unwrap()["auth_methods"], json!(["paste"]));
    assert_eq!(rows.iter().find(|r| r["provider"] == "codex").unwrap()["auth_methods"], json!(["device_code"]));
    assert_eq!(listed["result"]["selected"], Value::Null);
    assert_eq!(refreshed["result"], listed["result"]);
    // And a provider that is none is refused by name
    assert_eq!(error_code(&rig.ask("provider.refresh", json!({"provider": "gemini"})).await), "UNKNOWN_PROVIDER");
}

#[tokio::test]
async fn given_a_device_flow_when_a_login_starts_and_completes_then_the_prompt_comes_back_the_provider_is_signed_in_and_chosen_and_the_directory_was_made() {
    // Given Codex's login that prints an address and a code and then keeps a login
    let rig = rig();
    rig.login.script(Driver::Codex, LoginScript::new(&DEVICE, Ending::SignsIn));
    // When the login starts and the caller waits for its end
    let started = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    let done = rig.auth("provider.auth.complete", json!({"auth": started["auth"]})).await;
    // Then the start answered with the prompt of the CLI, in the method the CLI offers
    assert_eq!((started["method"].clone(), started["url"].clone(), started["code"].clone()), (json!("device_code"), json!("https://login.example.test/device"), json!("ABCD-1234")));
    // And the session ends succeeded, with the same prompt and no error
    assert_eq!((done["status"].clone(), done["url"].clone(), done["error"].clone()), (json!("succeeded"), json!("https://login.example.test/device"), Value::Null));
    // And Codex is signed in and, being the first login, the selected provider; the others are untouched
    let listed = rig.ask("provider.list", json!({})).await;
    let row = |name: &str| listed["result"]["providers"].as_array().unwrap().iter().find(|r| r["provider"] == name).unwrap().clone();
    assert_eq!((row("codex")["status"].clone(), row("codex")["selected"].clone(), row("opencode")["status"].clone()), (json!("signed_in"), json!(true), json!("signed_out")));
    assert_eq!(listed["result"]["selected"], json!("codex"));
    assert_eq!((rig.dirs.ensured(), rig.login.started()), (vec!["codex"], vec![(Driver::Codex, LoginMethod::DeviceCode)]));
}

#[tokio::test]
async fn given_a_login_running_when_another_starts_for_the_same_provider_or_a_method_the_cli_lacks_is_asked_then_each_is_refused_and_another_provider_may_start() {
    // Given Claude's login that waits for a pasted code
    let rig = rig();
    rig.login.script(Driver::ClaudeAgent, LoginScript::new(&DEVICE, Ending::Waits));
    let first = rig.auth("provider.auth.start", json!({"provider": "claudeAgent", "method": "paste"})).await;
    // When a second login of that provider starts, Codex is asked for a paste, a provider is unknown, and Codex is started by device code
    let again = rig.ask("provider.auth.start", json!({"provider": "claudeAgent"})).await;
    let paste = rig.ask("provider.auth.start", json!({"provider": "codex", "method": "paste"})).await;
    let unknown = rig.ask("provider.auth.start", json!({"provider": "gemini"})).await;
    let other = rig.ask("provider.auth.start", json!({"provider": "codex"})).await;
    // Then the second is refused naming the session that runs, Codex has no paste flow, the unknown one is refused, and Codex starts
    assert_eq!(error_code(&again), "AUTH_IN_PROGRESS");
    assert!(again["error"]["message"].as_str().unwrap().contains(first["auth"].as_str().unwrap()));
    assert_eq!(error_code(&paste), "UNSUPPORTED_METHOD");
    assert_eq!(error_code(&unknown), "UNKNOWN_PROVIDER");
    assert!(other["result"]["auth"]["auth"].is_string());
    assert_eq!(rig.login.started().iter().map(|(d, _)| d.id()).collect::<Vec<_>>(), ["claudeAgent", "codex"]);
    // And the status of Claude reads authenticating while its login runs
    let listed = rig.ask("provider.list", json!({})).await;
    assert_eq!(listed["result"]["providers"].as_array().unwrap().iter().find(|r| r["provider"] == "claudeAgent").unwrap()["status"], json!("authenticating"));
}

#[tokio::test]
async fn given_a_paste_flow_when_the_code_is_sent_then_the_login_ends_signed_in_and_a_session_that_pastes_nothing_or_has_ended_refuses_it() {
    // Given Claude's login that keeps a login once it is sent a line, and Codex's that signs in by itself
    let rig = rig();
    rig.login.script(Driver::ClaudeAgent, LoginScript::new(&["Open https://login.example.test/paste and paste the code."], Ending::SignsInOnInput));
    rig.login.script(Driver::Codex, LoginScript::new(&DEVICE, Ending::Waits));
    let paste = rig.auth("provider.auth.start", json!({"provider": "claudeAgent", "method": "paste"})).await;
    let device = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    // When the pasted code is sent to each, and the paste session is completed
    let to_device = rig.ask("provider.auth.respond", json!({"auth": device["auth"], "text": "x"})).await;
    let sent = rig.ask("provider.auth.respond", json!({"auth": paste["auth"], "text": "the-code"})).await;
    let done = rig.auth("provider.auth.complete", json!({"auth": paste["auth"]})).await;
    let late = rig.ask("provider.auth.respond", json!({"auth": paste["auth"], "text": "again"})).await;
    // Then the device flow refuses it, the paste flow took it and ended signed in, and an ended session refuses another
    assert_eq!(error_code(&to_device), "UNSUPPORTED_METHOD");
    assert_eq!(sent["result"]["auth"]["status"], json!("pending"));
    assert_eq!(rig.login.inputs(), ["the-code"]);
    assert_eq!(done["status"], json!("succeeded"));
    assert_eq!(error_code(&late), "AUTH_ENDED");
    // And the prompt's address is shown but the pasted text is not echoed anywhere in what came back
    assert!(!sent.to_string().contains("the-code") && !done.to_string().contains("the-code"));
    assert_eq!(error_code(&rig.ask("provider.auth.respond", json!({"auth": "auth-99", "text": "x"})).await), "UNKNOWN_AUTH");
}

#[tokio::test]
async fn given_a_login_that_fails_keeps_nothing_or_cannot_start_when_it_ends_then_the_session_says_why_and_no_provider_is_signed_in() {
    // Given Codex's login that exits with an error, Claude's that exits cleanly with no login kept, and opencode's CLI that is not installed
    let rig = rig();
    rig.login.script(Driver::Codex, LoginScript::new(&[], Ending::Fails("error: the device code expired".into())));
    rig.login.script(Driver::ClaudeAgent, LoginScript::new(&[], Ending::KeepsNothing));
    // When each is started and completed
    let codex = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    let failed = rig.auth("provider.auth.complete", json!({"auth": codex["auth"]})).await;
    let claude = rig.auth("provider.auth.start", json!({"provider": "claudeAgent"})).await;
    let nothing = rig.auth("provider.auth.complete", json!({"auth": claude["auth"]})).await;
    rig.login.cannot_start("not found");
    let missing = rig.ask("provider.auth.start", json!({"provider": "opencode"})).await;
    // Then the failures carry their reasons, the CLI that is missing is refused with its error, and every provider is still signed out
    assert_eq!((failed["status"].clone(), failed["error"].clone()), (json!("failed"), json!("error: the device code expired")));
    assert_eq!((nothing["status"].clone(), nothing["error"].clone()), (json!("failed"), json!("the login ended without keeping a credential")));
    assert_eq!(error_code(&missing), "LOGIN_UNAVAILABLE");
    assert!(missing["error"]["message"].as_str().unwrap().contains("not found"));
    let listed = rig.ask("provider.list", json!({})).await;
    assert!(listed["result"]["providers"].as_array().unwrap().iter().all(|r| r["status"] == "signed_out"));
    assert_eq!(listed["result"]["selected"], Value::Null);
}

#[tokio::test]
async fn given_a_running_login_when_it_is_cancelled_then_its_command_is_killed_the_session_reads_cancelled_and_a_second_cancel_is_refused() {
    // Given Codex's login that waits
    let rig = rig();
    rig.login.script(Driver::Codex, LoginScript::new(&DEVICE, Ending::Waits));
    let started = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    // When it is cancelled, then completed and cancelled again
    let cancelled = rig.auth("provider.auth.cancel", json!({"auth": started["auth"]})).await;
    let done = rig.auth("provider.auth.complete", json!({"auth": started["auth"], "timeout_ms": 1000})).await;
    let twice = rig.ask("provider.auth.cancel", json!({"auth": started["auth"]})).await;
    // Then the command was killed once, the session reads cancelled throughout, and the second cancel finds it ended
    assert_eq!((cancelled["status"].clone(), done["status"].clone(), rig.login.kills()), (json!("cancelled"), json!("cancelled"), 1));
    assert_eq!(error_code(&twice), "AUTH_ENDED");
    // And Codex may start a new login
    let next = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    assert_ne!(next["auth"], started["auth"]);
}

#[tokio::test]
async fn given_a_login_that_has_not_ended_when_complete_waits_a_short_time_then_it_answers_pending() {
    // Given a login that waits
    let rig = rig();
    rig.login.script(Driver::Codex, LoginScript::new(&DEVICE, Ending::Waits));
    let started = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    // When complete waits 50 ms
    let began = std::time::Instant::now();
    let done = rig.auth("provider.auth.complete", json!({"auth": started["auth"], "timeout_ms": 50})).await;
    // Then it answers the session as it is, pending, after about that time
    assert_eq!(done["status"], json!("pending"));
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(error_code(&rig.ask("provider.auth.complete", json!({"auth": "auth-99"})).await), "UNKNOWN_AUTH");
}

struct Follower {
    answer: Value,
    events: mpsc::UnboundedReceiver<Event>,
}

async fn follow(rig: &Rig, params: Value) -> Follower {
    follow_method(rig, "provider.auth.subscribe", params).await
}

async fn follow_method(rig: &Rig, method: &str, params: Value) -> Follower {
    let (tx, events) = mpsc::unbounded_channel();
    let connection = Connection::new(Arc::new(Queue(tx)));
    let answer = rig.api.dispatch_on(BUILD, &json!({"id": 1, "method": method, "params": params}).to_string(), &connection).await;
    for followed in connection.take_following() {
        tokio::spawn(followed.job);
    }
    Follower { answer, events }
}

impl Follower {
    async fn next(&mut self) -> Event {
        tokio::time::timeout(Duration::from_secs(5), self.events.recv()).await.expect("an event within 5 s").expect("the stream is open")
    }
}

#[tokio::test]
async fn given_a_session_when_it_is_followed_from_the_start_and_from_a_seq_then_the_events_come_in_order_once_and_the_live_ones_follow() {
    // Given a login that has printed its prompt and waits
    let rig = rig();
    rig.login.script(Driver::Codex, LoginScript::new(&DEVICE, Ending::Waits));
    let started = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    // When a follower subscribes from the beginning
    let mut all = follow(&rig, json!({"auth": started["auth"]})).await;
    let last = all.answer["result"]["seq"].as_u64().unwrap();
    // Then it is sent every change in order: the pending start, the address, and the code, ending at the seq the answer named
    let mut seen = Vec::new();
    while seen.last().is_none_or(|e: &Event| e.seq < last) {
        seen.push(all.next().await);
    }
    assert_eq!(seen.iter().map(|e| e.seq).collect::<Vec<_>>(), (1..=last).collect::<Vec<_>>());
    assert!(seen.iter().all(|e| e.kind == "provider.auth.updated" && e.data["auth"]["auth"] == started["auth"]));
    assert_eq!(seen.last().unwrap().data["auth"]["code"], json!("ABCD-1234"));
    // And a follower that resumes after the last seq it saw gets nothing old, and then the live end of the session
    let mut resumed = follow(&rig, json!({"auth": started["auth"], "after_seq": last})).await;
    rig.ask("provider.auth.cancel", json!({"auth": started["auth"]})).await;
    let (live, also) = (resumed.next().await, all.next().await);
    assert_eq!((live.seq, live.data["auth"]["status"].clone()), (last + 1, json!("cancelled")));
    assert_eq!(also.seq, last + 1);
    assert_eq!(error_code(&rig.ask("provider.auth.subscribe", json!({"auth": "auth-99"})).await), "UNKNOWN_AUTH");
}

#[tokio::test]
async fn given_the_provider_stream_when_a_provider_signs_in_then_the_whole_list_is_sent_again_and_nothing_is_sent_while_it_is_unchanged() {
    // Given providers that hold no login, and a follower of the provider list
    let rig = rig();
    let mut stream = follow_method(&rig, "provider.subscribe", json!({})).await;
    // Then the answer names the topic, and the first event is the list as it is now, numbered 1
    assert_eq!(stream.answer["result"], json!({"topic": "provider", "seq": 0}));
    let first = stream.next().await;
    assert_eq!((first.seq, first.kind.as_str(), first.data["selected"].clone()), (1, "provider.updated", Value::Null));
    assert!(first.data["providers"].as_array().unwrap().iter().all(|p| p["status"] == json!("signed_out")));
    // When Codex gets a login behind the service's back
    rig.dirs.sign_in(Driver::Codex);
    // Then the next event is the whole list again, with Codex signed in, numbered 2
    let second = stream.next().await;
    let codex = second.data["providers"].as_array().unwrap().iter().find(|p| p["provider"] == json!("codex")).unwrap().clone();
    assert_eq!((second.seq, codex["status"].clone()), (2, json!("signed_in")));
    // And nothing is sent while the list does not change, and params are refused
    assert!(tokio::time::timeout(Duration::from_millis(2500), stream.events.recv()).await.is_err());
    assert_eq!(error_code(&rig.ask("provider.subscribe", json!({"provider": "codex"})).await), "INVALID_PARAMS");
}

#[tokio::test]
async fn given_signed_in_providers_when_one_is_selected_and_logged_out_then_the_choice_follows_and_a_signed_out_one_cannot_be_chosen() {
    // Given Codex and Claude signed in, and opencode not
    let rig = rig();
    rig.dirs.sign_in(Driver::Codex);
    rig.dirs.sign_in(Driver::ClaudeAgent);
    // When opencode is selected, then Claude, and then Claude logs out
    let signed_out = rig.ask("provider.select", json!({"provider": "opencode"})).await;
    let chosen = rig.ask("provider.select", json!({"provider": "claudeAgent"})).await;
    let logout = rig.ask("provider.logout", json!({"provider": "claudeAgent"})).await;
    // Then opencode is refused as signed out, Claude is the selected provider, and after the logout it is signed out and nothing is selected
    assert_eq!(error_code(&signed_out), "SIGNED_OUT");
    assert_eq!(chosen["result"]["selected"], json!("claudeAgent"));
    assert_eq!((logout["result"]["provider"]["status"].clone(), logout["result"]["provider"]["selected"].clone()), (json!("signed_out"), json!(false)));
    assert_eq!(rig.ask("provider.list", json!({})).await["result"]["selected"], Value::Null);
    // And Codex still holds its login
    assert!(rig.dirs_signed(Driver::Codex));
}

impl Rig {
    fn dirs_signed(&self, driver: Driver) -> bool {
        use crate::ProviderDirs;
        self.dirs.signed_in(driver)
    }
}

#[tokio::test]
async fn given_a_login_running_when_its_provider_logs_out_then_the_login_is_cancelled_first() {
    // Given Codex's login that waits
    let rig = rig();
    rig.login.script(Driver::Codex, LoginScript::new(&DEVICE, Ending::Waits));
    let started = rig.auth("provider.auth.start", json!({"provider": "codex"})).await;
    // When Codex logs out
    rig.ask("provider.logout", json!({"provider": "codex"})).await;
    // Then the session reads cancelled and the command was killed
    assert_eq!(rig.auth("provider.auth.complete", json!({"auth": started["auth"], "timeout_ms": 100})).await["status"], json!("cancelled"));
    assert_eq!(rig.login.kills(), 1);
}

fn window(used: f64, minutes: u64) -> Window {
    Window { used_percent: used, window_minutes: minutes, resets_at_ms: Some(1_700_000_000_000) }
}

#[tokio::test]
async fn given_accounts_that_answer_when_usage_is_asked_then_only_a_signed_in_provider_is_asked_and_a_refusal_is_the_rows_reason() {
    // Given Codex signed in with limits, Claude signed in whose account cannot answer, and opencode signed out
    let rig = rig();
    rig.dirs.sign_in(Driver::Codex);
    rig.dirs.sign_in(Driver::ClaudeAgent);
    let limits = Limits { session: Some(window(12.5, 300)), weekly: None, plan: Some("plus".into()), credits: Some(ResetCredits { available: 2, total_earned: None, next_expires_ms: None }), updated_ms: 5 };
    rig.accounts.with_limits(Driver::Codex, Ok(Some(limits.clone())));
    rig.accounts.with_limits(Driver::ClaudeAgent, Err(AccountError::Unavailable("the Claude usage service answered 401".into())));
    rig.accounts.with_limits(Driver::Opencode, Ok(Some(limits.clone())));
    // When the summary is asked
    let summary = rig.ask("usage.summary", json!({})).await;
    // Then each provider has a row; Codex carries its limits, Claude the reason, and the signed-out provider was not asked
    let rows = summary["result"]["providers"].as_array().unwrap();
    assert_eq!(rows.len(), DRIVERS.len());
    let row = |name: &str| rows.iter().find(|r| r["provider"] == name).unwrap().clone();
    assert_eq!((row("codex")["status"].clone(), row("codex")["limits"]["session"]["used_percent"].clone(), row("codex")["limits"]["credits"]["available"].clone(), row("codex")["limits_error"].clone()), (json!("signed_in"), json!(12.5), json!(2), Value::Null));
    assert_eq!((row("claudeAgent")["limits"].clone(), row("claudeAgent")["limits_error"].clone()), (Value::Null, json!("the Claude usage service answered 401")));
    assert_eq!((row("opencode")["status"].clone(), row("opencode")["limits"].clone(), row("opencode")["limits_error"].clone()), (json!("signed_out"), Value::Null, Value::Null));
}

#[tokio::test]
async fn given_a_credit_and_an_update_when_asked_then_the_outcome_and_the_update_come_back_and_a_retry_shares_its_key_and_refusals_have_their_codes() {
    // Given Codex's account answering a spend and Claude's CLI an update, and nothing for the other providers
    let rig = rig();
    rig.accounts.with_spend(Driver::Codex, Ok(ResetOutcome::AlreadyRedeemed));
    rig.accounts.with_update(Driver::ClaudeAgent, Ok(Updated { ok: true, exit_code: 0, output: "Updated".into(), version: Some("2.2.0".into()) }));
    rig.accounts.with_spend(Driver::ClaudeAgent, Err(AccountError::Unavailable("the executor is gone".into())));
    // When a credit is spent with a key, and with none twice, and an update is run
    let keyed = rig.ask("provider.resetCredit", json!({"provider": "codex", "idempotency_key": "k-1"})).await;
    rig.ask("provider.resetCredit", json!({"provider": "codex"})).await;
    rig.ask("provider.resetCredit", json!({"provider": "codex"})).await;
    let update = rig.ask("provider.update", json!({"provider": "claudeAgent"})).await;
    // Then the outcome and the update are the account's, a named key is passed on, and two spends with no key get two different keys
    assert_eq!(keyed["result"], json!({"provider": "codex", "outcome": "already_redeemed"}));
    assert_eq!(update["result"], json!({"provider": "claudeAgent", "update": {"ok": true, "exit_code": 0, "output": "Updated", "version": "2.2.0"}}));
    let keys = rig.accounts.keys();
    assert_eq!((keys[0].as_str(), keys.len(), keys[1] != keys[2] && !keys[1].is_empty()), ("k-1", 3, true));
    // And a provider with no credit or update is NOT_SUPPORTED, an account that could not answer is ACCOUNT_UNAVAILABLE, and a bad provider or key is refused first
    assert_eq!(error_code(&rig.ask("provider.update", json!({"provider": "codex"})).await), "NOT_SUPPORTED");
    assert_eq!(error_code(&rig.ask("provider.resetCredit", json!({"provider": "claudeAgent"})).await), "ACCOUNT_UNAVAILABLE");
    assert_eq!(error_code(&rig.ask("provider.resetCredit", json!({"provider": "gemini"})).await), "UNKNOWN_PROVIDER");
    assert_eq!(error_code(&rig.ask("provider.resetCredit", json!({"provider": "codex", "idempotency_key": 3})).await), "INVALID_PARAMS");
}

/// An executor that answers the account actions as the real one does, from `answer(action, args)`, and the accounts over it.
fn host_accounts(answer: impl Fn(&str, &Value) -> Result<Value, RemoteError> + Send + Sync + 'static) -> (HostAccounts, Arc<ScriptedChannel>) {
    let channel = ScriptedChannel::new(move |tool, args| answer(tool, args).map(|result| ActionOutput { result, output: String::new() }));
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "w1", channel.clone()).unwrap();
    (HostAccounts::new(executors), channel)
}

#[tokio::test]
async fn given_an_executor_when_the_host_accounts_are_asked_then_codex_goes_through_its_app_server_claude_through_its_usage_and_a_token_is_never_asked_for() {
    // Given an executor answering Codex's app-server, Claude's usage and an update
    let (accounts, channel) = host_accounts(|tool, args| match (tool, args["method"].as_str(), args["request"].as_str()) {
        ("codex_app_server_request", Some("account/rateLimits/read"), _) => Ok(json!({"result": {"rateLimits": {"primary": {"usedPercent": 40, "windowDurationMins": 300, "resetsAt": 1_900_000_000}}, "rateLimitResetCredits": {"availableCount": 1}}})),
        ("codex_app_server_request", Some("account/rateLimitResetCredit/consume"), _) => Ok(json!({"result": {"code": "no_credit"}})),
        ("claude_account_request", _, Some("usage")) => Ok(json!({"status": 200, "body": {"five_hour": {"utilization": 7}}})),
        ("cli_update", ..) => Ok(json!({"program": args["program"], "ok": false, "exit_code": 3, "output": "no network"})),
        other => Err(RemoteError::Action { code: "UNKNOWN_TOOL".into(), message: format!("{other:?}") }),
    });
    // When each provider's limits are read, a credit is spent, and updates are run
    let codex = accounts.limits(Driver::Codex).await.unwrap().unwrap();
    let claude = accounts.limits(Driver::ClaudeAgent).await.unwrap().unwrap();
    let opencode = accounts.limits(Driver::Opencode).await.unwrap();
    let spent = accounts.reset_credit(Driver::Codex, "key-9").await;
    let updated = accounts.update(Driver::Opencode).await.unwrap();
    // Then Codex's windows and credits and Claude's session are read, opencode has no account, and the spend is the account's outcome with the key sent as the app-server expects it
    assert_eq!((codex.session.unwrap().used_percent, codex.credits.unwrap().available), (40.0, 1));
    assert_eq!((claude.session.unwrap().used_percent, opencode), (7.0, None));
    assert_eq!(spent, Ok(ResetOutcome::NoCredit));
    assert_eq!((updated.ok, updated.exit_code, updated.output.as_str(), updated.version), (false, 3, "no network", None));
    let spend = channel.calls().into_iter().find(|(_, a)| a["method"] == "account/rateLimitResetCredit/consume").unwrap();
    assert_eq!(spend.1["params"], json!({"idempotencyKey": "key-9"}));
}

#[tokio::test]
async fn given_executors_that_fail_or_answer_oddly_when_the_host_accounts_are_asked_then_each_is_unavailable_with_its_words_and_codex_has_no_update() {
    // Given an executor refusing everything, one answering a usage with status 401, one answering a spend with an unknown outcome, and none attached
    let (refusing, _) = host_accounts(|_, _| Err(RemoteError::Action { code: "IO_ERROR".into(), message: "the Codex app-server did not answer in time".into() }));
    let (unauthorised, _) = host_accounts(|_, _| Ok(json!({"status": 401, "body": {"error": "expired"}})));
    let (odd, _) = host_accounts(|_, _| Ok(json!({"result": {"outcome": "maybe"}})));
    let nobody = HostAccounts::new(Arc::<Registry>::default());
    // When each is asked
    let timed_out = refusing.limits(Driver::Codex).await;
    let refused = unauthorised.limits(Driver::ClaudeAgent).await;
    let unknown = odd.reset_credit(Driver::Codex, "k").await;
    let none = nobody.limits(Driver::Codex).await;
    // Then each is unavailable with its reason, a provider without credit or update is not supported, and nothing was asked for no executor
    assert_eq!(timed_out, Err(AccountError::Unavailable("the Codex app-server did not answer in time".into())));
    assert_eq!(refused, Err(AccountError::Unavailable("the Claude usage service answered 401".into())));
    assert!(matches!(unknown, Err(AccountError::Unavailable(why)) if why.contains("maybe")));
    assert!(matches!(none, Err(AccountError::Unavailable(why)) if why.contains("no executor")));
    assert!(matches!(refusing.update(Driver::Codex).await, Err(AccountError::NotSupported(_))));
    assert!(matches!(refusing.reset_credit(Driver::ClaudeAgent, "k").await, Err(AccountError::NotSupported(_))));
}

fn ai_rig(diff: &str, models: Arc<ScriptedModelApi>) -> (AgentApi, Arc<ScriptedChannel>) {
    let channel = ScriptedChannel::new({
        let diff = diff.to_string();
        move |tool, _| match tool {
            "git_diff" => Ok(ActionOutput { result: json!({"diff": diff}), output: String::new() }),
            other => Err(RemoteError::Action { code: "UNKNOWN_TOOL".into(), message: other.into() }),
        }
    });
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "w1", channel.clone()).unwrap();
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(GitAiGroup::new(executors, models))];
    (AgentApi::new(groups).unwrap(), channel)
}

async fn generate(api: &AgentApi, params: Value) -> Value {
    api.dispatch_text(BUILD, &json!({"id": 1, "method": "git.generateCommitMessage", "params": params}).to_string()).await
}

#[tokio::test]
async fn given_a_staged_change_when_a_message_is_generated_then_the_model_is_asked_with_that_diff_and_the_tidied_answer_comes_back() {
    // Given a staged diff and a model that wraps its answer in a code fence
    let models = ScriptedModelApi::replying("```\nfix(notes): keep the last line\n```", "double-model");
    let (api, channel) = ai_rig("diff --git a/n.txt b/n.txt\n+keep\n", models.clone());
    // When a message is generated for the workspace
    let answered = generate(&api, json!({"workspace": "w1"})).await;
    // Then the executor was asked for the staged diff, the model was given that diff, and the answer has no fence
    assert_eq!(channel.calls(), [("git_diff".to_string(), json!({"staged": true}))]);
    assert_eq!(answered["result"], json!({"message": "fix(notes): keep the last line", "model": "double-model"}));
    let asked = models.requests();
    assert!(asked.len() == 1 && asked[0].prompt.contains("+keep") && asked[0].prompt.contains("Conventional Commits"), "{asked:?}");
    // And the working tree is asked for when the caller says so, in plain style
    generate(&api, json!({"workspace": "w1", "staged": false, "style": "plain"})).await;
    assert_eq!(channel.calls()[1].1, json!({"staged": false}));
    assert!(!models.requests()[1].prompt.contains("Conventional Commits"));
}

#[tokio::test]
async fn given_no_change_a_failing_model_or_an_unknown_workspace_when_a_message_is_asked_then_each_is_refused_with_its_own_code_and_the_model_is_asked_only_when_there_is_a_change() {
    // Given an empty diff and a model that must not be asked
    let models = ScriptedModelApi::replying("x", "m");
    let (empty, _) = ai_rig("  \n", models.clone());
    // When a message is asked for it, for a workspace with no executor, and (with a diff) of a model that fails and one that answers nothing
    let nothing = generate(&empty, json!({"workspace": "w1"})).await;
    let no_executor = generate(&empty, json!({"workspace": "w2"})).await;
    let down = ScriptedModelApi::replying("x", "m");
    down.fails(ModelError::Unavailable("no key".into()));
    let (failing, _) = ai_rig("d", down);
    let blank = ScriptedModelApi::replying("  \"\"  ", "m");
    let (empty_answer, _) = ai_rig("d", blank);
    // Then each is refused by its code, and the first never reached the model
    assert_eq!(error_code(&nothing), "NOTHING_TO_COMMIT");
    assert_eq!(error_code(&no_executor), "NO_EXECUTOR");
    assert!(models.requests().is_empty());
    assert_eq!((error_code(&generate(&failing, json!({"workspace": "w1"})).await), generate(&failing, json!({"workspace": "w1"})).await["error"]["message"].clone()), ("MODEL_API_UNAVAILABLE", json!("no key")));
    assert_eq!(error_code(&generate(&empty_answer, json!({"workspace": "w1"})).await), "MODEL_API_UNAVAILABLE");
    assert_eq!(error_code(&generate(&empty, json!({})).await), "INVALID_PARAMS");
}

#[test]
fn given_a_long_diff_and_each_style_when_the_prompt_is_built_then_the_diff_is_cut_with_a_note_and_the_rule_matches_the_style() {
    // Given a diff of 30 000 characters and a short one
    let long = "\u{2603}".repeat(30_000);
    // When the prompts are built
    let cut = commit_message_prompt(&long, Style::Conventional);
    let short = commit_message_prompt("+a\n", Style::Plain);
    // Then the long diff is cut at 24 000 characters with a note, and the short one is whole; the rule follows the style
    assert_eq!(cut.prompt.matches('\u{2603}').count(), 24_000);
    assert!(cut.prompt.contains("the diff is cut here") && !short.prompt.contains("the diff is cut here"));
    assert!(cut.prompt.contains("Conventional Commits") && short.prompt.contains("one summary line") && short.prompt.contains("+a"));
}

#[test]
fn given_answers_of_a_model_when_tidied_then_fences_and_quotes_go_and_an_empty_answer_is_none() {
    // Given answers: plain, fenced with a language, fenced bare, quoted, padded, and empty ones
    // Then the message is what is inside, and nothing left is none
    assert_eq!(tidy_message("feat: a\n\nbody line").as_deref(), Some("feat: a\n\nbody line"));
    assert_eq!(tidy_message("```text\nfix: b\n```").as_deref(), Some("fix: b"));
    assert_eq!(tidy_message("```\nchore: c\n```\n").as_deref(), Some("chore: c"));
    assert_eq!(tidy_message("  \"docs: d\"  ").as_deref(), Some("docs: d"));
    assert_eq!((tidy_message(""), tidy_message("   "), tidy_message("\"\""), tidy_message("```\n```")), (None, None, None, None));
}

fn pr_rig(compare: Value, models: Arc<ScriptedModelApi>) -> (AgentApi, Arc<ScriptedChannel>) {
    let channel = ScriptedChannel::new(move |tool, _| match tool {
        "git_compare" => Ok(ActionOutput { result: compare.clone(), output: String::new() }),
        "git_diff" => Ok(ActionOutput { result: json!({"diff": "diff --git a/t.txt b/t.txt\n+t\n"}), output: String::new() }),
        other => Err(RemoteError::Action { code: "UNKNOWN_TOOL".into(), message: other.into() }),
    });
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "w1", channel.clone()).unwrap();
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(GitAiGroup::new(executors, models))];
    (AgentApi::new(groups).unwrap(), channel)
}

async fn generate_pull_request(api: &AgentApi, params: Value) -> Value {
    api.dispatch_text(BUILD, &json!({"id": 1, "method": "git.generatePullRequestFields", "params": params}).to_string()).await
}

fn ahead_by_one() -> Value {
    json!({"base": "main", "head": "HEAD", "merge_base": "abc123", "ahead": 1, "behind": 0, "truncated": false,
        "commits": [{"sha": "d3", "subject": "add t.txt"}], "files": [{"path": "t.txt", "status": "added", "additions": 1, "deletions": 0}]})
}

#[tokio::test]
async fn given_a_branch_ahead_of_its_base_when_pull_request_fields_are_generated_then_the_model_is_given_the_commits_and_the_patch_from_the_merge_base_and_its_json_comes_back() {
    // Given a branch one commit ahead of main and a model that wraps its JSON in a code fence
    let models = ScriptedModelApi::replying("unused", "m");
    models.replying_to_pull_requests("```json\n{\"title\": \" Add t.txt \", \"body\": \"## Problem\\nnone\", \"draft\": true}\n```", "double-model");
    let (api, channel) = pr_rig(ahead_by_one(), models.clone());
    // When the fields are generated against main with a title the person typed
    let answered = generate_pull_request(&api, json!({"workspace": "w1", "base": "main", "title": "wip", "body": "", "draft": false})).await;
    // Then the executor was asked how HEAD differs from main and then for the patch from the merge base, nothing was written
    assert_eq!(channel.calls(), [("git_compare".to_string(), json!({"base": "main", "head": "HEAD", "limit": 30})), ("git_diff".to_string(), json!({"base": "abc123", "head": "HEAD"}))]);
    // And the model was given the commit, the file, the patch and the person's title, and the fields are the model's, with the base asked for
    let asked = models.requests();
    assert!(asked.len() == 1 && asked[0].system == PULL_REQUEST_SYSTEM, "{asked:?}");
    for part in ["- add t.txt", "added t.txt (+1 -0)", "+t\n", "Current title: wip", "Base branch: main"] {
        assert!(asked[0].prompt.contains(part), "{part}: {}", asked[0].prompt);
    }
    assert_eq!(answered["result"], json!({"base": "main", "title": "Add t.txt", "body": "## Problem\nnone", "draft": true, "model": "double-model"}));
}

#[tokio::test]
async fn given_no_commit_ahead_a_bad_answer_a_failing_model_or_a_bad_request_when_fields_are_asked_then_each_is_refused_with_its_own_code_and_the_model_is_asked_only_for_a_real_change() {
    // Given a branch that is not ahead of its base, and a model that must not be asked
    let models = ScriptedModelApi::replying("x", "m");
    let (level, _) = pr_rig(json!({"merge_base": "abc", "ahead": 0, "behind": 0, "commits": [], "files": []}), models.clone());
    let nothing = generate_pull_request(&level, json!({"workspace": "w1", "base": "main"})).await;
    // And answers of the model that are not the fields: prose, a JSON without a title, a title that is not text
    let mut refused = Vec::new();
    for text in ["I would call it Add t.txt", "{\"body\": \"b\"}", "{\"title\": 3}", "{\"title\": \"t\", \"draft\": \"yes\"}"] {
        let models = ScriptedModelApi::replying("x", "m");
        models.replying_to_pull_requests(text, "m");
        let (api, _) = pr_rig(ahead_by_one(), models);
        refused.push(error_code(&generate_pull_request(&api, json!({"workspace": "w1", "base": "main"})).await).to_string());
    }
    // And a model that is down, a workspace with no executor and requests that are not valid
    let down = ScriptedModelApi::replying("x", "m");
    down.fails(ModelError::Unavailable("no key".into()));
    let (failing, _) = pr_rig(ahead_by_one(), down);
    let failed = generate_pull_request(&failing, json!({"workspace": "w1", "base": "main"})).await;
    let no_executor = generate_pull_request(&level, json!({"workspace": "w2", "base": "main"})).await;
    // Then each is refused by its code, and the first never reached the model
    assert_eq!(error_code(&nothing), "NO_BRANCH_CHANGES");
    assert!(models.requests().is_empty());
    assert_eq!(refused, ["MODEL_API_UNAVAILABLE"; 4]);
    assert_eq!((error_code(&failed), failed["error"]["message"].clone()), ("MODEL_API_UNAVAILABLE", json!("no key")));
    assert_eq!(error_code(&no_executor), "NO_EXECUTOR");
    for params in [json!({"workspace": "w1"}), json!({"workspace": "w1", "base": "--upload-pack=x"}), json!({"base": "main"})] {
        assert_eq!(error_code(&generate_pull_request(&level, params).await), "INVALID_PARAMS");
    }
}

#[test]
fn given_a_long_patch_and_answers_of_a_model_when_the_prompt_is_built_and_read_then_the_patch_is_cut_with_a_note_and_only_a_titled_json_object_is_the_fields() {
    // Given a form and a patch of 30 000 characters
    let p = PullRequestParams::parse(&json!({"workspace": "w1", "base": "main"})).unwrap();
    let long = "\u{2603}".repeat(30_000);
    // When the prompt is built
    let prompt = pull_request_prompt(&p, "- c", "added a", &long);
    // Then the patch is cut at 24 000 characters with a note and an empty form is said to be empty
    assert_eq!(prompt.prompt.matches('\u{2603}').count(), 24_000);
    assert!(prompt.prompt.contains("cut here") && prompt.prompt.contains("Current title: (empty)") && prompt.prompt.contains("Current draft: false"));
    // And the fields are read from plain or fenced JSON, with an absent body and draft defaulting, and nothing else is fields
    assert_eq!(read_pull_request_fields("{\"title\":\"t\"}"), Some(("t".into(), String::new(), false)));
    assert_eq!(read_pull_request_fields("```\n{\"title\":\"t\",\"body\":\"b\",\"draft\":true}\n```"), Some(("t".into(), "b".into(), true)));
    assert_eq!((read_pull_request_fields(""), read_pull_request_fields("[]"), read_pull_request_fields("{\"title\":\"  \"}"), read_pull_request_fields("{\"title\":\"t\",\"body\":1}")), (None, None, None, None));
}

async fn watch_providers(rig: &Rig) -> (tokio::task::JoinHandle<()>, mpsc::UnboundedReceiver<Event>) {
    let (tx, events) = mpsc::unbounded_channel();
    let connection = Connection::new(Arc::new(Queue(tx)));
    rig.api.dispatch_on(BUILD, &json!({"id": 1, "method": "provider.subscribe", "params": {}}).to_string(), &connection).await;
    (tokio::spawn(connection.take_following().pop().unwrap().job), events)
}

#[tokio::test]
async fn given_a_provider_watcher_when_its_peer_is_gone_then_the_watcher_ends_at_the_next_change_and_one_aborted_by_the_transport_polls_no_more() {
    // Given two followers of the provider list, each with its job running as the transport runs it
    let rig = rig();
    let (gone, mut gone_events) = watch_providers(&rig).await;
    let (aborted, _kept) = watch_providers(&rig).await;
    gone_events.recv().await.expect("the first list");
    // When one peer goes away and the other connection closes (the transport aborts its job), and a provider then signs in
    drop(gone_events);
    aborted.abort();
    rig.dirs.sign_in(Driver::Codex);
    // Then the watcher of the gone peer ends on its own and the aborted one is finished
    tokio::time::timeout(Duration::from_secs(6), gone).await.expect("the watcher ends once the peer is gone").unwrap();
    assert!(aborted.await.unwrap_err().is_cancelled());
}
