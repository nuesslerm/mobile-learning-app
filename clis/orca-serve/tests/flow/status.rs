use crate::support::*;
use orca_core::AgentError;

#[test]
fn given_a_probe_when_status_is_asked_then_only_the_runtime_id_comes_back_and_rca_is_not_asked() {
    // Given a paired phone
    let h = harness();
    let mut phone = paired(&h);
    // When it sends the liveness probe the phone's watchdog sends
    let reply = phone.call("status.get", json!({"probe": true}));
    // Then the result is the runtime id alone and no agent call was made for it
    assert_eq!(reply["result"], json!({"runtimeId": RUNTIME_ID}));
    assert!(h.agent.calls().is_empty());
}

#[test]
fn given_a_full_status_request_when_answered_then_it_carries_the_hosts_identity_the_pinned_protocol_and_the_paired_device() {
    // Given a paired phone and the protocol constants of the pinned reference
    let h = harness();
    let mut phone = paired(&h);
    let pin = std::fs::read_to_string(repo_root().join("references/orca/src/shared/protocol-version.ts"));
    // When it asks for the full status
    let result = phone.call("status.get", json!({}))["result"].clone();
    let platform = phone.call("host.platform", json!({}))["result"].clone();
    // Then the host names itself, the device it answers, and the protocol the pin declares
    assert_eq!(result["runtimeId"], RUNTIME_ID);
    assert_eq!(result["pairedDeviceId"], h.device.id.as_str());
    assert_eq!(result["deviceScope"], "mobile");
    assert_eq!(result["machineName"], "test rca");
    assert_eq!(result["hostPlatform"], platform["platform"]);
    assert_eq!(result["graphStatus"], "ready");
    assert!(result["capabilities"].is_array() && result.get("degradations").is_none(), "rca answers, so nothing is degraded: {result}");
    if let Ok(source) = pin {
        let constant = |name: &str| -> u64 { source.lines().find_map(|l| l.strip_prefix(&format!("export const {name} = "))).and_then(|v| v.trim().parse().ok()).unwrap_or_else(|| panic!("{name} is not in the pinned protocol-version.ts")) };
        assert_eq!(result["protocolVersion"], constant("RUNTIME_PROTOCOL_VERSION"));
        assert_eq!(result["minCompatibleMobileVersion"], constant("MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION"));
    }
    assert_eq!(h.agent.calls(), vec!["service.status"], "status asks rca for service.status once");
}

#[test]
fn given_rca_does_not_answer_when_status_is_asked_then_the_host_still_answers_with_a_degradation() {
    // Given rca's agent API failing
    let h = harness();
    h.agent.set_status(Err(AgentError::Failed("connection refused".into())));
    let mut phone = paired(&h);
    // When the phone asks for the full status
    let reply = phone.call("status.get", json!({}));
    // Then the answer is a success that says what is degraded, never a dead host
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["result"]["degradations"][0]["code"], "agent_unavailable");
    assert!(reply["result"]["degradations"][0]["message"].as_str().unwrap().contains("connection refused"));
}

#[test]
fn given_client_capabilities_when_updated_then_they_are_echoed_and_malformed_updates_are_refused() {
    // Given a paired phone
    let h = harness();
    let mut phone = paired(&h);
    // When it declares capabilities, and then sends malformed declarations
    let ok = phone.call("runtime.clientCapabilities.update", json!({"clientCapabilities": ["a.v1", "b.v2"]}));
    let too_many: Vec<String> = (0..65).map(|n| format!("c{n}")).collect();
    let bad = [
        json!({"clientCapabilities": ["a"], "extra": 1}),
        json!({"clientCapabilities": [""]}),
        json!({"clientCapabilities": ["x".repeat(129)]}),
        json!({"clientCapabilities": too_many}),
        json!({"clientCapabilities": "a"}),
        json!({}),
    ];
    let refused: Vec<Value> = bad.into_iter().map(|p| phone.call("runtime.clientCapabilities.update", p)).collect();
    // Then the valid one is echoed and each malformed one is invalid_argument
    assert_eq!(ok["result"], json!({"clientCapabilities": ["a.v1", "b.v2"]}));
    for r in refused {
        assert_eq!((r["ok"].clone(), r["error"]["code"].clone()), (json!(false), json!("invalid_argument")), "{r}");
    }
}

