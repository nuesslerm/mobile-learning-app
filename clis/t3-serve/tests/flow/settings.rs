//! The M10 rows through the real router over the agent double: the settings a phone changes (kept as device state), the provider logins (the
//! `accounts` group's recorded answers), the usage summary and the host's resources. What a request becomes (the `Op` the agent API is asked) is
//! read from the data the contract recorded, and so is what the phone is told; no token appears anywhere because the contract carries none.
use super::support::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use t3_core::ports::{AgentError, HostSample, Op, StorageSample};

/// A recorded result of `method` whose exchange is named like `name`.
fn recorded(method: &str, name: &str) -> Value {
    let fixtures = agent_api_accounts::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && e.succeeds());
    found.unwrap_or_else(|| panic!("no recorded {method} exchange named like {name}")).response["result"].clone()
}

fn refused(method: &str, name: &str) -> AgentError {
    let fixtures = agent_api_accounts::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && !e.succeeds()).unwrap_or_else(|| panic!("no recorded refusal of {method} named like {name}"));
    AgentError::Refused { code: found.response["error"]["code"].as_str().unwrap().into(), message: found.response["error"]["message"].as_str().unwrap().into() }
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let rpc = connect_with(&h, all);
    (h, rpc)
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

fn forget_ops(h: &Harness) {
    h.agent.ops.lock().unwrap().clear();
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

/// Two settings of the server's own defaults that are booleans, each with the value that is not the default.
fn two_flippable_settings() -> [(String, Value); 2] {
    let defaults = t3_contracts::wire::default_settings();
    let flipped: Vec<(String, Value)> = defaults.iter().filter_map(|(k, v)| Some((k.clone(), json!(!v.as_bool()?)))).take(2).collect();
    [flipped[0].clone(), flipped[1].clone()]
}

#[test]
fn settings_a_phone_changes_are_kept_as_device_state_and_every_later_read_of_the_config_and_update_shows_them() {
    // Given two boolean settings of the defaults, and an agent API that keeps device state
    let (h, mut rpc) = started();
    let kept = state_of_the_agent(&h);
    let [(first, first_value), (second, second_value)] = two_flippable_settings();
    // When the phone changes the first and then the second
    let one = rpc.call("server.updateSettings", json!({"patch": {first.clone(): first_value}}));
    let two = rpc.call("server.updateSettings", json!({"patch": {second.clone(): second_value}}));
    // Then the second answer holds both changes over the defaults, and the first only its own
    assert_eq!((success(&one)[&first].clone(), success(&one)[&second].clone()), (first_value.clone(), t3_contracts::wire::default_settings()[&second].clone()));
    assert_eq!((success(&two)[&first].clone(), success(&two)[&second].clone()), (first_value.clone(), second_value.clone()));
    // And device state holds the changes, and only them, under the t3 scope
    let stored = kept.lock().unwrap().get(&("t3".to_string(), "server.settings".to_string())).cloned();
    assert_eq!(stored, Some(json!({first.clone(): first_value, second.clone(): second_value})));
    // And the config the phone reads now carries them
    let config = rpc.call("server.getConfig", json!({}));
    assert_eq!((success(&config)["settings"][&first].clone(), success(&config)["settings"][&second].clone()), (first_value.clone(), second_value.clone()));
    // And a second server on the same agent API shows the same settings: they outlive the process
    let again = h.restart();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let mut after = connect_with(&again, all);
    assert_eq!(success(&after.call("server.getConfig", json!({})))["settings"][&first], first_value);
}

#[test]
fn a_setting_the_server_does_not_have_and_a_provider_instance_change_are_refused_and_nothing_is_stored() {
    // Given an agent API that keeps device state
    let (h, mut rpc) = started();
    let kept = state_of_the_agent(&h);
    let [(known, value), _] = two_flippable_settings();
    // When the phone patches a key no setting has, with a known one beside it, and asks for a provider instance mutation
    let unknown = rpc.call("server.updateSettings", json!({"patch": {known: value, "noSuchSetting": 1}}));
    let mutation = rpc.call("server.updateSettings", json!({"patch": {}, "providerInstanceMutation": {"type": "remove", "instanceId": "codex"}}));
    // Then each is a ServerSettingsError that names what was refused, and nothing was written
    assert_eq!((failure(&unknown)["_tag"].as_str(), failure(&unknown)["settingsPath"].as_str()), (Some("ServerSettingsError"), Some("patch.noSuchSetting")));
    assert_eq!((failure(&mutation)["_tag"].as_str(), failure(&mutation)["settingsPath"].as_str()), (Some("ServerSettingsError"), Some("providerInstanceMutation")));
    assert!(kept.lock().unwrap().is_empty());
    assert!(!ops(&h).iter().any(|op| matches!(op, Op::StateSet { .. })));
    // And a request with no patch is a defect on the wire
    let none = rpc.call("server.updateSettings", json!({}));
    assert_eq!(none["_tag"], "Failure");
}

#[test]
fn a_device_code_login_shows_the_address_and_code_the_cli_printed_and_a_paste_login_asks_for_the_code_and_hands_it_on() {
    // Given the recorded start of a device login for codex
    let (h, mut rpc) = started();
    let device = recorded("provider.auth.start", "starts a device login");
    let view = device["auth"].clone();
    reply(&h, "provider.auth.start", Ok(device.clone()));
    // When the phone starts the login
    let state = rpc.call("provider.auth.start", json!({"instanceId": "codex"}));
    let state = success(&state);
    // Then the agent API was asked to start codex's login, and the phone is waiting with the address and code the CLI printed
    assert_eq!(ops(&h), [Op::AuthStart { provider: "codex".into(), method: None }]);
    assert_eq!((state["instanceId"].clone(), state["phase"].clone(), state["flowId"].clone(), state["authorizationUrl"].clone()), (json!("codex"), json!("waiting"), view["auth"].clone(), view["url"].clone()));
    assert_eq!(state["interaction"], json!({"type": "deviceCode", "id": view["auth"], "url": view["url"], "userCode": view["code"]}));
    assert_eq!(state["credentialOwner"], json!("provider"));
    // And the state carries nothing but what the contract's view held
    assert!(!state.to_string().contains("token"));
    // Given the start of a paste login, which the CLI answers with an address and no code
    forget_ops(&h);
    let mut paste = device.clone();
    paste["auth"]["method"] = json!("paste");
    paste["auth"]["code"] = Value::Null;
    reply(&h, "provider.auth.start", Ok(paste));
    // When the phone starts it with the method named, and then answers the terminal interaction with the code the person pasted
    let asked = rpc.call("provider.auth.start", json!({"instanceId": "claudeAgent", "methodId": "paste"}));
    let flow = view["auth"].as_str().unwrap();
    reply(&h, "provider.auth.respond", Ok(json!({"auth": {"auth": flow, "provider": "claudeAgent", "method": "paste", "status": "pending", "url": view["url"], "code": null, "error": null}})));
    let answered = rpc.call("provider.auth.respond", json!({"instanceId": "claudeAgent", "flowId": flow, "interactionId": flow, "response": {"type": "terminal", "data": "pasted-code-1\r\n"}}));
    // Then the interaction was a terminal one naming the address, and the code went to the CLI without its line end
    assert_eq!(success(&asked)["interaction"]["type"], json!("terminal"));
    assert!(success(&asked)["interaction"]["output"].as_str().unwrap().contains(view["url"].as_str().unwrap()));
    assert_eq!(ops(&h), [Op::AuthStart { provider: "claudeAgent".into(), method: Some("paste".into()) }, Op::AuthRespond { auth: flow.into(), text: "pasted-code-1".into() }]);
    assert_eq!(success(&answered)["phase"], json!("waiting"));
}

#[test]
fn a_credential_is_never_taken_through_the_mapper_a_refused_login_is_a_setup_error_with_the_cli_s_words_and_a_paste_completion_hands_the_code_on() {
    // Given a started paste login
    let (h, mut rpc) = started();
    let device = recorded("provider.auth.start", "starts a device login");
    let flow = device["auth"]["auth"].as_str().unwrap().to_string();
    let mut paste = device.clone();
    paste["auth"]["method"] = json!("paste");
    paste["auth"]["code"] = Value::Null;
    reply(&h, "provider.auth.start", Ok(paste));
    rpc.call("provider.auth.start", json!({"instanceId": "claudeAgent", "methodId": "paste"}));
    forget_ops(&h);
    // When the phone answers with a credential form
    let credentials = rpc.call("provider.auth.respond", json!({"instanceId": "claudeAgent", "flowId": flow, "interactionId": flow, "response": {"type": "credentials", "values": {"token": "sk-secret"}}}));
    // Then it is refused with a ProviderSetupError and the agent API was not asked
    assert_eq!((failure(&credentials)["_tag"].as_str(), failure(&credentials)["instanceId"].as_str()), (Some("ProviderSetupError"), Some("claudeAgent")));
    assert!(!credentials.to_string().contains("sk-secret"));
    assert!(ops(&h).is_empty());
    // When the phone completes the login with the address it was sent back to, which carries the code
    reply(&h, "provider.auth.complete", Ok(recorded("provider.auth.complete", "answers succeeded")));
    let done = rpc.call("provider.auth.complete", json!({"instanceId": "claudeAgent", "flowId": flow, "callbackUrl": "https://login.example.test/cb?state=s&code=the-code&x=1"}));
    // Then the CLI got the code from the address, the login was waited for, and the phone sees it succeeded
    assert_eq!(ops(&h), [Op::AuthRespond { auth: flow.clone(), text: "the-code".into() }, Op::AuthComplete { auth: flow.clone(), timeout_ms: 15_000 }]);
    assert_eq!(success(&done)["phase"], json!("succeeded"));
    // When the CLI refuses a method it lacks
    reply(&h, "provider.auth.start", Err(refused("provider.auth.start", "a method the CLI lacks")));
    let lacks = rpc.call("provider.auth.start", json!({"instanceId": "codex", "methodId": "paste"}));
    // Then the phone is told the CLI's own words
    assert_eq!((failure(&lacks)["_tag"].as_str(), failure(&lacks)["detail"].as_str(), failure(&lacks)["operation"].as_str()), (Some("ProviderSetupError"), Some("codex signs in by device_code; paste is not one of them"), Some("start")));
}

#[test]
fn a_device_login_is_only_waited_for_and_cancel_and_logout_reach_the_agent_api_and_leave_the_provider_idle() {
    // Given a started device login and the recorded cancel, completion and logout
    let (h, mut rpc) = started();
    let device = recorded("provider.auth.start", "starts a device login");
    let flow = device["auth"]["auth"].as_str().unwrap().to_string();
    reply(&h, "provider.auth.start", Ok(device));
    reply(&h, "provider.auth.complete", Ok(recorded("provider.auth.complete", "answers succeeded")));
    reply(&h, "provider.auth.cancel", Ok(recorded("provider.auth.cancel", "cancels a running login")));
    reply(&h, "provider.logout", Ok(recorded("provider.logout", "signs a provider out")));
    reply(&h, "provider.list", Ok(recorded("provider.list", "lists every provider")));
    rpc.call("provider.auth.start", json!({"instanceId": "codex"}));
    forget_ops(&h);
    // When the phone completes it (a device code has no address to hand over), cancels it and signs the provider out
    let done = rpc.call("provider.auth.complete", json!({"instanceId": "codex", "flowId": flow, "callbackUrl": "https://login.example.test/cb"}));
    let cancelled = rpc.call("provider.auth.cancel", json!({"instanceId": "codex", "flowId": flow}));
    let out = rpc.call("provider.auth.logout", json!({"instanceId": "codex"}));
    // Then the CLI was never handed a code, the login was waited for, cancelled and signed out
    assert_eq!(ops(&h)[..3], [Op::AuthComplete { auth: flow.clone(), timeout_ms: 15_000 }, Op::AuthCancel { auth: flow.clone() }, Op::AccountsLogout { provider: "codex".into() }]);
    assert_eq!((success(&done)["phase"].clone(), success(&cancelled)["phase"].clone()), (json!("succeeded"), json!("cancelled")));
    // And after the logout the provider is idle with the login methods its CLI offers, and the session is forgotten
    assert_eq!((success(&out)["phase"].clone(), success(&out)["flowId"].clone()), (json!("idle"), Value::Null));
    let offered = recorded("provider.list", "lists every provider")["providers"].as_array().unwrap().iter().find(|p| p["provider"] == "codex").unwrap()["auth_methods"].clone();
    let ids: Vec<Value> = success(&out)["methods"].as_array().unwrap().iter().map(|m| m["id"].clone()).collect();
    assert_eq!(json!(ids), offered);
    // And a response naming no known type is a defect on the wire
    let defect = rpc.call("provider.auth.respond", json!({"instanceId": "codex", "flowId": flow, "interactionId": flow, "response": {"type": "nothing"}}));
    assert_eq!(defect["_tag"], "Failure");
}

#[test]
fn the_login_stream_says_where_the_login_stands_and_each_state_it_passes_through_and_follows_a_login_started_after_it_opened() {
    // Given an idle provider and the states a login passes through: waiting for the code, and then done
    let (h, mut rpc) = started();
    reply(&h, "provider.list", Ok(recorded("provider.list", "lists every provider")));
    let device = recorded("provider.auth.start", "starts a device login");
    let pending = device["auth"].clone();
    let flow = pending["auth"].as_str().unwrap().to_string();
    let mut succeeded = pending.clone();
    succeeded["status"] = json!("succeeded");
    h.agent.auth_states.lock().unwrap().insert(flow.clone(), vec![pending.clone(), succeeded]);
    // When the phone subscribes before any login has started
    let mut watch = connect_with(&h, PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() });
    let id = watch.send("provider.auth.subscribe", json!({"instanceId": "codex"}));
    let first = watch.chunk(&id);
    // Then the stream starts with the idle state
    assert_eq!((first[0]["phase"].clone(), first[0]["flowId"].clone()), (json!("idle"), Value::Null));
    // When a login is started on the other socket
    reply(&h, "provider.auth.start", Ok(device));
    rpc.call("provider.auth.start", json!({"instanceId": "codex"}));
    // Then the stream carries the states in order: the prompt and then success, with the code the CLI printed
    let mut phases: Vec<Value> = Vec::new();
    let mut waiting_code = Value::Null;
    let deadline = Instant::now() + Duration::from_secs(5);
    while phases.last() != Some(&json!("succeeded")) {
        assert!(Instant::now() < deadline, "the stream said {phases:?}");
        let Some(frame) = watch.try_recv(Duration::from_millis(300)) else { continue };
        if frame["_tag"] != "Chunk" {
            continue;
        }
        watch.ack(&id);
        for state in frame["values"].as_array().unwrap() {
            if state["phase"] == "waiting" {
                waiting_code = state["interaction"]["userCode"].clone();
            }
            phases.push(state["phase"].clone());
        }
    }
    assert_eq!((phases, waiting_code), (vec![json!("waiting"), json!("succeeded")], pending["code"].clone()));
}

