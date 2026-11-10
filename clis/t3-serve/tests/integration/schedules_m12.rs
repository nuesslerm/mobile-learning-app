//! The `scheduledTasks.*` rows through the chain: a task saved by the phone is a schedule of the real rca-service, run by hand it starts the scripted
//! CLI through the agent group (which writes a file through the real rca-exec), and a webhook task is started by an HTTP request outside the phone's
//! wire with the token the phone was shown: a wrong or rotated-out token is 401 and starts no CLI. A call counts for its row only when it was
//! answered and the agent API method ran in its window; the CLI's recorded starts and the file in the repository are what the turn really did.
use crate::chain::{last_window, writes};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::Script;
use std::time::{Duration, Instant};

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

fn save(title: &str, schedule: Value) -> Value {
    json!({"title": title, "prompt": "write the note on schedule", "enabled": true, "schedule": schedule, "projectId": "p1", "workspaceStrategy": {"type": "root"},
        "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}, "runtimeMode": "full-access", "interactionMode": "default"})
}

/// The method answered in the last call's window.
fn asked(case: &exec_testkit::Case, method: &str) {
    let (agent, _) = last_window(case);
    assert!(agent.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {agent:?}");
}

/// The id of the thread rca made for the schedule titled `title`, once it has one.
fn thread_of(direct: &mut Direct, title: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let listed = direct.call("agent.threads", "thread.list", json!({})).expect("rca lists its threads");
        if let Some(thread) = listed["threads"].as_array().into_iter().flatten().find(|t| t["title"] == title) {
            return thread["id"].as_str().unwrap().to_string();
        }
        assert!(Instant::now() < deadline, "rca never made a thread titled {title}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn prompts(chain: &Chain) -> Vec<String> {
    chain.cli.invocations("claude").iter().filter_map(|start| start.argv.iter().find(|a| a.contains("on schedule")).cloned()).collect()
}

#[test]
fn a_scheduled_task_is_saved_followed_run_by_hand_and_deleted_and_the_run_is_a_real_turn_that_writes_a_file_through_the_executor() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a CLI whose turn writes a note, a paired phone with a project, and a phone that follows the tasks
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("notes/scheduled.txt", "from the schedule")]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let (stream, first) = phone.stream(case, &chain, &["rpc:scheduledTasks.subscribe"], "scheduledTasks.subscribe", json!({}));
        assert_eq!(first[0]["tasks"], json!([]), "{first:?}");
        asked(case, "schedule.list");
        // When the phone saves a task that runs every two minutes
        let saved = phone.call(case, &chain, &["rpc:scheduledTasks.upsert"], "scheduledTasks.upsert", save("Scheduled note", json!({"type": "interval", "everyMs": 120_000})));
        asked(case, "schedule.upsert");
        let task = success(&saved)["task"].clone();
        let id = task["id"].as_str().expect("rca named the task").to_string();
        // Then the task is the phone's: its project, schedule and model as saved, due two minutes on, never run
        assert_eq!((task["projectId"].clone(), task["schedule"].clone(), task["modelSelection"]["model"].clone(), task["lastRunStatus"].clone(), task["runCount"].clone()), (json!("p1"), json!({"type": "interval", "everyMs": 120_000}), json!("opus"), json!("never"), json!(0)));
        assert!(task["nextRunAt"].is_string() && task["lastRunAt"].is_null(), "{task}");
        // And the stream the phone follows lists it
        let listed = phone.watch.chunk(&stream);
        assert_eq!(listed[0]["tasks"][0]["id"], json!(id), "{listed:?}");
        // When the phone runs it now
        let ran = phone.call(case, &chain, &["rpc:scheduledTasks.runNow"], "scheduledTasks.runNow", json!({"id": id}));
        asked(case, "schedule.runNow");
        assert_eq!(success(&ran)["task"]["runCount"], json!(1));
        // Then rca made a thread for it, ran the CLI there with the schedule's prompt, and the executor wrote the file the turn asked for
        let mut direct = chain.direct();
        let thread = thread_of(&mut direct, "Scheduled note");
        direct.wait_event(&thread, "turn.finished");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/scheduled.txt")).unwrap(), "from the schedule");
        assert_eq!(prompts(&chain), ["write the note on schedule"]);
        // And the stream tells the phone the run began and then how it ended
        let seen = Instant::now() + Duration::from_secs(20);
        let mut statuses = Vec::new();
        while !statuses.contains(&"succeeded".to_string()) {
            assert!(Instant::now() < seen, "the stream never said the run succeeded: {statuses:?}");
            for chunk in phone.watch.chunk(&stream) {
                statuses.push(chunk["tasks"][0]["lastRunStatus"].as_str().unwrap_or_default().to_string());
            }
        }
        // When the phone disables it, Then the task says so and has no next run
        let off = phone.call(case, &chain, &["rpc:scheduledTasks.setEnabled"], "scheduledTasks.setEnabled", json!({"id": id, "enabled": false}));
        asked(case, "schedule.setEnabled");
        assert_eq!((success(&off)["task"]["enabled"].clone(), success(&off)["task"]["nextRunAt"].clone()), (json!(false), Value::Null));
        // When it is deleted, Then the id comes back, the stream lists none, and a second delete is the declared error
        let gone = phone.call(case, &chain, &["rpc:scheduledTasks.delete"], "scheduledTasks.delete", json!({"id": id}));
        asked(case, "schedule.delete");
        assert_eq!(success(&gone), &json!({"id": id}));
        let emptied = std::iter::repeat_with(|| phone.watch.chunk(&stream)).take(3).flatten().any(|c| c["tasks"] == json!([]));
        assert!(emptied, "the stream never listed no task");
        let again = phone.calls.call("scheduledTasks.delete", json!({"id": id}));
        assert_eq!((failure(&again)["_tag"].clone(), failure(&again)["taskId"].clone()), (json!("ScheduledTaskError"), json!(id)));
    });
}

