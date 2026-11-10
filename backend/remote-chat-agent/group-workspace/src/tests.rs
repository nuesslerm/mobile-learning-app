//! The four groups over an executor registry and doubles of the executors' channels: what each method asks of the executor (the args it is
//! given, never a path outside the workspace), what it answers when it cannot, and how a worktree's setup is recorded, followed and stopped.
use super::*;
use agent_api_proto::{Event, Method};
use async_trait::async_trait;
use rca_agent_core::{AgentApi, Build, Connection, EventSink, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::{Executors, Registry};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

fn ok(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

/// An executor that answers every action with `{}` except the worktree ones, and notes each call.
fn executor() -> Arc<ScriptedChannel> {
    ScriptedChannel::new(|tool, args| match tool {
        "git_worktree_add" => ok(json!({"path": format!("/base/{}", args["name"].as_str().unwrap()), "branch": args["branch"], "head": "abc"})),
        "git_worktree_remove" => ok(json!({"removed": true})),
        "git_worktree_list" => ok(json!({"worktrees": []})),
        _ => ok(json!({})),
    })
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

impl Pushed {
    fn kinds(&self) -> Vec<(u64, String)> {
        self.0.lock().unwrap().iter().map(|e| (e.seq, e.kind.clone())).collect()
    }
}

struct Rig {
    api: AgentApi,
    executors: Arc<Registry>,
}

fn rig_with(executors: Arc<Registry>) -> Rig {
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(FilesGroup::new(executors.clone())), Arc::new(FilesEditGroup::new(executors.clone())), Arc::new(WorkspaceGroup::new(executors.clone())), Arc::new(WorktreesGroup::new(executors.clone()))];
    Rig { api: AgentApi::new(groups).unwrap(), executors }
}

/// A registry with the executor `e1` attached, its root workspace named `root`.
fn rig(channel: Arc<dyn ExecutorChannel>) -> Rig {
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", channel).unwrap();
    rig_with(executors)
}

impl Rig {
    async fn ask(&self, method: Method, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
    }

    async fn ask_on(&self, method: Method, params: Value) -> (Value, Arc<Pushed>) {
        let pushed = Arc::new(Pushed::default());
        let connection = Connection::new(pushed.clone());
        let answer = self.api.dispatch_on(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string(), &connection).await;
        for stream in connection.take_following() {
            tokio::spawn(stream.job);
        }
        (answer, pushed)
    }
}

fn code(answer: &Value) -> Value {
    answer["error"]["code"].clone()
}

#[tokio::test]
async fn given_a_workspace_that_is_a_directory_when_its_files_are_asked_for_then_every_path_goes_to_the_executor_below_that_directory_and_nothing_climbs_out() {
    // Given an executor and a workspace "proj" that is its directory apps/web
    let channel = executor();
    let rig = rig(channel.clone());
    let made = rig.ask(agent_api_workspace::WORKSPACE_CREATE, json!({"workspace": "proj", "path": "apps/./web"})).await;
    assert_eq!(made, json!({"id": 1, "result": {"workspace": "proj", "executor": "e1", "path": "apps/web"}}));
    // When files are read, listed, written, created, stat, read in chunks, searched and diffed in it
    use agent_api_workspace::{edit, FILE_DIFF, FILE_LIST, FILE_READ};
    for (method, params) in [
        (FILE_READ, json!({"workspace": "proj", "path": "src/a.ts", "limit": 3})),
        (FILE_LIST, json!({"workspace": "proj", "path": "."})),
        (edit::FILE_WRITE, json!({"workspace": "proj", "path": "n.txt", "content": "c", "create_dirs": true})),
        (edit::FILE_CREATE, json!({"workspace": "proj", "path": "m.txt"})),
        (edit::FILE_STAT, json!({"workspace": "proj", "path": "a"})),
        (edit::FILE_READ_CHUNK, json!({"workspace": "proj", "path": "b.bin", "offset": 4})),
        (edit::FILE_SEARCH, json!({"workspace": "proj", "query": "main"})),
        (FILE_DIFF, json!({"workspace": "proj"})),
        (edit::THREAD_DIFF, json!({"workspace": "proj", "path": "x.ts", "base": "main"})),
        (edit::TURN_DIFF, json!({"workspace": "proj", "base": "abc123"})),
    ] {
        assert!(rig.ask(method, params).await.get("result").is_some(), "{}", method.name());
    }
    // Then the executor was asked at apps/web, a search scoped to it, and a diff of the whole workspace limited to it
    let calls = channel.calls();
    let asked: Vec<(&str, Value)> = calls.iter().map(|(tool, args)| (tool.as_str(), args.clone())).collect();
    assert_eq!(
        asked,
        [
            ("read_file", json!({"path": "apps/web/src/a.ts", "limit": 3})),
            ("list_dir", json!({"path": "apps/web"})),
            ("write_file", json!({"path": "apps/web/n.txt", "content": "c", "create_dirs": true})),
            ("create_file", json!({"path": "apps/web/m.txt"})),
            ("stat", json!({"path": "apps/web/a"})),
            ("read_chunk", json!({"path": "apps/web/b.bin", "offset": 4})),
            ("search_entries", json!({"path": "apps/web", "query": "main"})),
            ("git_diff", json!({"path": "apps/web"})),
            ("git_diff", json!({"path": "apps/web/x.ts", "base": "main"})),
            ("git_diff", json!({"path": "apps/web", "base": "abc123"})),
        ]
    );
    // When paths that leave the workspace are named, in each of the methods that take one
    let before = channel.calls().len();
    for method in [FILE_READ, FILE_LIST, edit::FILE_WRITE, edit::FILE_CREATE, edit::FILE_STAT, edit::FILE_READ_CHUNK, edit::FILE_BROWSE, FILE_DIFF, edit::THREAD_DIFF] {
        for bad in ["../secret", "a/../../secret", "/etc/passwd"] {
            let answer = rig.ask(method, json!({"workspace": "proj", "path": bad, "content": "x", "base": "b"})).await;
            // Then each is INVALID_PARAMS, and the executor is never asked (browse may name an absolute path: the executor's allowlist decides)
            if method == edit::FILE_BROWSE && bad == "/etc/passwd" {
                continue;
            }
            assert_eq!(code(&answer), json!("INVALID_PARAMS"), "{} {bad}: {answer}", method.name());
        }
    }
    assert_eq!(channel.calls().len(), before + 1, "only the absolute browse reached the executor");
}

#[tokio::test]
async fn given_the_root_workspace_when_a_path_is_named_then_it_goes_to_the_executor_as_it_came_and_its_refusals_keep_their_code() {
    // Given an executor that refuses a path outside its root, and its root workspace
    let channel = ScriptedChannel::new(|_, args| match args["path"].as_str() {
        Some(p) if p.contains("..") => Err(RemoteError::Action { code: "PATH_OUTSIDE_ROOT".into(), message: format!("{p} is outside the root") }),
        _ => ok(json!({"entries": []})),
    });
    let rig = rig(channel.clone());
    // When a path with .. is read and one without
    let refused = rig.ask(agent_api_workspace::FILE_READ, json!({"workspace": "root", "path": "../x"})).await;
    let fine = rig.ask(agent_api_workspace::FILE_LIST, json!({"workspace": "root", "path": "src"})).await;
    // Then the executor decided, with its own code, and the root's path was not rewritten
    assert_eq!(refused, json!({"id": 1, "error": {"code": "PATH_OUTSIDE_ROOT", "message": "../x is outside the root"}}));
    assert!(fine.get("result").is_some());
    assert_eq!(channel.calls()[0].1, json!({"path": "../x"}));
    // And a browse names an absolute directory to the executor unchanged, a relative one under the workspace
    rig.ask(agent_api_workspace::edit::FILE_BROWSE, json!({"workspace": "root", "path": "/home/me", "show_hidden": true})).await;
    assert_eq!(channel.calls().last().unwrap(), &("browse".to_string(), json!({"path": "/home/me", "show_hidden": true})));
    // And a workspace nobody attached is NO_EXECUTOR before any action
    let asked = channel.calls().len();
    assert_eq!(code(&rig.ask(agent_api_workspace::FILE_READ, json!({"workspace": "w9", "path": "a"})).await), json!("NO_EXECUTOR"));
    assert_eq!(channel.calls().len(), asked);
}

#[tokio::test]
async fn given_workspaces_created_and_removed_when_asked_then_names_are_unique_roots_stay_and_repo_list_says_how_each_lies() {
    // Given two executors attached and a directory workspace made on the first
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", executor()).unwrap();
    executors.open_executor("e2", "other", executor()).unwrap();
    let rig = rig_with(executors);
    use agent_api_workspace::{WORKSPACE_CREATE, WORKSPACE_REMOVE};
    // When a workspace is created without naming an executor, with several attached
    let ambiguous = rig.ask(WORKSPACE_CREATE, json!({"workspace": "p", "path": "a"})).await;
    // Then it is refused naming the choices, and nothing is recorded
    assert_eq!(code(&ambiguous), json!("AMBIGUOUS_EXECUTOR"));
    assert!(ambiguous["error"]["message"].as_str().unwrap().ends_with("e1, e2"));
    assert_eq!(rig.executors.owner("p"), None);
    // When it names the executor, a name that is taken, an unknown executor and a path that climbs out
    let made = rig.ask(WORKSPACE_CREATE, json!({"workspace": "p", "executor": "e2", "path": "a"})).await;
    let taken = rig.ask(WORKSPACE_CREATE, json!({"workspace": "p", "executor": "e1", "path": "b"})).await;
    let taken_root = rig.ask(WORKSPACE_CREATE, json!({"workspace": "root", "executor": "e1", "path": "b"})).await;
    let unknown = rig.ask(WORKSPACE_CREATE, json!({"workspace": "q", "executor": "e9", "path": "b"})).await;
    let climbing = rig.ask(WORKSPACE_CREATE, json!({"workspace": "q", "executor": "e1", "path": "../b"})).await;
    // Then the first is made on e2, the name is never given twice, an unknown executor is NO_EXECUTOR and the climbing path INVALID_PARAMS
    assert_eq!(made["result"]["executor"], "e2");
    assert_eq!((code(&taken), code(&taken_root), code(&unknown), code(&climbing)), (json!("WORKSPACE_EXISTS"), json!("WORKSPACE_EXISTS"), json!("NO_EXECUTOR"), json!("INVALID_PARAMS")));
    assert_eq!(rig.executors.owner("p"), Some("e2".into()));
    // And repo.list says each workspace's owner, how it lies and whether the owner is attached
    let listed = rig.ask(agent_api_workspace::REPO_LIST, json!({})).await;
    assert_eq!(listed["result"]["repos"], json!([
        {"workspace": "other", "executor": "e2", "kind": "root", "attached": true},
        {"workspace": "p", "executor": "e2", "kind": "directory", "attached": true, "path": "a"},
        {"workspace": "root", "executor": "e1", "kind": "root", "attached": true},
    ]));
    // When a root, an unknown workspace and the directory are removed
    let root = rig.ask(WORKSPACE_REMOVE, json!({"workspace": "root"})).await;
    let none = rig.ask(WORKSPACE_REMOVE, json!({"workspace": "zzz"})).await;
    let gone = rig.ask(WORKSPACE_REMOVE, json!({"workspace": "p"})).await;
    // Then the root stays, the unknown one is UNKNOWN_WORKSPACE, and the directory is forgotten, so its files are NO_EXECUTOR again
    assert_eq!((code(&root), code(&none)), (json!("NOT_REMOVABLE"), json!("UNKNOWN_WORKSPACE")));
    assert_eq!(gone["result"], json!({"workspace": "p", "removed": true}));
    assert_eq!(code(&rig.ask(agent_api_workspace::FILE_READ, json!({"workspace": "p", "path": "x"})).await), json!("NO_EXECUTOR"));
    assert_eq!(rig.ask(agent_api_workspace::LIST, json!({})).await["result"]["workspaces"], json!(["other", "root"]));
}

#[tokio::test]
async fn given_a_worktree_made_for_a_workspace_when_its_files_and_diff_are_asked_then_they_are_its_own_and_it_is_removed_through_its_executor() {
    // Given an executor and a worktree wt of its root workspace, on a new branch from main
    let channel = executor();
    let rig = rig(channel.clone());
    use agent_api_workspace::worktrees::{WORKTREE_CREATE, WORKTREE_LIST, WORKTREE_REMOVE};
    let made = rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "wt", "branch": "feature/x", "base": "main"})).await;
    // Then the executor was asked to add it by name, and the answer names the workspace, its repository, owner and place
    assert_eq!(channel.calls(), [("git_worktree_add".to_string(), json!({"name": "wt", "branch": "feature/x", "base": "main"}))]);
    assert_eq!(made["result"], json!({"workspace": "wt", "repo": "root", "executor": "e1", "path": "/base/wt", "branch": "feature/x", "head": "abc", "setup": false}));
    // When a file is read in the worktree, a path that leaves it is named, and its diff and a search are asked
    rig.ask(agent_api_workspace::FILE_READ, json!({"workspace": "wt", "path": "src/a.rs"})).await;
    let climbing = rig.ask(agent_api_workspace::FILE_READ, json!({"workspace": "wt", "path": "../root-file"})).await;
    rig.ask(agent_api_workspace::edit::THREAD_DIFF, json!({"workspace": "wt", "path": "src/a.rs"})).await;
    rig.ask(agent_api_workspace::edit::FILE_SEARCH, json!({"workspace": "wt", "query": "a"})).await;
    // Then the read is at the worktree's path, the climbing path is refused here, the diff names the worktree, and the search is scoped to it
    let calls = channel.calls();
    assert_eq!(calls[1], ("read_file".to_string(), json!({"path": "/base/wt/src/a.rs"})));
    assert_eq!(code(&climbing), json!("INVALID_PARAMS"));
    assert_eq!(calls[2], ("git_diff".to_string(), json!({"path": "src/a.rs", "worktree": "wt"})));
    assert_eq!(calls[3], ("search_entries".to_string(), json!({"path": "/base/wt", "query": "a"})));
    // And a worktree cannot be made of a worktree, nor under a name that is taken, nor of a workspace nobody knows
    let nested = rig.ask(WORKTREE_CREATE, json!({"workspace": "wt", "worktree": "w2", "branch": "b"})).await;
    let taken = rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "wt", "branch": "b"})).await;
    let unknown = rig.ask(WORKTREE_CREATE, json!({"workspace": "zz", "worktree": "w3", "branch": "b"})).await;
    assert_eq!((code(&nested), code(&taken), code(&unknown)), (json!("INVALID_PARAMS"), json!("WORKSPACE_EXISTS"), json!("UNKNOWN_WORKSPACE")));
    assert_eq!(channel.calls().len(), 4, "none of the refused creates reached the executor");
    // When the worktrees are listed, a worktree is removed with force, and the root and a directory are removed as worktrees
    rig.ask(WORKTREE_LIST, json!({"workspace": "root"})).await;
    let removed = rig.ask(WORKTREE_REMOVE, json!({"workspace": "wt", "force": true})).await;
    let root = rig.ask(WORKTREE_REMOVE, json!({"workspace": "root"})).await;
    // Then the executor removed it by name with force, its workspace is gone, and the root is NOT_A_WORKTREE
    let calls = channel.calls();
    assert_eq!(calls[4].0, "git_worktree_list");
    assert_eq!(calls[5], ("git_worktree_remove".to_string(), json!({"name": "wt", "force": true})));
    assert_eq!(removed["result"], json!({"workspace": "wt", "removed": true}));
    assert_eq!((code(&root), rig.executors.owner("wt")), (json!("NOT_A_WORKTREE"), None));
    // And a refusal by the executor (git refuses a dirty worktree) comes back with its code and leaves the workspace
    let dirty = ScriptedChannel::new(|tool, args| match tool {
        "git_worktree_add" => ok(json!({"path": "/base/d", "branch": args["branch"], "head": "h"})),
        _ => Err(RemoteError::Action { code: "IO".into(), message: "contains modified files, use --force".into() }),
    });
    let rig = rig_with_one(dirty);
    rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "d", "branch": "b"})).await;
    assert_eq!(rig.ask(WORKTREE_REMOVE, json!({"workspace": "d"})).await["error"]["message"], "contains modified files, use --force");
    assert_eq!(rig.executors.owner("d"), Some("e1".into()));
}