#[test]
fn refreshing_the_providers_asks_the_accounts_group_to_look_again_and_shows_each_as_signed_in_or_out_with_the_login_offered() {
    // Given the recorded list with codex signed in
    let (h, mut rpc) = started();
    let mut list = recorded("provider.list", "lists every provider");
    list["providers"].as_array_mut().unwrap().iter_mut().for_each(|p| p["status"] = json!(if p["provider"] == "codex" { "signed_in" } else { "signed_out" }));
    reply(&h, "provider.refresh", Ok(recorded("provider.refresh", "refreshes one provider")));
    reply(&h, "provider.list", Ok(list.clone()));
    // When the phone refreshes every provider, and then only codex
    let all = rpc.call("server.refreshProviders", json!({}));
    let one = rpc.call("server.refreshProviders", json!({"instanceId": "codex"}));
    // Then the agent API refreshed (every provider, then one) and listed each time
    assert_eq!(ops(&h), [Op::AccountsRefresh { provider: None }, Op::AccountsList, Op::AccountsRefresh { provider: Some("codex".into()) }, Op::AccountsList]);
    // And every provider can be signed in, and its auth status follows the list
    for answer in [&all, &one] {
        for provider in success(answer)["providers"].as_array().unwrap() {
            let status = list["providers"].as_array().unwrap().iter().find(|p| p["provider"] == provider["instanceId"]).unwrap()["status"].clone();
            assert_eq!(provider["setup"]["canAuthenticate"], json!(true));
            assert_eq!(provider["auth"]["status"], json!(if status == "signed_in" { "authenticated" } else { "unauthenticated" }));
        }
    }
    // When the agent API refuses a provider it does not know
    reply(&h, "provider.refresh", Err(refused("provider.refresh", "a provider that is none")));
    let unknown = rpc.call("server.refreshProviders", json!({"instanceId": "gemini"}));
    // Then it is a ProviderSetupError with its words
    assert_eq!((failure(&unknown)["_tag"].as_str(), failure(&unknown)["detail"].as_str().map(|d| d.starts_with("gemini is not a provider"))), (Some("ProviderSetupError"), Some(true)));
}

