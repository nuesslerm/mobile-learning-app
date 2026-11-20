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
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let providers = Providers::default();
        providers.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(AccountsArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, providers, conn: host.connection("dev", "tok", vec![]), n: 0 }
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