fn rig_with_one(channel: Arc<ScriptedChannel>) -> Rig {
    rig(channel as Arc<dyn ExecutorChannel>)
}

/// An executor whose `worktree_setup` sends `first` events at once and then, when `hold` is set, never finishes: it notes that the call was
/// dropped (what cancelling does). Every other action is answered `{}`, the worktree add with its path.
struct SetupChannel {
    first: Vec<Value>,
    rest: Vec<Value>,
    hold: bool,
    release: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}

struct Dropped(Arc<AtomicBool>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl ExecutorChannel for SetupChannel {
    async fn call(&self, tool: &str, args: Value) -> Result<ActionOutput, RemoteError> {
        match tool {
            "git_worktree_add" => ok(json!({"path": format!("/base/{}", args["name"].as_str().unwrap()), "branch": "b", "head": "h"})),
            "git_worktree_remove" => ok(json!({"removed": true})),
            _ => ok(json!({})),
        }
    }

    async fn call_with_events(&self, tool: &str, _args: Value, events: mpsc::UnboundedSender<Value>) -> Result<ActionOutput, RemoteError> {
        assert_eq!(tool, "worktree_setup");
        let _guard = Dropped(self.dropped.clone());
        for event in &self.first {
            let _ = events.send(event.clone());
        }
        if self.hold {
            self.release.notified().await;
        }
        for event in &self.rest {
            let _ = events.send(event.clone());
        }
        std::mem::forget(_guard);
        ok(json!({"ok": true, "steps": [], "timed_out": false, "cancelled": false}))
    }

    fn close(&self, _: u16) {}

    fn is_closed(&self) -> bool {
        false
    }

    async fn closed(&self) {
        std::future::pending::<()>().await
    }
}

fn setup_channel(first: Vec<Value>, rest: Vec<Value>, hold: bool) -> (Arc<SetupChannel>, Arc<Notify>, Arc<AtomicBool>) {
    let (release, dropped) = (Arc::new(Notify::new()), Arc::new(AtomicBool::new(false)));
    (Arc::new(SetupChannel { first, rest, hold, release: release.clone(), dropped: dropped.clone() }), release, dropped)
}

async fn eventually(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what}");
}