#[test]
fn the_usage_summary_has_a_source_for_each_provider_the_accounts_group_lists_and_refuses_a_window_that_ends_before_it_starts() {
    // Given the recorded usage of the three providers, codex signed in
    let (h, mut rpc) = started();
    let mut usage = recorded("usage.summary", "shows each provider");
    usage["providers"].as_array_mut().unwrap().iter_mut().for_each(|p| {
        if p["provider"] == "codex" {
            p["status"] = json!("signed_in");
        }
    });
    reply(&h, "usage.summary", Ok(usage.clone()));
    // When the phone asks for a window
    let summary = rpc.call("server.getUsageSummary", json!({"sinceDay": "2026-11-01", "untilDay": "2026-11-04", "timeZone": "Europe/Berlin"}));
    let summary = success(&summary);
    // Then the agent API was asked for the usage, and the answer has one source per provider, ok for the signed in one and missing for the others
    assert_eq!(ops(&h), [Op::UsageSummary]);
    let sources: Vec<(Value, Value)> = summary["sources"].as_array().unwrap().iter().map(|s| (s["fingerprint"]["provider"].clone(), s["status"].clone())).collect();
    let expected: Vec<(Value, Value)> = usage["providers"].as_array().unwrap().iter().map(|p| (json!(if p["provider"] == "claudeAgent" { "claude" } else { p["provider"].as_str().unwrap() }), json!(if p["status"] == "signed_in" { "ok" } else { "missing" }))).collect();
    assert_eq!(sources, expected);
    assert_eq!((summary["sinceDay"].clone(), summary["untilDay"].clone(), summary["timeZone"].clone(), summary["buckets"].clone(), summary["pricing"]["status"].clone()), (json!("2026-11-01"), json!("2026-11-04"), json!("Europe/Berlin"), json!([]), json!("unavailable")));
    assert_eq!(summary["readAt"], json!(t3_contracts::time::iso(START_MS)));
    // When the window ends before it starts
    forget_ops(&h);
    let backwards = rpc.call("server.getUsageSummary", json!({"sinceDay": "2026-11-04", "untilDay": "2026-11-01", "timeZone": "UTC"}));
    // Then it is a UsageReadError and the agent API was not asked, and a day that is no date is a defect
    assert_eq!((failure(&backwards)["_tag"].as_str(), failure(&backwards)["reason"].as_str()), (Some("UsageReadError"), Some("invalidWindow")));
    assert!(ops(&h).is_empty());
    assert_eq!(rpc.call("server.getUsageSummary", json!({"sinceDay": "yesterday", "untilDay": "2026-11-01", "timeZone": "UTC"}))["_tag"], "Failure");
}

