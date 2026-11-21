//! The program that stands in for the model API (see `exec_testkit::model`): an OpenAI-compatible `POST /v1/chat/completions` on `$PORT` of
//! 127.0.0.1 that answers the prompts the rca e2e asks from a script, as a stream (what opencode asks for) or as one JSON body.
//! `GET /v1/requests` returns the request bodies it has seen, so a test pins the shape opencode sends.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// What the model answers: text, or a call of a tool the request offered.
enum Reply {
    Text(String),
    Tool { name: &'static str, arguments: Value },
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn texts(messages: &[Value], role: &str) -> Vec<String> {
    messages.iter().filter(|m| m["role"] == role).map(|m| text_of(&m["content"])).collect()
}

/// The word after `marker` in `text`, without trailing punctuation.
fn word_after(text: &str, marker: &str) -> Option<String> {
    let rest = text.split(marker).nth(1)?;
    rest.split_whitespace().next().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '/').trim_end_matches('.').to_string())
}

fn decide(request: &Value) -> Reply {
    let messages = request["messages"].as_array().cloned().unwrap_or_default();
    let tools: Vec<&str> = request["tools"].as_array().map(|t| t.iter().filter_map(|t| t["function"]["name"].as_str()).collect()).unwrap_or_default();
    let last = messages.last().cloned().unwrap_or_default();
    if last["role"] == "tool" {
        return Reply::Text("DONE".into());
    }
    let system = texts(&messages, "system").join("\n");
    let users = texts(&messages, "user");
    let ask = users.last().cloned().unwrap_or_default();
    if let (Some(file), Some(content)) = (word_after(&ask, "Overwrite "), word_after(&ask, "with the text ")) {
        return if tools.contains(&"write") { Reply::Tool { name: "write", arguments: json!({"filePath": file, "content": content}) } } else { Reply::Text("DONE".into()) };
    }
    if system.contains("ARRR") {
        return Reply::Text("ARRR".into());
    }
    if ask.contains("What was the secret word") {
        let known = users.iter().rev().skip(1).find_map(|u| word_after(u, "secret word "));
        return Reply::Text(known.unwrap_or_else(|| "I do not know".into()));
    }
    if ask.contains("Remember the secret word") {
        return Reply::Text("OK".into());
    }
    if let Some(word) = word_after(&ask, "Reply with exactly the word ") {
        return Reply::Text(word);
    }
    Reply::Text("OK".into())
}

fn chunk(delta: Value, finish: Value) -> String {
    format!("data: {}\n\n", json!({"id": "chatcmpl-fake", "object": "chat.completion.chunk", "created": 0, "model": "fake", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}))
}

fn usage_chunk() -> String {
    format!("data: {}\n\n", json!({"id": "chatcmpl-fake", "object": "chat.completion.chunk", "created": 0, "model": "fake", "choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16}}))
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let _ = write!(stream, "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
}

fn serve(mut stream: TcpStream, seen: Arc<Mutex<Vec<Value>>>) {
    let mut reader = BufReader::new(stream.try_clone().expect("a handle"));
    let mut head = String::new();
    if reader.read_line(&mut head).unwrap_or(0) == 0 {
        return;
    }
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length];
    let _ = reader.read_exact(&mut body);
    let path = head.split_whitespace().nth(1).unwrap_or_default();
    if head.starts_with("GET") && path.ends_with("/requests") {
        return respond(&mut stream, "200 OK", "application/json", &Value::Array(seen.lock().unwrap().clone()).to_string());
    }
    if head.starts_with("GET") && path.ends_with("/models") {
        return respond(&mut stream, "200 OK", "application/json", &json!({"object": "list", "data": [{"id": "fake", "object": "model"}]}).to_string());
    }
    if !(head.starts_with("POST") && path.ends_with("/chat/completions")) {
        return respond(&mut stream, "404 Not Found", "application/json", &json!({"error": {"message": "not scripted"}}).to_string());
    }
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();
    seen.lock().unwrap().push(request.clone());
    let reply = decide(&request);
    if request["stream"] != true {
        let message = match &reply {
            Reply::Text(t) => json!({"role": "assistant", "content": t}),
            Reply::Tool { name, arguments } => json!({"role": "assistant", "content": null, "tool_calls": [{"id": "call_fake", "type": "function", "function": {"name": name, "arguments": arguments.to_string()}}]}),
        };
        let finish = if matches!(reply, Reply::Tool { .. }) { "tool_calls" } else { "stop" };
        let out = json!({"id": "chatcmpl-fake", "object": "chat.completion", "created": 0, "model": "fake", "choices": [{"index": 0, "message": message, "finish_reason": finish}], "usage": {"prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16}});
        return respond(&mut stream, "200 OK", "application/json", &out.to_string());
    }
    let mut out = chunk(json!({"role": "assistant", "content": ""}), Value::Null);
    match reply {
        Reply::Text(t) => {
            out += &chunk(json!({"content": t}), Value::Null);
            out += &chunk(json!({}), json!("stop"));
        }
        Reply::Tool { name, arguments } => {
            out += &chunk(json!({"tool_calls": [{"index": 0, "id": "call_fake", "type": "function", "function": {"name": name, "arguments": arguments.to_string()}}]}), Value::Null);
            out += &chunk(json!({}), json!("tool_calls"));
        }
    }
    out += &usage_chunk();
    out += "data: [DONE]\n\n";
    let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n{out}");
}

fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).expect("PORT is set");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the port is free");
    println!("listening {port}");
    let seen = Arc::new(Mutex::new(Vec::new()));
    for stream in listener.incoming().flatten() {
        let seen = seen.clone();
        std::thread::spawn(move || serve(stream, seen));
    }
}