#[test]
fn a_webhook_task_runs_when_its_token_is_posted_and_a_wrong_or_rotated_out_token_is_401_and_starts_no_cli() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a CLI whose turn writes a note, a paired phone with a project and a webhook task whose prompt names the body
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("notes/hook.txt", "from the webhook")]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let mut payload = save("Hook note", json!({"type": "webhook", "signature": null}));
        payload["prompt"] = json!("review {{body.pr}} on schedule");
        let saved = phone.call(case, &chain, &["rpc:scheduledTasks.upsert"], "scheduledTasks.upsert", payload);
        let id = success(&saved)["task"]["id"].as_str().unwrap().to_string();
        assert_eq!(success(&saved)["task"]["webhook"]["path"], json!(format!("/hooks/v1/schedules/{id}")));
        // When the phone rotates the token twice, the way it shows a new address
        let rotate = |phone: &mut Phone, case: &mut exec_testkit::Case| {
            let rotated = phone.call(case, &chain, &["rpc:scheduledTasks.rotateWebhookToken"], "scheduledTasks.rotateWebhookToken", json!({"id": id}));
            asked(case, "schedule.rotateWebhookToken");
            success(&rotated)["task"]["webhook"]["url"].as_str().expect("a rotation shows the address with its token").to_string()
        };
        let (old, current) = (rotate(&mut phone, case), rotate(&mut phone, case));
        assert_ne!(old, current);
        // Then the address is the path and the token, and the service's webhook route, with no bearer, lets only the current token in
        assert!(current.starts_with(&format!("/hooks/v1/schedules/{id}/")), "{current}");
        let post = |url: &str, body: &str| http(chain.service.addr(), "POST", url, &[("Content-Type", "application/json")], body);
        let wrong = post(&format!("/hooks/v1/schedules/{id}/not-the-token"), r#"{"pr": 7}"#);
        let rotated_out = post(&old, r#"{"pr": 7}"#);
        assert_eq!((wrong.status, rotated_out.status), (401, 401), "{:?} {:?}", wrong.body, rotated_out.body);
        assert!(prompts(&chain).is_empty(), "a refused request started the CLI");
        let accepted = post(&current, r#"{"pr": 42}"#);
        assert_eq!(accepted.status, 202, "{:?}", accepted.body);
        // And the accepted request ran the CLI with the body's field in the prompt, and its turn wrote its file
        let mut direct = chain.direct();
        let thread = thread_of(&mut direct, "Hook note");
        direct.wait_event(&thread, "turn.finished");
        assert_eq!(prompts(&chain), ["review 42 on schedule"]);
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/hook.txt")).unwrap(), "from the webhook");
    });
}
