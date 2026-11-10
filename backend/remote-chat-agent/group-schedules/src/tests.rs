use crate::domain::{self, Request};
use crate::ports::{Refusal, Stored};
use crate::service::{Schedules, WebhookOutcome};
use crate::testing::{CountingIds, FakeClock, MemoryStore, ScriptedTrigger};
use agent_api_proto::Event;
use agent_api_schedules::{RunSource, RunState, RunStatus, Schedule, UpsertParams};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// A Tuesday, 22:13:20 UTC.
const NOW: u64 = 1_700_000_000_000;
const DAY: u64 = 86_400_000;
const MINUTE: u64 = 60_000;

struct Rig {
    store: Arc<MemoryStore>,
    clock: Arc<FakeClock>,
    trigger: Arc<ScriptedTrigger>,
    schedules: Arc<Schedules>,
}

fn rig_keeping(keep: usize) -> Rig {
    let (store, clock, trigger) = (Arc::new(MemoryStore::default()), Arc::new(FakeClock::at(NOW)), Arc::new(ScriptedTrigger::default()));
    let schedules = Schedules::keeping(store.clone(), clock.clone(), Arc::new(CountingIds::default()), trigger.clone(), 1000, keep);
    Rig { store, clock, trigger, schedules }
}

fn rig() -> Rig {
    rig_keeping(256)
}

fn params(schedule: Value, enabled: bool) -> UpsertParams {
    UpsertParams::parse(&json!({"title": "Nightly", "prompt": "run the checks", "enabled": enabled, "schedule": schedule, "workspace": "w1", "driver": "claudeAgent"})).unwrap()
}

fn every_minute() -> Value {
    json!({"type": "interval", "every_ms": MINUTE})
}

fn webhook() -> Value {
    json!({"type": "webhook"})
}

fn kinds(rig: &Rig) -> Vec<(String, Value)> {
    rig.schedules.since(1000).events.iter().map(summary).collect()
}

fn summary(event: &Event) -> (String, Value) {
    let key = match event.kind.as_str() {
        "schedule.run" => event.data["run"]["state"].clone(),
        "schedule.webhook" => event.data["outcome"].clone(),
        _ => event.data["change"].clone(),
    };
    (event.kind.clone(), key)
}

