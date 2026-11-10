//! The automation rows through the `Host` dispatcher with schedules in memory as rca's `schedules` group: each reply is read the way the phone reads it, and a
//! change is read back from the schedule rca keeps. Given/When/Then in each test.
use agent_api_schedules::Schedule;
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_schedules::testing::{Schedules, NOW_MS};
use orca_map_schedules::SchedulesArea;
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    schedules: Schedules,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        Workspaces::default().workspace("main", "root", Some("/srv/main")).install(&agent);
        let schedules = Schedules::default();
        schedules.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(SchedulesArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, schedules, conn: host.connection("dev", "tok", vec![]), n: 0 }
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

    fn saves(&self) -> usize {
        self.agent.requests().iter().filter(|(m, _)| m == "schedule.upsert").count()
    }
}

fn weekday_nine() -> Value {
    json!({"name": "Morning triage", "prompt": "Triage the open issues", "agentId": "codex", "repo": "id:main::/srv/main", "workspace": "id:main::/srv/main", "workspaceMode": "existing", "setupDecision": "inherit", "reuseSession": false, "precheck": null, "timezone": "UTC", "rrule": "0 9 * * 1-5", "dtstart": 1790942400000u64})
}

#[test]
fn given_the_phones_weekday_automation_when_created_then_rca_keeps_a_schedule_at_that_time_and_the_phone_reads_back_what_it_sent() {
    // Given a schedule another client made, and the phone's default cron of nine on weekdays in UTC
    let mut rig = Rig::new();
    rig.schedules.foreign("t3-1", "A T3 task");
    // When the phone creates the automation
    let made = rig.ok("automation.create", weekday_nine())["automation"].clone();
    // Then rca keeps one more schedule that runs the prompt at 09:00 on Monday to Friday in the workspace, by the codex driver
    let kept = rig.schedules.tasks().into_iter().find(|t| t.id == made["id"].as_str().unwrap()).unwrap();
    assert_eq!((kept.title.as_str(), kept.prompt.as_str(), kept.workspace.as_str(), kept.driver.as_str(), kept.enabled), ("Morning triage", "Triage the open issues", "main", "codex", true));
    assert_eq!(kept.schedule, Schedule::FixedTime { time_of_day: "09:00".into(), weekdays: Some(vec![1, 2, 3, 4, 5]), utc_offset_minutes: None });
    // And the phone reads back its own recurrence, timezone, start and selectors, the agent, and when rca will run it next
    assert_eq!((made["name"].clone(), made["agentId"].clone(), made["workspaceMode"].clone(), made["rrule"].clone(), made["timezone"].clone()), (json!("Morning triage"), json!("codex"), json!("existing"), json!("0 9 * * 1-5"), json!("UTC")));
    assert_eq!((made["workspaceId"].clone(), made["projectId"].clone(), made["dtstart"].clone(), made["nextRunAt"].clone(), made["enabled"].clone()), (json!("id:main::/srv/main"), json!("id:main::/srv/main"), json!(1790942400000u64), json!(NOW_MS + 3_600_000), json!(true)));
    // And the list holds it with its owner and not the other client's schedule, and show reads the same record
    let listed = rig.ok("automation.list", json!({}));
    assert_eq!((listed["automations"].as_array().unwrap().len(), listed["items"][0]["automationId"].clone(), listed["items"][0]["selector"].clone()), (1, made["id"].clone(), json!({"kind": "self"})));
    assert_eq!(rig.ok("automation.show", json!({"id": made["id"]}))["automation"], made);
}

#[test]
fn given_an_automation_when_updated_then_rca_keeps_the_changed_schedule_and_a_disabled_one_still_shows_when_it_would_run() {
    // Given an automation
    let mut rig = Rig::new();
    let id = rig.ok("automation.create", weekday_nine())["automation"]["id"].as_str().unwrap().to_string();
    // When the phone renames it, moves it to a daily 18:30 at UTC-5, changes the agent and switches it off
    let updated = rig.ok("automation.update", json!({"id": id, "updates": {"name": "Evening triage", "rrule": "FREQ=DAILY;BYHOUR=18;BYMINUTE=30", "timezone": "Etc/GMT+5", "agentId": "claude", "enabled": false}}))["automation"].clone();
    // Then rca keeps the new title, time, offset, driver and the off switch, and the same prompt and workspace
    let kept = rig.schedules.tasks().remove(0);
    assert_eq!((kept.title.as_str(), kept.prompt.as_str(), kept.workspace.as_str(), kept.driver.as_str(), kept.enabled), ("Evening triage", "Triage the open issues", "main", "claudeAgent", false));
    assert_eq!(kept.schedule, Schedule::FixedTime { time_of_day: "18:30".into(), weekdays: None, utc_offset_minutes: Some(-300) });
    // And the phone reads the new text back, and a next run for the disabled one that rca does not name
    assert_eq!((updated["name"].clone(), updated["rrule"].clone(), updated["timezone"].clone(), updated["agentId"].clone(), updated["enabled"].clone()), (json!("Evening triage"), json!("FREQ=DAILY;BYHOUR=18;BYMINUTE=30"), json!("Etc/GMT+5"), json!("claude"), json!(false)));
    assert!(kept.next_run_ms.is_none() && updated["nextRunAt"].as_u64().unwrap() > NOW_MS);
    // And an update that names only the prompt keeps the schedule as it was
    rig.ok("automation.update", json!({"id": id, "updates": {"prompt": "Triage and label"}}));
    let kept = rig.schedules.tasks().remove(0);
    assert_eq!((kept.prompt.as_str(), kept.title.as_str()), ("Triage and label", "Evening triage"));
    assert_eq!(kept.schedule, Schedule::FixedTime { time_of_day: "18:30".into(), weekdays: None, utc_offset_minutes: Some(-300) });
}

