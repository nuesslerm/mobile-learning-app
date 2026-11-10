//! The relay over doubles of the executors' channels, and the tabs and their stream over a clock and ids that move when told.
use super::*;
use crate::testing::{CountingIds, FakeClock};
use agent_api_preview as api;
use agent_api_preview::sessions::{self as tabs, AdjustParams, CloseParams, Clear, ColorScheme, ListParams, Nav, OpenParams, ReportParams, ResizeParams, Viewport};
use agent_api_preview::watch::kind;
use agent_api_proto::Method;
use rca_agent_core::{AgentApi, Build, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::Registry;
use serde_json::{json, Value};
use std::sync::Arc;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

fn ok(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

async fn ask(api: &AgentApi, method: Method, params: Value) -> Value {
    api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
}

fn relay_rig(channel: Arc<dyn ExecutorChannel>) -> AgentApi {
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", channel).unwrap();
    AgentApi::new(vec![Arc::new(PreviewGroup::new(executors)) as Arc<dyn Group>]).unwrap()
}

fn previews() -> (Arc<Previews>, Arc<FakeClock>) {
    let clock = Arc::new(FakeClock::at(1_000));
    (Previews::new(clock.clone(), Arc::new(CountingIds::default()), 0), clock)
}

fn open(p: &Previews, thread: &str, url: Option<&str>) -> tabs::SessionView {
    p.open(OpenParams { thread: thread.into(), url: url.map(str::to_string), viewport: None, profile: None, reveal: None }).unwrap()
}

#[test]
fn given_the_served_rows_when_compared_with_the_contract_then_each_method_has_one_in_order_naming_the_action_of_its_spec() {
    let rows: Vec<(Method, Vec<&str>)> = group::SERVED.iter().map(|s| (s.method, s.actions.to_vec())).collect();
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), api::METHODS);
    for spec in &api::SPECS {
        assert_eq!(rows.iter().find(|r| r.0 == spec.method).unwrap().1, [spec.action]);
    }
}

#[tokio::test]
async fn given_every_server_method_when_called_then_the_executor_is_asked_its_action_with_the_forwarded_params_and_its_answer_comes_back_as_it_came() {
    // Given an executor that answers each action with a marker naming it
    let channel = ScriptedChannel::new(|tool, _| ok(json!({"action": tool})));
    let api = relay_rig(channel.clone());
    for spec in &api::SPECS {
        // When the method is called with every key it forwards and one it does not
        let mut params = json!({"workspace": "root", "unforwarded": 1});
        for key in spec.keys {
            params[*key] = json!("v");
        }
        let answer = ask(&api, spec.method, params).await;
        // Then the answer is the executor's, and the executor saw the keys of the spec and not the other one
        assert_eq!(answer["result"], json!({"action": spec.action}), "{}", spec.method.name());
        let (tool, args) = channel.calls().pop().unwrap();
        assert_eq!(tool, spec.action);
        assert!(args.get("unforwarded").is_none() && args.get("workspace").is_none(), "{args}");
        assert_eq!(args.as_object().unwrap().len(), spec.keys.len(), "{args}");
    }
}

#[tokio::test]
async fn given_a_refusal_a_missing_workspace_and_a_detached_one_when_started_then_each_answer_keeps_its_code() {
    // Given an executor that refuses a start with the code of a taken port
    let channel = ScriptedChannel::new(|_, _| Err(RemoteError::Action { code: "ALREADY_EXISTS".into(), message: "port 4000 is already in use".into() }));
    let api = relay_rig(channel.clone());
    // When a start is asked, one without a workspace, and one for a workspace nobody attached
    let refused = ask(&api, api::START, json!({"workspace": "root", "command": "x", "port": 4000})).await;
    let none = ask(&api, api::START, json!({"command": "x", "port": 4000})).await;
    let detached = ask(&api, api::STOP, json!({"workspace": "other", "id": "p"})).await;
    // Then the refusal is the executor's, the others are INVALID_PARAMS and NO_EXECUTOR, and only the first reached the executor
    assert_eq!((refused["error"]["code"].clone(), refused["error"]["message"].clone()), (json!("ALREADY_EXISTS"), json!("port 4000 is already in use")));
    assert_eq!((none["error"]["code"].clone(), detached["error"]["code"].clone()), (json!("INVALID_PARAMS"), json!("NO_EXECUTOR")));
    assert_eq!(channel.calls().len(), 1);
}