async fn settled(rig: &Rig, schedule: &str, status: RunStatus) {
    for _ in 0..200 {
        if rig.schedules.list().unwrap().iter().any(|t| t.id == schedule && t.last_run_status == status) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("{schedule} never reached {status:?}: {:?}", rig.schedules.list());
}

#[test]
fn given_the_schedule_kinds_when_the_next_run_is_read_then_an_interval_counts_on_a_fixed_time_is_the_next_such_time_on_its_weekday_at_its_offset_and_a_webhook_has_none() {
    // Given a clock at a Tuesday evening, and the midnight that follows it
    let midnight = (NOW / DAY + 1) * DAY;
    let fixed = |time: &str, weekdays: Option<Vec<u8>>, offset: Option<i32>| Schedule::FixedTime { time_of_day: time.into(), weekdays, utc_offset_minutes: offset };
    // When the next runs are read, Then an interval is its length on
    assert_eq!(domain::next_run(&Schedule::Interval { every_ms: 90_000 }, NOW), Some(NOW + 90_000));
    // And 09:30 daily is tomorrow's, and 23:00 daily is tonight's
    assert_eq!(domain::next_run(&fixed("09:30", None, None), NOW), Some(midnight + 9 * 60 * MINUTE + 30 * MINUTE));
    assert_eq!(domain::next_run(&fixed("23:00", None, None), NOW), Some(midnight - 60 * MINUTE));
    // And only on Mondays (1) is the Monday six days on, and on Tuesdays (2) a time already passed today is next week's
    assert_eq!(domain::next_run(&fixed("09:30", Some(vec![1]), None), NOW), Some(midnight + 5 * DAY + 9 * 60 * MINUTE + 30 * MINUTE));
    assert_eq!(domain::next_run(&fixed("09:30", Some(vec![2]), None), NOW), Some(midnight + 6 * DAY + 9 * 60 * MINUTE + 30 * MINUTE));
    // And at two hours east of UTC 00:30 local is 22:30 UTC tonight, and a run exactly due is not due again
    assert_eq!(domain::next_run(&fixed("00:30", None, Some(120)), NOW), Some(midnight - 90 * MINUTE));
    let due = domain::next_run(&fixed("23:00", None, None), NOW).unwrap();
    assert_eq!(domain::next_run(&fixed("23:00", None, None), due), Some(due + DAY));
    assert_eq!(domain::next_run(&Schedule::Webhook {}, NOW), None);
}

#[test]
fn given_a_prompt_with_placeholders_when_rendered_then_each_is_what_the_request_carried_and_an_unknown_one_stays() {
    // Given a request with a JSON body, a query and a header
    let request = Request { body: r#"{"action": "opened", "pr": {"number": 7, "labels": ["a", "b"]}}"#.into(), query: "env=prod&x=1".into(), headers: vec![("X-Event".into(), "pull_request".into())] };
    // When prompts that name them are rendered
    let rendered = domain::render("{{body.action}} #{{ body.pr.number }} ({{body.pr.labels.1}}) in {{query.env}} by {{headers.x-event}}; {{body.missing}}|{{other}}|{{body.action", &request);
    // Then each is replaced, a missing path is nothing, and what is no placeholder is as written
    assert_eq!(rendered, "opened #7 (b) in prod by pull_request; |{{other}}|{{body.action");
    assert_eq!(domain::render("{{body}}", &request), request.body);
    assert_eq!(serde_json::from_str::<Value>(&domain::render("{{request}}", &request)).unwrap()["headers"]["X-Event"], json!("pull_request"));
}

#[test]
fn given_two_digests_when_compared_then_only_equal_ones_match_whatever_their_length() {
    assert!(domain::digests_equal("abc", "abc") && !domain::digests_equal("abc", "abd") && !domain::digests_equal("abc", "ab") && !domain::digests_equal("", "a"));
}

#[tokio::test]
async fn given_a_due_interval_schedule_when_the_clock_passes_it_then_a_turn_starts_through_the_trigger_and_the_run_is_an_event_and_nothing_starts_before() {
    // Given an enabled schedule that runs every minute, saved now
    let rig = rig();
    let saved = rig.schedules.upsert(params(every_minute(), true)).unwrap();
    assert_eq!(saved.task.next_run_ms, Some(NOW + MINUTE));
    // When the clock is just short of it, and then reaches it
    rig.clock.advance(MINUTE - 1);
    assert_eq!(rig.schedules.tick().await, 0);
    rig.clock.advance(1);
    assert_eq!(rig.schedules.tick().await, 1);
    // Then one turn was asked for with the schedule's prompt, workspace and driver, on a new thread
    let asked = rig.trigger.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!((asked[0].prompt.as_str(), asked[0].workspace.as_str(), asked[0].driver.as_str(), asked[0].thread.clone()), ("run the checks", "w1", "claudeAgent", None));
    // And the schedule is moved a minute on, shows the run and is running, and the run is the second event, after the save
    let task = rig.schedules.list().unwrap().remove(0);
    assert_eq!((task.next_run_ms, task.last_run_ms, task.last_run_status, task.run_count), (Some(NOW + 2 * MINUTE), Some(NOW + MINUTE), RunStatus::Running, 1));
    assert_eq!(kinds(&rig), [("schedule.changed".to_string(), json!("saved")), ("schedule.run".to_string(), json!("started"))]);
    let run = &rig.schedules.since(1000).events[1].data["run"];
    assert_eq!((run["source"].clone(), run["thread"].clone(), run["turn"].clone()), (json!("timer"), json!("th-1"), json!("tu-1")));
    // And a second tick at the same time starts nothing: the schedule is not due again
    assert_eq!(rig.schedules.tick().await, 0);
}

#[tokio::test]
async fn given_a_turn_that_ends_when_it_ends_then_the_schedule_and_the_stream_say_how_and_a_due_run_while_one_runs_is_skipped() {
    // Given a schedule whose turn runs
    let rig = rig();
    rig.schedules.upsert(params(every_minute(), true)).unwrap();
    rig.clock.advance(MINUTE);
    rig.schedules.tick().await;
    // When the next run falls due while the turn still runs
    rig.clock.advance(MINUTE);
    assert_eq!(rig.schedules.tick().await, 0);
    // Then it is recorded as skipped, no second turn was asked for, and the run count is still one
    assert_eq!(rig.trigger.asked().len(), 1);
    assert_eq!(rig.schedules.list().unwrap()[0].run_count, 1);
    assert_eq!(kinds(&rig).last().unwrap(), &("schedule.run".to_string(), json!("skipped")));
    // When the turn ends with a failure
    rig.trigger.end("tu-1", Err("interrupted".into()));
    settled(&rig, "sch-1", RunStatus::Failed).await;
    // Then the schedule keeps the reason, the stream has the failed run, and the next due run starts a turn
    assert_eq!(rig.schedules.list().unwrap()[0].last_run_error.as_deref(), Some("interrupted"));
    assert_eq!(kinds(&rig).last().unwrap(), &("schedule.run".to_string(), json!("failed")));
    rig.clock.advance(MINUTE);
    assert_eq!(rig.schedules.tick().await, 1);
    rig.trigger.end("tu-2", Ok(()));
    settled(&rig, "sch-1", RunStatus::Succeeded).await;
    assert_eq!(rig.schedules.list().unwrap()[0].last_run_error, None);
}

#[tokio::test]
async fn given_a_disabled_schedule_when_the_clock_passes_it_then_nothing_starts_and_enabling_it_starts_the_count_again() {
    // Given an enabled schedule that is then disabled
    let rig = rig();
    rig.schedules.upsert(params(every_minute(), true)).unwrap();
    let off = rig.schedules.set_enabled("sch-1", false).unwrap();
    assert_eq!(off.task.next_run_ms, None);
    // When the clock passes its time, Then no turn is asked for
    rig.clock.advance(5 * MINUTE);
    assert_eq!(rig.schedules.tick().await, 0);
    assert!(rig.trigger.asked().is_empty());
    // When it is enabled again, Then it is due a minute from then, not at once
    let on = rig.schedules.set_enabled("sch-1", true).unwrap();
    assert_eq!(on.task.next_run_ms, Some(NOW + 6 * MINUTE));
    assert_eq!(rig.schedules.tick().await, 0);
    // And setting what already is leaves no event, and an unknown id is not found
    rig.schedules.set_enabled("sch-1", true).unwrap();
    assert_eq!(kinds(&rig).iter().map(|(_, what)| what.clone()).collect::<Vec<_>>(), [json!("saved"), json!("disabled"), json!("enabled")]);
    assert_eq!(rig.schedules.set_enabled("nope", true).unwrap_err().code, "SCHEDULE_NOT_FOUND");
}

#[tokio::test]
async fn given_a_disabled_schedule_when_it_is_run_now_then_its_turn_starts_and_a_refused_turn_is_the_error_and_a_failed_run() {
    // Given a disabled schedule that goes on thread t9
    let rig = rig();
    let mut p = params(every_minute(), false);
    p.thread = Some("t9".into());
    rig.schedules.upsert(p).unwrap();
    // When it is run now
    let ran = rig.schedules.run_now("sch-1").await.unwrap();
    // Then the turn was asked for on t9, the run is a manual one, and the schedule shows it
    assert_eq!(rig.trigger.asked()[0].thread.as_deref(), Some("t9"));
    assert_eq!((ran.run.source, ran.run.state, ran.run.thread.as_deref(), ran.task.run_count), (RunSource::Manual, RunState::Started, Some("t9"), 1));
    // When the agent group refuses the next turn
    rig.trigger.refuse_with(Some(Refusal { code: "TURN_RUNNING".into(), message: "thread t9 already has a running turn".into() }));
    let refused = rig.schedules.run_now("sch-1").await.unwrap_err();
    // Then the refusal is the agent group's own, and the stream has a failed run with its message
    assert_eq!((refused.code.as_str(), refused.message.as_str()), ("TURN_RUNNING", "thread t9 already has a running turn"));
    let last = rig.schedules.since(1000).events.pop().unwrap();
    assert_eq!((last.data["run"]["state"].clone(), last.data["run"]["error"].clone()), (json!("failed"), json!("thread t9 already has a running turn")));
    assert_eq!(rig.schedules.run_now("nope").await.unwrap_err().code, "SCHEDULE_NOT_FOUND");
}

#[tokio::test]
async fn given_a_webhook_schedule_when_requests_come_then_only_the_right_token_starts_a_turn_and_each_request_is_an_event() {
    // Given a webhook schedule whose prompt names the body, and its token, shown once and kept only as a digest
    let rig = rig();
    let mut p = params(webhook(), true);
    p.prompt = "review {{body.pr}}".into();
    let saved = rig.schedules.upsert(p).unwrap();
    let token = saved.webhook_token.clone().expect("a webhook gets its token with its first save");
    assert_eq!((saved.task.webhook.as_ref().map(|w| (w.path.as_str(), w.has_token)), saved.task.next_run_ms), (Some(("/hooks/v1/schedules/sch-1", true)), None));
    let kept = rig.store.kept();
    assert_eq!(kept[0].token_digest.as_deref(), Some(rca_agent_core::token_digest(&token).as_str()));
    assert!(!serde_json::to_string(&kept).unwrap().contains(&token));
    let body = Request { body: r#"{"pr": 42}"#.into(), ..Request::default() };
    // When a request with a wrong token, one for an unknown schedule, and one with the right token arrive
    let wrong = rig.schedules.webhook("sch-1", "not-the-token", &body).await;
    let unknown = rig.schedules.webhook("sch-9", &token, &body).await;
    assert_eq!((wrong, unknown), (WebhookOutcome::Rejected, WebhookOutcome::Rejected));
    // Then no turn was asked for, and both are events that say the token was rejected
    assert!(rig.trigger.asked().is_empty());
    assert_eq!(kinds(&rig)[1..], [("schedule.webhook".to_string(), json!("rejected_token")), ("schedule.webhook".to_string(), json!("rejected_token"))]);
    // When the right one arrives
    let accepted = rig.schedules.webhook("sch-1", &token, &body).await;
    // Then the turn was asked for with the prompt rendered from the body, and the run is a webhook one
    assert!(matches!(&accepted, WebhookOutcome::Accepted(run) if run.source == RunSource::Webhook));
    assert_eq!(rig.trigger.asked()[0].prompt, "review 42");
    assert_eq!(kinds(&rig)[3..], [("schedule.webhook".to_string(), json!("accepted")), ("schedule.run".to_string(), json!("started"))]);
    // And a disabled webhook lets nobody in, and a clock tick never starts one
    rig.schedules.set_enabled("sch-1", false).unwrap();
    assert_eq!(rig.schedules.webhook("sch-1", &token, &body).await, WebhookOutcome::Disabled);
    rig.clock.advance(10 * DAY);
    assert_eq!(rig.schedules.tick().await, 0);
    assert_eq!(rig.trigger.asked().len(), 1);
}

#[tokio::test]
async fn given_a_rotated_token_when_the_old_and_the_new_are_used_then_only_the_new_one_gets_in_and_a_schedule_that_is_no_webhook_cannot_be_rotated() {
    // Given a webhook schedule with its first token, and an interval schedule
    let rig = rig();
    let first = rig.schedules.upsert(params(webhook(), true)).unwrap().webhook_token.unwrap();
    rig.schedules.upsert(params(every_minute(), true)).unwrap();
    // When the token is rotated
    let rotated = rig.schedules.rotate_webhook_token("sch-1").unwrap();
    let second = rotated.webhook_token.clone().unwrap();
    // Then the new token differs, the old one is rejected and the new one starts a turn
    assert_ne!(first, second);
    let nothing = Request::default();
    assert_eq!(rig.schedules.webhook("sch-1", &first, &nothing).await, WebhookOutcome::Rejected);
    assert!(matches!(rig.schedules.webhook("sch-1", &second, &nothing).await, WebhookOutcome::Accepted(_)));
    // And saving the schedule again does not make a new token, and an interval schedule is not a webhook
    assert_eq!(rig.schedules.upsert(UpsertParams { id: Some("sch-1".into()), ..params(webhook(), true) }).unwrap().webhook_token, None);
    assert_eq!(rig.schedules.rotate_webhook_token("sch-2").unwrap_err().code, "NOT_A_WEBHOOK");
    assert_eq!(rig.schedules.webhook("sch-2", &second, &nothing).await, WebhookOutcome::Rejected);
    // And changing a webhook to a timer drops its token
    rig.schedules.upsert(UpsertParams { id: Some("sch-1".into()), ..params(every_minute(), true) }).unwrap();
    assert_eq!((rig.store.kept()[0].token_digest.clone(), rig.schedules.list().unwrap()[0].webhook.clone()), (None, None));
}

#[tokio::test]
async fn given_saves_and_deletes_when_listed_then_a_save_of_a_known_id_changes_it_in_place_and_keeps_its_runs_and_require_existing_refuses_a_gone_one() {
    // Given a schedule that has run once
    let rig = rig();
    rig.schedules.upsert(params(every_minute(), true)).unwrap();
    rig.schedules.run_now("sch-1").await.unwrap();
    // When it is saved again under its id with another title and another time
    rig.clock.advance(1000);
    let mut again = params(json!({"type": "fixed_time", "time_of_day": "09:30"}), true);
    (again.id, again.title, again.meta) = (Some("sch-1".into()), "Morning".into(), Some(json!({"project": "p1"})));
    let saved = rig.schedules.upsert(again).unwrap().task;
    // Then it is the same schedule, edited: created when it was, run count kept, next run the time of day, and the mapper's data kept as given
    assert_eq!(saved.meta, Some(json!({"project": "p1"})));
    assert_eq!((saved.title.as_str(), saved.created_at_ms, saved.updated_at_ms, saved.run_count), ("Morning", NOW, NOW + 1000, 1));
    assert_eq!(saved.next_run_ms, domain::next_run(&saved.schedule, NOW + 1000));
    assert_eq!(rig.schedules.list().unwrap().len(), 1);
    // And a delete removes it, a second one finds none, and a save that requires it to exist is refused once it is gone
    rig.schedules.delete("sch-1").unwrap();
    assert_eq!(rig.schedules.delete("sch-1").unwrap_err().code, "SCHEDULE_NOT_FOUND");
    let mut gone = params(every_minute(), true);
    (gone.id, gone.require_existing) = (Some("sch-1".into()), Some(true));
    assert_eq!(rig.schedules.upsert(gone).unwrap_err().code, "SCHEDULE_NOT_FOUND");
    assert!(rig.schedules.list().unwrap().is_empty());
}

#[tokio::test]
async fn given_a_store_that_fails_when_a_schedule_is_saved_then_it_is_unavailable_and_no_event_is_numbered() {
    // Given a service whose store cannot be written
    let rig = rig();
    rig.store.fail_with(Some("disk full"));
    // When a schedule is saved and when the schedules are listed
    let saved = rig.schedules.upsert(params(every_minute(), true)).unwrap_err();
    let listed = rig.schedules.list().unwrap_err();
    // Then each is SCHEDULES_UNAVAILABLE with the store's reason, the stream is where it was, and the service works again once the store does
    assert_eq!((saved.code.as_str(), saved.message.as_str(), listed.code.as_str()), ("SCHEDULES_UNAVAILABLE", "disk full", "SCHEDULES_UNAVAILABLE"));
    assert!(rig.schedules.since(1000).events.is_empty());
    rig.store.fail_with(None);
    assert_eq!(rig.schedules.upsert(params(every_minute(), true)).unwrap().task.id, "sch-1");
    assert_eq!(rig.schedules.since(1000).events[0].seq, 1001);
}

#[tokio::test]
async fn given_a_schedule_a_crash_left_running_when_the_service_recovers_then_it_is_failed_so_the_clock_can_start_it_again() {
    // Given a stored schedule that was running when the service stopped
    let rig = rig();
    rig.schedules.upsert(params(every_minute(), true)).unwrap();
    let mut kept: Vec<Stored> = rig.store.kept();
    kept[0].task.last_run_status = RunStatus::Running;
    rig.store.put(kept);
    // When the service recovers, and the schedule falls due
    assert_eq!(rig.schedules.recover(), Ok(1));
    rig.clock.advance(MINUTE);
    // Then a turn starts for it
    assert_eq!(rig.schedules.tick().await, 1);
    assert_eq!(rig.schedules.recover(), Ok(1));
}

#[test]
fn given_a_stream_that_keeps_two_events_when_callers_open_it_then_a_caller_within_the_tail_resumes_and_one_behind_it_or_ahead_of_it_is_reset() {
    // Given three saves on a stream that keeps the last two (numbered from 1001)
    let rig = rig_keeping(2);
    for _ in 0..3 {
        rig.schedules.upsert(params(every_minute(), true)).unwrap();
    }
    let open = |after: Option<u64>| rig.schedules.open(after).unwrap();
    // When it is opened with no seq, with the last, with one just inside the tail, with one behind it, and with one ahead
    let (fresh, last, inside, behind, ahead) = (open(None), open(Some(1003)), open(Some(1001)), open(Some(1000)), open(Some(9999)));
    // Then the schedules come with each, only a caller behind the tail or ahead of the stream is reset, and a resuming caller continues after its seq
    assert_eq!(fresh.result.tasks.len(), 3);
    assert_eq!([&fresh, &last, &inside, &behind, &ahead].map(|o| o.result.reset), [false, false, false, true, true]);
    assert_eq!([&fresh, &last, &inside, &behind, &ahead].map(|o| o.from), [1003, 1003, 1001, 1003, 1003]);
    assert_eq!(rig.schedules.since(1001).events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1002, 1003]);
    assert!(rig.schedules.since(1000).lost && !rig.schedules.since(1001).lost);
}
