//! Traffic as cassette interactions: one per HTTP exchange, one per socket with every frame in wire order. Values are kept whole,
//! so the result is only fit for a file after `sanitize`.
use super::traffic::{Event, Headers};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mimic_core::cassette::{Dir, Interaction, Outcome, Status, Step, StepKind};
use mimic_core::wire::{raw_steps, WireEvent};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const OP_HTTP: &str = "http";
pub const OP_WS: &str = "ws";
pub const CHANNEL_HTTP: &str = "http";
pub const CHANNEL_WS: &str = "ws";
pub const CHANNEL_OPEN: &str = "ws:open";
pub const CHANNEL_CLOSE: &str = "ws:close";

pub fn route(target: &str) -> &str {
    target.split(['?', '#']).next().unwrap_or("")
}

fn header_pairs(headers: &Headers) -> Value {
    Value::Array(headers.iter().map(|(k, v)| json!([k, v])).collect())
}

/// A body as text when it is text, else whole as base64 beside a null `body`.
fn message(mut fields: Map<String, Value>, headers: &Headers, body: &[u8]) -> Value {
    fields.insert("headers".into(), header_pairs(headers));
    match std::str::from_utf8(body) {
        Ok("") => fields.insert("body".into(), Value::Null),
        Ok(text) => fields.insert("body".into(), json!(text)),
        Err(_) => {
            fields.insert("body".into(), Value::Null);
            fields.insert("body_b64".into(), json!(STANDARD.encode(body)))
        }
    };
    Value::Object(fields)
}

fn step(dir: Dir, payload: Value) -> Step {
    Step { dir, channel: Some(CHANNEL_HTTP.into()), kind: StepKind::Json, rid: None, len: None, payload }
}

fn call(method: &str, target: &str, headers: &Headers, body: &[u8]) -> Step {
    let mut fields = Map::new();
    fields.insert("method".into(), json!(method));
    fields.insert("path".into(), json!(target));
    step(Dir::Call, message(fields, headers, body))
}

pub fn ret(status: u16, headers: &Headers, body: &[u8]) -> Step {
    let mut fields = Map::new();
    fields.insert("status".into(), json!(status));
    step(Dir::Ret, message(fields, headers, body))
}

fn exchange(seq: u64, input: Value, method: &str, target: &str, first: Step) -> (u64, Interaction) {
    let unanswered = Outcome { status: Status::Error, code: Some("no-answer".into()), reference_accepts: None };
    let it = Interaction {
        id: String::new(),
        row: Some(format!("http:{method} {}", route(target))),
        op: Some(OP_HTTP.into()),
        input,
        steps: vec![first],
        effects: Vec::new(),
        outcome: unanswered,
        tags: Vec::new(),
    };
    (seq, it)
}

