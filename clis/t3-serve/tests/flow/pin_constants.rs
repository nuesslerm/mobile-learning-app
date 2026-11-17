//! The RPCs the pinned server answers with a constant or an empty state, and the failures it gives a thread or an instance it does not hold.
use super::support::*;
use std::time::Duration;

fn operator(h: &Harness) -> Rpc {
    connect_with(h, PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() })
}

#[test]
fn the_reads_of_a_host_that_keeps_nothing_answer_the_pins_empty_state() {
    // Given a session allowed everything
    let h = Harness::start();
    let mut rpc = operator(&h);

    // When it scans for agent sessions, asks for the relay client and the webhook deliveries of a task, and lists a worktree's threads
    let scan = rpc.call("agentSessions.scan", json!({}));
    let relay = rpc.call("cloud.getRelayClientStatus", json!({}));
    let deliveries = rpc.call("scheduledTasks.listWebhookDeliveries", json!({"id": "s1"}));
    let delivery = rpc.call("scheduledTasks.getWebhookDelivery", json!({"id": "s1", "deliveryId": "d1"}));
    let worktree = rpc.call("orchestration.getWorktreeThreadIds", json!({"worktreePath": "/base/wt"}));

    // Then no candidate is found, the relay client is missing, a task has no deliveries and a delivery is not found, and no thread is in the worktree
    assert_eq!((success(&scan)["candidates"].clone(), success(&scan)["scannedAt"].is_string()), (json!([]), true));
    assert_eq!(success(&relay), &json!({"status": "missing", "version": "2026.10.4"}));
    assert_eq!(success(&deliveries), &json!({"deliveries": []}));
    assert_eq!((failure(&delivery)["_tag"].as_str(), failure(&delivery)["message"].as_str(), failure(&delivery)["taskId"].as_str()), (Some("ScheduledTaskError"), Some("Webhook delivery not found."), Some("s1")));
    assert_eq!(success(&worktree), &json!({"threadIds": []}));
}

#[test]
fn the_calls_that_have_nothing_to_do_succeed_with_null_and_a_process_that_is_not_listed_is_not_signalled() {
    // Given a session allowed everything
    let h = Harness::start();
    let mut rpc = operator(&h);

    // When it closes a device, clears and reports preview profiles, deletes an attachment, closes a preview and invalidates pull requests
    let void = [
        rpc.call("device.close", json!({"threadId": "t1"})),
        rpc.call("preview.clearProfile", json!({"profileId": "p"})),
        rpc.call("preview.reportProfiles", json!({"profiles": [], "defaultProfileId": "p"})),
        rpc.call("attachments.delete", json!({"attachmentId": "a"})),
        rpc.call("preview.close", json!({"threadId": "t1"})),
        rpc.call("pullRequests.invalidate", json!({})),
    ];
    let signal = rpc.call("server.signalProcess", json!({"pid": 1, "startTimeMs": 0, "signal": "SIGINT"}));

    // Then each answers a success whose value is null, and the process no longer matches the identity asked for
    for exit in &void {
        assert_eq!(success(exit), &Value::Null, "{exit}");
    }
    assert_eq!(success(&signal), &json!({"pid": 1, "signal": "SIGINT", "signaled": false, "message": {"_tag": "Some", "value": "Process 1 no longer matches the selected process identity."}}));
}

#[test]
fn a_storage_clean_up_finds_nothing_and_its_report_stream_opens_with_that_report() {
    // Given a session allowed everything
    let h = Harness::start();
    let mut rpc = operator(&h);

    // When it runs a clean-up and opens the report stream
    let run = rpc.call("server.runStorageCleanup", json!({}));
    let id = rpc.send("server.getStorageCleanupReport", json!({}));
    let first = rpc.chunk(&id);

    // Then both report nothing removed, kept or failed, a manual run and an automatic report
    let zero = |report: &Value, trigger: &str| (report["trigger"].clone(), report["counts"].clone(), report["bytesFreed"].clone(), report["entries"].clone(), report["omittedCount"].clone()) == (json!(trigger), json!({"removed": 0, "kept": 0, "failed": 0}), json!(0), json!([]), json!(0));
    assert!(zero(success(&run), "manual"), "{run}");
    assert!(zero(&first[0], "automatic"), "{first:?}");
}

#[test]
fn a_thread_the_server_holds_nothing_of_fails_each_read_with_the_error_that_rpc_declares() {
    // Given a session with no thread
    let h = Harness::start();
    let mut rpc = operator(&h);
    let cause = json!({"cause": {"message": "No orchestration projection exists for thread t9.", "name": "ProjectionStoreThreadNotFoundError"}, "message": "Failed to load orchestration projection for thread t9.", "name": "OrchestratorProjectionError"});

    // When each RPC that names the thread is called
    let projection = rpc.call("orchestration.getThreadProjection", json!({"threadId": "t9"}));
    let subscribed = rpc.call("orchestration.subscribeThread", json!({"threadId": "t9"}));
    let stop = rpc.call("orchestration.forceStopThread", json!({"threadId": "t9"}));
    let workflow = rpc.call("orchestration.stopWorkflow", json!({"threadId": "t9", "subagentId": "w"}));
    let peer = rpc.call("peerLinks.answerRequest", json!({"threadId": "t9", "turnItemId": "i", "answer": {"type": "decline"}}));
    let secret = rpc.call("secrets.answerRequest", json!({"threadId": "t9", "turnItemId": "i", "answer": {"type": "decline"}}));
    let output = rpc.call("orchestration.subscribeCommandOutput", json!({"threadId": "t9", "itemId": "i"}));

    // Then each is that RPC's own error, with the missing projection as its cause
    let projection_error = json!({"_tag": "OrchestrationV2GetThreadProjectionError", "cause": cause, "message": "Failed to load orchestration V2 thread t9", "threadId": "t9"});
    assert_eq!(failure(&projection), &projection_error);
    assert_eq!(failure(&subscribed), &projection_error);
    assert_eq!(failure(&stop), &json!({"_tag": "OrchestrationV2ForceStopThreadError", "cause": cause, "threadId": "t9"}));
    assert_eq!(failure(&workflow), &json!({"_tag": "OrchestrationV2StopWorkflowError", "cause": cause, "reason": "unavailable", "subagentId": "w", "threadId": "t9"}));
    assert_eq!(failure(&peer), &json!({"_tag": "PeerLinkRequestError", "cause": cause, "reason": "load_failed"}));
    assert_eq!(failure(&secret), &json!({"_tag": "SecretRequestError", "cause": cause, "reason": "load_failed"}));
    assert_eq!(failure(&output), &json!({"_tag": "OrchestrationV2CommandOutputError", "message": "No such command.", "threadId": "t9"}));
}

