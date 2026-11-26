//! The phone's workspace view preferences through the real router over the agent double: the defaults a host answers, the patch a phone
//! sends, the blob the agent API keeps, and the refusals of a patch this server does not store. The pinned T3 API has no method for them, so
//! these two tags are our extension.
use super::support::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use t3_core::ports::Op;

/// The preferences the server answers when the phone has changed none.
fn defaults() -> Value {
    json!({
        "groupBy": "repo",
        "sortBy": "recent",
        "projectOrderBy": "manual",
        "cardLayout": "detailed",
        "worktreeCardProperties": ["status", "unread", "issue", "linear-issue", "jira-issue", "pr", "automation", "cli", "comment", "ports", "inline-agents", "host"],
        "agentActivityDisplayMode": "compact",
        "filters": {
            "hideSleepingWorkspaces": false,
            "alwaysShowDefaultBranchWorkspace": true,
            "hideDefaultBranchWorkspace": false,
            "hideAutomationGeneratedWorkspaces": false,
            "hideCliCreatedWorkspaces": false,
            "hideDetachedHeadWorkspaces": false,
            "hideWorkspacesFromOtherDevices": false,
            "filterRepoIds": [],
            "visibleHostIds": null,
        },
    })
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let rpc = connect_with(&h, all);
    (h, rpc)
}

/// The device state of the double's agent API: `state.set` keeps a value and `state.get` answers it, as the service's `state` group does.
fn state_of_the_agent(h: &Harness) -> Arc<Mutex<HashMap<(String, String), Value>>> {
    let kept: Arc<Mutex<HashMap<(String, String), Value>>> = Arc::default();
    let answers = kept.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::StateSet { scope, key, value } => {
            answers.lock().unwrap().insert((scope.clone(), key.clone()), value.clone());
            Some(Ok(json!({})))
        }
        Op::StateGet { scope, key } => Some(Ok(json!({"value": answers.lock().unwrap().get(&(scope.clone(), key.clone())).cloned()}))),
        _ => None,
    }));
    kept
}

#[test]
fn the_view_preferences_of_a_phone_that_changed_none_are_the_defaults_and_a_patch_is_answered_merged_over_them() {
    // Given a server whose agent API keeps device state
    let (h, mut rpc) = started();
    let kept = state_of_the_agent(&h);
    // When the phone reads the preferences
    let read = rpc.call("mobile.viewPreferences.get", json!({}));
    // Then they are the server's defaults
    assert_eq!(success(&read), &json!({"version": 1, "preferences": defaults()}));
    // And when the phone groups the workspaces by none, the answer holds that over the defaults
    let one = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"groupBy": "none"}}));
    assert_eq!(success(&one)["preferences"]["groupBy"], json!("none"));
    assert_eq!(success(&one)["preferences"]["sortBy"], json!("recent"));
    assert_eq!(success(&one)["preferences"]["filters"], defaults()["filters"]);
    // And when it hides the sleeping workspaces afterwards, the first change is still there
    let two = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"filters": {"hideSleepingWorkspaces": true}}}));
    assert_eq!(success(&two)["preferences"]["groupBy"], json!("none"));
    assert_eq!(success(&two)["preferences"]["filters"]["hideSleepingWorkspaces"], json!(true));
    // And the phone reads both back
    let again = rpc.call("mobile.viewPreferences.get", json!({}));
    assert_eq!(success(&again)["preferences"]["groupBy"], json!("none"));
    assert_eq!(success(&again)["preferences"]["filters"]["hideSleepingWorkspaces"], json!(true));
    // And the state holds the whole preferences under the t3 scope
    let stored = kept.lock().unwrap().get(&("t3".to_string(), "mobile.viewPreferences".to_string())).cloned();
    let mut expected = defaults();
    expected["groupBy"] = json!("none");
    expected["filters"]["hideSleepingWorkspaces"] = json!(true);
    assert_eq!(stored, Some(json!({"version": 1, "preferences": expected})));
}

#[test]
fn the_view_preferences_are_kept_as_the_versioned_blob_the_state_group_holds() {
    // Given a server whose agent API keeps device state
    let (h, mut rpc) = started();
    let kept = state_of_the_agent(&h);
    // When the phone changes the card layout
    rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"cardLayout": "compact"}}));
    // Then the state holds the blob of the version and the whole preferences, and nothing else
    let mut expected = defaults();
    expected["cardLayout"] = json!("compact");
    assert_eq!(kept.lock().unwrap().get(&("t3".to_string(), "mobile.viewPreferences".to_string())).cloned(), Some(json!({"version": 1, "preferences": expected})));
    assert_eq!(kept.lock().unwrap().len(), 1);
}

