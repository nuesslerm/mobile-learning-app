//! `agent.requests`, `agent.queue` and `agent.lifecycle` over the real thread engine, with the CLIs scripted and the executor a double.
//! The engine decides; these are the groups' own behaviours: what a request does to a waiting turn, which turn starts next, what a steer or a forced stop does to the process.
use super::engine_threads::EngineThreads;
use agent_api_proto::Event;
use rca_agent_core::{AgentApi, Build, Group};
use rca_cli_driver::testing::{CliScript, ScriptedSpawner};
use rca_exec_channel::mcp::ServiceTools;
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::Registry;
use rca_group_agent::{AgentLifecycleGroup, AgentQueueGroup, AgentRequestsGroup, AgentThreadsGroup, McpEndpoint, Runner, ThreadEngine};
use remote_chat_agent_core::application::engine::Engine;
use remote_chat_agent_core::application::store::MemoryStore;
use remote_chat_agent_core::testing::{CountingIds, FixedClock};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const NOW: u64 = 1_700_000_000_000;
const BUILD: Build<'static> = Build { version: "1", commit: "c" };

struct Mcp;

impl McpEndpoint for Mcp {
    fn url(&self, thread: &str) -> String {
        format!("http://127.0.0.1:1/mcp/{thread}")
    }
}

struct Rig {
    api: AgentApi,
    runner: Arc<Runner>,
    engine: Arc<EngineThreads>,
    spawner: Arc<ScriptedSpawner>,
    /// The executor actions asked of the workspace's channel, with their args, in order.
    asked: Arc<std::sync::Mutex<Vec<(String, Value)>>>,
}

fn rig() -> Rig {
    let engine = Arc::new(EngineThreads::new(Arc::new(Engine::new(Box::new(MemoryStore::default()), "/work".into(), Arc::new(FixedClock(NOW)), Arc::new(CountingIds::default())))));
    let (spawner, executors) = (ScriptedSpawner::new(), Arc::new(Registry::default()));
    let asked: Arc<std::sync::Mutex<Vec<(String, Value)>>> = Arc::default();
    let log = asked.clone();
    let channel = ScriptedChannel::new(move |tool, args| {
        log.lock().unwrap().push((tool.to_string(), args.clone()));
        Ok(ActionOutput { result: json!({}), output: String::new() })
    });
    executors.open("w1", channel as Arc<dyn ExecutorChannel>).unwrap();
    let runner = Runner::new(engine.clone(), executors, spawner.clone(), Arc::new(Mcp), PathBuf::from("/srv/rca"));
    let groups: Vec<Arc<dyn Group>> = vec![
        Arc::new(AgentThreadsGroup::with_runner(runner.clone())),
        Arc::new(AgentRequestsGroup::new(runner.clone())),
        Arc::new(AgentQueueGroup::new(runner.clone())),
        Arc::new(AgentLifecycleGroup::new(runner.clone())),
    ];
    Rig { api: AgentApi::new(groups).unwrap(), runner, engine, spawner, asked }
}

fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

fn init(session: &str) -> String {
    json!({"type": "system", "subtype": "init", "session_id": session}).to_string()
}

fn script(lines: &[String], hold: bool) -> CliScript {
    CliScript { lines: lines.to_vec(), hold, ..CliScript::default() }
}

