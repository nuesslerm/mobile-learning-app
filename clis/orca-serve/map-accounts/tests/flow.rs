//! The accounts rows through the `Host` dispatcher with the three providers of a double as rca: each answer is read the way the phone's accounts
//! screen decodes it, a selection is read back from the next list.
//! Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_accounts::testing::Providers;
use orca_map_accounts::AccountsArea;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    providers: Providers,
    conn: Connection,
    host: Arc<Host>,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let providers = Providers::default();
        providers.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(AccountsArea::default())], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        let conn = host.connection("dev", "tok", vec![]);
        Rig { agent, providers, conn, host, n: 0 }
    }

    /// A second phone paired with the same host.
    fn other_phone(&self) -> Connection {
        self.host.connection("dev2", "tok2", vec![])
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        serde_json::from_str(&self.conn.on_message(&frame)[0]).unwrap()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> (String, String) {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }
}

fn ids(side: &Value) -> Vec<String> {
    side["accounts"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn given_two_signed_in_providers_when_the_accounts_are_listed_then_each_is_the_one_account_of_its_kind_and_the_selected_provider_is_the_active_one() {
    // Given Claude and Codex signed in, Claude the provider rca's turns use, and opencode (which Orca's accounts screen has no section for) signed out
    let mut rig = Rig::new();
    rig.providers.signed_in("claudeAgent").signed_in("codex");
    rig.ok("accounts.selectClaude", json!({"accountId": "claudeAgent"}));
    // When the phone lists the accounts
    let snapshot = rig.ok("accounts.list", json!({"refreshUsage": true}));
    // Then each provider has its one account, Claude's is active and Codex's is not, and the limits are the host's unavailable status, not a claimed window
    assert_eq!((ids(&snapshot["claude"]), ids(&snapshot["codex"])), (vec!["claudeAgent".to_string()], vec!["codex".to_string()]));
    assert_eq!((snapshot["claude"]["activeAccountId"].clone(), snapshot["codex"]["activeAccountId"].clone()), (json!("claudeAgent"), Value::Null));
    assert_eq!((snapshot["rateLimits"]["claude"]["status"].clone(), snapshot["rateLimits"]["codex"]["status"].clone(), snapshot["rateLimits"]["claude"]["session"].clone(), snapshot["rateLimits"]["inactiveCodexAccounts"].clone()), (json!("unavailable"), json!("unavailable"), Value::Null, json!([])));
    assert_eq!((snapshot["claude"]["activeAccountIdsByRuntime"].clone(), snapshot["codex"]["activeAccountIdsByRuntime"].clone(), snapshot["devin"].clone()), (json!({"host": "claudeAgent", "wsl": {}}), json!({"host": null, "wsl": {}}), json!({"accounts": [], "activeAccountId": null})));
    // And the fields the phone's decoder requires of an account are there
    assert!(snapshot["claude"]["accounts"][0]["email"].as_str().is_some_and(|e| !e.is_empty()));
    assert!(snapshot["codex"]["accounts"][0]["updatedAt"].is_u64());
    assert_eq!(snapshot["rateLimits"]["codexTarget"], json!({"runtime": "host", "wslDistro": null}));
}

#[test]
fn given_a_signed_out_provider_when_the_accounts_are_listed_then_it_has_no_account_and_it_cannot_be_selected() {
    // Given only Codex signed in
    let mut rig = Rig::new();
    rig.providers.signed_in("codex");
    // When the phone lists, and then tries to select Claude's account
    let snapshot = rig.ok("accounts.list", json!({}));
    let (code, message) = rig.err("accounts.selectClaude", json!({"accountId": "claudeAgent"}));
    // Then Claude has none, and the selection is refused with rca's reason and changed nothing
    assert_eq!(ids(&snapshot["claude"]), Vec::<String>::new());
    assert_eq!(code, "runtime_error");
    assert!(message.contains("holds no login"), "{message}");
    assert_eq!(rig.providers.selected(), "opencode");
}

#[test]
fn given_codex_signed_in_when_it_is_selected_by_either_row_then_rca_uses_it_and_the_list_shows_it_active() {
    // Given Claude and Codex signed in with Claude selected
    let mut rig = Rig::new();
    rig.providers.signed_in("claudeAgent").signed_in("codex");
    rig.ok("accounts.selectClaude", json!({"accountId": "claudeAgent"}));
    // When Codex is selected, and then Claude again through the row that names a target
    let picked = rig.ok("accounts.selectCodex", json!({"accountId": "codex"}));
    // Then the answer is the new snapshot, with Codex active and Claude not
    assert_eq!((picked["codex"]["activeAccountId"].clone(), picked["claude"]["activeAccountId"].clone()), (json!("codex"), Value::Null));
    assert_eq!(rig.providers.selected(), "codex");
    // When Codex is selected for the host target after Claude took it back
    rig.ok("accounts.selectClaude", json!({"accountId": "claudeAgent"}));
    rig.ok("accounts.selectCodexForTarget", json!({"accountId": "codex", "target": {"runtime": "host", "wslDistro": null}}));
    // Then rca uses Codex again
    assert_eq!(rig.providers.selected(), "codex");
}

#[test]
fn given_a_selection_rca_cannot_honour_when_it_is_asked_then_it_is_refused_before_rca_is_asked() {
    let mut rig = Rig::new();
    rig.providers.signed_in("codex");
    for (method, params, says) in [
        ("accounts.selectCodex", json!({"accountId": null}), "no unmanaged default"),
        ("accounts.selectCodex", json!({}), "Invalid input"),
        ("accounts.selectCodexForTarget", json!({"accountId": "codex", "target": {"runtime": "wsl", "wslDistro": "Ubuntu"}}), "no WSL"),
    ] {
        // When the phone asks, Then it is refused as an invalid argument saying why
        let (code, message) = rig.err(method, params);
        assert_eq!(code, "invalid_argument", "{method}");
        assert!(message.contains(says), "{method}: {message}");
    }
    // And an account the host does not hold is its runtime error with the host's words
    let (code, message) = rig.err("accounts.selectCodex", json!({"accountId": "someone-else"}));
    assert_eq!((code.as_str(), message.as_str()), ("runtime_error", "That Codex rate limit account no longer exists."));
    assert!(rig.agent.requests().is_empty());
}

#[test]
fn given_rca_answers_the_provider_list_without_a_required_field_when_the_accounts_are_listed_then_the_call_is_refused() {
    for (answer, missing) in [
        (json!({"selected": null}), "no providers list"),
        (json!({"providers": [{"status": "signed_in", "selected": true}]}), "a provider with no name"),
        (json!({"providers": [{"provider": "codex", "selected": true}]}), "a provider with no status"),
        (json!({"providers": [{"provider": "codex", "status": "signed_in"}]}), "a provider with no selection flag"),
    ] {
        // Given such an answer from rca
        let mut rig = Rig::new();
        rig.providers.answer_list_with(&rig.agent, answer);
        // When the phone lists the accounts
        let (code, message) = rig.err("accounts.list", json!({}));
        // Then the call is refused naming what was missing, not answered with an account list that omits the provider
        assert_eq!(code, "runtime_error");
        assert!(message.contains(missing), "{message}");
    }
}

#[test]
fn given_a_wrong_typed_refresh_flag_when_the_accounts_are_listed_then_it_is_refused_as_the_schema_refuses_it() {
    // Given a host
    let mut rig = Rig::new();
    // When the phone lists the accounts with a string where a flag belongs
    let refused = rig.err("accounts.list", json!({"refreshUsage": "wrong"}));
    // Then the schema's own message answers, and a boolean is still accepted
    assert_eq!(refused, ("invalid_argument".into(), "Invalid input: expected boolean, received string".into()));
    rig.ok("accounts.list", json!({"refreshUsage": false, "refreshStaleUsage": true}));
}

impl Rig {
    fn frames(&mut self, method: &str, params: Value) -> (String, Vec<Value>) {
        self.n += 1;
        let id = format!("r{}", self.n);
        let frame = json!({"id": id, "method": method, "params": params}).to_string();
        let out = self.conn.on_message(&frame).iter().map(|f| serde_json::from_str(f).unwrap()).collect();
        (id, out)
    }

    fn events_after_waiting(&mut self) -> Vec<Value> {
        std::thread::sleep(std::time::Duration::from_millis(600));
        self.conn.poll().iter().map(|f| serde_json::from_str::<Value>(f).unwrap()["result"].clone()).collect()
    }
}

#[test]
fn given_a_phone_watching_the_accounts_when_a_provider_signs_in_then_it_gets_the_snapshot_first_and_a_new_one_only_for_the_change() {
    // Given Codex signed in and a phone that subscribes
    let mut rig = Rig::new();
    rig.providers.signed_in("codex");
    let (id, frames) = rig.frames("accounts.subscribe", json!({}));
    // Then the stream opens with `ready`, the id of the stream and the snapshot accounts.list answers
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!((frames[0]["id"].clone(), frames[0]["streaming"].clone(), frames[0]["result"]["type"].clone()), (json!(id), json!(true), json!("ready")));
    let stream = frames[0]["result"]["subscriptionId"].as_str().unwrap().to_string();
    assert!(stream.starts_with("accounts-"), "{stream}");
    assert_eq!(frames[0]["result"]["snapshot"], rig.ok("accounts.list", json!({})));
    // When nothing changed for longer than the stream's poll
    assert_eq!(rig.events_after_waiting(), Vec::<Value>::new());
    // And when Claude signs in
    rig.providers.signed_in("claudeAgent");
    let events = rig.events_after_waiting();
    // Then one `snapshot` event carries the list with Claude's account
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["type"], "snapshot");
    assert_eq!(ids(&events[0]["snapshot"]["claude"]), vec!["claudeAgent".to_string()]);
    assert_eq!(events[0]["snapshot"], rig.ok("accounts.list", json!({})));
    // And a list that is unchanged is not sent again
    assert_eq!(rig.events_after_waiting(), Vec::<Value>::new());
}

#[test]
fn given_an_open_accounts_stream_when_the_phone_unsubscribes_then_the_stream_ends_and_an_unknown_id_ends_nothing() {
    // Given an open stream
    let mut rig = Rig::new();
    let (id, frames) = rig.frames("accounts.subscribe", json!({}));
    let stream = frames[0]["result"]["subscriptionId"].clone();
    // When the phone unsubscribes from an id that is none of its streams
    assert_eq!(rig.ok("accounts.unsubscribe", json!({"subscriptionId": "accounts-other-1"})), json!({"unsubscribed": true}));
    // Then the stream still runs: a change still reaches it
    rig.providers.signed_in("codex");
    assert_eq!(rig.events_after_waiting().len(), 1);
    // When it unsubscribes from the stream
    let (_, closing) = rig.frames("accounts.unsubscribe", json!({"subscriptionId": stream}));
    // Then the answer is `unsubscribed`, the stream closes with `end` on its own request, and no change reaches the phone after it
    let answer = closing.iter().find(|f| f["result"]["unsubscribed"] == true).unwrap_or_else(|| panic!("{closing:?}"));
    assert_eq!(answer["ok"], true);
    let end = closing.iter().find(|f| f["id"] == json!(id)).unwrap_or_else(|| panic!("{closing:?}"));
    assert_eq!((end["streaming"].clone(), end["result"].clone()), (json!(true), json!({"type": "end"})));
    rig.providers.signed_in("claudeAgent");
    assert_eq!(rig.events_after_waiting(), Vec::<Value>::new());
    // And a missing or mistyped id is refused in the words of the host
    assert_eq!(rig.err("accounts.unsubscribe", json!({"subscriptionId": 7})), ("invalid_argument".into(), "Missing subscriptionId".into()));
    assert_eq!(rig.err("accounts.unsubscribe", json!({})).1, "Missing subscriptionId");
}

#[test]
fn given_rca_cannot_list_its_providers_when_the_phone_subscribes_then_it_is_refused_and_a_later_failed_read_keeps_the_stream_open() {
    // Given an rca whose provider list lacks the providers
    let mut rig = Rig::new();
    rig.providers.answer_list_with(&rig.agent, json!({"selected": null}));
    // When the phone subscribes, Then it is refused naming what was missing
    let (_, frames) = rig.frames("accounts.subscribe", json!({}));
    assert_eq!(frames[0]["ok"], false);
    assert!(frames[0]["error"]["message"].as_str().unwrap().contains("no providers list"), "{frames:?}");
    // Given a stream that opened, When rca then answers an unreadable list, Then the stream stays open and sends nothing
    let mut rig = Rig::new();
    rig.frames("accounts.subscribe", json!({}));
    rig.providers.answer_list_with(&rig.agent, json!({"selected": null}));
    assert_eq!(rig.events_after_waiting(), Vec::<Value>::new());
    // And once rca answers again with a change, the phone gets it
    rig.providers.install(&rig.agent);
    rig.providers.signed_in("codex");
    assert_eq!(rig.events_after_waiting().len(), 1);
}

#[test]
fn given_an_open_accounts_stream_when_the_phone_drops_without_unsubscribing_then_the_stream_is_ended_and_rca_is_not_asked_again() {
    // Given a phone with an open accounts stream
    let mut rig = Rig::new();
    rig.frames("accounts.subscribe", json!({}));
    assert_eq!(rig.conn.open_streams(), 1);
    // When the connection closes with no `accounts.unsubscribe`
    rig.conn.close();
    let asked = rig.agent.calls().iter().filter(|m| m.as_str() == "provider.list").count();
    // Then no stream is left, and a later change in the providers is never read for it
    assert_eq!(rig.conn.open_streams(), 0);
    rig.providers.signed_in("codex");
    assert_eq!(rig.events_after_waiting(), Vec::<Value>::new());
    assert_eq!(rig.agent.calls().iter().filter(|m| m.as_str() == "provider.list").count(), asked);
}

const CLAUDE_PAGE: &str = "https://claude.com/cai/oauth/authorize?code=true";

fn window(used: f64, minutes: u64, resets: u64) -> Value {
    json!({"used_percent": used, "window_minutes": minutes, "resets_at_ms": resets})
}

#[test]
fn given_provider_accounts_that_answer_when_the_accounts_are_listed_then_each_signed_in_provider_has_its_own_windows_and_a_refusal_is_an_error_status_and_the_answer_is_kept() {
    // Given Codex signed in with windows and credits, Claude signed in whose account refused, and rca's usage.summary saying so
    let mut rig = Rig::new();
    rig.providers.signed_in("claudeAgent").signed_in("codex");
    rig.providers.usage_is(json!({"providers": [
        {"provider": "opencode", "status": "signed_out", "limits": null, "limits_error": null},
        {"provider": "claudeAgent", "status": "signed_in", "limits": null, "limits_error": "the Claude usage service answered 401"},
        {"provider": "codex", "status": "signed_in", "limits": {"session": window(40.0, 300, 1_900_000_000_000), "weekly": window(10.0, 10_080, 1_900_500_000_000), "plan": "plus", "credits": {"available": 2, "total_earned": 5, "next_expires_ms": 1_950_000_000_000u64}, "updated_ms": 1_800_000_000_000u64}, "limits_error": null}]}));
    // When the phone lists the accounts twice
    let first = rig.ok("accounts.list", json!({"refreshUsage": false}));
    let again = rig.ok("accounts.list", json!({"refreshUsage": false}));
    // Then Codex's limits are its windows in the shape the screen reads, with the plan and the credits, and Claude's are an error with the reason
    let codex = &first["rateLimits"]["codex"];
    assert_eq!((codex["status"].clone(), codex["session"].clone(), codex["updatedAt"].clone()), (json!("ok"), json!({"usedPercent": 40.0, "windowMinutes": 300, "resetsAt": 1_900_000_000_000u64, "resetDescription": null}), json!(1_800_000_000_000u64)));
    assert_eq!((codex["weekly"]["usedPercent"].clone(), codex["planType"].clone(), codex["rateLimitResetCredits"].clone()), (json!(10.0), json!("plus"), json!({"availableCount": 2, "totalEarnedCount": 5, "nextExpiresAt": 1_950_000_000_000u64})));
    assert_eq!((first["rateLimits"]["claude"]["status"].clone(), first["rateLimits"]["claude"]["error"].clone(), first["rateLimits"]["claude"]["session"].clone()), (json!("error"), json!("the Claude usage service answered 401"), Value::Null));
    // And the host's accounts were asked once for the two lists, and again only for a forced refresh
    assert_eq!(first, again);
    assert_eq!(rig.agent.calls().iter().filter(|m| m.as_str() == "usage.summary").count(), 1);
    rig.ok("accounts.list", json!({"refreshUsage": true}));
    assert_eq!(rig.agent.calls().iter().filter(|m| m.as_str() == "usage.summary").count(), 2);
}

#[test]
fn given_a_signed_out_provider_when_the_accounts_are_listed_then_its_account_is_not_asked_for_limits() {
    // Given no provider signed in
    let mut rig = Rig::new();
    // When the phone lists the accounts
    let snapshot = rig.ok("accounts.list", json!({}));
    // Then both limits are the host's unavailable status and usage.summary was never asked
    assert_eq!((snapshot["rateLimits"]["claude"]["status"].clone(), snapshot["rateLimits"]["codex"]["status"].clone()), (json!("unavailable"), json!("unavailable")));
    assert!(!rig.agent.calls().contains(&"usage.summary".to_string()));
}

fn scope(account: &str) -> Value {
    json!({"target": {"runtime": "host", "wslDistro": null}, "accountId": account, "accountRevision": 0, "offerRevision": "v1:[2]"})
}

const KEY: &str = "0b9d8f3c-5a1e-4c52-9c7e-2f4a6d1b3e58";

#[test]
fn given_a_codex_credit_when_it_is_spent_then_the_account_is_asked_with_the_key_the_outcome_and_the_accounts_come_back_and_a_retry_is_the_accounts_to_settle() {
    // Given Codex signed in whose account answers a spend with a reset, and then with already redeemed
    let mut rig = Rig::new();
    rig.providers.signed_in("codex");
    // When the phone spends a credit, and again with the same key
    let spent = rig.ok("accounts.consumeCodexResetCredit", json!({"idempotencyKey": KEY, "expectedScope": scope("codex")}));
    rig.providers.spend_answers("already_redeemed");
    let retried = rig.ok("accounts.consumeCodexResetCredit", json!({"idempotencyKey": KEY, "expectedScope": scope("codex")}));
    // Then each outcome is the account's, the scope is echoed, the snapshot has both account lists and the limits, and both spends carried the one key
    assert_eq!((spent["outcome"].clone(), retried["outcome"].clone(), spent["scope"].clone()), (json!("reset"), json!("alreadyRedeemed"), scope("codex")));
    assert_eq!((ids(&spent["snapshot"]["codex"]), spent["snapshot"]["rateLimits"]["codex"]["provider"].clone()), (vec!["codex".to_string()], json!("codex")));
    assert_eq!(rig.providers.spends(), vec![("codex".to_string(), KEY.to_string()), ("codex".to_string(), KEY.to_string())]);
}

#[test]
fn given_a_credit_spend_the_schema_or_rca_refuses_when_it_is_asked_then_it_is_refused_before_the_account_is_asked() {
    // Given Codex signed in
    let mut rig = Rig::new();
    rig.providers.signed_in("codex");
    // When the spend has a key that is no uuid, no scope, an account that is none, a WSL target, and a scope with a bad revision
    let bad_key = rig.err("accounts.consumeCodexResetCredit", json!({"idempotencyKey": "a", "expectedScope": scope("codex")}));
    let no_payload = rig.err("accounts.consumeCodexResetCredit", Value::Null);
    let no_scope = rig.err("accounts.consumeCodexResetCredit", json!({"idempotencyKey": KEY}));
    let other = rig.err("accounts.consumeCodexResetCredit", json!({"idempotencyKey": KEY, "expectedScope": scope("someone")}));
    let wsl = rig.err("accounts.consumeCodexResetCredit", json!({"idempotencyKey": KEY, "expectedScope": {"target": {"runtime": "wsl", "wslDistro": "Ubuntu"}, "accountId": "codex", "accountRevision": 0, "offerRevision": "v1:[]"}}));
    let offer = rig.err("accounts.consumeCodexResetCredit", json!({"idempotencyKey": KEY, "expectedScope": {"target": {"runtime": "host", "wslDistro": null}, "accountId": "codex", "accountRevision": 0, "offerRevision": "x"}}));
    // Then each is refused with its reason and nothing was spent
    assert_eq!((bad_key.0.as_str(), bad_key.1.as_str()), ("invalid_argument", "Invalid idempotencyKey"));
    assert_eq!(no_payload.1, "Invalid idempotencyKey");
    assert_eq!(no_scope.0, "invalid_argument");
    assert_eq!((other.0.as_str(), other.1.as_str()), ("runtime_error", "That Codex rate limit account no longer exists."));
    assert!(wsl.1.contains("no WSL") && offer.1 == "Invalid offerRevision");
    assert!(rig.providers.spends().is_empty());
}

fn start_flow(rig: &mut Rig, provider: &str) -> Value {
    rig.ok("accounts.hostSignInStart", json!({"provider": provider}))
}

#[test]
fn given_claude_when_the_phone_signs_it_in_from_the_host_then_the_prompt_is_waited_for_the_pasted_code_is_sent_once_and_the_flow_ends_signed_in_with_the_account() {
    // Given a rca whose Claude login is not running yet
    let mut rig = Rig::new();
    // When the phone starts the sign-in
    let started = start_flow(&mut rig, "claude");
    let flow = started["flowId"].as_str().unwrap().to_string();
    // Then it answers the state before the CLI printed anything: one revision, starting, no prompt
    assert_eq!((started["provider"].clone(), started["revision"].clone(), started["phase"].clone(), started.get("prompt")), (json!("claude"), json!(1), json!("starting"), None));
    assert_eq!(flow.len(), 36);
    // And a code cannot be sent before the page is shown
    assert_eq!(rig.err("accounts.hostSignInSubmitCode", json!({"flowId": flow, "code": "x"})).1, "This sign-in is not waiting for a code.");
    // And when the CLI prints its address, a wait after revision 1 returns the prompt at revision 2
    rig.providers.prints(CLAUDE_PAGE, None);
    let waiting = rig.ok("accounts.hostSignInWait", json!({"flowId": flow, "afterRevision": 1}));
    assert_eq!((waiting["revision"].clone(), waiting["phase"].clone(), waiting["prompt"].clone()), (json!(2), json!("waiting"), json!({"url": CLAUDE_PAGE, "userCode": null, "needsCode": true})));
    // And the pasted code goes to rca's login once and the flow is verifying; a second code is refused
    let verifying = rig.ok("accounts.hostSignInSubmitCode", json!({"flowId": flow, "code": "pasted-1"}));
    assert_eq!((verifying["phase"].clone(), verifying["revision"].clone()), (json!("verifying"), json!(3)));
    assert_eq!(rig.providers.pasted().as_deref(), Some("pasted-1"));
    assert_eq!(rig.err("accounts.hostSignInSubmitCode", json!({"flowId": flow, "code": "pasted-2"})).1, "This sign-in is not waiting for a code.");
    // And when the login ends the flow is signed in and carries the Claude account
    rig.providers.login_ends(true, None);
    let done = rig.ok("accounts.hostSignInWait", json!({"flowId": flow, "afterRevision": 3}));
    assert_eq!((done["phase"].clone(), done["revision"].clone(), ids(&done["claude"])), (json!("signedIn"), json!(4), vec!["claudeAgent".to_string()]));
    assert!(done.get("prompt").is_none() && done.get("codex").is_none());
}

#[test]
fn given_codex_when_the_phone_signs_it_in_by_device_code_then_the_prompt_carries_the_user_code_and_needs_no_paste_and_a_page_off_the_providers_host_is_never_shown() {
    // Given Codex's login that first prints an address on a host that is not OpenAI's, then the real one with a code
    let mut rig = Rig::new();
    let flow = start_flow(&mut rig, "codex")["flowId"].as_str().unwrap().to_string();
    rig.providers.prints("https://evil.example.test/device", Some("ABCD-1234"));
    // When the phone asks for a change after the start
    let hidden = rig.ok("accounts.hostSignInWait", json!({"flowId": flow, "afterRevision": 0}));
    // Then the foreign address is not shown: the flow is still starting at its first revision
    assert_eq!((hidden["phase"].clone(), hidden["revision"].clone(), hidden.get("prompt")), (json!("starting"), json!(1), None));
    // And the provider's own page is, with the device code and no paste
    rig.providers.prints("https://auth.openai.com/codex/device", Some("ABCD-1234"));
    let shown = rig.ok("accounts.hostSignInWait", json!({"flowId": flow, "afterRevision": 1}));
    assert_eq!(shown["prompt"], json!({"url": "https://auth.openai.com/codex/device", "userCode": "ABCD-1234", "needsCode": false}));
    assert_eq!(rig.err("accounts.hostSignInSubmitCode", json!({"flowId": flow, "code": "x"})).1, "This sign-in is not waiting for a code.");
    // And a login that fails ends the flow failed with the CLI's words
    rig.providers.login_ends(false, Some("the device code expired"));
    let failed = rig.ok("accounts.hostSignInWait", json!({"flowId": flow, "afterRevision": 2}));
    assert_eq!((failed["phase"].clone(), failed["error"].clone()), (json!("failed"), json!("the device code expired")));
}

#[test]
fn given_a_running_sign_in_when_another_starts_another_phone_asks_or_it_is_cancelled_then_each_is_answered_as_the_host_answers() {
    // Given a Claude sign-in running for this phone
    let mut rig = Rig::new();
    let flow = start_flow(&mut rig, "claude")["flowId"].as_str().unwrap().to_string();
    // When the same phone starts Claude again, starts Codex, and another phone asks about the flow
    let same = start_flow(&mut rig, "claude");
    let other_provider = rig.err("accounts.hostSignInStart", json!({"provider": "codex"}));
    let mut stranger = rig.other_phone();
    let asked = serde_json::from_str::<Value>(&stranger.on_message(&json!({"id": "x", "method": "accounts.hostSignInWait", "params": {"flowId": flow}}).to_string())[0]).unwrap();
    // Then the same provider answers the running flow, another provider is refused, and a flow another phone started is no longer active for this one
    assert_eq!(same["flowId"], json!(flow));
    assert_eq!(other_provider.1, "Another account sign-in is already running on this host. Finish or cancel it first.");
    assert_eq!((asked["ok"].clone(), asked["error"]["message"].clone()), (json!(false), json!("This sign-in is no longer active in this client.")));
    // And a cancel stops rca's login and answers the flow cancelled; an unknown flow is no longer active
    let cancelled = rig.ok("accounts.hostSignInCancel", json!({"flowId": flow}));
    assert_eq!(cancelled["phase"], json!("cancelled"));
    assert_eq!(rig.err("accounts.hostSignInCancel", json!({"flowId": "nope"})).1, "This sign-in is no longer active in this client.");
    // And a provider that is neither claude nor codex, and a flow id of the wrong type, are refused by the schema's words
    assert_eq!(rig.err("accounts.hostSignInStart", json!({"provider": "opencode"})).1, "Invalid option: expected one of \"claude\"|\"codex\"");
    assert_eq!(rig.err("accounts.hostSignInWait", json!({"flowId": 7})).1, "Invalid input: expected string, received number");
    assert_eq!(rig.err("accounts.hostSignInSubmitCode", json!({"flowId": "a"})).1, "Invalid input: expected string, received undefined");
}
