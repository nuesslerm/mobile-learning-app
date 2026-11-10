//! The group over an executor registry and doubles of the executors' channels: what each method asks of the executor, the worktree it is aimed
//! at, how the executor's refusals come back, and the status stream relayed as numbered events.
use super::*;
use agent_api_git as api;
use agent_api_proto::{Event, Method};
use async_trait::async_trait;
use rca_agent_core::{AgentApi, Build, Connection, EventSink, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::{Executors, Place, Registry};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

fn ok(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

#[derive(Default)]
struct Pushed(Mutex<Vec<Event>>);

#[async_trait]
impl EventSink for Pushed {
    fn send(&self, event: &Event) -> bool {
        self.0.lock().unwrap().push(event.clone());
        true
    }

    async fn send_replay(&self, event: &Event) -> bool {
        self.send(event)
    }
}

struct Rig {
    api: AgentApi,
    executors: Arc<Registry>,
}

fn rig(channel: Arc<dyn ExecutorChannel>) -> Rig {
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", channel).unwrap();
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(GitGroup::new(executors.clone()))];
    Rig { api: AgentApi::new(groups).unwrap(), executors }
}

impl Rig {
    async fn ask(&self, method: Method, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
    }
}

#[test]
fn given_the_served_rows_when_compared_with_the_contract_then_each_method_has_one_in_order_naming_the_action_its_spec_does() {
    // Given the rows and the contract
    let rows: Vec<(Method, Vec<&str>)> = SERVED.iter().map(|s| (s.method, s.actions.to_vec())).collect();
    // Then the rows are the contract's methods in order, and each request method calls the action of its spec and the subscription the watch
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), api::METHODS);
    for spec in &api::SPECS {
        assert_eq!(rows.iter().find(|r| r.0 == spec.method).unwrap().1, [spec.action]);
    }
    assert_eq!(rows.last().unwrap().1, [exec_git::GIT_WATCH]);
}

#[tokio::test]
async fn given_every_request_method_when_called_then_the_executor_is_asked_its_action_with_the_params_the_spec_forwards_and_the_result_comes_back_as_it_came() {
    // Given an executor that answers each action with a marker naming it
    let channel = ScriptedChannel::new(|tool, _| ok(json!({"action": tool})));
    let rig = rig(channel.clone());
    for spec in &api::SPECS {
        // When the method is called with every key it forwards and one it does not
        let mut params = json!({"workspace": "root", "unforwarded": 1});
        for key in spec.keys {
            params[*key] = json!("v");
        }
        let answer = rig.ask(spec.method, params).await;
        // Then the result is the executor's and the executor got the keys and not the extra one
        assert_eq!(answer, json!({"id": 1, "result": {"action": spec.action}}), "{}", spec.method.name());
        let (tool, args) = channel.calls().pop().unwrap();
        assert_eq!(tool, spec.action);
        let mut keys: Vec<&str> = args.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut want = spec.keys.to_vec();
        want.sort_unstable();
        assert_eq!(keys, want, "{}", spec.method.name());
    }
}

#[tokio::test]
async fn given_a_worktree_workspace_when_a_method_is_called_then_the_executor_is_aimed_at_the_worktree_and_the_repositorys_own_workspace_is_not() {
    // Given the repository's workspace and a worktree workspace of it
    let channel = ScriptedChannel::new(|_, _| ok(json!({})));
    let rig = rig(channel.clone());
    rig.executors.add_workspace_at("e1", "wt1", Place { prefix: Some("/base/wt1".into()), worktree: Some("wt1".into()) }).unwrap();
    // When status is asked of each, and a blob is read in the worktree
    rig.ask(api::STATUS, json!({"workspace": "root"})).await;
    rig.ask(api::STATUS, json!({"workspace": "wt1"})).await;
    rig.ask(api::BLOB, json!({"workspace": "wt1", "rev": "HEAD", "path": "a/b.txt"})).await;
    // Then only the worktree's calls name it, and the blob's path is the repository's own, never moved under the worktree path
    let calls = channel.calls();
    assert_eq!(calls[0].1, json!({}));
    assert_eq!(calls[1].1, json!({"worktree": "wt1"}));
    assert_eq!(calls[2].1, json!({"rev": "HEAD", "path": "a/b.txt", "worktree": "wt1"}));
}