#[test]
fn given_a_thread_when_tabs_are_opened_then_each_is_listed_with_what_it_was_opened_with_and_the_stream_counts_the_changes() {
    // Given a service with a clock and a thread with two tabs opened a moment apart
    let (p, clock) = previews();
    let first = open(&p, "t1", Some("http://127.0.0.1:3000/"));
    clock.advance(5);
    let second = open(&p, "t1", None);
    open(&p, "t2", None);
    // When the thread's tabs are listed
    let listed = p.list(ListParams { thread: "t1".into() });
    // Then they are t1's two, the first loading its address and the second idle on a full viewport, oldest change first
    assert_eq!(listed.sessions.iter().map(|s| s.tab.as_str()).collect::<Vec<_>>(), [first.tab.as_str(), second.tab.as_str()]);
    assert_eq!(listed.sessions[0].nav, Nav::Loading { url: "http://127.0.0.1:3000/".into(), title: String::new() });
    assert_eq!((listed.sessions[1].nav.clone(), listed.sessions[1].viewport.clone(), listed.sessions[1].updated_ms), (Nav::Idle {}, Viewport::Fill {}, 1_005));
    // And three opens are three revisions of this run's epoch
    assert_eq!((listed.revision, listed.epoch.as_str()), (3, "epoch-1"));
}

#[test]
fn given_a_tab_when_reported_resized_and_adjusted_then_each_change_is_kept_and_recorded_as_the_event_of_its_kind() {
    // Given a tab loading a page
    let (p, clock) = previews();
    let tab = open(&p, "t", Some("http://127.0.0.1:3000/")).tab;
    clock.advance(10);
    // When the app reports success, then a failure, the viewport changes and the colour scheme, zoom and a reload are adjusted
    let ok = p.report(ReportParams { thread: "t".into(), tab: tab.clone(), nav: Nav::Success { url: "http://127.0.0.1:3000/".into(), title: "App".into() }, can_go_back: true, can_go_forward: false }).unwrap();
    p.report(ReportParams { thread: "t".into(), tab: tab.clone(), nav: Nav::Failed { url: "http://127.0.0.1:3000/x".into(), title: String::new(), code: -105, description: "name not resolved".into() }, can_go_back: true, can_go_forward: false }).unwrap();
    p.resize(ResizeParams { thread: "t".into(), tab: tab.clone(), viewport: Viewport::Preset { width: 390, height: 844, preset: "iphone-14".into() } }).unwrap();
    let adjusted = p.adjust(AdjustParams { thread: "t".into(), tab: tab.clone(), color_scheme: Some(ColorScheme::Dark), zoom: Some(1.25), hard_reload: Some(true), clear: Some(Clear::Cache) }).unwrap();
    // Then the tab holds all of it, and the stream has the open and four changes, in order and of the kinds
    assert_eq!((ok.can_go_back, adjusted.zoom, adjusted.color_scheme, adjusted.viewport.clone(), adjusted.updated_ms), (true, Some(1.25), Some(ColorScheme::Dark), Viewport::Preset { width: 390, height: 844, preset: "iphone-14".into() }, 1_010));
    let events = p.since(0).events;
    assert_eq!(events.iter().map(|e| (e.seq, e.kind.as_str())).collect::<Vec<_>>(), [(1, kind::OPENED), (2, kind::NAVIGATED), (3, kind::FAILED), (4, kind::RESIZED), (5, kind::RESIZED)]);
    assert_eq!((events[2].data["code"].clone(), events[2].data["description"].clone()), (json!(-105), json!("name not resolved")));
    assert_eq!(events[4].data["request"], json!({"hard_reload": true, "clear": "cache"}));
    // And an adjustment that sets nothing keeps what was set
    let kept = p.adjust(AdjustParams { thread: "t".into(), tab, color_scheme: None, zoom: None, hard_reload: None, clear: None }).unwrap();
    assert_eq!((kept.zoom, kept.color_scheme), (Some(1.25), Some(ColorScheme::Dark)));
}