#[test]
fn a_reset_credit_the_model_api_gave_back_is_a_reset_and_a_refusal_is_the_usage_source_error_with_the_agent_apis_words() {
    // Given the recorded credit reset for codex
    let (h, mut rpc) = started();
    reply(&h, "provider.resetCredit", Ok(recorded("provider.resetCredit", "gives back the credit")));
    // When the phone consumes it
    let reset = rpc.call("provider.consumeResetCredit", json!({"instanceId": "codex"}));
    // Then the agent API was asked for codex and the outcome is a reset
    assert_eq!(ops(&h), [Op::ResetCredit { provider: "codex".into() }]);
    assert_eq!(success(&reset), &json!({"outcome": "reset"}));
    // When the agent API refuses, and when the request names a usage limit source this server does not have
    forget_ops(&h);
    reply(&h, "provider.resetCredit", Err(refused("provider.resetCredit", "a provider that is none")));
    let refused_reset = rpc.call("provider.consumeResetCredit", json!({"instanceId": "gemini"}));
    let source = rpc.call("provider.consumeResetCredit", json!({"sourceId": "s", "accountId": "a", "creditId": "c"}));
    // Then each is a UsageLimitSourceError, and the source request did not reach the agent API
    assert_eq!((failure(&refused_reset)["_tag"].as_str(), failure(&refused_reset)["detail"].as_str().map(|d| d.starts_with("gemini is not a provider"))), (Some("UsageLimitSourceError"), Some(true)));
    assert_eq!(failure(&source)["_tag"], json!("UsageLimitSourceError"));
    assert_eq!(ops(&h).len(), 1);
}

