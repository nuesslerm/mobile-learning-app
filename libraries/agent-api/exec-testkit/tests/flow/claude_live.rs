//! The scripted `claude` started as the service starts it (`-p --input-format stream-json`): a process that stays up, takes the handshake, the person's
//! messages and control requests on its input, and says what the real CLI says around the script's lines. A test speaks to it as the service does.
use exec_testkit::{Script, ScriptedCli, Turn};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

struct Live {
    child: Child,
    input: Option<ChildStdin>,
    lines: mpsc::Receiver<Value>,
}

fn start(cli: &ScriptedCli, extra: &[&str]) -> Live {
    let mut child = Command::new(cli.dir().join("claude"))
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--permission-prompt-tool", "stdio"])
        .args(extra)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (input, out) = (child.stdin.take(), child.stdout.take().unwrap());
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Ok(frame) = serde_json::from_str::<Value>(&line) {
                let _ = tx.send(frame);
            }
        }
    });
    Live { child, input, lines }
}

impl Live {
    fn send(&mut self, frame: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{frame}").unwrap();
        input.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.lines.recv_timeout(Duration::from_secs(10)).expect("the process said something")
    }

    /// The frames up to and including the next result.
    fn through_result(&self) -> Vec<Value> {
        let mut all = Vec::new();
        loop {
            let frame = self.next();
            let done = frame["type"] == "result";
            all.push(frame);
            if done {
                return all;
            }
        }
    }

    fn close(mut self) -> Option<i32> {
        drop(self.input.take());
        self.child.wait().unwrap().code()
    }
}

fn user(uuid: &str, text: &str, priority: Option<&str>) -> Value {
    let mut message = json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}, "parent_tool_use_id": null, "uuid": uuid});
    if let Some(priority) = priority {
        message["priority"] = json!(priority);
    }
    message
}

fn control(id: &str, request: Value) -> Value {
    json!({"type": "control_request", "request_id": id, "request": request})
}

fn types(frames: &[Value]) -> Vec<String> {
    frames.iter().map(|f| f["type"].as_str().unwrap().to_string() + &f["subtype"].as_str().map(|s| format!("/{s}")).unwrap_or_default()).collect()
}

#[test]
fn a_live_claude_nobody_scripted_answers_the_handshake_and_completes_each_message_with_an_empty_result_while_it_stays_up() {
    // Given the program started live with no script and a conversation to resume
    let cli = ScriptedCli::install();
    let mut live = start(&cli, &["--resume", "6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de"]);
    // When it is sent the handshake, a message, and another
    live.send(control("1", json!({"subtype": "initialize"})));
    let answer = live.next();
    live.send(user("u1", "hello", None));
    let first = live.through_result();
    live.send(user("u2", "again", None));
    let second = live.through_result();
    // Then it answered the handshake under its id, echoed the message, said init in the conversation it runs in, and ended each turn naming the message it consumed
    assert_eq!((answer["type"].clone(), answer["response"]["request_id"].clone(), answer["response"]["subtype"].clone()), (json!("control_response"), json!("1"), json!("success")));
    assert_eq!(types(&first), ["user", "system/init", "result/success"]);
    assert_eq!((first[0]["isReplay"].clone(), first[1]["session_id"].clone()), (json!(true), json!("6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de")));
    assert_eq!((first[2]["user_message_uuids"].clone(), first[2]["is_error"].clone(), second[2]["user_message_uuids"].clone()), (json!(["u1"]), json!(false), json!(["u2"])));
    // And it stays up until its input ends, and ends with code 0 then
    assert_eq!(live.close(), Some(0));
    assert_eq!(cli.invocations("claude").len(), 1);
}