#[test]
fn given_changes_to_a_tab_that_is_not_there_when_made_then_each_is_refused_and_nothing_is_recorded() {
    // Given a thread with one tab
    let (p, _) = previews();
    let tab = open(&p, "t", None).tab;
    // When another thread's tab and an unknown tab are changed
    let wrong_thread = p.resize(ResizeParams { thread: "other".into(), tab: tab.clone(), viewport: Viewport::Fill {} }).unwrap_err();
    let unknown = p.adjust(AdjustParams { thread: "t".into(), tab: "nope".into(), color_scheme: None, zoom: Some(2.0), hard_reload: None, clear: None }).unwrap_err();
    // Then each is PREVIEW_SESSION_NOT_FOUND naming the tab and the stream still holds only the open
    assert_eq!((wrong_thread.code.as_str(), unknown.message.as_str()), ("PREVIEW_SESSION_NOT_FOUND", "thread t has no preview tab nope"));
    assert_eq!(p.since(0).events.len(), 1);
}

#[test]
fn given_a_thread_with_tabs_when_one_or_all_are_closed_then_those_go_with_an_event_each_and_a_repeated_close_is_no_failure() {
    // Given a thread with three tabs and another thread with one
    let (p, _) = previews();
    let tabs_of_t: Vec<String> = (0..3).map(|_| open(&p, "t", None).tab).collect();
    let other = open(&p, "u", None).tab;
    // When one tab is closed, the same again, and then the rest of the thread
    let one = p.close(CloseParams { thread: "t".into(), tab: Some(tabs_of_t[1].clone()) });
    let again = p.close(CloseParams { thread: "t".into(), tab: Some(tabs_of_t[1].clone()) });
    let rest = p.close(CloseParams { thread: "t".into(), tab: None });
    // Then each close names what it closed, the other thread keeps its tab, and every closed tab has a `closed` event
    assert_eq!((one.closed, again.closed, rest.closed), (vec![tabs_of_t[1].clone()], Vec::<String>::new(), vec![tabs_of_t[0].clone(), tabs_of_t[2].clone()]));
    assert_eq!(p.list(ListParams { thread: "u".into() }).sessions[0].tab, other);
    assert_eq!(p.since(4).events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), [kind::CLOSED; 3]);
}

#[test]
fn given_as_many_tabs_as_allowed_when_one_more_is_opened_then_it_is_refused_for_the_thread_and_for_the_service() {
    // Given a thread with the most tabs a thread keeps
    let (p, _) = previews();
    for _ in 0..tabs::MAX_TABS_PER_THREAD {
        open(&p, "t", None);
    }
    // When it opens one more, and other threads fill the service
    let thread_full = p.open(OpenParams { thread: "t".into(), url: None, viewport: None, profile: None, reveal: None }).unwrap_err();
    for n in 0..(tabs::MAX_TABS - tabs::MAX_TABS_PER_THREAD) {
        open(&p, &format!("u{}", n / tabs::MAX_TABS_PER_THREAD), None);
    }
    let service_full = p.open(OpenParams { thread: "v".into(), url: None, viewport: None, profile: None, reveal: None }).unwrap_err();
    // Then each refusal is PREVIEW_SESSION_LIMIT with its reason
    assert_eq!((thread_full.code.as_str(), thread_full.message.as_str()), ("PREVIEW_SESSION_LIMIT", "thread t already has 16 preview tabs"));
    assert_eq!(service_full.message, "the service already holds 128 preview tabs");
}

