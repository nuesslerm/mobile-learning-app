//! The phone's automation rows through the chain: an automation saved by the phone is a schedule of the real rca-service (read back through a side door),
//! run by hand it starts the scripted CLI through the agent group and the real rca-exec writes the file the turn asked for, and what rca cannot run is
//! refused before the service is asked to save anything. A row counts only when its agent methods were answered in its call's window.
use crate::rig::*;
use exec_testkit::script::{Script, Turn};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const SELECTOR: &str = "id:w1::";

fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

fn create_params() -> Value {
    json!({"name": "Weekday note", "prompt": "write the note on schedule", "agentId": "claude", "repo": SELECTOR, "workspace": SELECTOR, "workspaceMode": "existing", "setupDecision": "inherit", "reuseSession": false,
        "precheck": null, "timezone": "UTC", "rrule": "0 9 * * 1-5", "dtstart": 1_790_942_400_000u64})
}

fn schedules(direct: &mut Direct) -> Vec<Value> {
    direct.call("schedules", "schedule.list", json!({})).expect("rca lists its schedules")["tasks"].as_array().cloned().unwrap_or_default()
}

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

#[test]
fn an_automation_is_saved_listed_updated_run_by_hand_and_deleted_and_the_run_is_a_real_turn_that_writes_a_file_through_the_executor() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a CLI whose turn writes a note, a paired phone and a side door to rca
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("writing the note")]).then_calling("k1", "write_file", json!({"path": "notes/automation.txt", "content": "from the automation", "create_dirs": true})).then_line(&said("done"))]));
        let mut phone = Phone::connect(case, &chain);
        let mut direct = chain.direct();
        // When the phone creates the weekday automation
        let made = phone.call(case, &chain, &["rpc:automation.create"], "automation.create", create_params());
        answered(case, "schedule.upsert", true);
        let automation = made["result"]["automation"].clone();
        let id = automation["id"].as_str().expect("rca named the schedule").to_string();
        // Then rca holds a schedule at 09:00 on Monday to Friday in the executor's workspace, run by the claude driver with the phone's prompt
        let kept = schedules(&mut direct);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!((kept[0]["id"].clone(), kept[0]["title"].clone(), kept[0]["prompt"].clone(), kept[0]["workspace"].clone(), kept[0]["driver"].clone(), kept[0]["enabled"].clone()), (json!(id), json!("Weekday note"), json!("write the note on schedule"), json!("w1"), json!("claudeAgent"), json!(true)));
        assert_eq!(kept[0]["schedule"], json!({"type": "fixed_time", "time_of_day": "09:00", "weekdays": [1, 2, 3, 4, 5]}));
        // And the phone's record carries its own recurrence, the agent and rca's next run
        assert_eq!((automation["rrule"].clone(), automation["timezone"].clone(), automation["agentId"].clone(), automation["workspaceId"].clone(), automation["nextRunAt"].clone()), (json!("0 9 * * 1-5"), json!("UTC"), json!("claude"), json!(SELECTOR), kept[0]["next_run_ms"].clone()));
        // When the phone lists the automations and shows this one, Then both read the schedule rca keeps
        let listed = phone.call(case, &chain, &["rpc:automation.list"], "automation.list", json!({}));
        answered(case, "schedule.list", true);
        assert_eq!((listed["result"]["automations"][0]["id"].clone(), listed["result"]["items"][0]["selector"].clone()), (json!(id), json!({"kind": "self"})));
        let shown = phone.call(case, &chain, &["rpc:automation.show"], "automation.show", json!({"id": id}));
        assert_eq!(shown["result"]["automation"], automation);
        // When the phone asks for what rca cannot run (a worktree per run, an hourly recurrence)
        for (patch, reason) in [(json!({"workspaceMode": "new_per_run"}), "never in a worktree"), (json!({"rrule": "FREQ=HOURLY"}), "hourly")] {
            let mut params = create_params();
            params.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            let refused = phone.refused(case, &chain, &["rpc:automation.create"], "automation.create", params);
            // Then it is refused with the reason, and rca was not asked to save anything
            assert_eq!(refused["error"]["code"], json!("invalid_argument"), "{refused}");
            assert!(refused["error"]["message"].as_str().unwrap().contains(reason), "{refused}");
            assert!(!last_window(case).iter().any(|(m, _)| m == "schedule.upsert"), "{:?}", last_window(case));
        }
        assert_eq!(schedules(&mut direct).len(), 1);
        // When the phone renames it and moves it to a daily 18:30 at UTC-5
        let updated = phone.call(case, &chain, &["rpc:automation.update"], "automation.update", json!({"id": id, "updates": {"name": "Evening note", "rrule": "FREQ=DAILY;BYHOUR=18;BYMINUTE=30", "timezone": "Etc/GMT+5"}}));
        answered(case, "schedule.upsert", true);
        // Then rca holds the changed schedule, and the phone reads the new text
        let kept = schedules(&mut direct);
        assert_eq!((kept[0]["title"].clone(), kept[0]["schedule"].clone()), (json!("Evening note"), json!({"type": "fixed_time", "time_of_day": "18:30", "utc_offset_minutes": -300})));
        assert_eq!((updated["result"]["automation"]["name"].clone(), updated["result"]["automation"]["rrule"].clone()), (json!("Evening note"), json!("FREQ=DAILY;BYHOUR=18;BYMINUTE=30")));
        // When the phone runs it now, with the turn settled before the call's window closes
        let ran = phone.call_settled(case, &chain, &["rpc:automation.runNow"], "automation.runNow", json!({"id": id}), |_| {
            let thread = thread_of(&mut direct, "Evening note");
            direct.wait_events(&thread, "turn.finished", 1);
        });
        answered(case, "schedule.runNow", true);
        // Then the run is a manual one on the thread rca made, the executor wrote the file the turn asked for, and the CLI was given the schedule's prompt
        let thread = thread_of(&mut direct, "Evening note");
        let run = ran["result"]["run"].clone();
        assert_eq!((run["automationId"].clone(), run["trigger"].clone(), run["chatSessionId"].clone(), run["error"].clone()), (json!(id), json!("manual"), json!(thread), Value::Null), "{run}");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/automation.txt")).unwrap(), "from the automation");
        let prompts: Vec<String> = chain.cli.invocations("claude").iter().filter_map(|s| s.argv.iter().find(|a| a.contains("on schedule")).cloned()).collect();
        assert_eq!(prompts, ["write the note on schedule"]);
        assert!(ran_executor(case), "the run did not reach the executor: {}", chain.evidence());
        // And the list says the last run is on its way or done, from rca's own record
        let after = phone.call(case, &chain, &["rpc:automation.list"], "automation.list", json!({}));
        assert!(after["result"]["items"][0]["usageSummary"]["lastRunAt"].is_u64(), "{after}");
        // When the phone deletes it, Then the id comes back, rca keeps no schedule, and a second delete is the host's not-found
        let gone = phone.call(case, &chain, &["rpc:automation.delete"], "automation.delete", json!({"id": id}));
        answered(case, "schedule.delete", true);
        assert_eq!(gone["result"], json!({"removed": true, "id": id}));
        assert_eq!(schedules(&mut direct), Vec::<Value>::new());
        let again = phone.refused(case, &chain, &["rpc:automation.delete"], "automation.delete", json!({"id": id}));
        assert_eq!(again["error"]["message"], json!("Automation not found."), "{again}");
    });
}

/// That the executor ran a tool in the last call's window: the scripted turn's write.
fn ran_executor(case: &exec_testkit::Case) -> bool {
    ran(case, "write_file")
}