#[test]
fn a_provider_update_and_the_chatgpt_profile_rows_are_refused_with_their_reason_and_never_reach_the_agent_api() {
    // Given a server
    let (h, mut rpc) = started();
    // When the phone asks for a provider update, imports a ChatGPT profile, opens a handoff and asks to reconnect a profile
    let update = rpc.call("server.updateProvider", json!({"provider": "codex"}));
    let import = rpc.call("provider.chatgpt.import-profile", json!({"instanceId": "codex", "profile": {}}));
    let handoff = rpc.call("provider.chatgpt.handoff.subscribe", json!({"instanceId": "codex", "environmentId": ENVIRONMENT_ID, "attemptId": "a", "returnUrl": "https://x.test", "profile": null}));
    let reconnect = rpc.call("provider.chatgpt.reconnect-profile", json!({"instanceId": "codex", "methodId": "chatgpt"}));
    // Then the update is the declared error with the reason, the import and the handoff are setup errors that say no token is taken, and a
    // reconnect has no profile to give
    assert_eq!((failure(&update)["_tag"].as_str(), failure(&update)["provider"].as_str()), (Some("ServerProviderUpdateError"), Some("codex")));
    for refused in [&import, &handoff] {
        assert_eq!((failure(refused)["_tag"].as_str(), failure(refused)["instanceId"].as_str()), (Some("ProviderSetupError"), Some("codex")));
        assert!(failure(refused)["detail"].as_str().unwrap().contains("never holds"));
    }
    assert_eq!(success(&reconnect), &Value::Null);
    // And none of them reached the agent API
    assert!(ops(&h).is_empty());
}