#[test]
fn the_view_preferences_outlive_the_server_that_stored_them() {
    // Given a server whose agent API keeps device state and a phone that changed the sort order
    let (h, mut rpc) = started();
    let _kept = state_of_the_agent(&h);
    rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"sortBy": "smart"}}));
    // When a second server starts on the same agent API
    let again = h.restart();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let mut after = connect_with(&again, all);
    // Then it answers the same preferences: they are device state, not the server's memory
    assert_eq!(success(&after.call("mobile.viewPreferences.get", json!({})))["preferences"]["sortBy"], json!("smart"));
}

#[test]
fn a_patch_the_server_does_not_store_is_refused_with_the_path_it_is_at_and_nothing_is_written() {
    // Given a server whose agent API keeps device state
    let (h, mut rpc) = started();
    let kept = state_of_the_agent(&h);
    // When the phone sends a patch with a key no preference has, a value of the wrong type, a value outside its set, a card property that is
    // none, a repository id that is no name and host ids that are no names
    let unknown = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"noSuchPreference": 1}}));
    let unknown_filter = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"filters": {"noSuchFilter": true}}}));
    let wrong_type = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"groupBy": 7}}));
    let bad_literal = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"sortBy": "by-mood"}}));
    let bad_property = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"worktreeCardProperties": ["mood"]}}));
    let bad_repo = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"filters": {"filterRepoIds": [7]}}}));
    let bad_hosts = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"filters": {"visibleHostIds": 7}}}));
    // Then each is a typed refusal that names the path, and nothing was written
    for (exit, path) in [
        (&unknown, "preferences.noSuchPreference"),
        (&unknown_filter, "preferences.filters.noSuchFilter"),
        (&wrong_type, "preferences.groupBy"),
        (&bad_literal, "preferences.sortBy"),
        (&bad_property, "preferences.worktreeCardProperties[0]"),
        (&bad_repo, "preferences.filters.filterRepoIds[0]"),
        (&bad_hosts, "preferences.filters.visibleHostIds"),
    ] {
        assert_eq!((failure(exit)["_tag"].as_str(), failure(exit)["path"].as_str()), (Some("MobileViewPreferencesError"), Some(path)), "{exit}");
        assert!(failure(exit)["message"].as_str().unwrap().contains(path), "{exit}");
    }
    assert!(kept.lock().unwrap().is_empty());
}

#[test]
fn a_version_the_server_does_not_write_is_refused_and_a_stored_blob_of_another_version_is_never_read() {
    // Given a server whose agent API keeps device state
    let (h, mut rpc) = started();
    let kept = state_of_the_agent(&h);
    // When the phone sets a version this server does not write
    let other = rpc.call("mobile.viewPreferences.set", json!({"version": 2, "preferences": {"groupBy": "none"}}));
    // Then it is refused naming the version, and nothing was written
    assert_eq!(failure(&other)["_tag"], json!("MobileViewPreferencesError"));
    assert!(failure(&other)["message"].as_str().unwrap().contains('2'), "{other}");
    assert!(kept.lock().unwrap().is_empty());
    // And when the state holds a blob of another version, the read is refused naming it
    kept.lock().unwrap().insert(("t3".to_string(), "mobile.viewPreferences".to_string()), json!({"version": 2, "preferences": {"groupBy": "none"}}));
    let read = rpc.call("mobile.viewPreferences.get", json!({}));
    assert_eq!(failure(&read)["_tag"], json!("MobileViewPreferencesError"));
    assert!(failure(&read)["message"].as_str().unwrap().contains('2'), "{read}");
}

#[test]
fn a_phone_paired_with_the_scopes_a_plain_pairing_grants_reads_and_writes_the_view_preferences() {
    // Given a phone paired with the six scopes a pairing without a scope of its own grants
    let h = Harness::start();
    let mut rpc = connect_with(&h, PairingRequest { scopes: t3_contracts::scopes::DEFAULT_PAIRING_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() });
    // When it reads the preferences and changes the group by
    let read = rpc.call("mobile.viewPreferences.get", json!({}));
    let set = rpc.call("mobile.viewPreferences.set", json!({"version": 1, "preferences": {"groupBy": "none"}}));
    // Then both are answered: these are the phone's own preferences, not the host's settings
    assert_eq!(success(&read)["preferences"]["groupBy"], json!("repo"));
    assert_eq!(success(&set)["preferences"]["groupBy"], json!("none"));
}