#[tokio::test]
async fn given_a_worktree_with_setup_commands_when_a_client_subscribes_then_it_gets_the_recorded_events_then_the_live_ones_and_a_resume_has_no_gap() {
    // Given an executor whose setup has sent two events and waits, and a worktree created with setup commands
    let (channel, release, _) = setup_channel(
        vec![json!({"kind": "setup.started", "steps": 1}), json!({"kind": "step.started", "step": 0, "command": "make"})],
        vec![json!({"kind": "step.output", "step": 0, "stream": "stdout", "data": "ok\n"}), json!({"kind": "step.finished", "step": 0, "exit_code": 0}), json!({"kind": "setup.finished", "ok": true})],
        true,
    );
    let rig = rig(channel);
    use agent_api_workspace::worktrees::{SETUP_SUBSCRIBE, WORKTREE_CREATE};
    let made = rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "wt", "branch": "b", "setup": ["make"]})).await;
    // Then the answer came after the executor started the setup
    assert_eq!(made["result"]["setup"], true);
    // When a client subscribes from the start while the setup is running
    let (answer, first) = rig.ask_on(SETUP_SUBSCRIBE, json!({"workspace": "wt"})).await;
    // Then it is told where the replay ends and that the setup runs, and gets the two recorded events at once
    assert_eq!(answer["result"], json!({"workspace": "wt", "after_seq": 0, "last_seq": 2, "running": true}));
    eventually("the replay", || first.kinds().len() == 2).await;
    assert_eq!(first.kinds(), [(1, "setup.started".to_string()), (2, "step.started".to_string())]);
    // When the executor goes on
    release.notify_one();
    // Then the live events follow in order, the last one is the executor's setup.finished, and the stream ends
    eventually("the live events", || first.kinds().len() == 5).await;
    assert_eq!(first.kinds().iter().map(|(seq, _)| *seq).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert_eq!(first.kinds().last().unwrap().1, "setup.finished");
    assert!(first.0.lock().unwrap()[2].data["workspace"] == "wt" && first.0.lock().unwrap()[2].data["data"] == "ok\n");
    // When a client comes back having seen up to 3, and another after the end
    let (resumed, second) = rig.ask_on(SETUP_SUBSCRIBE, json!({"workspace": "wt", "after_seq": 3})).await;
    let (ended, third) = rig.ask_on(SETUP_SUBSCRIBE, json!({"workspace": "wt", "after_seq": 5})).await;
    // Then the first gets exactly 4 and 5, and the second none, and both are told the setup is over
    assert_eq!((resumed["result"]["running"].clone(), ended["result"]["running"].clone()), (json!(false), json!(false)));
    eventually("the resume", || second.kinds().len() == 2).await;
    assert_eq!(second.kinds().iter().map(|(seq, _)| *seq).collect::<Vec<_>>(), [4, 5]);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(third.kinds().is_empty());
}