#[test]
fn the_host_s_resources_are_what_the_probe_reads_and_a_probe_that_cannot_read_is_the_declared_host_error() {
    // Given a host of four processors with a quarter busy and three quarters of the memory free
    let (h, mut rpc) = started();
    let sample = h.host.0.lock().unwrap().clone().unwrap();
    // When the phone asks for the host's resources
    let host = rpc.call("server.getHostResources", json!({}));
    // Then they are the reading, stamped with the server's time
    assert_eq!(success(&host), &json!({"sampledAt": START_MS, "cpuUtilization": sample.cpu_utilization, "cpuCount": sample.cpu_count, "availableMemoryBytes": sample.available_memory_bytes, "totalMemoryBytes": sample.total_memory_bytes}));
    // When the probe has no reading of the processors, and then cannot read at all
    *h.host.0.lock().unwrap() = Ok(HostSample { cpu_utilization: None, ..sample });
    assert_eq!(success(&rpc.call("server.getHostResources", json!({})))["cpuUtilization"], Value::Null);
    *h.host.0.lock().unwrap() = Err("/proc/meminfo: no such file".into());
    let broken = rpc.call("server.getHostResources", json!({}));
    // Then the phone gets the host error with the reason
    assert_eq!((failure(&broken)["_tag"].as_str(), failure(&broken)["message"].as_str()), (Some("ExecutionHostError"), Some("/proc/meminfo: no such file")));
}