#[test]
fn given_settings_rca_cannot_run_when_an_automation_is_created_or_updated_then_it_is_refused_with_the_reason_and_nothing_is_saved() {
    // Given an automation, and a phone that asks for what an rca schedule does not do
    let mut rig = Rig::new();
    let id = rig.ok("automation.create", weekday_nine())["automation"]["id"].as_str().unwrap().to_string();
    let saved = rig.saves();
    for (patch, reason) in [
        (json!({"workspaceMode": "new_per_run"}), "never in a worktree"),
        (json!({"precheck": {"command": "test -f ok", "timeoutSeconds": 30}}), "a precheck command is not offered"),
        (json!({"extraAgentArgs": "--fast"}), "extra agent arguments"),
        (json!({"reuseSession": true}), "a reused session"),
        (json!({"missedRunGraceMinutes": 30}), "a grace for missed runs"),
        (json!({"baseBranch": "main"}), "a base branch"),
        (json!({"sourceContext": {"kind": "task-source"}}), "a task source"),
        (json!({"destination": {"selector": {"kind": "ssh", "targetId": "t", "targetGeneration": 1}}}), "ssh"),
        (json!({"rrule": "FREQ=HOURLY"}), "hourly"),
        (json!({"rrule": "*/5 * * * *"}), "several minutes or hours"),
        (json!({"timezone": "America/Los_Angeles"}), "no timezone database"),
        (json!({"agentId": "gemini"}), "Unknown provider"),
    ] {
        // When the phone creates one with it, Then it is refused as an invalid argument naming what rca lacks
        let mut create = weekday_nine();
        create.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let (code, message) = rig.err("automation.create", create);
        assert_eq!(code, "invalid_argument", "{patch}");
        assert!(message.contains(reason), "{patch}: {message}");
        // And when the phone updates one with it, the same
        let (code, message) = if patch.get("destination").is_some() { rig.err("automation.update", json!({"id": id, "updates": {}, "destination": patch["destination"]})) } else { rig.err("automation.update", json!({"id": id, "updates": patch})) };
        assert_eq!(code, "invalid_argument", "{patch}");
        assert!(message.contains(reason), "{patch}: {message}");
    }
    // And no schedule was saved by any of them
    assert_eq!((rig.saves(), rig.schedules.tasks().len()), (saved, 1));
    // And a create naming neither a project nor a workspace, one without a start, and an unknown id are refused in the host's words
    let mut nowhere = weekday_nine();
    nowhere.as_object_mut().unwrap().retain(|k, _| k != "repo" && k != "workspace");
    assert_eq!(rig.err("automation.create", nowhere), ("invalid_argument".into(), "Automation requires --repo or --workspace.".into()));
    let mut undated = weekday_nine();
    undated.as_object_mut().unwrap().remove("dtstart");
    assert_eq!(rig.err("automation.create", undated).1, "Missing trigger start time");
    assert_eq!(rig.err("automation.show", json!({"id": "nope"})), ("runtime_error".into(), "Automation not found.".into()));
}

#[test]
fn given_a_creation_key_when_the_phone_retries_the_create_then_the_same_automation_comes_back_and_rca_keeps_one_schedule() {
    // Given a create with a creation key that was answered
    let mut rig = Rig::new();
    let first = rig.ok("automation.create", json!({"creationKey": "k1", "name": "Triage", "prompt": "Go", "agentId": "codex", "workspace": "main", "workspaceMode": "existing", "timezone": "UTC", "rrule": "0 6 * * *", "dtstart": 1}));
    // When the phone sends it again
    let again = rig.ok("automation.create", json!({"creationKey": "k1", "name": "Triage", "prompt": "Go", "agentId": "codex", "workspace": "main", "workspaceMode": "existing", "timezone": "UTC", "rrule": "0 6 * * *", "dtstart": 1}));
    // Then it is the same automation and rca holds one schedule
    assert_eq!((again["automation"]["id"].clone(), rig.schedules.tasks().len(), rig.saves()), (first["automation"]["id"].clone(), 1, 1));
}