#[tokio::test]
async fn given_a_running_setup_when_it_is_cancelled_or_its_worktree_removed_then_the_executor_call_is_dropped_and_the_log_ends_cancelled() {
    use agent_api_workspace::worktrees::{SETUP_CANCEL, SETUP_SUBSCRIBE, WORKTREE_CREATE, WORKTREE_REMOVE};
    // Given a setup that has started and never finishes
    let (channel, _release, dropped) = setup_channel(vec![json!({"kind": "setup.started", "steps": 1})], vec![], true);
    let rig = rig(channel);
    rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "wt", "branch": "b", "setup": ["sleep 99"]})).await;
    let (_, live) = rig.ask_on(SETUP_SUBSCRIBE, json!({"workspace": "wt"})).await;
    eventually("the started event", || live.kinds().len() == 1).await;
    // When it is cancelled, and cancelled again
    let cancelled = rig.ask(SETUP_CANCEL, json!({"workspace": "wt"})).await;
    let again = rig.ask(SETUP_CANCEL, json!({"workspace": "wt"})).await;
    // Then the executor's call was dropped (which sends cancel), the follower got a final setup.finished that says cancelled, and a second cancel finds it over
    assert_eq!(cancelled["result"], json!({"workspace": "wt", "cancelled": true}));
    assert_eq!(again["result"], json!({"workspace": "wt", "cancelled": false}));
    eventually("the executor call to be dropped", || dropped.load(Ordering::SeqCst)).await;
    eventually("the final event", || live.kinds().len() == 2).await;
    let last = live.0.lock().unwrap()[1].clone();
    assert_eq!((last.kind.as_str(), last.data["ok"].clone(), last.data["cancelled"].clone()), ("setup.finished", json!(false), json!(true)));
    // And a worktree that never had a setup, a workspace that is no worktree and an unknown one are each refused by their code
    rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "plain", "branch": "c"})).await;
    let none = rig.ask(SETUP_CANCEL, json!({"workspace": "plain"})).await;
    let not_wt = rig.ask(SETUP_SUBSCRIBE, json!({"workspace": "root"})).await;
    let unknown = rig.ask(SETUP_CANCEL, json!({"workspace": "zz"})).await;
    assert_eq!((code(&none), code(&not_wt), code(&unknown)), (json!("NO_SETUP"), json!("NOT_A_WORKTREE"), json!("UNKNOWN_WORKSPACE")));
    // And removing a worktree whose setup runs ends the run the same way, and forgets it
    let (channel, _release, dropped) = setup_channel(vec![json!({"kind": "setup.started", "steps": 1})], vec![], true);
    let rig = rig_with_executor(channel);
    rig.ask(WORKTREE_CREATE, json!({"workspace": "root", "worktree": "rm", "branch": "b", "setup": ["sleep 99"]})).await;
    assert_eq!(rig.ask(WORKTREE_REMOVE, json!({"workspace": "rm"})).await["result"]["removed"], true);
    eventually("the executor call to be dropped", || dropped.load(Ordering::SeqCst)).await;
    assert_eq!(code(&rig.ask(SETUP_SUBSCRIBE, json!({"workspace": "rm"})).await), json!("UNKNOWN_WORKSPACE"));
}

