//! The `Wire` port: a tap between a client and a target on a byte stream, the record side of a `ws+http` reference. The adapters (the
//! `WsTap`/`HttpTap` of the thin crate) forward everything untouched and report what crossed as `WireEvent`s, whole frames, nothing
//! truncated; `steps` turns events into cassette steps under the rules, with the dialect of the T3 recorder (the old TS recorder's golden.ts, retired in X5).
use crate::cassette::{Dir, Step, StepKind};
use crate::rules::Normalizer;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Map, Value};

pub const OP_TEXT: u8 = 1;
pub const OP_BINARY: u8 = 2;

/// Header lines as the wire carried them; repeated names are joined by the adapter with ", ".
pub type Headers = Vec<(String, String)>;

#[derive(Clone, Debug, PartialEq)]
pub enum WireEvent {
    /// A whole HTTP exchange (also an upgrade the target refused).
    Http { method: String, path: String, req_headers: Headers, req_body: String, status: u16, res_headers: Headers, res_body: String },
    WsOpen { path: String, status: u16 },
    Ws { dir: Dir, opcode: u8, data: Vec<u8> },
    /// A close frame, or with `dir` `None` the connection dropping without one.
    WsClose { dir: Option<Dir>, code: Option<u16>, reason: String },
}

/// A tap that is listening; the client is pointed at `url` instead of the target.
pub trait Wire {
    fn url(&self) -> String;
    /// Where the log stands now; `since` gives what came after.
    fn mark(&self) -> usize;
    fn since(&self, mark: usize) -> Vec<WireEvent>;
}

fn header(headers: &Headers, name: &str) -> String {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()).unwrap_or_default()
}

/// The request a frame belongs to: its `id`, or for a reply the `requestId` it answers.
pub fn rid(frame: &Value) -> Option<String> {
    match frame.get("id").or_else(|| frame.get("requestId"))? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn step(dir: Dir, channel: &str, kind: StepKind, rid: Option<String>, len: Option<u64>, payload: Value) -> Step {
    Step { dir, channel: Some(channel.into()), kind, rid, len, payload }
}

fn headers_json(h: &Headers) -> Value {
    Value::Object(h.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())
}

fn text_or_null(s: &str) -> Value {
    if s.is_empty() { Value::Null } else { Value::String(s.to_string()) }
}

/// The steps of what a tap saw, as the wire carried it: an HTTP exchange is a call and a return with the raw header lines and the body text,
/// a text frame is JSON when it parses, a binary frame is kept whole as base64. Masking is the rule set's (`mask_step`).
pub fn raw_steps(events: &[WireEvent]) -> Vec<Step> {
    let mut out = Vec::new();
    for ev in events {
        match ev {
            WireEvent::Http { method, path, req_headers, req_body, status, res_headers, res_body } => {
                out.push(step(Dir::Call, "http", StepKind::Json, None, None, json!({"method": method, "path": path, "headers": headers_json(req_headers), "body": text_or_null(req_body)})));
                out.push(step(Dir::Ret, "http", StepKind::Json, None, None, json!({"status": status, "headers": headers_json(res_headers), "body": text_or_null(res_body)})));
            }
            WireEvent::WsOpen { path, status } => out.push(step(Dir::S2c, "ws:open", StepKind::Json, None, None, json!({"path": path, "status": status}))),
            WireEvent::Ws { dir, opcode, data } if *opcode == OP_BINARY => {
                out.push(step(*dir, "ws", StepKind::Binary, None, Some(data.len() as u64), Value::String(STANDARD.encode(data))));
            }
            WireEvent::Ws { dir, data, .. } => {
                let text = String::from_utf8_lossy(data);
                match serde_json::from_str::<Value>(&text) {
                    Ok(frame) => out.push(step(*dir, "ws", StepKind::Json, rid(&frame), None, frame)),
                    Err(_) => out.push(step(*dir, "ws", StepKind::Text, None, None, Value::String(text.into_owned()))),
                }
            }
            WireEvent::WsClose { dir, code, reason } => {
                let payload = json!({"code": code, "reason": reason, "tcp": dir.is_none()});
                out.push(step(dir.unwrap_or(Dir::S2c), "ws:close", StepKind::Json, None, None, payload));
            }
        }
    }
    out
}

/// `raw_steps` masked under the rules.
pub fn steps(events: &[WireEvent], nz: &mut Normalizer) -> Vec<Step> {
    raw_steps(events).iter().map(|s| mask_step(nz, s)).collect()
}

fn headers_of(v: Option<&Value>) -> Headers {
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), String::from)).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    };
    match v {
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), text(v))).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(|p| p.as_array().filter(|p| p.len() == 2).map(|p| (text(&p[0]), text(&p[1])))).collect(),
        _ => Vec::new(),
    }
}