#[test]
fn given_an_automation_when_it_is_run_now_then_the_run_is_the_one_rca_started_and_a_run_that_failed_says_why() {
    // Given an automation
    let mut rig = Rig::new();
    let id = rig.ok("automation.create", weekday_nine())["automation"]["id"].as_str().unwrap().to_string();
    // When the phone runs it now
    let run = rig.ok("automation.runNow", json!({"id": id}))["run"].clone();
    // Then the run is a manual one on the thread rca started, dispatched at the instant rca named
    assert_eq!((run["automationId"].clone(), run["title"].clone(), run["trigger"].clone(), run["status"].clone(), run["chatSessionId"].clone()), (json!(id), json!("Morning triage"), json!("manual"), json!("dispatched"), json!("thread-1")));
    assert_eq!((run["scheduledFor"].clone(), run["startedAt"].clone(), run["error"].clone(), run["workspaceId"].clone()), (json!(NOW_MS), json!(NOW_MS), Value::Null, json!("id:main::/srv/main")));
    // And the list says the last run is dispatched
    assert_eq!(rig.ok("automation.list", json!({}))["items"][0]["usageSummary"], json!({"lastRunStatus": "dispatched", "lastRunAt": NOW_MS}));
    // When rca cannot start the next run
    rig.schedules.next_run_fails("the workspace's executor is not connected");
    let failed = rig.ok("automation.runNow", json!({"id": id}))["run"].clone();
    // Then it is a failed dispatch with rca's words and no start
    assert_eq!((failed["status"].clone(), failed["error"].clone(), failed["startedAt"].clone()), (json!("dispatch_failed"), json!("the workspace's executor is not connected"), Value::Null));
}

#[test]
fn given_an_automation_when_it_is_deleted_then_rca_keeps_no_schedule_and_a_second_delete_is_the_hosts_not_found() {
    // Given an automation and a schedule of another client
    let mut rig = Rig::new();
    rig.schedules.foreign("t3-1", "A T3 task");
    let id = rig.ok("automation.create", weekday_nine())["automation"]["id"].as_str().unwrap().to_string();
    // When the phone deletes the automation
    assert_eq!(rig.ok("automation.delete", json!({"id": id})), json!({"removed": true, "id": id}));
    // Then only the other client's schedule remains, and deleting the other client's schedule or the same one again is not found
    assert_eq!(rig.schedules.tasks().iter().map(|t| t.id.clone()).collect::<Vec<_>>(), vec!["t3-1".to_string()]);
    assert_eq!(rig.err("automation.delete", json!({"id": id})).1, "Automation not found.");
    assert_eq!(rig.err("automation.delete", json!({"id": "t3-1"})).1, "Automation not found.");
    assert_eq!(rig.schedules.tasks().len(), 1);
}

#[test]
fn given_a_list_for_another_owner_or_a_schedule_rca_answers_unreadably_when_asked_then_the_owner_is_refused_and_the_unreadable_one_is_named() {
    // Given an automation
    let mut rig = Rig::new();
    rig.ok("automation.create", weekday_nine());
    // When the phone lists the orphans, Then there are none; and for an ssh target the call is refused
    assert_eq!(rig.ok("automation.list", json!({"selector": {"kind": "orphan"}})), json!({"automations": [], "items": []}));
    assert!(rig.err("automation.list", json!({"selector": {"kind": "ssh", "targetId": "t", "expectedTargetGeneration": 1}})).1.contains("no ssh targets"));
    // When rca answers a schedule whose automation record has lost its fields
    rig.agent.reply("schedule.list", |_| Ok(json!({"tasks": [{"id": "s9", "title": "t", "prompt": "p", "enabled": true, "schedule": {"type": "interval", "every_ms": 60000}, "workspace": "main", "driver": "codex", "model": null, "thread": null, "runtime_mode": null, "interaction_mode": null, "created_at_ms": 1, "updated_at_ms": 1, "next_run_ms": 2, "last_run_ms": null, "last_run_status": "never", "last_run_error": null, "run_count": 0, "webhook": null, "meta": {"orca": {"project_id": "p"}}}]})));
    // Then the list is refused naming the schedule rather than showing blank fields
    let (code, message) = rig.err("automation.list", json!({}));
    assert_eq!(code, "runtime_error");
    assert!(message.contains("s9") && message.contains("unreadable") && message.contains("missing field"), "{message}");
}
