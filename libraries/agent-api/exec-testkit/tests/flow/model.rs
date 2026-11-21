//! The scripted model API: its answers over the chat-completions protocol, and the request shape the real opencode sends it.
use exec_testkit::{opencode::scripted_env, FakeModel};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;

fn post(model: &FakeModel, body: &Value) -> String {
    let addr = model.base_url().trim_start_matches("http://").trim_end_matches("/v1").to_string();
    let body = body.to_string();
    let mut stream = TcpStream::connect(&addr).unwrap();
    write!(stream, "POST /v1/chat/completions HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    raw
}

fn deltas(sse: &str) -> Vec<Value> {
    sse.lines().filter_map(|l| l.strip_prefix("data: ")).filter(|d| *d != "[DONE]").map(|d| serde_json::from_str(d).unwrap()).collect()
}

#[test]
fn a_streamed_request_is_answered_as_server_sent_chunks_that_end_with_the_usage_and_done() {
    // Given the scripted model and a streamed request for the word PONG
    let model = FakeModel::start();
    let request = json!({"model": "m", "stream": true, "stream_options": {"include_usage": true}, "messages": [{"role": "user", "content": "Reply with exactly the word PONG and nothing else."}]});
    // When it is asked
    let raw = post(&model, &request);
    // Then the answer is an event stream whose text chunks say PONG, which stops, reports its tokens and ends with [DONE]
    assert!(raw.starts_with("HTTP/1.1 200") && raw.contains("content-type: text/event-stream"), "{raw}");
    let chunks = deltas(&raw);
    let text: String = chunks.iter().filter_map(|c| c["choices"][0]["delta"]["content"].as_str()).collect();
    assert_eq!(text, "PONG");
    assert!(chunks.iter().any(|c| c["choices"][0]["finish_reason"] == "stop"));
    assert_eq!(chunks.last().unwrap()["usage"]["total_tokens"], 16);
    assert!(raw.trim_end().ends_with("data: [DONE]"));
}

#[test]
fn a_request_to_overwrite_a_file_calls_write_when_the_tool_is_offered_and_says_done_after_its_result() {
    // Given a request that offers the write tool
    let model = FakeModel::start();
    let ask = json!({"role": "user", "content": "Overwrite a.txt with the text CHANGED using your file tools, then say DONE."});
    let offered = json!({"model": "m", "stream": true, "tools": [{"type": "function", "function": {"name": "write"}}], "messages": [ask]});
    // When it is asked, Then the model calls write with the file and the text
    let call = deltas(&post(&model, &offered)).into_iter().find_map(|c| c["choices"][0]["delta"]["tool_calls"][0]["function"].as_object().cloned()).expect("a tool call");
    assert_eq!(call["name"], "write");
    assert_eq!(serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap(), json!({"filePath": "a.txt", "content": "CHANGED"}));
    // And once the tool's result is in the conversation it says DONE
    let after = json!({"model": "m", "stream": true, "messages": [ask, {"role": "assistant", "tool_calls": [{"id": "call_fake", "type": "function", "function": {"name": "write", "arguments": "{}"}}]}, {"role": "tool", "tool_call_id": "call_fake", "content": "ok"}]});
    let text: String = deltas(&post(&model, &after)).iter().filter_map(|c| c["choices"][0]["delta"]["content"].as_str().map(str::to_string)).collect();
    assert_eq!(text, "DONE");
    // And with no write tool offered (a read-only agent) it writes nothing
    let read_only = json!({"model": "m", "stream": true, "tools": [{"type": "function", "function": {"name": "read"}}], "messages": [ask]});
    assert!(deltas(&post(&model, &read_only)).iter().all(|c| c["choices"][0]["delta"]["tool_calls"].is_null()));
}

#[test]
fn the_secret_word_is_given_back_only_when_an_earlier_turn_set_it() {
    // Given a model that was told the word in a first turn
    let model = FakeModel::start();
    let (set, get) = ("Remember the secret word PELICAN. Reply with only OK.", "What was the secret word? Reply with just the word.");
    let say = |messages: Value| -> String { deltas(&post(&model, &json!({"stream": true, "messages": messages}))).iter().filter_map(|c| c["choices"][0]["delta"]["content"].as_str().map(str::to_string)).collect() };
    // When the first turn and then the second are asked, Then it says OK and then PELICAN
    assert_eq!(say(json!([{"role": "user", "content": set}])), "OK");
    assert_eq!(say(json!([{"role": "user", "content": set}, {"role": "assistant", "content": "OK"}, {"role": "user", "content": get}])), "PELICAN");
    // And a fresh session that was never told does not know it
    assert_eq!(say(json!([{"role": "user", "content": get}])), "I do not know");
}

/// The real opencode, its `opencode-go` provider pointed at the scripted model, asked for PONG with no key and no network: what it sends is
/// what the script reads (a streamed request with usage, the user's text, the model id without the provider), and what the script
/// answers is what opencode prints.
#[test]
fn opencode_run_against_the_scripted_model_prints_its_answer_and_sends_the_request_shape_the_script_reads() {
    // Given an opencode whose provider is the scripted model
    let model = FakeModel::start();
    let dir = tempfile::tempdir().unwrap();
    // When opencode run asks for PONG
    let out = std::process::Command::new("opencode")
        .args(["run", "-m", "opencode-go/deepseek-v4.1-flash", "Reply with exactly the word PONG and nothing else."])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .envs(scripted_env(&model.base_url()))
        .output()
        .expect("opencode starts");
    // Then it prints PONG
    assert!(String::from_utf8_lossy(&out.stdout).contains("PONG"), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // And the request the script read is a streamed chat completion for the model, carrying the user's text
    let requests = model.requests();
    let chat = requests.iter().find(|r| r["messages"].as_array().is_some_and(|m| m.iter().any(|m| m.to_string().contains("exactly the word PONG")))).expect("the prompt reached the model");
    assert_eq!(chat["stream"], true);
    assert_eq!(chat["model"], "deepseek-v4.1-flash");
    assert_eq!(chat["stream_options"]["include_usage"], true);
    assert!(chat["messages"].as_array().unwrap().iter().any(|m| m["role"] == "system"));
    assert!(chat["messages"].as_array().unwrap().iter().any(|m| m["role"] == "user"));
}
