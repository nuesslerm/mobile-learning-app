//! `agent.lifecycle` end to end over the agent socket, with the real `rca-exec` and the scripted CLI: a thread launched with its first turn, forked,
//! searched, settled, archived and deleted, a workflow stopped, a failed turn retried and a conversation detached. The follower of a thread is sent
//! each change as an event of the thread's own topic.
use crate::agent_threads::{chain, create, kinds, said, start_turn};
use crate::harness::{eventually, service, Env, Options};
use crate::mapper::Agent;
use agent_api_proto::Event;
use exec_testkit::script::Turn;
use serde_json::{json, Value};
use std::time::Duration;

async fn until_idle(agent: &mut crate::mapper::Agent, thread: &str) {
    for _ in 0..300 {
        if agent.ask("thread.get", json!({"thread": thread})).await["result"]["thread"]["status"] == json!("idle") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{thread} never became idle");
}

fn write(path: &str, content: &str) -> Value {
    json!({"path": path, "content": content, "create_dirs": true})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_thread_launched_with_a_prompt_when_it_is_forked_searched_and_merged_back_then_each_answers_from_the_engine_and_the_follower_is_sent_the_events() {
    // Given a CLI that writes a file and says something, a thread launched with its first turn, and a mapper following it
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("the plan is ready")]).then_calling("k1", "write_file", write("notes/plan.txt", "plan")), Turn::replaying(&[said("the fork found a bug")]), Turn::replaying(&[said("back in the parent")])]);
    let mut agent = chain.agent().await;
    let launched = agent.ask("thread.launch", json!({"thread": "p", "title": "Parent plan", "workspace": "w1", "driver": "claudeAgent", "prompt": "make the plan"})).await;
    let first = launched["result"]["turn"].as_str().unwrap().to_string();
    agent.ask("thread.subscribe", json!({"thread": "p"})).await;
    until_idle(&mut agent, "p").await;
    // Then the launch answered the thread and the turn that began, and the file was written through the executor
    assert_eq!((launched["result"]["thread"]["id"].clone(), launched["result"]["thread"]["title"].clone()), (json!("p"), json!("Parent plan")));
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/plan.txt")).unwrap(), "plan");
    // When the thread is forked at its turn, the fork is given a turn of its own, and merged back
    let forked = agent.ask("thread.fork", json!({"thread": "p", "fork": "k", "turn": first})).await;
    agent.ask("turn.start", json!({"thread": "k", "workspace": "w1", "driver": "claudeAgent", "prompt": "look for bugs"})).await;
    until_idle(&mut agent, "k").await;
    let merged = agent.ask("thread.mergeBack", json!({"thread": "k", "into": "p"})).await;
    // Then the fork says what it holds and where it came from, and the parent says a merge waits for its next prompt
    assert_eq!((forked["result"]["turns"].clone(), forked["result"]["source"].clone(), forked["result"]["thread"]["lineage"]["root"].clone()), (json!([first]), json!("p"), json!("p")));
    assert_eq!(merged["result"]["thread"]["pending_merge_back"], json!("k"));
    // When the parent's next prompt is sent, and the family and a search are asked
    start_turn(&mut agent, "p").await;
    until_idle(&mut agent, "p").await;
    let family = agent.ask("thread.family", json!({"thread": "k"})).await;
    let found = agent.ask("thread.search", json!({"query": "bug"})).await;
    // Then the CLI got the fork's work as context, the family is the root and both threads, and the search finds the fork by what it said
    let starts = chain.cli.invocations("claude");
    assert!(starts[2].argv.iter().any(|a| a.starts_with("Context carried over from the side thread k") && a.contains("look for bugs")), "{:?}", starts[2].argv);
    assert_eq!((family["result"]["root"].clone(), family["result"]["threads"].as_array().unwrap().len()), (json!("p"), 2));
    assert_eq!(found["result"]["matches"].as_array().unwrap().iter().map(|m| m["thread"].clone()).collect::<Vec<_>>(), [json!("k")]);
    // And the follower of the parent was sent the merge and its consumption among the turn's events, each of them of the parent
    let history = agent.ask("thread.history", json!({"thread": "p"})).await;
    let logged: Vec<&str> = history["result"]["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).filter(|k| k.starts_with("thread.")).collect();
    assert_eq!(logged, ["thread.merged_back", "thread.merge_consumed"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_threads_when_settled_archived_and_deleted_then_the_shelves_and_the_follower_see_each_and_a_delete_ends_the_turn_that_runs() {
    // Given an idle thread with a turn that ran, one that runs, and a follower of the first
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("done")]), Turn::replaying(&[said("working")]).hanging()]);
    let mut agent = chain.agent().await;
    create(&mut agent, "a").await;
    agent.ask("thread.subscribe", json!({"thread": "a"})).await;
    start_turn(&mut agent, "a").await;
    until_idle(&mut agent, "a").await;
    create(&mut agent, "busy").await;
    start_turn(&mut agent, "busy").await;
    eventually("the running turn's CLI started", || chain.cli.invocations("claude").len() == 2).await;
    // When a is settled, snoozed to a time that is past, archived twice, brought back and archived again
    let settled = agent.ask("thread.settle", json!({"thread": "a", "state": "settled"})).await;
    let past = agent.ask("thread.settle", json!({"thread": "a", "state": "snoozed", "until_ms": 5})).await;
    let archived = agent.ask("thread.archive", json!({"thread": "a"})).await;
    let twice = agent.ask("thread.archive", json!({"thread": "a"})).await;
    let back = agent.ask("thread.archive", json!({"thread": "a", "archived": false})).await;
    let shelved = agent.ask("thread.shelf", json!({"shelf": "settled"})).await;
    agent.ask("thread.archive", json!({"thread": "a"})).await;
    let archive_shelf = agent.ask("thread.shelf", json!({"shelf": "archived"})).await;
    // Then the settle is recorded, the past snooze and the second archive are refused, and the shelves hold a: settled while live, archived after
    assert_eq!(settled["result"]["thread"]["settle"]["state"], json!("settled"));
    assert_eq!((past["error"]["code"].clone(), twice["error"]["code"].clone()), (json!("SNOOZE_NOT_FUTURE"), json!("ALREADY_ARCHIVED")));
    assert!(archived["result"]["thread"]["archived_at_ms"].is_u64() && back["result"]["thread"].get("archived_at_ms").is_none());
    assert_eq!(shelved["result"]["threads"][0]["thread"]["id"], json!("a"));
    assert_eq!((archive_shelf["result"]["threads"][0]["thread"]["id"].clone(), archive_shelf["result"]["total"].clone()), (json!("a"), json!(1)));
    // And the follower was sent each change as an event of a, in order
    let mut sent: Vec<Event> = Vec::new();
    while sent.last().is_none_or(|e| e.kind != "thread.archived" || sent.iter().filter(|e| e.kind == "thread.archived").count() < 2) {
        sent.push(agent.event().await);
    }
    let changes: Vec<&str> = kinds(&sent).into_iter().filter(|k| k.starts_with("thread.")).collect();
    assert_eq!(changes, ["thread.settle_changed", "thread.archived", "thread.unarchived", "thread.archived"]);
    // When the running thread is deleted, and a deleted thread is deleted again
    let deleted = agent.ask("thread.delete", json!({"thread": "busy"})).await;
    let again = agent.ask("thread.delete", json!({"thread": "busy"})).await;
    until_idle(&mut agent, "busy").await;
    // Then the delete answers a seq and the repeat none, the process was killed, and the thread is out of the list
    assert_eq!((deleted["result"]["deleted"].clone(), deleted["result"]["seq"].is_u64(), again["result"]["deleted"].clone(), again["result"].get("seq").is_none()), (json!(true), true, json!(true), true));
    assert_eq!(chain.cli.invocations("claude").len(), 2);
    let listed = agent.ask("thread.list", json!({})).await;
    assert!(listed["result"]["threads"].as_array().unwrap().iter().all(|t| t["id"] != json!("busy")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_failed_turn_a_workflow_and_a_conversation_when_retried_stopped_and_detached_then_each_acts_on_the_process_and_the_next_turn() {
    // Given a CLI that fails once, then announces conversation s-3 and says something, and then stays up
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![
        Turn::replaying(&[]).exiting(1).complaining("not logged in"),
        Turn::replaying(&[json!({"type": "system", "subtype": "init", "session_id": "s-3"}).to_string(), said("recovered")]),
        Turn::replaying(&[said("resumed")]).hanging(),
        Turn::replaying(&[said("fresh")]),
    ]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let failed = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    until_idle(&mut agent, "t1").await;
    // When the failed turn is retried
    let retried = agent.ask("turn.retry", json!({"thread": "t1", "turn": failed})).await;
    until_idle(&mut agent, "t1").await;
    // Then the retry is a new turn of the failed one's prompt that completed, and the failed turn cannot be retried again
    assert_eq!(retried["result"]["retry_of"], json!(failed));
    assert_ne!(retried["result"]["turn"], json!(failed));
    assert!(chain.cli.invocations("claude")[1].argv.contains(&"make a note".to_string()));
    assert_eq!(agent.ask("turn.retry", json!({"thread": "t1", "turn": failed})).await["error"]["code"], json!("NOT_RETRYABLE"));
    // When the next turn runs, a workflow of another turn is stopped, then the running turn's
    let next = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    while chain.cli.invocations("claude").len() < 3 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let wrong = agent.ask("workflow.stop", json!({"thread": "t1", "turn": "turn-0"})).await;
    let stopped = agent.ask("workflow.stop", json!({"thread": "t1", "turn": next, "workflow": "agent-1"})).await;
    until_idle(&mut agent, "t1").await;
    // Then that turn resumed the conversation the retry announced, the wrong turn was refused, and the stop ended the turn interrupted
    assert!(chain.cli.invocations("claude")[2].argv.windows(2).any(|w| w == ["--resume", "s-3"]), "{:?}", chain.cli.invocations("claude")[2].argv);
    assert_eq!((wrong["error"]["code"].clone(), stopped["result"]["turn"].clone()), (json!("NO_TURN"), json!(next)));
    let history = agent.ask("thread.history", json!({"thread": "t1"})).await;
    let last = history["result"]["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["type"].clone(), last["reason"].clone()), (json!("turn.finished"), json!("interrupted")));
    // When the conversation is detached, and the next turn runs
    let detached = agent.ask("thread.detach", json!({"thread": "t1", "session": "s-3"})).await;
    let other = agent.ask("thread.detach", json!({"thread": "t1", "session": "s-3"})).await;
    start_turn(&mut agent, "t1").await;
    until_idle(&mut agent, "t1").await;
    // Then the detach names the session, a second one is refused, and the next turn started a conversation of its own
    assert_eq!((detached["result"]["session"].clone(), other["error"]["code"].clone()), (json!("s-3"), json!("SESSION_NOT_OWNED")));
    assert!(!chain.cli.invocations("claude")[3].argv.contains(&"--resume".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_thread_with_a_transcript_when_an_item_is_read_and_the_thread_is_rolled_back_then_the_item_is_the_row_of_its_seq_and_a_rollback_that_restores_files_is_refused_without_a_checkpoint_and_restores_with_one() {
    // Given a thread whose turn wrote a file and said it, then a second turn
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("writing")]).then_calling("k1", "write_file", write("notes/a.txt", "a")).then_line(&said("wrote it")), Turn::replaying(&[said("more")])]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    let first = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    until_idle(&mut agent, "t1").await;
    let second = start_turn(&mut agent, "t1").await["result"]["turn"].as_str().unwrap().to_string();
    until_idle(&mut agent, "t1").await;
    // When the tool row is read as an item, and a seq that opened no row
    let transcript = agent.ask("thread.transcript", json!({"thread": "t1"})).await;
    let tool = transcript["result"]["items"].as_array().unwrap().iter().find(|i| i["kind"] == json!("tool")).unwrap().clone();
    let item = agent.ask("item.get", json!({"thread": "t1", "seq": tool["seq"]})).await;
    let missing = agent.ask("item.get", json!({"thread": "t1", "seq": 1})).await;
    // Then the item is that row, with the call's tool, and the other is ITEM_NOT_FOUND
    assert_eq!((item["result"]["item"].clone(), item["result"]["item"]["tool"].clone()), (tool, json!("write_file")));
    assert_eq!(missing["error"]["code"], json!("ITEM_NOT_FOUND"));
    // When the checkpoints are gone and the thread is rolled back to its first turn asking for the files back
    let kept = chain.repo.git(&["for-each-ref", "--format=%(refname) %(objectname)", "refs/rca/checkpoints/"]);
    assert_eq!(kept.lines().count(), 2, "{kept}");
    for line in kept.lines() {
        chain.repo.git(&["update-ref", "-d", line.split(' ').next().unwrap()]);
    }
    let missing = agent.ask("thread.rollback", json!({"thread": "t1", "turn": first})).await;
    // Then it is refused by name and drops nothing
    assert_eq!(missing["error"]["code"], json!("NO_CHECKPOINT"));
    assert!(missing["error"]["message"].as_str().unwrap().contains(&second), "{missing}");
    // When the checkpoints are back, the second turn's file is added, and the thread is rolled back to its first turn, asking for the files back
    for line in kept.lines() {
        let (name, sha) = line.split_once(' ').unwrap();
        chain.repo.git(&["update-ref", name, sha]);
    }
    std::fs::write(chain.repo.path().join("notes/later.txt"), "x").unwrap();
    let rolled = agent.ask("thread.rollback", json!({"thread": "t1", "turn": first})).await;
    // Then the second turn is dropped, the files are said to be restored, and what the second turn started with is there while what came after is not
    assert_eq!((rolled["result"]["dropped"].clone(), rolled["result"]["restored_files"].clone()), (json!([second]), json!(true)));
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/a.txt")).unwrap(), "a");
    assert!(!chain.repo.path().join("notes/later.txt").exists());
    // And a rollback that asks to leave the files drops nothing more and restores none
    let again = agent.ask("thread.rollback", json!({"thread": "t1", "turn": first, "restore_files": false})).await;
    assert_eq!((again["result"]["dropped"].clone(), again["result"]["restored_files"].clone()), (json!([]), json!(false)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_devices_pins_order_and_auto_settle_choices_when_the_service_starts_again_then_the_state_group_gives_them_back_and_they_are_not_the_threads() {
    // Given a thread, and the choices a device keeps for its list of threads: pins, their order, the order of the active ones and auto-settle
    let env = Env::process(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("phone").await;
    let mut agent = Agent::open(port, &token).await;
    agent.ask("thread.create", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent"})).await;
    let choices = [("thread.pins", json!(["t2", "t1"])), ("thread.active-order", json!(["t1", "t2"])), ("thread.auto-settle", json!({"enabled": true, "after_ms": 86_400_000}))];
    for (key, value) in &choices {
        let set = agent.ask("state.set", json!({"scope": "device-1", "key": key, "value": value})).await;
        assert!(set.get("result").is_some(), "{key}: {set}");
    }
    // When the service starts again over the same volume, and the device asks on a new socket, and another device asks
    drop(agent);
    let restarted = service(&env, Options::default());
    let restarted_port = restarted.listen().await;
    let mut again = Agent::open(restarted_port, &token).await;
    // Then each choice comes back as it was set, another device has none, and the thread itself holds none of it
    for (key, value) in &choices {
        assert_eq!(again.ask("state.get", json!({"scope": "device-1", "key": key})).await["result"]["value"], *value, "{key}");
        assert_eq!(again.ask("state.get", json!({"scope": "device-2", "key": key})).await["result"]["value"], Value::Null, "{key}");
    }
    let family = again.ask("thread.family", json!({"thread": "t1"})).await;
    assert!(family["result"]["threads"][0].get("settle").is_none(), "{family}");
}
