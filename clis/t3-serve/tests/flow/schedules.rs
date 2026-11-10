//! The `scheduledTasks.*` rows through the real router over the agent double: what a phone's save becomes (the `Op::Schedule` the agent API is
//! asked: snake_case schedule, the model's instance as the driver, the project's directory as the workspace, the phone's own fields in `meta`),
//! what the phone is told of the task, what rca does not offer (refused by name, nothing sent), and the stream. The chain
//! (`tests/integration/schedules_m12.rs`) runs the same calls through the real service and executor.
use super::support::*;
use t3_core::ports::{AgentError, Op};

const PROJECT_ROOT: &str = "/workspace/demo";

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let mut rpc = connect_with(&h, all);
    seed(&mut rpc);
    (h, rpc)
}

fn asked(h: &Harness) -> Vec<(String, Option<String>, Value)> {
    h.agent.ops.lock().unwrap().iter().filter_map(|op| if let Op::Schedule { method, root, params } = op { Some((method.to_string(), root.clone(), params.clone())) } else { None }).collect()
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

/// A schedule as the agent API keeps it after the phone's save below.
fn kept(extra: Value) -> Value {
    let mut task = json!({
        "id": "s1", "title": "Nightly", "prompt": "run the checks", "enabled": true, "schedule": {"type": "interval", "every_ms": 120_000}, "workspace": "w1", "driver": "claudeAgent", "model": "opus",
        "thread": null, "runtime_mode": "full-access", "interaction_mode": "default", "created_at_ms": 1_700_000_000_000_i64, "updated_at_ms": 1_700_000_000_000_i64, "next_run_ms": 1_700_000_120_000_i64,
        "last_run_ms": null, "last_run_status": "never", "last_run_error": null, "run_count": 0, "webhook": null,
        "meta": {"projectId": "p1", "workspaceStrategy": {"type": "root"}, "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}, "createdBy": "user", "creationSource": "mobile"},
    });
    task.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    task
}

fn save() -> Value {
    json!({"title": "Nightly", "prompt": "run the checks", "enabled": true, "schedule": {"type": "interval", "everyMs": 120_000}, "projectId": "p1", "workspaceStrategy": {"type": "root"},
        "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}, "runtimeMode": "full-access", "interactionMode": "default"})
}

fn refused(code: &str, message: &str) -> Result<Value, AgentError> {
    Err(AgentError::Refused { code: code.into(), message: message.into() })
}

fn webhook_save() -> Value {
    let mut payload = save();
    payload["schedule"] = json!({"type": "webhook", "signature": null});
    payload
}

#[test]
fn a_task_saved_by_the_phone_is_a_schedule_in_the_projects_workspace_with_the_phones_fields_kept_and_comes_back_as_the_phone_saved_it() {
    // Given a project and an agent API that keeps what it is sent
    let (h, mut rpc) = started();
    reply(&h, "schedule.upsert", Ok(json!({"task": kept(json!({}))})));
    // When the phone saves a task that runs every two minutes
    let exit = rpc.call("scheduledTasks.upsert", save());
    // Then the agent API was asked to upsert in the project's directory, with the schedule in its own words, the model's instance as the driver and the phone's fields as meta
    let ops = asked(&h);
    assert_eq!(ops.len(), 1);
    let (method, root, params) = &ops[0];
    assert_eq!((method.as_str(), root.as_deref()), ("schedule.upsert", Some(PROJECT_ROOT)));
    assert_eq!((params["schedule"].clone(), params["driver"].clone(), params["model"].clone(), params["enabled"].clone(), params["runtime_mode"].clone()), (json!({"type": "interval", "every_ms": 120_000}), json!("claudeAgent"), json!("opus"), json!(true), json!("full-access")));
    assert_eq!(params["meta"], kept(json!({}))["meta"]);
    assert!(params.get("id").is_none() && params.get("thread").is_none());
    // And the phone is told the task in its words: ids, ISO times, the schedule as it saved it
    let task = &success(&exit)["task"];
    assert_eq!((task["id"].clone(), task["projectId"].clone(), task["schedule"].clone(), task["nextRunAt"].clone(), task["lastRunStatus"].clone(), task["modelSelection"]["model"].clone()), (json!("s1"), json!("p1"), json!({"type": "interval", "everyMs": 120_000}), json!("2023-11-14T22:15:20.000Z"), json!("never"), json!("opus")));
    assert!(task.get("webhook").is_none());
    // And an edit names its id, may name a thread and may require the task to exist
    let mut edit = save();
    edit["id"] = json!("s1");
    edit["requireExisting"] = json!(true);
    edit["threadId"] = json!("t1");
    rpc.call("scheduledTasks.upsert", edit);
    let (_, _, edited) = asked(&h).remove(1);
    assert_eq!((edited["id"].clone(), edited["require_existing"].clone(), edited["thread"].clone()), (json!("s1"), json!(true), json!("t1")));
}

#[test]
fn what_rca_does_not_offer_is_refused_by_name_and_nothing_is_sent_and_the_agent_apis_refusals_reach_the_phone_with_the_task() {
    let (h, mut rpc) = started();
    // Given a webhook with a signature check, a webhook with filters, a worktree for each run, a project that does not exist
    let mut signed = webhook_save();
    signed["schedule"]["signature"] = json!({"header": "x-hub-signature-256", "encoding": "hex", "prefix": "sha256="});
    let mut filtered = webhook_save();
    filtered["schedule"]["filters"] = json!([{"path": "body.action", "op": "exists"}]);
    let mut worktree = save();
    worktree["workspaceStrategy"] = json!({"type": "worktree", "baseRef": "main"});
    let mut lost = save();
    lost["projectId"] = json!("p-none");
    // When each is saved
    let answers: Vec<Value> = [signed, filtered, worktree, lost].into_iter().map(|payload| rpc.call("scheduledTasks.upsert", payload)).collect();
    // Then each is a ScheduledTaskError that says what is not offered, and the agent API was asked nothing
    let messages: Vec<String> = answers.iter().map(|a| failure(a)["message"].as_str().unwrap().to_string()).collect();
    assert!(answers.iter().all(|a| failure(a)["_tag"] == "ScheduledTaskError"));
    assert!(messages[0].contains("signature is not offered") && messages[1].contains("filters is not offered") && messages[2].contains("worktree or a branch") && messages[3].contains("project p-none was not found"), "{messages:?}");
    assert!(asked(&h).is_empty());
    // And a refusal of the agent API carries its message and the task it was about
    reply(&h, "schedule.setEnabled", refused("SCHEDULE_NOT_FOUND", "no schedule has the id s9"));
    let exit = rpc.call("scheduledTasks.setEnabled", json!({"id": "s9", "enabled": false}));
    assert_eq!((failure(&exit)["_tag"].clone(), failure(&exit)["message"].clone(), failure(&exit)["taskId"].clone()), (json!("ScheduledTaskError"), json!("no schedule has the id s9"), json!("s9")));
    // And a request that leaves out a required key is a defect, as the decoder says
    let bare = rpc.call("scheduledTasks.upsert", json!({"title": "x"}));
    assert!(bare.to_string().contains("Missing key"), "{bare}");
}

#[test]
fn enabling_deleting_running_and_rotating_are_one_request_each_and_the_rotated_webhook_has_an_address_with_the_new_token() {
    let (h, mut rpc) = started();
    // Given a webhook task the agent API keeps
    let hook = kept(json!({"schedule": {"type": "webhook"}, "next_run_ms": null, "webhook": {"path": "/hooks/v1/schedules/s1", "has_token": true}}));
    reply(&h, "schedule.setEnabled", Ok(json!({"task": kept(json!({"enabled": false, "next_run_ms": null}))})));
    reply(&h, "schedule.delete", Ok(json!({"id": "s1"})));
    reply(&h, "schedule.runNow", Ok(json!({"task": kept(json!({"last_run_status": "running", "run_count": 1, "last_run_ms": 1_700_000_001_000_i64})), "run": {"run": "r1", "schedule": "s1", "source": "manual", "at_ms": 1_700_000_001_000_i64, "state": "started", "thread": "th1", "turn": "tu1", "error": null}})));
    reply(&h, "schedule.rotateWebhookToken", Ok(json!({"task": hook, "webhook_token": "tok-new"})));
    // When the phone disables, runs, rotates and deletes it
    let off = rpc.call("scheduledTasks.setEnabled", json!({"id": "s1", "enabled": false}));
    let ran = rpc.call("scheduledTasks.runNow", json!({"id": "s1"}));
    let rotated = rpc.call("scheduledTasks.rotateWebhookToken", json!({"id": "s1"}));
    let deleted = rpc.call("scheduledTasks.delete", json!({"id": "s1"}));
    // Then each was one request of its method naming the task (no workspace: the task is already placed), and the phone got the task or its id
    assert_eq!(asked(&h).iter().map(|(m, root, p)| (m.as_str(), root.clone(), p["id"].clone())).collect::<Vec<_>>(), [("schedule.setEnabled", None, json!("s1")), ("schedule.runNow", None, json!("s1")), ("schedule.rotateWebhookToken", None, json!("s1")), ("schedule.delete", None, json!("s1"))]);
    assert_eq!((success(&off)["task"]["enabled"].clone(), success(&off)["task"]["nextRunAt"].clone()), (json!(false), Value::Null));
    assert_eq!((success(&ran)["task"]["lastRunStatus"].clone(), success(&ran)["task"]["runCount"].clone()), (json!("running"), json!(1)));
    assert_eq!(success(&deleted), &json!({"id": "s1"}));
    // And the rotated task's webhook address holds the new token, for the phone to show once
    assert_eq!(success(&rotated)["task"]["webhook"], json!({"path": "/hooks/v1/schedules/s1", "url": "/hooks/v1/schedules/s1/tok-new", "hasSecret": false}));
}

#[test]
fn the_stream_lists_the_tasks_at_once_and_again_when_a_run_or_a_change_makes_them_differ_and_not_when_nothing_did() {
    // Given an agent API whose list changes after its first answer (a run began), and a stream that reports a change, an event that is none, and a run
    let (h, mut rpc) = started();
    let lists = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = lists.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::Schedule { method: "schedule.list", .. } => {
            let nth = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let task = if nth < 2 { kept(json!({})) } else { kept(json!({"last_run_status": "running", "run_count": 1})) };
            Some(Ok(json!({"tasks": [task]})))
        }
        _ => None,
    }));
    *h.agent.schedule_events.lock().unwrap() = vec![json!({"seq": 1, "type": "schedule.webhook", "schedule": "s1", "outcome": "rejected_token"}), json!({"seq": 2, "type": "schedule.run", "schedule": "s1"})];
    // When the phone subscribes
    let stream = rpc.send("scheduledTasks.subscribe", json!({}));
    let first = rpc.chunk(&stream);
    // Then the first chunk is the list as the agent API had it
    assert_eq!((first[0]["tasks"][0]["id"].clone(), first[0]["tasks"][0]["lastRunStatus"].clone()), (json!("s1"), json!("never")));
    // And the next is the list after the run, sent once: the webhook request changed no task, so it made no chunk
    let next = rpc.chunk(&stream);
    assert_eq!((next[0]["tasks"][0]["lastRunStatus"].clone(), next[0]["tasks"][0]["runCount"].clone()), (json!("running"), json!(1)));
    assert!(rpc.try_recv(std::time::Duration::from_millis(300)).is_none());
    assert_eq!(asked(&h).iter().filter(|(m, ..)| m == "schedule.list").count(), 3);
}