fn http_part(nz: &mut Normalizer, payload: &Value) -> Value {
    let Some(obj) = payload.as_object() else { return nz.json(payload, None) };
    let headers = headers_of(obj.get("headers"));
    let body = match obj.get("body") {
        Some(Value::String(raw)) => nz.body(raw, &header(&headers, "content-type")),
        Some(other) => nz.json(other, None),
        None => Value::Null,
    };
    let mut out = Map::new();
    for (k, v) in obj {
        let masked = match k.as_str() {
            "body" => body.clone(),
            "headers" => json!(nz.headers(&headers)),
            "path" => match v.as_str() {
                Some(p) => Value::String(nz.path(p)),
                None => nz.json(v, None),
            },
            _ => nz.json(v, Some(k)),
        };
        out.insert(k.clone(), masked);
    }
    Value::Object(out)
}

/// Masks one step as a tap's steps are masked, so a driver in any language may report the wire raw: an HTTP exchange's headers, path and
/// body (a string body as the wire carried it), an upgrade's path. Every other step is plain JSON. Masking what is already masked changes nothing.
pub fn mask_step(nz: &mut Normalizer, s: &Step) -> Step {
    if s.kind == StepKind::Binary {
        return s.clone();
    }
    let payload = match (s.channel.as_deref(), s.dir) {
        (Some("http"), Dir::Call | Dir::Ret) => http_part(nz, &s.payload),
        (Some("ws:open"), _) => http_part(nz, &s.payload),
        _ => nz.json(&s.payload, None),
    };
    Step { payload, ..s.clone() }
}

pub const OP_CLOSE: u8 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct WsMessage {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// An incremental WebSocket frame parser: feed raw TCP chunks, get whole messages (fragments joined, control frames as they came).
#[derive(Default)]
pub struct FrameParser {
    buf: Vec<u8>,
    fragments: Vec<u8>,
    fragment_opcode: u8,
}

impl FrameParser {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<WsMessage> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((fin, opcode, payload)) = self.read_frame() {
            if opcode >= 8 {
                out.push(WsMessage { opcode, payload });
                continue;
            }
            if opcode != 0 {
                self.fragment_opcode = opcode;
                self.fragments.clear();
            }
            self.fragments.extend_from_slice(&payload);
            if fin {
                out.push(WsMessage { opcode: self.fragment_opcode, payload: std::mem::take(&mut self.fragments) });
            }
        }
        out
    }

    fn read_frame(&mut self) -> Option<(bool, u8, Vec<u8>)> {
        let b = &self.buf;
        if b.len() < 2 {
            return None;
        }
        let (fin, opcode, masked) = (b[0] & 0x80 != 0, b[0] & 0x0f, b[1] & 0x80 != 0);
        let (len, offset) = match b[1] & 0x7f {
            126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as usize, 4),
            127 if b.len() >= 10 => (u64::from_be_bytes(b[2..10].try_into().ok()?) as usize, 10),
            126 | 127 => return None,
            n => (n as usize, 2),
        };
        let key_len = if masked { 4 } else { 0 };
        let end = offset + key_len + len;
        if b.len() < end {
            return None;
        }
        let mut payload = b[offset + key_len..end].to_vec();
        if masked {
            let key = &b[offset..offset + 4];
            payload.iter_mut().enumerate().for_each(|(i, byte)| *byte ^= key[i % 4]);
        }
        self.buf.drain(..end);
        Some((fin, opcode, payload))
    }
}

/// One unfragmented frame; `mask` is the key of a client frame.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
    let (mask_bit, len) = (if mask.is_some() { 0x80 } else { 0 }, payload.len());
    let mut out = vec![0x80 | opcode];
    match len {
        0..=125 => out.push(mask_bit | len as u8),
        126..=65535 => out.extend([mask_bit | 126, (len >> 8) as u8, len as u8]),
        _ => {
            out.push(mask_bit | 127);
            out.extend((len as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            out.extend(key);
            out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        }
        None => out.extend(payload),
    }
    out
}

/// The status code and reason of a close frame's payload.
pub fn close_info(payload: &[u8]) -> (Option<u16>, String) {
    match payload {
        [a, b, reason @ ..] => (Some(u16::from_be_bytes([*a, *b])), String::from_utf8_lossy(reason).into_owned()),
        _ => (None, String::new()),
    }
}