fn rig_with_executor(channel: Arc<SetupChannel>) -> Rig {
    rig(channel as Arc<dyn ExecutorChannel>)
}

#[tokio::test]
async fn given_an_executor_that_cannot_start_the_setup_when_the_worktree_is_created_then_it_exists_and_the_answer_says_why_the_setup_did_not_run() {
    // Given an executor whose worktree_setup fails at once
    struct Failing;
    #[async_trait]
    impl ExecutorChannel for Failing {
        async fn call(&self, _tool: &str, args: Value) -> Result<ActionOutput, RemoteError> {
            ok(json!({"path": format!("/base/{}", args["name"].as_str().unwrap_or("x")), "branch": "b", "head": "h"}))
        }

        async fn call_with_events(&self, _tool: &str, _args: Value, _events: mpsc::UnboundedSender<Value>) -> Result<ActionOutput, RemoteError> {
            Err(RemoteError::Action { code: "NOT_FOUND".into(), message: "worktree wt does not exist".into() })
        }

        fn close(&self, _: u16) {}

        fn is_closed(&self) -> bool {
            false
        }

        async fn closed(&self) {
            std::future::pending::<()>().await
        }
    }
    let rig = rig(Arc::new(Failing));
    // When a worktree is created with a setup
    let made = rig.ask(agent_api_workspace::worktrees::WORKTREE_CREATE, json!({"workspace": "root", "worktree": "wt", "branch": "b", "setup": ["make"]})).await;
    // Then the worktree exists, the setup did not start, and the reason is in the answer and in the log
    assert_eq!((made["result"]["setup"].clone(), made["result"]["setup_error"].clone()), (json!(false), json!("worktree wt does not exist")));
    assert_eq!(rig.executors.owner("wt"), Some("e1".into()));
    let (answer, pushed) = rig.ask_on(agent_api_workspace::worktrees::SETUP_SUBSCRIBE, json!({"workspace": "wt"})).await;
    assert_eq!(answer["result"]["running"], false);
    eventually("the finished event", || pushed.kinds().len() == 1).await;
    assert_eq!(pushed.kinds(), [(1, "setup.finished".to_string())]);
}