impl Rig {
    async fn ask(&self, method: &str, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method, "params": params}).to_string()).await
    }

    /// The result of a call that must succeed.
    async fn ok(&self, method: &str, params: Value) -> Value {
        let answer = self.ask(method, params).await;
        assert!(answer.get("result").is_some(), "{method}: {answer}");
        answer["result"].clone()
    }

    async fn create(&self, thread: &str) {
        self.ok("thread.create", json!({"thread": thread, "title": format!("title of {thread}"), "workspace": "w1", "driver": "claudeAgent"})).await;
    }

    async fn start(&self, thread: &str, prompt: &str) -> String {
        self.ok("turn.start", json!({"thread": thread, "workspace": "w1", "driver": "claudeAgent", "prompt": prompt})).await["turn"].as_str().unwrap().to_string()
    }

    fn events(&self, thread: &str) -> Vec<Event> {
        self.engine.history(thread, 0, usize::MAX).unwrap().unwrap().events
    }

    fn kinds(&self, thread: &str) -> Vec<String> {
        self.events(thread).iter().map(|e| e.kind.clone()).collect()
    }

    async fn until(&self, what: &str, mut done: impl FnMut(&Rig) -> bool) {
        for _ in 0..500 {
            if done(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("never happened: {what}");
    }

    async fn idle(&self, thread: &str) {
        self.until(&format!("{thread} idle"), |r| r.engine.get(thread).unwrap().unwrap().turn.is_none()).await;
    }

    /// The `nth` (from 1) `approval.requested` or `input.requested` event of the thread, once there is one.
    async fn open_request(&self, thread: &str, kind: &str, nth: usize) -> Event {
        self.until(&format!("{kind} number {nth} on {thread}"), |r| r.events(thread).iter().filter(|e| e.kind == kind).count() >= nth).await;
        self.events(thread).into_iter().filter(|e| e.kind == kind).nth(nth - 1).unwrap()
    }

    /// A call of a tool of the thread's MCP server that the service answers, running on a task of its own until the person answers.
    fn calling(&self, thread: &str, tool: &str, args: Value) -> tokio::task::JoinHandle<Value> {
        let (runner, thread, tool) = (self.runner.clone(), thread.to_string(), tool.to_string());
        tokio::spawn(async move { runner.call(&thread, &tool, &args).await })
    }
}

fn text_of(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn code_of(answer: &Value) -> Value {
    answer["error"]["code"].clone()
}

#[tokio::test]
async fn given_a_turn_that_asks_to_run_a_tool_when_the_person_answers_then_the_waiting_call_gets_the_answer_and_the_log_says_who_asked_and_what_was_answered() {
    // Given a running turn and its permission prompt, asking to write a file
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script("claude", script(&[said("working")], true));
    let turn = rig.start("t", "make a note").await;
    let args = json!({"tool_name": "write_file", "input": {"path": "a.txt", "content": "x"}});
    let waiting = rig.calling("t", "approval_prompt", args.clone());
    let asked = rig.open_request("t", "approval.requested", 1).await;
    // Then the request is an event of the running turn naming the tool and what it is called with, and the call has not returned
    assert_eq!((asked.turn.as_deref(), asked.data["tool"].clone(), asked.data["args"].clone()), (Some(turn.as_str()), json!("write_file"), args["input"].clone()));
    assert!(!waiting.is_finished());
    // When the person accepts
    let approval = asked.data["approval"].as_str().unwrap().to_string();
    let answered = rig.ok("approval.answer", json!({"thread": "t", "approval": approval, "decision": "accept"})).await;
    // Then the answer says what was decided and the seq of the event that recorded it, and the call returns the CLI's allow with the input it asked about
    let recorded = rig.events("t").into_iter().find(|e| e.kind == "approval.answered").unwrap();
    assert_eq!(answered, json!({"thread": "t", "approval": approval, "decision": "accept", "seq": recorded.seq}));
    assert_eq!(recorded.turn.as_deref(), Some(turn.as_str()));
    assert_eq!(text_of(&waiting.await.unwrap()), json!({"behavior": "allow", "updatedInput": args["input"]}));
    // And the same request cannot be answered twice, and an approval is not a question
    assert_eq!(code_of(&rig.ask("approval.answer", json!({"thread": "t", "approval": approval, "decision": "decline"})).await), json!("REQUEST_NOT_OPEN"));
    rig.runner.interrupt("t", None);
}

#[tokio::test]
async fn given_decisions_other_than_accept_when_a_tool_is_asked_about_then_session_acceptance_allows_the_next_without_asking_and_decline_and_cancel_deny() {
    // Given a running turn
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script("claude", script(&[], true));
    rig.start("t", "work").await;
    let ask = |tool: &str| rig.calling("t", "approval_prompt", json!({"tool_name": tool, "input": {}}));
    // When the person accepts a tool for the session, and the turn asks about it again, and about another tool
    let first = ask("run_command");
    let id = rig.open_request("t", "approval.requested", 1).await.data["approval"].as_str().unwrap().to_string();
    rig.ok("approval.answer", json!({"thread": "t", "approval": id, "decision": "accept_for_session"})).await;
    assert_eq!(text_of(&first.await.unwrap())["behavior"], json!("allow"));
    let again = text_of(&ask("run_command").await.unwrap());
    // Then the second ask of the tool is allowed with no new request
    assert_eq!(again["behavior"], json!("allow"));
    assert_eq!(rig.kinds("t").iter().filter(|k| *k == "approval.requested").count(), 1);
    // And a decline of another tool is denied with the reason, and the turn goes on
    let declined = ask("write_file");
    let id = rig.open_request("t", "approval.requested", 2).await.data["approval"].as_str().unwrap().to_string();
    rig.ok("approval.answer", json!({"thread": "t", "approval": id, "decision": "decline"})).await;
    let denied = text_of(&declined.await.unwrap());
    assert_eq!((denied["behavior"].clone(), denied.get("interrupt").cloned()), (json!("deny"), None));
    assert!(rig.engine.get("t").unwrap().unwrap().turn.is_some());
    // And a cancel is denied with the turn's end, which stops the CLI and finishes the turn as interrupted
    let cancelled = ask("edit_file");
    let id = rig.open_request("t", "approval.requested", 3).await.data["approval"].as_str().unwrap().to_string();
    rig.ok("approval.answer", json!({"thread": "t", "approval": id, "decision": "cancel"})).await;
    assert_eq!(text_of(&cancelled.await.unwrap())["interrupt"], json!(true));
    rig.idle("t").await;
    assert_eq!(rig.events("t").last().unwrap().data["reason"], json!("interrupted"));
    assert_eq!(rig.spawner.kills(), 1);
}

#[tokio::test]
async fn given_a_question_a_turn_asks_when_it_is_answered_dismissed_or_the_turn_ends_then_the_tool_returns_the_text_or_says_it_was_closed_and_a_wrong_kind_is_refused() {
    // Given a running turn that asks two questions and an approval
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script("claude", script(&[], true));
    rig.start("t", "work").await;
    let first = rig.calling("t", "ask_user", json!({"question": "Which?", "options": ["a", "b"]}));
    let asked = rig.open_request("t", "input.requested", 1).await;
    // Then the question is an event with its prompt and options
    assert_eq!((asked.data["prompt"].clone(), asked.data["options"].clone()), (json!("Which?"), json!(["a", "b"])));
    // When it is answered
    let request = asked.data["request"].as_str().unwrap().to_string();
    let answered = rig.ok("input.answer", json!({"thread": "t", "request": request, "answer": "the second"})).await;
    // Then the tool returns the text, and the answer names the request
    assert_eq!(first.await.unwrap()["content"][0]["text"], json!("the second"));
    assert_eq!((answered["request"].clone(), answered["thread"].clone()), (json!(request), json!("t")));
    // When a second question is dismissed
    let second = rig.calling("t", "ask_user", json!({"question": "And?"}));
    let request = rig.open_request("t", "input.requested", 2).await.data["request"].as_str().unwrap().to_string();
    rig.ok("input.dismiss", json!({"thread": "t", "request": request})).await;
    // Then the tool says it was closed, not answered
    assert!(second.await.unwrap()["content"][0]["text"].as_str().unwrap().contains("without answering"));
    // And an approval cannot be dismissed or answered as a question, and a question cannot be answered as an approval
    let approval = rig.calling("t", "approval_prompt", json!({"tool_name": "run_command", "input": {}}));
    let id = rig.open_request("t", "approval.requested", 1).await.data["approval"].as_str().unwrap().to_string();
    let wrong = [rig.ask("input.dismiss", json!({"thread": "t", "request": id})).await, rig.ask("input.answer", json!({"thread": "t", "request": id, "answer": "x"})).await, rig.ask("approval.answer", json!({"thread": "t", "approval": request, "decision": "accept"})).await];
    assert_eq!(wrong.iter().map(|a| a["error"]["code"].clone()).collect::<Vec<_>>(), [json!("WRONG_REQUEST_KIND"), json!("WRONG_REQUEST_KIND"), json!("REQUEST_NOT_OPEN")]);
    // When the turn is interrupted while the approval waits
    rig.ok("turn.interrupt", json!({"thread": "t"})).await;
    // Then the waiting call is told the turn ended, and nothing is left open to answer
    assert!(text_of(&approval.await.unwrap())["message"].as_str().unwrap().contains("ended before"));
    assert_eq!(code_of(&rig.ask("approval.answer", json!({"thread": "t", "approval": id, "decision": "accept"})).await), json!("REQUEST_NOT_OPEN"));
}

#[tokio::test]
async fn given_a_queue_when_prompts_are_added_edited_moved_and_deleted_then_the_queue_is_the_order_the_calls_made_and_each_refusal_names_what_it_refused() {
    // Given a thread with a turn running and three prompts queued, one of them added twice
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script("claude", script(&[], true));
    rig.start("t", "work").await;
    let mut added = Vec::new();
    for (item, prompt) in [("q1", "one"), ("q2", "two"), ("q3", "three"), ("q1", "one again")] {
        added.push(rig.ok("queue.add", json!({"thread": "t", "item": item, "prompt": prompt})).await);
    }
    // Then the positions are the order of the adds, the repeat answers the item as it was queued, and records nothing
    assert_eq!(added.iter().map(|a| a["position"].clone()).collect::<Vec<_>>(), [json!(1), json!(2), json!(3), json!(1)]);
    assert_eq!(added[3]["item"]["prompt"], json!("one"));
    assert_eq!(rig.kinds("t").iter().filter(|k| *k == "queue.added").count(), 3);
    // When q1 is edited, q3 goes before q1, and q2 is deleted
    rig.ok("queue.edit", json!({"thread": "t", "item": "q1", "prompt": "better"})).await;
    let moved = rig.ok("queue.move", json!({"thread": "t", "item": "q3", "before": "q1"})).await;
    let deleted = rig.ok("queue.delete", json!({"thread": "t", "item": "q2"})).await;
    // Then the move says the order, and the list is that order with the edit and the hold off
    assert_eq!(moved["order"], json!(["q3", "q1", "q2"]));
    assert_eq!(deleted["item"], json!("q2"));
    let listed = rig.ok("queue.list", json!({"thread": "t"})).await;
    assert_eq!((listed["held"].clone(), listed["items"].as_array().unwrap().iter().map(|i| (i["item"].clone(), i["prompt"].clone())).collect::<Vec<_>>()), (json!(false), vec![(json!("q3"), json!("three")), (json!("q1"), json!("better"))]));
    // And the refusals are the contract's: an item that is not queued, a target that is not, attachments, a thread nobody made, and a thread with no defaults
    rig.ok("thread.create", json!({"thread": "bare"})).await;
    let refused = [
        rig.ask("queue.edit", json!({"thread": "t", "item": "ghost", "prompt": "x"})).await,
        rig.ask("queue.move", json!({"thread": "t", "item": "q1", "before": "ghost"})).await,
        rig.ask("queue.add", json!({"thread": "t", "prompt": "x", "attachments": ["a1"]})).await,
        rig.ask("queue.list", json!({"thread": "nope"})).await,
        rig.ask("queue.add", json!({"thread": "bare", "prompt": "x"})).await,
    ];
    assert_eq!(refused.iter().map(code_of).collect::<Vec<_>>(), [json!("ITEM_NOT_QUEUED"), json!("ITEM_NOT_QUEUED"), json!("INVALID_PARAMS"), json!("THREAD_NOT_FOUND"), json!("NO_DEFAULTS")]);
    assert_eq!(refused[1]["error"]["message"], json!("Queue target ghost is not queued."));
    rig.runner.interrupt("t", None);
}

#[tokio::test]
async fn given_prompts_queued_behind_a_running_turn_when_it_ends_then_each_starts_by_itself_as_a_turn_of_its_own_id_in_order_and_the_queue_is_empty() {
    // Given a turn that stays up, then two that finish, and two prompts queued behind the first
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script_each("claude", vec![script(&[], true), script(&[said("one done")], false), script(&[said("two done")], false)]);
    let first = rig.start("t", "work").await;
    rig.ok("queue.add", json!({"thread": "t", "item": "q1", "prompt": "first queued"})).await;
    rig.ok("queue.add", json!({"thread": "t", "item": "q2", "prompt": "second queued", "model": "small"})).await;
    // When the running turn ends
    rig.ok("turn.interrupt", json!({"thread": "t"})).await;
    rig.until("both queued turns finished", |r| r.kinds("t").iter().filter(|k| *k == "turn.finished").count() == 3).await;
    rig.idle("t").await;
    // Then the turns that started are the first, q1 and q2, in that order, and each queued turn began with the queue's own event
    let events = rig.events("t");
    let started: Vec<&str> = events.iter().filter(|e| e.kind == "turn.started").map(|e| e.turn.as_deref().unwrap()).collect();
    assert_eq!(started, [first.as_str(), "q1", "q2"]);
    let position = |kind: &str, turn: &str| events.iter().position(|e| e.kind == kind && e.turn.as_deref() == Some(turn)).unwrap();
    assert!(position("queue.started", "q1") < position("turn.started", "q1") && position("queue.started", "q2") < position("turn.started", "q2"));
    // And each ran the prompt it was queued with, the second with the model it named, the replies are theirs, and the queue is empty
    let spawned = rig.spawner.spawned();
    assert_eq!(spawned.len(), 3);
    assert!(spawned[1].args.contains(&"first queued".to_string()) && spawned[2].args.contains(&"second queued".to_string()) && spawned[2].args.windows(2).any(|w| w == ["--model", "small"]));
    assert!(events.iter().any(|e| e.kind == "message.delta" && e.turn.as_deref() == Some("q2") && e.data["text"] == json!("two done")));
    assert_eq!(events.iter().filter(|e| e.kind == "turn.finished").map(|e| e.data["reason"].clone()).collect::<Vec<_>>(), [json!("interrupted"), json!("completed"), json!("completed")]);
    assert_eq!(rig.ok("queue.list", json!({"thread": "t"})).await["items"], json!([]));
}

#[tokio::test]
async fn given_a_forced_stop_when_the_turn_ends_then_its_queue_is_held_until_resumed_and_a_resume_names_the_turn_it_started() {
    // Given a turn that stays up with a prompt queued behind it, and a second run that finishes
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script_each("claude", vec![script(&[said("stuck")], true), script(&[said("resumed")], false)]);
    let turn = rig.start("t", "work").await;
    rig.ok("queue.add", json!({"thread": "t", "item": "q1", "prompt": "after"})).await;
    // When a forced stop names another turn, then the running one
    let late = rig.ask("thread.forceStop", json!({"thread": "t", "turn": "turn-0"})).await;
    let stopped = rig.ok("thread.forceStop", json!({"thread": "t", "turn": turn})).await;
    rig.idle("t").await;
    // Then the first is refused as superseded and nothing was killed, the second killed the process and held the queue, and the turn ended interrupted
    assert_eq!(code_of(&late), json!("SUPERSEDED"));
    assert_eq!((stopped["turn"].clone(), stopped["outcome"].clone(), stopped["shared_with"].clone()), (json!(turn), json!("killed"), json!([])));
    assert_eq!(rig.spawner.kills(), 1);
    assert_eq!(rig.kinds("t").iter().filter(|k| k.starts_with("thread.force") || *k == "queue.held").count(), 2);
    assert_eq!(rig.events("t").iter().rev().find(|e| e.kind == "turn.finished").unwrap().data["reason"], json!("interrupted"));
    // And the queued prompt did not start by itself, and the list says the queue is held
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(rig.spawner.spawned().len(), 1);
    assert_eq!(rig.ok("queue.list", json!({"thread": "t"})).await["held"], json!(true));
    // When the queue is resumed
    let resumed = rig.ok("queue.resume", json!({"thread": "t"})).await;
    rig.until("the queued turn finished", |r| r.kinds("t").iter().filter(|k| *k == "turn.finished").count() == 2).await;
    // Then it names the turn it started, which is the queued item's id, and the CLI ran its prompt
    assert_eq!((resumed["held"].clone(), resumed["started"].clone()), (json!(false), json!("q1")));
    assert!(rig.spawner.spawned()[1].args.contains(&"after".to_string()));
    // And a prompt sent now while a turn runs is refused, and so is a resume of a thread that is gone
    rig.spawner.script("claude", script(&[], true));
    rig.start("t", "again").await;
    rig.ok("queue.add", json!({"thread": "t", "item": "q2", "prompt": "later"})).await;
    assert_eq!(rig.ask("queue.resume", json!({"thread": "t", "item": "q2"})).await["error"]["message"], json!("A queued user message can only be sent now while the thread is stopped."));
    rig.ok("thread.delete", json!({"thread": "t"})).await;
    assert_eq!(rig.ask("queue.resume", json!({"thread": "t"})).await["error"]["message"], json!("Thread t is not active."));
}

#[tokio::test]
async fn given_a_running_turn_when_it_is_steered_then_its_cli_is_ended_and_another_runs_the_steer_prompt_in_the_same_turn_and_the_thread_ends_once() {
    // Given a turn whose CLI announces its conversation and stays up, a second CLI that finishes, and a prompt in the thread's queue
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script_each("claude", vec![script(&[init("s-1"), said("going right")], true), script(&[said("going left")], false)]);
    let turn = rig.start("t", "go right").await;
    rig.says("t", "going right").await;
    rig.ok("queue.add", json!({"thread": "t", "item": "q1", "prompt": "go left"})).await;
    // When it is steered naming both a prompt and an item, then with the queued item
    let both = rig.ask("turn.steer", json!({"thread": "t", "prompt": "x", "item": "q1"})).await;
    let steered = rig.ok("turn.steer", json!({"thread": "t", "item": "q1"})).await;
    rig.idle("t").await;
    // Then the first is refused, the answer names the turn that goes on, and the item left the queue to be the steer
    assert_eq!(code_of(&both), json!("INVALID_PARAMS"));
    assert_eq!(steered["turn"], json!(turn));
    assert_eq!(rig.ok("queue.list", json!({"thread": "t"})).await["items"], json!([]));
    // And the first process was killed and the second ran the steer prompt in the conversation the first announced
    let spawned = rig.spawner.spawned();
    assert_eq!((rig.spawner.kills(), spawned.len()), (1, 2));
    assert!(spawned[1].args.contains(&"go left".to_string()) && spawned[1].args.windows(2).any(|w| w == ["--resume", "s-1"]), "{:?}", spawned[1].args);
    // And the log has one start and one finish of the turn, with the steer between what the two processes said, and the turn completed
    let events = rig.events("t");
    assert_eq!(events.iter().filter(|e| e.kind == "turn.started").count(), 1);
    let finishes: Vec<&Event> = events.iter().filter(|e| e.kind == "turn.finished").collect();
    assert_eq!((finishes.len(), finishes[0].data["reason"].clone(), finishes[0].turn.as_deref()), (1, json!("completed"), Some(turn.as_str())));
    let order: Vec<String> = events.iter().filter(|e| e.kind == "turn.steered" || (e.kind == "message.delta" && e.data["role"] == json!("assistant"))).map(|e| if e.kind == "turn.steered" { format!("steered {}", e.data["prompt"].as_str().unwrap()) } else { e.data["text"].as_str().unwrap().to_string() }).collect();
    assert_eq!(order, ["going right", "steered go left", "going left"]);
    // And a steer of a thread that runs nothing is refused
    assert_eq!(code_of(&rig.ask("turn.steer", json!({"thread": "t", "prompt": "now"})).await), json!("NO_TURN"));
}

impl Rig {
    /// Waits until the thread's log holds an assistant message with this text.
    async fn says(&self, thread: &str, text: &str) {
        self.until(&format!("{thread} says {text}"), |r| r.events(thread).iter().any(|e| e.kind == "message.delta" && e.data["text"] == json!(text))).await;
    }
}

#[tokio::test]
async fn given_a_thread_launched_and_one_that_cannot_start_when_launch_is_called_then_the_first_has_its_turn_and_the_second_leaves_no_thread_behind() {
    // Given a CLI that says something
    let rig = rig();
    rig.spawner.script("claude", script(&[said("hello")], false));
    // When a thread is launched with a prompt, again with its id, with a driver nobody runs, and in a workspace with no executor
    let launched = rig.ok("thread.launch", json!({"thread": "l1", "title": "Launched", "workspace": "w1", "driver": "claudeAgent", "prompt": "begin"})).await;
    rig.idle("l1").await;
    let again = rig.ask("thread.launch", json!({"thread": "l1", "workspace": "w1", "driver": "claudeAgent", "prompt": "again"})).await;
    let driver = rig.ask("thread.launch", json!({"thread": "l2", "workspace": "w1", "driver": "nobody", "prompt": "x"})).await;
    let away = rig.ask("thread.launch", json!({"thread": "l3", "workspace": "w9", "driver": "claudeAgent", "prompt": "x"})).await;
    // Then the first is a thread with a turn that ran the prompt, the others are refused with their codes, and the failed ones left no thread
    assert_eq!((launched["thread"]["id"].clone(), launched["thread"]["title"].clone(), launched["turn"].is_string()), (json!("l1"), json!("Launched"), true));
    assert!(rig.kinds("l1").contains(&"turn.finished".to_string()));
    assert_eq!([code_of(&again), code_of(&driver), code_of(&away)], [json!("THREAD_EXISTS"), json!("UNKNOWN_DRIVER"), json!("NO_EXECUTOR")]);
    assert!(rig.engine.get("l2").unwrap().is_none() && rig.engine.get("l3").unwrap().is_none());
    assert_eq!(rig.spawner.spawned().len(), 1);
}

#[tokio::test]
async fn given_threads_with_turns_when_forked_searched_and_listed_by_family_then_the_fork_holds_the_finished_turns_and_the_family_and_search_find_what_they_should() {
    // Given a parent with two finished turns, in a workspace
    let rig = rig();
    rig.create("parent").await;
    rig.spawner.script_each("claude", vec![script(&[said("alpha reply")], false), script(&[said("beta reply")], false), script(&[said("fork reply")], false)]);
    let first = rig.start("parent", "alpha question").await;
    rig.idle("parent").await;
    rig.start("parent", "beta question").await;
    rig.idle("parent").await;
    // When it is forked at its first turn, and the fork is asked for a fork of a thread nobody made and one that exists
    let forked = rig.ok("thread.fork", json!({"thread": "parent", "fork": "zkid", "turn": first})).await;
    let nobody = rig.ask("thread.fork", json!({"thread": "ghost"})).await;
    let exists = rig.ask("thread.fork", json!({"thread": "parent", "fork": "zkid"})).await;
    // Then the fork holds the first turn only, is titled as a fork of the parent, and says where it came from
    assert_eq!((forked["source"].clone(), forked["turns"].clone(), forked["thread"]["thread"]["title"].clone()), (json!("parent"), json!([first]), json!("title of parent fork")));
    assert_eq!((forked["thread"]["lineage"]["parent"].clone(), forked["thread"]["lineage"]["root"].clone()), (json!("parent"), json!("parent")));
    assert_eq!([code_of(&nobody), code_of(&exists)], [json!("THREAD_NOT_FOUND"), json!("THREAD_EXISTS")]);
    let copied: Vec<String> = rig.events("zkid").iter().filter(|e| e.kind == "message.delta").map(|e| e.data["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(copied, ["alpha question", "alpha reply"]);
    // And the family of either has the root and both live threads, oldest first
    for member in ["parent", "zkid"] {
        let family = rig.ok("thread.family", json!({"thread": member})).await;
        assert_eq!((family["root"].clone(), family["threads"].as_array().unwrap().iter().map(|t| t["thread"]["id"].clone()).collect::<Vec<_>>()), (json!("parent"), vec![json!("parent"), json!("zkid")]), "{member}");
    }
    // And a search finds the parent by what was said in it, the title first when the words are in a title, and nothing for words nobody said
    let by_reply = rig.ok("thread.search", json!({"query": "beta reply"})).await;
    assert_eq!(by_reply["matches"].as_array().unwrap().iter().map(|m| (m["thread"].clone(), m["source"].clone())).collect::<Vec<_>>(), [(json!("parent"), json!("assistant"))]);
    let by_title = rig.ok("thread.search", json!({"query": "title parent"})).await;
    assert_eq!(by_title["matches"][0]["source"], json!("title"));
    assert_eq!(rig.ok("thread.search", json!({"query": "zeppelin"})).await["matches"], json!([]));
    // When the fork finishes a turn of its own and is merged back, and the parent's next prompt is sent
    rig.start("zkid", "gamma question").await;
    rig.idle("zkid").await;
    let merged = rig.ok("thread.mergeBack", json!({"thread": "zkid", "into": "parent"})).await;
    assert_eq!((merged["source"].clone(), merged["thread"]["pending_merge_back"].clone()), (json!("zkid"), json!("zkid")));
    rig.start("parent", "carry on").await;
    rig.idle("parent").await;
    // Then the CLI was given the fork's work as context before the prompt, the log says the context was taken, and nothing waits
    let spawned = rig.spawner.spawned();
    let given = spawned.last().unwrap().args.iter().find(|a| a.contains("carry on")).unwrap();
    assert!(given.starts_with("Context carried over from the side thread zkid") && given.contains("gamma question") && given.ends_with("carry on"), "{given}");
    assert!(rig.kinds("parent").contains(&"thread.merge_consumed".to_string()));
    assert_eq!(rig.ok("thread.family", json!({"thread": "parent"})).await["threads"][0]["pending_merge_back"], json!(null));
}

#[tokio::test]
async fn given_threads_when_settled_snoozed_archived_and_deleted_then_each_state_is_what_the_call_set_the_shelves_page_and_a_delete_ends_what_runs() {
    // Given three idle threads with a turn each, and one running
    let rig = rig();
    rig.spawner.script_each("claude", vec![script(&[said("done")], false), script(&[said("done")], false), script(&[said("done")], false), script(&[], true)]);
    for thread in ["a", "b", "c"] {
        rig.create(thread).await;
        rig.start(thread, "work").await;
        rig.idle(thread).await;
    }
    rig.create("busy").await;
    rig.start("busy", "work").await;
    // When a is settled, b is snoozed (and again with a time that is past), c is archived twice
    let settled = rig.ok("thread.settle", json!({"thread": "a", "state": "settled"})).await;
    let snoozed = rig.ok("thread.settle", json!({"thread": "b", "state": "snoozed", "until_ms": NOW + 60_000})).await;
    let past = rig.ask("thread.settle", json!({"thread": "b", "state": "snoozed", "until_ms": 5})).await;
    let archived = rig.ok("thread.archive", json!({"thread": "c"})).await;
    let twice = rig.ask("thread.archive", json!({"thread": "c"})).await;
    // Then the states are those asked for, with the contract's refusals
    assert_eq!((settled["thread"]["settle"]["state"].clone(), settled["thread"]["settle"]["settled_at_ms"].clone()), (json!("settled"), json!(NOW)));
    assert_eq!(snoozed["thread"]["settle"]["snoozed_until_ms"], json!(NOW + 60_000));
    assert_eq!((code_of(&past), code_of(&twice)), (json!("SNOOZE_NOT_FUTURE"), json!("ALREADY_ARCHIVED")));
    assert_eq!(archived["thread"]["archived_at_ms"], json!(NOW));
    // And the shelves hold the archived and the settled thread, a page of one at a time, with the count and the cursor of the next
    let shelf = rig.ok("thread.shelf", json!({"shelf": "archived", "limit": 1})).await;
    assert_eq!((shelf["threads"][0]["thread"]["id"].clone(), shelf["total"].clone(), shelf.get("next").cloned()), (json!("c"), json!(1), None));
    let settled_shelf = rig.ok("thread.shelf", json!({"shelf": "settled"})).await;
    assert_eq!(settled_shelf["threads"].as_array().unwrap().iter().map(|t| t["thread"]["id"].clone()).collect::<Vec<_>>(), [json!("a")]);
    assert_eq!(rig.ok("thread.shelf", json!({"shelf": "settled", "limit": 0})).await["threads"], json!([]));
    // When a used thread is deleted only if unused, then outright, and the running thread is deleted
    let kept = rig.ok("thread.delete", json!({"thread": "a", "only_if_unused": true})).await;
    let gone = rig.ok("thread.delete", json!({"thread": "a"})).await;
    let killed = rig.ok("thread.delete", json!({"thread": "busy"})).await;
    // Then the first kept the thread, the second deleted it and left it out of the lists, and the third ended the turn that ran
    assert_eq!((kept["deleted"].clone(), kept.get("seq").cloned()), (json!(false), None));
    assert_eq!(gone["deleted"], json!(true));
    assert_eq!(killed["deleted"], json!(true));
    rig.idle("busy").await;
    assert_eq!(rig.spawner.kills(), 1);
    assert!(rig.ok("thread.list", json!({})).await["threads"].as_array().unwrap().iter().all(|t| t["id"] != json!("a") && t["id"] != json!("busy")));
    assert_eq!(rig.ok("thread.shelf", json!({"shelf": "settled"})).await["threads"], json!([]));
}

#[tokio::test]
async fn given_a_failed_turn_when_it_is_retried_and_rolled_back_then_the_retry_runs_the_same_prompt_as_a_new_turn_and_a_rollback_that_restores_files_asks_the_executor_for_them_from_the_checkpoint_of_the_first_dropped_turn() {
    // Given a turn that fails and a CLI that then succeeds
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script_each("claude", vec![CliScript { exit: Some("exited with 1: not logged in".into()), ..CliScript::default() }, script(&[said("fine")], false)]);
    let failed = rig.start("t", "do the thing").await;
    rig.idle("t").await;
    // When the turn is retried, and the retried one again (it completed)
    let retried = rig.ok("turn.retry", json!({"thread": "t", "turn": failed})).await;
    rig.idle("t").await;
    let again = rig.ask("turn.retry", json!({"thread": "t", "turn": failed})).await;
    // Then the retry is a new turn that names the failed one and ran its prompt, and the second is not retryable
    assert_eq!(retried["retry_of"], json!(failed));
    let new_turn = retried["turn"].as_str().unwrap();
    assert_ne!(new_turn, failed);
    assert!(rig.spawner.spawned()[1].args.contains(&"do the thing".to_string()));
    assert!(rig.events("t").iter().any(|e| e.kind == "turn.finished" && e.turn.as_deref() == Some(new_turn) && e.data["reason"] == json!("completed")));
    assert_eq!(code_of(&again), json!("NOT_RETRYABLE"));
    // When the thread is rolled back to the failed turn while asking for the files back, and again without
    let restoring = rig.ok("thread.rollback", json!({"thread": "t", "turn": failed})).await;
    let leaving = rig.ok("thread.rollback", json!({"thread": "t", "turn": failed, "restore_files": false})).await;
    // Then the first drops the retry and restores the files from the checkpoint taken as the retry started, and the second drops nothing more
    assert_eq!((restoring["dropped"].clone(), restoring["restored_files"].clone()), (json!([new_turn]), json!(true)));
    let asked = rig.asked.lock().unwrap().clone();
    let discard = asked.iter().find(|(tool, _)| tool == "git_discard").expect("the files were asked back").1.clone();
    assert_eq!((discard["source"].clone(), discard["paths"].clone(), discard["untracked"].clone()), (json!(format!("refs/rca/checkpoints/t/{new_turn}")), json!(["."]), json!(true)));
    assert_eq!(asked.iter().filter(|(tool, _)| tool == "git_checkpoint").count(), 2, "each turn was checkpointed as it started");
    assert_eq!((leaving["dropped"].clone(), leaving["restored_files"].clone()), (json!([]), json!(false)));
    assert_eq!(code_of(&rig.ask("thread.rollback", json!({"thread": "nope", "turn": "x"})).await), json!("THREAD_NOT_FOUND"));
}

#[tokio::test]
async fn given_a_thread_whose_cli_announced_a_conversation_when_the_next_turn_runs_and_then_it_is_detached_then_the_turn_resumes_it_and_the_one_after_the_detach_does_not() {
    // Given a CLI that announces conversation s-7 in every turn
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script("claude", script(&[init("s-7"), said("hi")], false));
    rig.start("t", "first").await;
    rig.idle("t").await;
    rig.until("the session is kept", |r| r.engine.provider_session("t").unwrap().is_some()).await;
    // When the next turn runs
    rig.start("t", "second").await;
    rig.idle("t").await;
    // Then it resumed the conversation and the first did not
    let spawned = rig.spawner.spawned();
    assert!(!spawned[0].args.contains(&"--resume".to_string()) && spawned[1].args.windows(2).any(|w| w == ["--resume", "s-7"]), "{:?}", spawned.iter().map(|s| s.args.clone()).collect::<Vec<_>>());
    // When another conversation is detached, then the thread's own, then it again
    let other = rig.ask("thread.detach", json!({"thread": "t", "session": "s-9"})).await;
    let detached = rig.ok("thread.detach", json!({"thread": "t", "session": "s-7", "reason": "stale"})).await;
    let twice = rig.ask("thread.detach", json!({"thread": "t", "session": "s-7"})).await;
    // Then only its own is detached, with the contract's refusal for the others
    assert_eq!((code_of(&other), code_of(&twice)), (json!("SESSION_NOT_OWNED"), json!("SESSION_NOT_OWNED")));
    assert_eq!((detached["thread"].clone(), detached["session"].clone()), (json!("t"), json!("s-7")));
    // And the next turn starts without resuming (it announces s-7 again and the thread keeps it for the one after)
    rig.start("t", "third").await;
    rig.idle("t").await;
    assert!(!rig.spawner.spawned()[2].args.contains(&"--resume".to_string()));
    assert_eq!(rig.engine.provider_session("t").unwrap().as_deref(), Some("s-7"));
}

#[tokio::test]
async fn given_a_turn_with_a_workflow_and_a_transcript_when_the_workflow_is_stopped_and_an_item_is_read_then_the_turn_ends_interrupted_and_the_item_is_the_row_of_that_seq() {
    // Given a turn that says something and stays up
    let rig = rig();
    rig.create("t").await;
    rig.spawner.script("claude", script(&[said("working on it")], true));
    let turn = rig.start("t", "work").await;
    rig.says("t", "working on it").await;
    // When a workflow of another turn is stopped, then of the running turn naming the workflow
    let wrong = rig.ask("workflow.stop", json!({"thread": "t", "turn": "turn-0"})).await;
    let stopped = rig.ok("workflow.stop", json!({"thread": "t", "turn": turn, "workflow": "agent-7"})).await;
    rig.idle("t").await;
    // Then the first is not a running turn, and the second ended the turn's process and the turn, with the stop in the log before the finish
    assert_eq!(code_of(&wrong), json!("NO_TURN"));
    assert_eq!(stopped["turn"], json!(turn));
    assert_eq!(rig.spawner.kills(), 1);
    let kinds = rig.kinds("t");
    assert!(kinds.iter().position(|k| k == "workflow.stopped").unwrap() < kinds.iter().position(|k| k == "turn.finished").unwrap());
    assert_eq!(rig.events("t").last().unwrap().data["reason"], json!("interrupted"));
    // And an item is the transcript row the event at its seq opened, and a seq that opened none is ITEM_NOT_FOUND
    let transcript = rig.ok("thread.transcript", json!({"thread": "t"})).await;
    let reply = transcript["items"].as_array().unwrap().iter().find(|i| i["role"] == json!("assistant")).unwrap().clone();
    let item = rig.ok("item.get", json!({"thread": "t", "seq": reply["seq"]})).await;
    assert_eq!((item["item"].clone(), item["thread"].clone()), (reply, json!("t")));
    let missing = rig.ask("item.get", json!({"thread": "t", "seq": 1})).await;
    assert_eq!((code_of(&missing), missing["error"]["message"].clone()), (json!("ITEM_NOT_FOUND"), json!("thread t has no item opened at seq 1")));
}
