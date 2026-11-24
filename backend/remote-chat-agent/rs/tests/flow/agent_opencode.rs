//! opencode as a live session end to end: the service keeps one `opencode serve` for the workspace at its host through the real `rca-exec` (which makes the
//! server's credential and speaks to it over HTTP for the service), and each thread is a session in it. The server is the scripted `opencode serve` of
//! `exec-testkit`: HTTP Basic auth, the event stream, sessions that outlive a restart, and the permission and question flows, playing the turns a test
//! wrote in the dialect of `opencode run --format json` (`docs/solutions/architecture/opencode-server-protocol.md`); none is a run of the real server by this
//! service, which `docs/gaps/opencode-live-session-not-run-against-the-real-cli.md` says.
use crate::agent_threads::{chain_with, created_by, kinds, looked_up, Chain};
use crate::harness::{eventually, Env, Options};
use crate::mapper::Agent;
use agent_api_proto::Event;
use exec_testkit::script::{Script, Turn};
use serde_json::{json, Value};
use std::time::Duration;

fn says(text: &str) -> String {
    json!({"type": "text", "part": {"text": text}}).to_string()
}

fn ran(call: &str, tool: &str, input: Value, output: &str) -> String {
    json!({"type": "tool_use", "part": {"callID": call, "tool": tool, "state": {"status": "completed", "input": input, "output": output}}}).to_string()
}

fn script(chain: &Chain, turns: Vec<Turn>) {
    chain.cli.script("opencode", &Script::of(turns).printing_version("0.0.0-flow"));
}

async fn thread(agent: &mut Agent, id: &str) {
    let made = agent.ask("thread.create", json!({"thread": id, "title": "opencode", "workspace": "w1", "driver": "opencode"})).await;
    assert!(made.get("result").is_some(), "{made}");
    agent.ask("thread.subscribe", json!({"thread": id})).await;
}

async fn turn(agent: &mut Agent, id: &str, prompt: &str, runtime_mode: &str) -> Value {
    let started = agent.ask("turn.start", json!({"thread": id, "workspace": "w1", "driver": "opencode", "prompt": prompt, "runtime_mode": runtime_mode})).await;
    assert!(started.get("result").is_some(), "{started}");
    started
}

fn spoken(events: &[Event]) -> String {
    events.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == json!("assistant")).map(|e| e.data["text"].as_str().unwrap()).collect()
}

fn reason(events: &[Event]) -> Value {
    events.iter().rev().find(|e| e.kind == "turn.finished").unwrap().data["reason"].clone()
}

fn requests(chain: &Chain) -> Vec<Value> {
    chain.cli.opencode_requests("opencode")
}

/// The requests of the server whose path starts with `prefix`, in order.
fn sent_to(chain: &Chain, method: &str, prefix: &str) -> Vec<Value> {
    requests(chain).into_iter().filter(|r| r["method"] == method && r["path"].as_str().is_some_and(|p| p.starts_with(prefix))).collect()
}

fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_streams_text_and_runs_a_tool_when_it_runs_over_the_agent_socket_then_the_text_and_the_tool_arrive_as_events_the_turn_ends_and_the_credential_is_nowhere() {
    // Given the service, the real executor, and an opencode whose turn says something, runs a command and says it found a file
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_line(&says("looking")).then_line(&ran("k1", "bash", json!({"command": "ls"}), "a.txt")).then_line(&says("found a.txt"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    // When a turn runs
    turn(&mut agent, "t1", "list the files", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    // Then the events are the turn's: the person's words, the text, the tool as one call that succeeded, and the end
    assert_eq!(spoken(&events), "lookingfound a.txt");
    let started: Vec<&Event> = events.iter().filter(|e| e.kind == "tool.started").collect();
    assert_eq!((started.len(), started[0].data["tool"].clone(), started[0].data["args"].clone()), (1, json!("bash"), json!({"command": "ls"})));
    assert_eq!(events.iter().find(|e| e.kind == "tool.finished").unwrap().data["ok"], json!(true));
    assert_eq!((kinds(&events)[0], reason(&events)), ("turn.started", json!("completed")));
    // And the server was started once at the workspace host's root with its own arguments and nothing of the credential, and every request carried it
    let starts = chain.cli.invocations("opencode");
    assert_eq!((starts.len(), starts[0].argv.clone(), starts[0].cwd.clone()), (1, ["serve", "--hostname", "127.0.0.1", "--port", "0"].map(String::from).to_vec(), chain.repo.path().to_string_lossy().into_owned()));
    assert!(requests(&chain).iter().all(|r| r["authorized"] == true));
    // And the session was made with full access, the prompt named its agent, and the credential is in no event, no answer of the service and no argument
    let create = sent_to(&chain, "POST", "/session");
    let create = create.iter().find(|r| r["path"] == "/session").unwrap();
    assert_eq!(create["body"]["permission"], json!([{"permission": "*", "pattern": "*", "action": "allow"}]));
    assert_eq!(sent_to(&chain, "POST", "/session/ses_scripted0001/prompt_async")[0]["body"]["agent"], json!("build"));
    let password = chain.cli.opencode_password("opencode").expect("the server was given a password");
    let history = agent.ask("thread.history", json!({"thread": "t1"})).await.to_string();
    assert!(password.len() >= 32 && !format!("{events:?}{history}").contains(&password) && !starts[0].argv.concat().contains(&password));
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_two_threads_of_a_workspace_when_each_runs_a_turn_then_they_are_two_sessions_of_one_server() {
    // Given two threads and a turn scripted for each
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_line(&says("one")), Turn::default().then_line(&says("two"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    thread(&mut agent, "t2").await;
    // When each runs a turn
    turn(&mut agent, "t1", "first", "full-access").await;
    let first = agent.events_through("turn.finished").await;
    turn(&mut agent, "t2", "second", "full-access").await;
    let second = agent.events_through("turn.finished").await;
    // Then each answer is its thread's, and the one server made two sessions and ran a prompt in each
    assert_eq!((spoken(&first), spoken(&second)), ("one".to_string(), "two".to_string()));
    assert!(first.iter().all(|e| e.thread.as_deref() == Some("t1")) && second.iter().all(|e| e.thread.as_deref() == Some("t2")));
    assert_eq!((chain.cli.invocations("opencode").len(), created_by(&requests(&chain), 1)), (1, 2));
    assert_eq!((sent_to(&chain, "POST", "/session/ses_scripted0001/prompt_async").len(), sent_to(&chain, "POST", "/session/ses_scripted0002/prompt_async").len()), (1, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_asks_to_run_a_tool_when_the_person_answers_then_the_ask_is_an_approval_event_the_reply_reaches_the_server_and_the_turn_goes_on() {
    // Given an opencode whose turn asks before it runs a command, waits for the answer, and then says it ran it, in the mode that asks
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_calling("c1", "approval_prompt", json!({"tool_name": "bash", "input": {"command": "touch a"}})).then_line(&says("done"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "run it", "approval-required").await;
    // When the turn asks
    let events = agent.events_through("approval.requested").await;
    let asked = events.last().unwrap();
    // Then the request names the tool, and the turn waits on it
    assert_eq!(asked.data["tool"], json!("bash"));
    assert_ne!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
    // When the person accepts it
    let approval = asked.data["approval"].as_str().unwrap().to_string();
    let answered = agent.ask("approval.answer", json!({"thread": "t1", "approval": approval, "decision": "accept"})).await;
    assert!(answered.get("result").is_some(), "{answered}");
    let rest = agent.events_through("turn.finished").await;
    // Then the turn finished with its answer, and the server was sent the reply once, under the id of its own request
    assert_eq!((reason(&rest), spoken(&rest)), (json!("completed"), "done".to_string()));
    let replies = sent_to(&chain, "POST", "/permission/");
    assert_eq!((replies.len(), replies[0]["body"].clone()), (1, json!({"reply": "once"})));
    assert!(replies[0]["path"].as_str().unwrap().contains(&approval), "{replies:?}");
    // And the session asked about the shell because the thread's mode asks
    let create = sent_to(&chain, "POST", "/session").into_iter().find(|r| r["path"] == "/session").unwrap();
    assert!(create["body"]["permission"].as_array().unwrap().iter().any(|r| r["permission"] == "bash" && r["action"] == "ask"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_asks_to_run_a_tool_when_the_person_declines_then_the_server_is_sent_a_reject_and_the_turn_goes_on() {
    // Given an opencode whose turn asks before it runs a command and goes on whatever the answer is
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_calling("c1", "approval_prompt", json!({"tool_name": "bash", "input": {"command": "rm -rf x"}})).then_line(&says("understood"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "run it", "approval-required").await;
    let asked = agent.events_through("approval.requested").await.pop().unwrap();
    // When the person declines
    let approval = asked.data["approval"].as_str().unwrap().to_string();
    agent.ask("approval.answer", json!({"thread": "t1", "approval": approval, "decision": "decline"})).await;
    let rest = agent.events_through("turn.finished").await;
    // Then the server was sent a reject, and the turn went on to its end
    assert_eq!((reason(&rest), spoken(&rest)), (json!("completed"), "understood".to_string()));
    assert_eq!(sent_to(&chain, "POST", "/permission/")[0]["body"], json!({"reply": "reject"}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_asks_a_question_when_the_person_answers_then_the_question_is_an_input_event_and_the_answer_reaches_the_server() {
    // Given an opencode whose turn asks which library to use and goes on with the answer
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_calling("q1", "ask_user", json!({"question": "Which library?", "options": ["date-fns", "dayjs"]})).then_line(&says("chose"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "pick one", "full-access").await;
    // When the turn asks
    let asked = agent.events_through("input.requested").await.pop().unwrap();
    // Then the request carries the question and its options
    assert_eq!((asked.data["prompt"].clone(), asked.data["options"].clone()), (json!("Which library?"), json!(["date-fns", "dayjs"])));
    // When the person picks one
    let request = asked.data["request"].as_str().unwrap().to_string();
    agent.ask("input.answer", json!({"thread": "t1", "request": request, "answer": "dayjs"})).await;
    let rest = agent.events_through("turn.finished").await;
    // Then the server got the answer as the chosen label and the turn went on
    assert_eq!((reason(&rest), spoken(&rest)), (json!("completed"), "chose".to_string()));
    assert_eq!(sent_to(&chain, "POST", "/question/")[0]["body"], json!({"answers": [["dayjs"]]}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_is_working_when_the_mapper_steers_it_then_the_prompt_goes_to_the_same_session_and_the_turn_ends_once_when_it_is_interrupted() {
    // Given an opencode whose turn says something and goes on
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_line(&says("working")).then_sleeping(60_000)]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "go right", "full-access").await;
    agent.events_through("message.delta").await;
    agent.events_through("message.delta").await;
    // When it is steered
    let steered = agent.ask("turn.steer", json!({"thread": "t1", "prompt": "go left"})).await;
    assert!(steered.get("result").is_some(), "{steered}");
    // Then the session was given the second prompt while the first turn went on, and no second server was started
    eventually("the steer reached the session", || sent_to(&chain, "POST", "/session/ses_scripted0001/prompt_async").len() == 2).await;
    let texts: Vec<Value> = sent_to(&chain, "POST", "/session/ses_scripted0001/prompt_async").iter().map(|r| r["body"]["parts"][0]["text"].clone()).collect();
    assert_eq!((texts, chain.cli.invocations("opencode").len()), (vec![json!("go right"), json!("go left")], 1));
    // And the one turn ends once, as interrupted when the person stops it
    agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    let end = agent.events_through("turn.finished").await;
    assert_eq!(reason(&end), json!("interrupted"));
    assert_eq!(end.iter().filter(|e| e.kind == "turn.started").count(), 0, "{:?}", kinds(&end));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_turn_that_is_working_when_it_is_interrupted_then_the_session_is_aborted_the_turn_ends_interrupted_and_the_next_turn_runs_on_the_same_server() {
    // Given a turn that is working and a script for the turn after it
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_line(&says("working")).then_sleeping(60_000), Turn::default().then_line(&says("back"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "work", "full-access").await;
    agent.events_through("message.delta").await;
    agent.events_through("message.delta").await;
    // When it is interrupted
    let interrupted = agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    assert!(interrupted.get("result").is_some(), "{interrupted}");
    let end = agent.events_through("turn.finished").await;
    // Then the server was told to abort the session, the turn ended interrupted with no error, and the next turn runs on the same server
    assert_eq!(sent_to(&chain, "POST", "/session/ses_scripted0001/abort").len(), 1);
    assert_eq!((reason(&end), end.iter().any(|e| e.kind == "error")), (json!("interrupted"), false), "{:?}", kinds(&end));
    turn(&mut agent, "t1", "again", "full-access").await;
    let next = agent.events_through("turn.finished").await;
    assert_eq!((reason(&next), spoken(&next), chain.cli.invocations("opencode").len()), (json!("completed"), "back".to_string(), 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_server_that_sat_idle_when_the_idle_time_passes_then_it_is_closed_and_the_next_turn_starts_another_that_resumes_the_session_by_its_id() {
    // Given a thread that ran a turn, and an idle time of 300 ms
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options { opencode_idle: Duration::from_millis(300), ..Options::default() }).await;
    script(&chain, vec![Turn::default().then_line(&says("first")), Turn::default().then_line(&says("second"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "one", "full-access").await;
    agent.events_through("turn.finished").await;
    let pid: u32 = chain.cli.opencode_pid("opencode").expect("the server wrote its pid");
    // When nothing runs for longer than that
    eventually("the server to be closed", || gone(pid)).await;
    // Then the thread's next turn starts a second server that looks the session up by its id and makes none
    turn(&mut agent, "t1", "two", "full-access").await;
    let events = agent.events_through("turn.finished").await;
    assert_eq!((reason(&events), spoken(&events)), (json!("completed"), "second".to_string()));
    assert_eq!(chain.cli.invocations("opencode").len(), 2);
    let all = requests(&chain);
    assert_eq!((created_by(&all, 2), looked_up(&all, 2, "ses_scripted0001")), (0, true), "{all:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_server_that_is_killed_in_the_middle_of_a_turn_when_the_thread_runs_again_then_the_turn_failed_naming_why_and_a_new_server_resumes_the_session() {
    // Given a turn that is working, and a script for the turn after it
    let env = Env::new(&[]);
    let chain = chain_with(&env, Options::default()).await;
    script(&chain, vec![Turn::default().then_line(&says("working")).then_sleeping(60_000), Turn::default().then_line(&says("back"))]);
    let mut agent = chain.agent().await;
    thread(&mut agent, "t1").await;
    turn(&mut agent, "t1", "work", "full-access").await;
    agent.events_through("message.delta").await;
    agent.events_through("message.delta").await;
    // When the server is killed from outside
    let pid = chain.cli.opencode_pid("opencode").expect("the server wrote its pid");
    std::process::Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap();
    let end = agent.events_through("turn.finished").await;
    // Then the turn failed with an error that says the server ended, and the thread is idle
    assert_eq!(reason(&end), json!("failed"));
    assert!(end.iter().any(|e| e.kind == "error" && e.data["message"].as_str().is_some_and(|m| m.contains("ended"))), "{:?}", kinds(&end));
    // And the next turn starts a new server and goes on in the session of the first
    turn(&mut agent, "t1", "again", "full-access").await;
    let next = agent.events_through("turn.finished").await;
    assert_eq!((reason(&next), spoken(&next)), (json!("completed"), "back".to_string()));
    let all = requests(&chain);
    assert_eq!((chain.cli.invocations("opencode").len(), created_by(&all, 2), looked_up(&all, 2, "ses_scripted0001")), (2, 0, true), "{all:?}");
}
