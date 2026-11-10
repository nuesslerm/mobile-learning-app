//! `agent.queue` end to end over the agent socket, with the real `rca-exec` serving a temporary repository and the scripted CLI standing in for
//! `claude`: prompts queued behind a running turn run by themselves when it ends, each as a turn of its own and each writing its file through the
//! executor; a steer ends the CLI and runs another in the same turn; a forced stop holds the queue until it is resumed. Each flow also shows what a
//! mapper that follows the thread is sent.
use crate::agent_threads::{chain, create, kinds, said, seqs, start_turn};
use crate::harness::{eventually, Env};
use agent_api_proto::Event;
use exec_testkit::script::Turn;
use serde_json::{json, Value};
use std::time::Duration;

fn write(path: &str, content: &str) -> Value {
    json!({"path": path, "content": content, "create_dirs": true})
}

async fn until_idle(agent: &mut crate::mapper::Agent, thread: &str) {
    for _ in 0..300 {
        if agent.ask("thread.get", json!({"thread": thread})).await["result"]["thread"]["status"] == json!("idle") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{thread} never became idle");
}

fn turns_started(events: &[Event]) -> Vec<String> {
    events.iter().filter(|e| e.kind == "turn.started").map(|e| e.turn.clone().unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_prompts_queued_behind_a_running_turn_when_it_ends_then_each_runs_by_itself_through_the_executor_in_the_order_the_calls_left_it_and_the_follower_saw_each_change() {
    // Given a first turn that writes a file and waits, and two queued turns that each write a file, a mapper following the thread
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![
        Turn::replaying(&[said("first")]).then_calling("k1", "write_file", write("notes/first.txt", "1")).then_sleeping(1500),
        Turn::replaying(&[said("second")]).then_calling("k2", "write_file", write("notes/second.txt", "2")),
        Turn::replaying(&[said("third")]).then_calling("k3", "write_file", write("notes/third.txt", "3")),
        Turn::replaying(&[said("never")]).then_calling("k4", "write_file", write("notes/never.txt", "4")),
    ]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let first = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    // When three prompts are queued behind it (one added twice), one is edited, one goes first, and one is deleted
    let added: Vec<Value> = {
        let mut all = Vec::new();
        for (item, prompt) in [("q1", "third"), ("q2", "second"), ("q3", "never"), ("q1", "third again")] {
            all.push(agent.ask("queue.add", json!({"thread": "t1", "item": item, "prompt": prompt})).await);
        }
        all
    };
    agent.ask("queue.edit", json!({"thread": "t1", "item": "q2", "prompt": "second, edited"})).await;
    let moved = agent.ask("queue.move", json!({"thread": "t1", "item": "q2", "before": "q1"})).await;
    let deleted = agent.ask("queue.delete", json!({"thread": "t1", "item": "q3"})).await;
    let listed = agent.ask("queue.list", json!({"thread": "t1"})).await;
    // Then the adds answered their positions (the repeat the first item as it was), the move the order, the list that order with the edit, held off
    assert_eq!(added.iter().map(|a| a["result"]["position"].clone()).collect::<Vec<_>>(), [json!(1), json!(2), json!(3), json!(1)]);
    assert_eq!(added[3]["result"]["item"]["prompt"], json!("third"));
    assert_eq!(moved["result"]["order"], json!(["q2", "q1", "q3"]));
    assert_eq!(deleted["result"]["item"], json!("q3"));
    assert_eq!((listed["result"]["held"].clone(), listed["result"]["items"].as_array().unwrap().iter().map(|i| i["prompt"].clone()).collect::<Vec<_>>()), (json!(false), vec![json!("second, edited"), json!("third")]));
    // When the running turn ends by itself, and the queued turns run
    let mut seen: Vec<Event> = Vec::new();
    for _ in 0..3 {
        seen.extend(agent.events_through("turn.finished").await);
    }
    until_idle(&mut agent, "t1").await;
    // Then the files were written by the executor, in the order of the queue, and the one deleted from it never ran
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/first.txt")).unwrap(), "1");
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/second.txt")).unwrap(), "2");
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/third.txt")).unwrap(), "3");
    assert!(!chain.repo.path().join("notes/never.txt").exists());
    // And the turns that started are the first and the queued items by id, in the order the queue had, each after the queue's own start
    assert_eq!(turns_started(&seen), [first.as_str(), "q2", "q1"]);
    assert!(seen.iter().filter(|e| e.kind == "queue.started").map(|e| e.data["item"].clone()).eq([json!("q2"), json!("q1")]));
    assert!(seqs(&seen).windows(2).all(|pair| pair[0] < pair[1]));
    // And the follower was sent each change of the queue as it happened, as events of the thread
    let history = agent.ask("thread.history", json!({"thread": "t1"})).await;
    let logged: Vec<String> = history["result"]["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap().to_string()).collect();
    assert_eq!(logged.iter().filter(|k| k.starts_with("queue.")).map(String::as_str).collect::<Vec<_>>(), ["queue.added", "queue.added", "queue.added", "queue.edited", "queue.moved", "queue.deleted", "queue.started", "queue.started"]);
    assert_eq!(chain.cli.invocations("claude").len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_running_turn_when_it_is_steered_then_its_cli_is_killed_and_the_next_writes_its_file_through_the_executor_in_the_same_turn() {
    // Given a turn that says it is going right and stays up, and the CLI the steer starts, which writes a file
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![
        Turn::replaying(&[json!({"type": "system", "subtype": "init", "session_id": "s-1"}).to_string(), said("going right")]).hanging(),
        Turn::replaying(&[said("going left")]).then_calling("k1", "write_file", write("notes/left.txt", "left")),
    ]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let turn = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    let mut seen = agent.events_through("message.delta").await;
    seen.extend(agent.events_through("message.delta").await);
    // When it is steered, and a steer names neither a prompt nor an item
    let steered = agent.ask("turn.steer", json!({"thread": "t1", "turn": turn, "prompt": "go left"})).await;
    let neither = agent.ask("turn.steer", json!({"thread": "t1"})).await;
    seen.extend(agent.events_through("turn.finished").await);
    // Then the answer names the turn, the second CLI ran the steer prompt in the conversation the first announced, and its file is in the repository
    assert_eq!(steered["result"]["turn"], json!(turn));
    assert_eq!(neither["error"]["code"], json!("INVALID_PARAMS"));
    let starts = chain.cli.invocations("claude");
    assert_eq!(starts.len(), 2);
    assert!(starts[1].argv.contains(&"go left".to_string()) && starts[1].argv.windows(2).any(|w| w == ["--resume", "s-1"]), "{:?}", starts[1].argv);
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/left.txt")).unwrap(), "left");
    // And the log has one start and one finish of the turn, with the steer between the two CLIs' words, and a steer after it is refused
    let order: Vec<&str> = kinds(&seen).into_iter().filter(|k| ["turn.started", "turn.steered", "turn.finished"].contains(k)).collect();
    assert_eq!(order, ["turn.started", "turn.steered", "turn.finished"]);
    assert_eq!(seen.last().unwrap().data["reason"], json!("completed"));
    assert_eq!(agent.ask("turn.steer", json!({"thread": "t1", "prompt": "again"})).await["error"]["code"], json!("NO_TURN"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_forced_stop_when_the_turn_ends_then_the_queue_waits_until_it_is_resumed_and_the_resumed_turn_writes_its_file_through_the_executor() {
    // Given a turn that stays up with a prompt queued behind it, and the turn the queue runs after, which writes a file
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("stuck")]).hanging(), Turn::replaying(&[said("resumed")]).then_calling("k1", "write_file", write("notes/after.txt", "after"))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let turn = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    agent.ask("queue.add", json!({"thread": "t1", "item": "q1", "prompt": "after"})).await;
    // The first delta is the prompt the service records before it starts the CLI, so the wait is on the CLI's own start in its log
    agent.events_through("message.delta").await;
    eventually("the turn's CLI started", || chain.cli.invocations("claude").len() == 1).await;
    // When the turn is force stopped
    let stopped = agent.ask("thread.forceStop", json!({"thread": "t1", "turn": turn})).await;
    let ended = agent.events_through("turn.finished").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Then the process was killed, the queue is held with its prompt in it, and the prompt did not start by itself
    assert_eq!((stopped["result"]["outcome"].clone(), stopped["result"]["turn"].clone()), (json!("killed"), json!(turn)));
    assert!(kinds(&ended).contains(&"thread.force_stopped") && kinds(&ended).contains(&"queue.held"), "{:?}", kinds(&ended));
    assert_eq!(ended.last().unwrap().data["reason"], json!("interrupted"));
    assert_eq!(chain.cli.invocations("claude").len(), 1);
    let held = agent.ask("queue.list", json!({"thread": "t1"})).await;
    assert_eq!((held["result"]["held"].clone(), held["result"]["items"].as_array().unwrap().len()), (json!(true), 1));
    assert!(!chain.repo.path().join("notes/after.txt").exists());
    // When the queue is resumed
    let resumed = agent.ask("queue.resume", json!({"thread": "t1"})).await;
    let rest = agent.events_through("turn.finished").await;
    // Then the answer names the turn it started, the item's id, and the file the queued turn wrote is in the repository
    assert_eq!((resumed["result"]["held"].clone(), resumed["result"]["started"].clone()), (json!(false), json!("q1")));
    assert_eq!(rest.last().unwrap().turn.as_deref(), Some("q1"));
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/after.txt")).unwrap(), "after");
    assert!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["last_seq"].as_u64().unwrap() > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_resume_from_beyond_the_newest_event_when_the_thread_changes_then_the_new_socket_replays_nothing_and_is_sent_the_change_live() {
    // Given a thread with a queued prompt, and a mapper that knew a later seq than the thread has (the service started again over a shorter log)
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("queue.add", json!({"thread": "t1", "item": "q1", "prompt": "one"})).await;
    let newest = agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["last_seq"].as_u64().unwrap();
    // When a new socket subscribes after a seq far past the newest, and the queue then changes
    let mut late = chain.agent().await;
    let followed = late.ask("thread.subscribe", json!({"thread": "t1", "after_seq": newest + 1000})).await;
    late.ask("queue.edit", json!({"thread": "t1", "item": "q1", "prompt": "one, edited"})).await;
    // Then the answer says where the thread is, no replay came, and the first event is the edit, sent live
    assert_eq!((followed["result"]["after_seq"].clone(), followed["result"]["last_seq"].clone()), (json!(newest + 1000), json!(newest)));
    let edited = late.event().await;
    assert_eq!((edited.kind.as_str(), edited.data["prompt"].clone()), ("queue.edited", json!("one, edited")));
    assert!(edited.seq > newest);
}