#[test]
fn given_a_client_events_subscription_when_opened_and_ended_then_ready_carries_the_snapshot_and_unsubscribe_ends_only_its_own_stream() {
    // Given rca keeping queued follow-ups off for this device, and two paired phones
    let h = harness();
    h.agent.set_state("orca", "nativeChatQueueFollowUps", json!(false));
    let mut phone = paired(&h);
    let mut other = paired(&h);
    // When the first subscribes
    let id = phone.send_request("runtime.clientEvents.subscribe", json!({}));
    let ready = phone.next_stream_frame(&id);
    let subscription = ready["result"]["subscriptionId"].as_str().unwrap().to_string();
    // Then it is a streaming frame whose snapshot reads rca's state, and the id says which connection it belongs to
    assert_eq!((ready["streaming"].clone(), ready["result"]["type"].clone()), (json!(true), json!("ready")));
    assert_eq!(ready["result"]["snapshot"], json!({"sshStates": [], "nativeChatQueueFollowUps": false}));
    assert!(subscription.starts_with("runtime-client-events-"));
    assert!(h.agent.calls().contains(&"state.get".to_string()));
    // When the other phone tries to end that stream, and the owner ends it
    let foreign = other.call("runtime.clientEvents.unsubscribe", json!({"subscriptionId": subscription}));
    let own_request = phone.send_request("runtime.clientEvents.unsubscribe", json!({"subscriptionId": subscription}));
    let mut frames = [phone.recv_sealed().unwrap(), phone.recv_sealed().unwrap()];
    frames.sort_by_key(|f| f["streaming"] == true);
    // Then the foreign attempt changed nothing, and the owner got `unsubscribed` and the end of the stream
    assert_eq!(foreign["result"], json!({"unsubscribed": false}));
    assert_eq!((frames[0]["id"].clone(), frames[0]["result"].clone()), (json!(own_request), json!({"unsubscribed": true})));
    assert_eq!((frames[1]["id"].clone(), frames[1]["result"].clone()), (json!(id), json!({"type": "end"})));
    // And the subscription id is not reusable
    let again = phone.call("runtime.clientEvents.unsubscribe", json!({"subscriptionId": ""}));
    assert_eq!(again["error"]["code"], "invalid_argument");
}

#[test]
fn given_the_default_state_when_a_stream_opens_then_queued_follow_ups_are_on() {
    // Given rca keeping no setting
    let h = harness();
    let mut phone = paired(&h);
    // When a client-events stream opens
    let id = phone.send_request("runtime.clientEvents.subscribe", json!({}));
    // Then the snapshot has the pin's default
    assert_eq!(phone.next_stream_frame(&id)["result"]["snapshot"]["nativeChatQueueFollowUps"], true);
}

use serde_json::Value;

#[test]
fn given_the_methods_a_mapper_uses_when_the_startup_groups_are_derived_then_each_group_but_the_unlisted_workspace_is_required() {
    // Given the methods the workspace and agent mappers declare
    let wanted: Vec<&str> = orca_map_workspace::USES.iter().chain(orca_map_agent::USES).chain(orca_map_terminal::USES).chain(orca_map_git::USES).chain(orca_map_accounts::USES).chain(orca_map_state::USES).chain(orca_map_hosted::USES).chain(orca_map_schedules::USES).map(|m| m.group().as_str()).filter(|g| *g != "workspace").collect();
    // When the startup check's groups are derived
    let needed = orca_serve::adapters::rca::needed_groups();
    // Then every one of them is required, and the unlisted group is not
    assert!(wanted.iter().all(|g| needed.contains(g)), "{wanted:?} not in {needed:?}");
    assert!(wanted.contains(&"git") && wanted.contains(&"workspace.worktrees") && wanted.contains(&"agent.threads") && wanted.contains(&"agent.queue") && wanted.contains(&"agent.requests") && wanted.contains(&"attachments") && wanted.contains(&"accounts") && wanted.contains(&"git.ai") && wanted.contains(&"state"), "the mappers use git, worktree, agent and attachment methods: {wanted:?}");
    assert!(!needed.contains(&"workspace"));
}