#[test]
fn a_scripted_live_claude_plays_the_next_turn_of_its_script_for_each_message_and_the_script_goes_on_after_a_restart() {
    // Given a script of two turns that each say something
    let cli = ScriptedCli::install();
    cli.script("claude", &Script::of(vec![Turn::replaying(&[json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "one"}]}}).to_string()]), Turn::replaying(&[json!({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "two"}]}}).to_string()])]));
    let said = |frames: &[Value]| frames.iter().filter(|f| f["type"] == "assistant").map(|f| f["message"]["content"][0]["text"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    // When the first process is sent a message, and a second process started after it ended is sent another
    let mut first = start(&cli, &["--session-id", "6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de"]);
    first.send(user("u1", "a", None));
    let one = first.through_result();
    first.close();
    let mut second = start(&cli, &["--resume", "6b0b2f0e-6d0e-4a0a-9a52-8d54b4b9a4de"]);
    second.send(user("u2", "b", None));
    let two = second.through_result();
    second.close();
    // Then each played its own turn, and the log has the messages of both with the start each was sent to
    assert_eq!((said(&one), said(&two)), (vec!["one".to_string()], vec!["two".to_string()]));
    assert_eq!(cli.claude_prompts("claude"), ["a", "b"]);
    assert_eq!(cli.claude_log("claude").iter().map(|r| r.start).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn a_message_with_priority_now_aborts_the_turn_that_plays_and_plays_the_next_one_and_an_interrupt_ends_the_turn_with_an_aborted_result() {
    // Given a script whose first turn waits for a long time, whose second says something, and whose third waits again
    let cli = ScriptedCli::install();
    let left = json!({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "left"}]}}).to_string();
    cli.script("claude", &Script::of(vec![Turn::default().then_sleeping(60_000), Turn::replaying(&[left]), Turn::default().then_sleeping(60_000)]));
    let mut live = start(&cli, &[]);
    live.send(control("1", json!({"subtype": "initialize"})));
    live.next();
    live.send(user("u1", "go right", None));
    assert_eq!(types(&[live.next(), live.next()]), ["user", "system/init"]);
    // When it is steered
    live.send(user("u2", "go left", Some("now")));
    let steered = live.through_result();
    let rest = live.through_result();
    // Then the first turn ended with the aborted result naming its message, and the steer's turn said its line and ended with a result naming the steer
    assert_eq!((steered[0]["type"].clone(), steered[0]["is_error"].clone(), steered[0]["terminal_reason"].clone(), steered[0]["user_message_uuids"].clone()), (json!("result"), json!(true), json!("aborted_streaming"), json!(["u1"])));
    assert_eq!(types(&rest), ["user", "system/init", "assistant", "result/success"]);
    assert_eq!(rest[3]["user_message_uuids"], json!(["u2"]));
    // When the next turn waits and is interrupted
    live.send(user("u3", "wait", None));
    assert_eq!(types(&[live.next(), live.next()]), ["user", "system/init"]);
    live.send(control("9", json!({"subtype": "interrupt", "cancel_queued": true})));
    let stopped = live.through_result();
    // Then the interrupt was answered under its id before the aborted result of the turn, and the process lives on
    assert_eq!((stopped[0]["response"]["request_id"].clone(), stopped[0]["response"]["response"]["still_queued"].clone()), (json!("9"), json!([])));
    assert_eq!((stopped[1]["is_error"].clone(), stopped[1]["user_message_uuids"].clone()), (json!(true), json!(["u3"])));
    live.send(control("10", json!({"subtype": "set_permission_mode", "mode": "plan"})));
    assert_eq!(live.next()["response"]["response"]["mode"], json!("plan"));
    live.send(control("11", json!({"subtype": "bogus"})));
    let refused = live.next();
    assert_eq!((refused["response"]["subtype"].clone(), refused["response"]["error_code"].clone()), (json!("error"), json!("unsupported_request")));
    assert_eq!(live.close(), Some(0));
}

#[test]
fn a_turn_that_reads_a_line_waits_for_the_answer_to_its_request_and_a_turn_that_exits_ends_the_process_with_no_result() {
    // Given a script whose first turn asks to use a tool and waits for the answer, and whose second prints a line and exits with 3
    let cli = ScriptedCli::install();
    let ask = json!({"type": "control_request", "request_id": "c1", "request": {"subtype": "can_use_tool", "tool_name": "Write", "input": {"file_path": "a"}, "tool_use_id": "toolu_1"}}).to_string();
    cli.script("claude", &Script::of(vec![Turn::replaying(&[ask]).then_reading_a_line().then_line(&json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "written"}]}}).to_string()), Turn::default().complaining("not logged in").exiting(3)]));
    let mut live = start(&cli, &[]);
    live.send(user("u1", "write", None));
    assert_eq!(types(&[live.next(), live.next()]), ["user", "system/init"]);
    // When it has asked, and the answer is sent under the id of its request
    let asked = live.next();
    live.send(json!({"type": "control_response", "response": {"subtype": "success", "request_id": "c1", "response": {"behavior": "allow", "updatedInput": {"file_path": "a"}}}}));
    let rest = live.through_result();
    // Then the ask was a can_use_tool request, and the turn went on after the answer to say its line and end with a result naming the message
    assert_eq!((asked["type"].clone(), asked["request"]["tool_name"].clone()), (json!("control_request"), json!("Write")));
    assert_eq!(types(&rest), ["assistant", "result/success"]);
    // And the next turn prints and exits with the code, saying no result for its message
    live.send(user("u2", "again", None));
    assert_eq!(types(&[live.next(), live.next()]), ["user", "system/init"]);
    assert_eq!(live.child.wait().unwrap().code(), Some(3));
    assert!(live.lines.recv_timeout(Duration::from_secs(2)).is_err());
}
