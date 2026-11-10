//! The `mimic-driver/1` process of the mask check (`testing::mimic::masked_by_wf_mimic`): it answers each case with `input.payload` as the
//! one step, so what `wf mimic record` writes is that value after the set's rules.
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    let out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.unwrap()).unwrap_or(Value::Null);
        let reply = if v.get("hello").is_some() {
            json!({"hello": {"driver": {"name": "agent-api-proto-echo", "version": "1", "protocol": "mimic-driver/1"}, "coverage": false}})
        } else {
            json!({"id": v["id"], "steps": [{"dir": "ret", "kind": "json", "payload": v["input"]["payload"]}], "effects": [], "outcome": {"status": "ok"}})
        };
        writeln!(out.lock(), "{reply}").unwrap();
    }
}