#[test]
fn a_phone_without_the_settings_scope_is_denied_the_update_and_the_agent_api_is_not_asked() {
    // Given a phone paired with the read scopes only
    let h = Harness::start();
    let read: Vec<String> = t3_contracts::scopes::ALL_SCOPES.iter().filter(|s| s.ends_with(":read")).map(|s| s.to_string()).collect();
    let mut rpc = connect_with(&h, PairingRequest { scopes: read, ..PairingRequest::default() });
    // When it updates the settings and starts a login
    let update = rpc.call("server.updateSettings", json!({"patch": {}}));
    let login = rpc.call("provider.auth.start", json!({"instanceId": "codex"}));
    // Then both are authorization errors and nothing reached the agent API
    assert_eq!(failure(&update)["_tag"], json!("EnvironmentAuthorizationError"));
    assert_eq!(failure(&login)["_tag"], json!("EnvironmentAuthorizationError"));
    assert!(ops(&h).is_empty());
}

#[test]
fn the_database_storage_stream_says_how_big_the_database_is_and_what_a_vacuum_gives_back_and_says_again_when_it_changes() {
    // Given a database of the size the probe reads, with free pages in it
    let (h, _rpc) = started();
    let sample = h.storage.0.lock().unwrap().clone().unwrap();
    let mut watch = connect_with(&h, PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() });
    // When the phone subscribes
    let id = watch.send("server.getDatabaseStorage", json!({}));
    let first = watch.chunk(&id);
    // Then the first status is the reading, with the command that compacts that file
    assert_eq!(first, [json!({"databaseBytes": sample.database_bytes, "reclaimableBytes": sample.reclaimable_bytes, "incrementalVacuumEnabled": sample.incremental_vacuum, "compactCommand": format!("sqlite3 {} VACUUM", sample.path)})]);
    // When the database grows
    *h.storage.0.lock().unwrap() = Ok(StorageSample { database_bytes: sample.database_bytes * 2, ..sample.clone() });
    // Then the stream says so, and only once the size changed
    let deadline = Instant::now() + Duration::from_secs(12);
    let grown = loop {
        assert!(Instant::now() < deadline, "the stream never said the database grew");
        let Some(frame) = watch.try_recv(Duration::from_millis(500)) else { continue };
        if frame["_tag"] == "Chunk" {
            watch.ack(&id);
            break frame["values"][0].clone();
        }
    };
    assert_eq!(grown["databaseBytes"], json!(sample.database_bytes * 2));
    // And a database that cannot be read is a null status
    *h.storage.0.lock().unwrap() = Err("database is locked".into());
    let mut other = connect_with(&h, PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() });
    let again = other.send("server.getDatabaseStorage", json!({}));
    assert_eq!(other.chunk(&again), [Value::Null]);
}