#[test]
fn a_turn_item_a_whole_diff_and_a_worktree_script_of_a_thread_that_never_was_are_empty_or_invalid_not_a_missing_thread() {
    // Given a session with no thread
    let h = Harness::start();
    let mut rpc = operator(&h);

    // When it asks for a turn item, the whole diff and a script by a relative path
    let item = rpc.call("orchestration.getTurnItem", json!({"threadId": "t9", "itemId": "i"}));
    let diff = rpc.call("orchestration.getFullThreadDiff", json!({"threadId": "t9", "toTurnCount": 0}));
    let script = rpc.call("orchestration.getWorkflowScript", json!({"threadId": "t9", "scriptPath": "a"}));

    // Then the item is null, the diff is empty and the path is refused as the pin words it
    assert_eq!(success(&item), &json!({"item": null}));
    assert_eq!(success(&diff), &json!({"threadId": "t9", "fromTurnCount": 0, "toTurnCount": 0, "diff": ""}));
    assert_eq!(failure(&script), &json!({"_tag": "OrchestrationGetWorkflowScriptError", "cause": {"message": "Workflow files must be absolute paths with the expected extension.", "name": "OrchestrationWorkflowFileError"}, "reason": "invalid-path", "scriptPath": "a"}));
}

#[test]
fn a_subscription_that_resumes_after_a_sequence_needs_no_thread_and_a_refresh_stream_says_nothing_until_something_changes() {
    // Given a session with no thread
    let h = Harness::start();
    let mut rpc = operator(&h);

    // When it resumes a thread's stream after sequence 0 asking for the completion marker, and opens the pull request refresh stream
    let id = rpc.send("orchestration.subscribeThread", json!({"threadId": "t9", "afterSequence": 0, "requestCompletionMarker": true}));
    let marker = rpc.chunk(&id);
    let refreshes = rpc.send("pullRequests.subscribeRefreshes", json!({}));

    // Then the first stream holds only the marker, and the second sends no value
    assert_eq!(marker, [json!({"kind": "synchronized"})]);
    assert!(rpc.try_recv(Duration::from_millis(300)).is_none(), "{refreshes}");
}

#[test]
fn a_mutation_of_a_project_the_server_holds_none_of_names_it_in_the_cause_and_a_delete_of_a_full_one_names_the_threads_in_it() {
    // Given a project with a thread
    let h = Harness::start();
    let mut rpc = connect(&h);
    rpc.call("projects.mutate", project_create("c-p", "p1"));
    rpc.call("orchestration.dispatchCommand", thread_create("c-t", "t1", "p1"));

    // When a project that is not there is deleted, and the full one is
    let missing = rpc.call("projects.mutate", json!({"type": "project.delete", "commandId": "c-d1", "projectId": "nope"}));
    let full = rpc.call("projects.mutate", json!({"type": "project.delete", "commandId": "c-d2", "projectId": "p1"}));

    // Then each is a ProjectMutationError with its cause
    assert_eq!(failure(&missing), &json!({"_tag": "ProjectMutationError", "commandId": "c-d1", "message": "Failed to mutate project.", "cause": {"message": "Project nope was not found.", "name": "ProjectNotFoundError"}}));
    assert_eq!(failure(&full), &json!({"_tag": "ProjectMutationError", "commandId": "c-d2", "message": "Project p1 is not empty.", "cause": {"message": "Project p1 is not empty.", "name": "ProjectNotEmptyError"}}));
}

#[test]
fn an_instance_that_is_not_configured_is_no_longer_available_to_every_provider_call_and_deleting_a_missing_schedule_is_done() {
    // Given a session allowed everything
    let h = Harness::start();
    let mut rpc = operator(&h);

    // When each provider auth call names an instance that is not configured, and a schedule that is not there is deleted
    let calls = [
        ("start", rpc.call("provider.auth.start", json!({"instanceId": "a"}))),
        ("cancel", rpc.call("provider.auth.cancel", json!({"instanceId": "a", "flowId": "f"}))),
        ("complete", rpc.call("provider.auth.complete", json!({"instanceId": "a", "flowId": "f", "callbackUrl": "u"}))),
        ("logout", rpc.call("provider.auth.logout", json!({"instanceId": "a"}))),
        ("subscribe", rpc.call("provider.auth.subscribe", json!({"instanceId": "a"}))),
    ];
    let deleted = rpc.call("scheduledTasks.delete", json!({"id": "s9"}));

    // Then each is a ProviderSetupError of its operation saying so, and the delete answers its id
    for (operation, exit) in &calls {
        assert_eq!(failure(exit), &json!({"_tag": "ProviderSetupError", "detail": "This provider instance is no longer available.", "instanceId": "a", "operation": operation}));
    }
    assert_eq!(success(&deleted), &json!({"id": "s9"}));
}