#[test]
fn workspace_preferences_changed_over_http_are_shared_with_rpc_and_enforce_write_scope() {
    // Given two clients of the same host, one with read-only access
    let (h, mut rpc) = started();
    let _state = state_of_the_agent(&h);
    let auth = |request| format!("Bearer {}", bearer_of(h.addr, &h.pair(request).code));
    let reader = auth(PairingRequest { scopes: vec!["orchestration:read".into()], ..Default::default() });
    let writer = auth(PairingRequest::default());
    let path = "/api/mobile/workspace-preferences";
    let post = |token: &str, body: &str| http(h.addr, "POST", path, &[("Authorization", token), ("Content-Type", "application/json")], body);
    let patch = json!({"version": 1, "preferences": {"cardLayout": "compact"}}).to_string();
    // When a reader tries to write, then only an operator can apply the change
    assert_eq!(http(h.addr, "GET", path, &[], "").status, 401);
    assert_eq!(post(&reader, &patch).status, 403);
    assert_eq!(post(&writer, &patch).body["preferences"]["cardLayout"], "compact");
    assert_eq!(post(&writer, r#"{"version":1,"preferences":{"cardLayout":"unknown"}}"#).status, 422);
    // Then HTTP and RPC readers see the same complete saved configuration
    let read = http(h.addr, "GET", path, &[("Authorization", &reader)], "");
    assert_eq!(read.status, 200);
    assert_eq!(read.body, *success(&rpc.call("mobile.viewPreferences.get", json!({}))));
    assert_eq!(read.body["preferences"]["cardLayout"], "compact");
}

#[test]
fn changing_one_disclosure_preserves_other_host_updates_and_is_shared_by_paired_clients() {
    // Given a host with a folded project and a second writer that pins a group during the first edit
    let (h, _rpc) = started();
    let ui = Arc::new(Mutex::new(json!({"collapsedGroups":["repo:old"],"theme":"light"})));
    let store = ui.clone();
    let conflict = std::sync::atomic::AtomicBool::new(true);
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::StateGet { scope, key } if scope == "orca" && key == "ui" => Some(Ok(json!({"value":*store.lock().unwrap()}))),
        Op::StateCompareAndSet { scope, key, expected, value } if scope == "orca" && key == "ui" => {
            let mut record = store.lock().unwrap();
            if conflict.swap(false, std::sync::atomic::Ordering::SeqCst) {
                record["collapsedGroups"] = json!(["repo:old","pinned"]);
                record["theme"] = json!("dark");
            }
            if &*record != expected {
                Some(Ok(json!({"applied":false,"value":*record})))
            } else {
                *record = value.clone();
                Some(Ok(json!({"applied":true})))
            }
        }
        _ => None,
    }));
    let auth = |req| format!("Bearer {}", bearer_of(h.addr, &h.pair(req).code));
    let reader = auth(PairingRequest { scopes: vec!["orchestration:read".into()], ..Default::default() });
    let writer = auth(PairingRequest::default());
    let path = "/api/mobile/workspace-disclosure";
    let get = |token: &str| http(h.addr, "GET", path, &[("Authorization", token)], "");
    let change = |token: &str, key: &str, collapsed| http(h.addr, "POST", path, &[("Authorization", token), ("Content-Type", "application/json")], &json!({"key":key,"collapsed":collapsed}).to_string());
    assert_eq!(get("").status, 401);
    assert_eq!(get(&reader).body, json!(["repo:old"]));
    assert_eq!(change(&reader, "workspace-status:review", true).status, 403);
    assert_eq!(change(&writer, "", true).status, 422);
    // When the writer folds a status, repeats it, and unfolds the original project
    let folded = json!(["repo:old","pinned","workspace-status:review"]);
    assert_eq!(change(&writer, "workspace-status:review", true).body, folded);
    assert_eq!(change(&writer, "workspace-status:review", true).body, folded);
    assert_eq!(change(&writer, "repo:old", false).body, json!(["pinned","workspace-status:review"]));
    // Then the independent reader and the native UI record agree, including the competing edit
    assert_eq!(get(&reader).body, json!(["pinned","workspace-status:review"]));
    assert_eq!(*ui.lock().unwrap(), json!({"collapsedGroups":["pinned","workspace-status:review"],"theme":"dark"}));
}
