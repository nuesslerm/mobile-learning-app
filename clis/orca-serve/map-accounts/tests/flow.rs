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
    // Then each provider has its one account, Claude's is active and Codex's is not, and no limits are claimed
    assert_eq!((ids(&snapshot["claude"]), ids(&snapshot["codex"])), (vec!["claudeAgent".to_string()], vec!["codex".to_string()]));
    assert_eq!((snapshot["claude"]["activeAccountId"].clone(), snapshot["codex"]["activeAccountId"].clone()), (json!("claudeAgent"), Value::Null));
    assert_eq!((snapshot["rateLimits"]["claude"].clone(), snapshot["rateLimits"]["codex"].clone(), snapshot["rateLimits"]["inactiveCodexAccounts"].clone()), (Value::Null, Value::Null, json!([])));
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
        ("accounts.selectCodex", json!({"accountId": "someone-else"}), "Unknown account"),
        ("accounts.selectCodex", json!({"accountId": null}), "no unmanaged default"),
        ("accounts.selectCodex", json!({}), "no unmanaged default"),
        ("accounts.selectCodexForTarget", json!({"accountId": "codex", "target": {"runtime": "wsl", "wslDistro": "Ubuntu"}}), "no WSL"),
    ] {
        // When the phone asks, Then it is refused as an invalid argument saying why
        let (code, message) = rig.err(method, params);
        assert_eq!(code, "invalid_argument", "{method}");
        assert!(message.contains(says), "{method}: {message}");
    }
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