/// The interactions of a run, in the order their first event was seen. Ids are `h0001…` for exchanges and `c01…` for sockets.
pub fn interactions(events: &[Event]) -> Vec<Interaction> {
    let mut out: Vec<(u64, Interaction)> = Vec::new();
    let mut exchanges: BTreeMap<u32, usize> = BTreeMap::new();
    let mut sockets: BTreeMap<u32, (usize, Vec<WireEvent>)> = BTreeMap::new();
    for (i, event) in events.iter().enumerate() {
        let seq = i as u64 + 1;
        match event {
            Event::HttpRequest { exchange: e, method, target, headers, body } => {
                exchanges.insert(*e, out.len());
                out.push(exchange(seq, json!({"seq": seq, "exchange": format!("e{e}")}), method, target, call(method, target, headers, body)));
            }
            Event::HttpResponse { exchange: e, status, headers, body } => {
                if let Some(at) = exchanges.remove(e) {
                    out[at].1.steps.push(ret(*status, headers, body));
                    out[at].1.outcome = Outcome::default();
                }
            }
            Event::WsRefused { connection, target, status, body } => {
                let input = json!({"seq": seq, "connection": format!("c{connection}"), "upgrade": true});
                let (seq, mut it) = exchange(seq, input, "GET", target, call("GET", target, &Vec::new(), &[]));
                it.steps.push(ret(*status, &Vec::new(), body));
                it.outcome = Outcome::default();
                out.push((seq, it));
            }
            Event::WsOpen { connection, target, status } => {
                let it = Interaction {
                    id: String::new(),
                    row: Some(format!("ws:{}", route(target))),
                    op: Some(OP_WS.into()),
                    input: json!({"seq": seq, "connection": format!("c{connection}")}),
                    steps: Vec::new(),
                    effects: Vec::new(),
                    outcome: Outcome::default(),
                    tags: Vec::new(),
                };
                sockets.insert(*connection, (out.len(), vec![WireEvent::WsOpen { path: target.clone(), status: *status }]));
                out.push((seq, it));
            }
            Event::WsFrame { connection, direction, opcode, data } => {
                if let Some((_, seen)) = sockets.get_mut(connection) {
                    seen.push(WireEvent::Ws { dir: *direction, opcode: *opcode, data: data.clone() });
                }
            }
            Event::WsClose { connection, direction, code } => {
                if let Some((_, seen)) = sockets.get_mut(connection) {
                    seen.push(WireEvent::WsClose { dir: *direction, code: *code, reason: String::new() });
                }
            }
            Event::Failed { .. } => {}
        }
    }
    for (at, seen) in sockets.into_values() {
        out[at].1.steps = raw_steps(&seen);
    }
    out.sort_by_key(|(seq, _)| *seq);
    let (mut http, mut ws) = (0, 0);
    out.into_iter()
        .map(|(_, mut it)| {
            it.id = if it.op.as_deref() == Some(OP_WS) {
                ws += 1;
                format!("c{ws:02}")
            } else {
                http += 1;
                format!("h{http:04}")
            };
            it
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Headers {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn given_an_exchange_and_a_socket_when_turned_into_interactions_then_each_keeps_its_values_and_its_order() {
        // Given
        let events = vec![
            Event::HttpRequest { exchange: 1, method: "GET".into(), target: "/health?x=1".into(), headers: headers(&[("accept", "*/*")]), body: Vec::new() },
            Event::WsOpen { connection: 1, target: "/ws".into(), status: 101 },
            Event::HttpResponse { exchange: 1, status: 200, headers: headers(&[("set-cookie", "a=1"), ("set-cookie", "b=2")]), body: br#"{"ok":true}"#.to_vec() },
            Event::WsFrame { connection: 1, direction: Dir::C2s, opcode: 1, data: br#"{"id":"7","tag":"ping"}"#.to_vec() },
            Event::WsFrame { connection: 1, direction: Dir::S2c, opcode: 2, data: vec![0xff, 0x00] },
            Event::WsClose { connection: 1, direction: None, code: None },
        ];

        // When
        let got = interactions(&events);

        // Then
        assert_eq!(got.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["h0001", "c01"]);
        assert_eq!(got[0].row.as_deref(), Some("http:GET /health"));
        assert_eq!(got[0].steps[1].payload["headers"], json!([["set-cookie", "a=1"], ["set-cookie", "b=2"]]));
        assert_eq!(got[0].steps[1].payload["body"], json!(r#"{"ok":true}"#));
        let channels: Vec<_> = got[1].steps.iter().map(|s| s.channel.clone().unwrap_or_default()).collect();
        assert_eq!(channels, [CHANNEL_OPEN, CHANNEL_WS, CHANNEL_WS, CHANNEL_CLOSE]);
        assert_eq!(got[1].steps[1].rid.as_deref(), Some("7"));
        assert_eq!(got[1].steps[2].kind, StepKind::Binary);
    }

    #[test]
    fn given_a_request_nobody_answered_when_turned_into_an_interaction_then_it_says_so() {
        // Given
        let events = vec![Event::HttpRequest { exchange: 1, method: "POST".into(), target: "/x".into(), headers: Vec::new(), body: vec![0xff] }];

        // When
        let got = interactions(&events);

        // Then
        assert_eq!(got[0].outcome.code.as_deref(), Some("no-answer"));
        assert_eq!(got[0].steps[0].payload["body_b64"], json!("/w=="));
    }
}