#[test]
fn given_callers_of_the_stream_when_they_subscribe_then_the_ones_that_cannot_resume_are_reset_and_the_ones_that_can_are_sent_what_they_missed() {
    // Given a service that kept only the last three of five events
    let (clock, ids) = (Arc::new(FakeClock::at(1)), Arc::new(CountingIds::default()));
    let p = Previews::keeping(clock, ids, 0, 3);
    for _ in 0..5 {
        open(&p, "t", None);
    }
    let epoch = p.list(ListParams { thread: "t".into() }).epoch;
    // When a new caller, one of this run within the tail, one older than the tail, one ahead and one of another run subscribe
    let new = p.open_stream(None, None);
    let within = p.open_stream(Some(&epoch), Some(3));
    let old = p.open_stream(Some(&epoch), Some(1));
    let ahead = p.open_stream(Some(&epoch), Some(9));
    let other_run = p.open_stream(Some("epoch-9"), Some(3));
    // Then only the new caller is not reset, and it follows from the end; the one within the tail is not reset and follows from its seq
    assert_eq!((new.result.reset, new.from, new.result.seq), (false, 5, 5));
    assert_eq!((within.result.reset, within.from), (false, 3));
    assert_eq!([old.result.reset, ahead.result.reset, other_run.result.reset], [true; 3]);
    // And the events after a seq are the kept ones that follow it, and a seq older than the tail is lost
    assert_eq!(p.since(3).events.iter().map(|e| e.seq).collect::<Vec<_>>(), [4, 5]);
    assert!(p.since(1).lost && !p.since(2).lost);
}

#[tokio::test]
async fn given_the_recorded_exchanges_of_the_contract_when_replayed_on_a_service_of_their_own_then_each_response_is_what_was_recorded() {
    // Given the recorded exchanges of every method of the three groups, which together cover each method
    let fixtures = agent_api_preview::testing::fixtures();
    let mut named: Vec<&str> = fixtures.iter().map(|f| f.method.as_str()).collect();
    let mut methods: Vec<&str> = [api::METHODS, tabs::METHODS, api::watch::METHODS].concat().iter().map(|m| m.name()).collect();
    named.sort_unstable();
    methods.sort_unstable();
    assert_eq!(named, methods);
    for fixture in fixtures.iter().filter(|f| ![api::START, api::STOP, api::SERVERS].iter().any(|m| m.name() == f.method)) {
        for exchange in &fixture.exchanges {
            // When an exchange's setup and request are made on a service whose clock and ids count from one
            let previews = Previews::new(Arc::new(FakeClock::at(1)), Arc::new(CountingIds::default()), 0);
            let groups: Vec<Arc<dyn Group>> = vec![Arc::new(PreviewSessionGroup::new(previews.clone())), Arc::new(PreviewWatchGroup::new(previews))];
            let api = AgentApi::new(groups).unwrap();
            for before in &exchange.before {
                let setup = api.dispatch_text(BUILD, &before.to_string()).await;
                assert!(setup.get("result").is_some(), "{}: the setup {before} failed: {setup}", exchange.name);
            }
            let answer = api.dispatch_text(BUILD, &exchange.request.to_string()).await;
            // Then the answer is the recorded one, apart from the times
            assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
        }
    }
}

#[test]
fn given_a_service_run_that_counts_from_a_base_when_a_caller_of_an_earlier_run_resumes_with_no_epoch_then_it_is_reset() {
    // Given a run whose stream starts above the numbers the earlier run gave out, and a tab opened in it
    let p = Previews::new(Arc::new(FakeClock::at(1)), Arc::new(CountingIds::default()), 1_000);
    open(&p, "t", None);
    // When a caller of the earlier run (seq 40) and one that kept up with this run (seq 1000) resume, naming no epoch
    let earlier = p.open_stream(None, Some(40));
    let current = p.open_stream(None, Some(1_000));
    // Then the first is reset and the second is sent the open it missed
    assert_eq!((earlier.result.reset, current.result.reset, current.from), (true, false, 1_000));
    assert_eq!(p.since(1_000).events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1_001]);
    assert_eq!(p.list(ListParams { thread: "t".into() }).revision, 1_001);
}
