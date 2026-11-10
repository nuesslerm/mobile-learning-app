//! The `mimic-driver/1` process of the D2b spike (`tests/integration.rs`): what this crate does with a request frame, over JSON lines on
//! stdio. `--tweak` plants a defect (the parsed frame loses a key) so the test can show a replay finds it.
use agent_api_proto::Request;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn observed(frame: &str, tweak: bool) -> Value {
    let mut payload = match Request::parse(frame) {
        Ok(request) => request.to_value(),
        Err(refusal) => refusal.to_value(),
    };
    if tweak {
        if let Some(o) = payload.as_object_mut() {
            o.remove("method").or_else(|| o.remove("error"));
        }
    }
    json!({"steps": [{"dir": "ret", "kind": "json", "payload": payload}], "effects": [], "outcome": {"status": "ok"}})
}

fn main() {
    let tweak = std::env::args().any(|a| a == "--tweak");
    let out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap_or(Value::Null);
        let reply = if v.get("hello").is_some() {
            json!({"hello": {"driver": {"name": "agent-api-proto", "version": "1", "protocol": "mimic-driver/1"}, "coverage": false}})
        } else {
            let mut reply = observed(v["input"]["frame"].as_str().unwrap_or(""), tweak);
            reply["id"] = v["id"].clone();
            reply
        };
        writeln!(out.lock(), "{reply}").unwrap();
    }
}