#[tokio::test]
async fn given_bad_requests_and_a_refusing_executor_when_called_then_each_answers_its_own_code_before_or_from_the_executor() {
    // Given an executor that refuses a commit the way the real one does for an empty message
    let channel = ScriptedChannel::new(|tool, _| match tool {
        "git_commit" => Err(RemoteError::Action { code: "INVALID_ARGS".into(), message: "`message` must not be empty".into() }),
        _ => ok(json!({})),
    });
    let rig = rig(channel.clone());
    // When called with no workspace, with one nobody attached, and with the refused commit
    let none = rig.ask(api::STATUS, json!({})).await;
    let detached = rig.ask(api::STATUS, json!({"workspace": "w9"})).await;
    let refused = rig.ask(api::COMMIT, json!({"workspace": "root", "message": " "})).await;
    // Then the first two never reach the executor and the third keeps the executor's code and message
    assert_eq!((none["error"]["code"].clone(), none["error"]["message"].clone()), (json!("INVALID_PARAMS"), json!("workspace is required")));
    assert_eq!(detached["error"]["code"], json!("NO_EXECUTOR"));
    assert_eq!(refused, json!({"id": 1, "error": {"code": "INVALID_ARGS", "message": "`message` must not be empty"}}));
    assert_eq!(channel.calls().len(), 1);
}

/// An executor whose watch sends the given events and then stays open until the call is dropped.
struct Watching {
    events: Vec<Value>,
    asked: Mutex<Vec<(String, Value)>>,
    ended: Arc<std::sync::atomic::AtomicBool>,
}

struct Ends(Arc<std::sync::atomic::AtomicBool>);

impl Drop for Ends {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait]
impl ExecutorChannel for Watching {
    async fn call(&self, _tool: &str, _args: Value) -> Result<ActionOutput, RemoteError> {
        ok(json!({}))
    }

    async fn call_with_events(&self, tool: &str, args: Value, events: mpsc::UnboundedSender<Value>) -> Result<ActionOutput, RemoteError> {
        self.asked.lock().unwrap().push((tool.to_string(), args));
        let _ends = Ends(self.ended.clone());
        for event in &self.events {
            let _ = events.send(event.clone());
        }
        std::future::pending::<()>().await;
        ok(json!({}))
    }

    fn close(&self, _: u16) {}

    fn is_closed(&self) -> bool {
        false
    }

    async fn closed(&self) {
        std::future::pending::<()>().await
    }
}

#[tokio::test]
async fn given_a_status_watch_when_subscribed_then_each_status_of_the_executor_is_an_event_numbered_after_the_callers_last_and_ending_the_stream_ends_the_watch() {
    // Given an executor whose watch reports a status and then a changed one
    let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watching = Arc::new(Watching { events: vec![json!({"kind": "git.status", "status": {"branch": "main", "clean": true}}), json!({"kind": "git.status", "status": {"branch": "main", "clean": false}})], asked: Mutex::default(), ended: ended.clone() });
    let rig = rig(watching.clone());
    let pushed = Arc::new(Pushed::default());
    let connection = Connection::new(pushed.clone());
    // When a caller that last saw event 4 subscribes with its own interval
    let answer = rig.api.dispatch_on(BUILD, &json!({"id": 1, "method": "git.subscribe", "params": {"workspace": "root", "after_seq": 4, "interval_ms": 250}}).to_string(), &connection).await;
    let streams = connection.take_following();
    let task = tokio::spawn(streams.into_iter().next().unwrap().job);
    for _ in 0..200 {
        if pushed.0.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Then the answer says where the numbering stands, the events carry the statuses numbered 5 and 6, and the watch was asked with the interval
    assert_eq!(answer, json!({"id": 1, "result": {"workspace": "root", "seq": 4}}));
    let events: Vec<(u64, String, Value)> = pushed.0.lock().unwrap().iter().map(|e| (e.seq, e.kind.clone(), e.data["status"]["clean"].clone())).collect();
    assert_eq!(events, [(5, "git.status".to_string(), json!(true)), (6, "git.status".to_string(), json!(false))]);
    assert_eq!(watching.asked.lock().unwrap()[0], ("git_watch".to_string(), json!({"interval_ms": 250})));
    // And when the stream is stopped the watch call is dropped, which cancels it at the executor
    task.abort();
    let _ = task.await;
    assert!(ended.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn given_a_directory_workspace_when_git_is_asked_then_it_is_refused_and_the_executor_is_never_called() {
    // Given a workspace that is a directory inside the repository
    let channel = ScriptedChannel::new(|_, _| ok(json!({})));
    let rig = rig(channel.clone());
    rig.executors.add_workspace_at("e1", "dir", Place::directory("apps/web").unwrap()).unwrap();
    // When staging, discarding and reading status are asked in it
    let mut answers = Vec::new();
    for (spec, params) in [(api::STAGE, json!({"paths": ["index.ts"]})), (api::DISCARD, json!({"paths": ["index.ts"], "untracked": true})), (api::STATUS, json!({}))] {
        let mut params = params;
        params["workspace"] = json!("dir");
        answers.push(rig.ask(spec, params).await);
    }
    // Then each is refused as invalid arguments, and no root file was touched
    for answer in &answers {
        assert_eq!(answer["error"]["code"], json!("INVALID_ARGS"), "{answer}");
    }
    assert!(channel.calls().is_empty());
}
