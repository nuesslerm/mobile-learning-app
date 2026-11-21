//! `terminal.subscribe` over rca's `terminal.read` and a held `terminal.attach`: the refusals, the lease and exited answers and the JSON
//! stream (`scrollback`, `data`, `end`). The scrollback replays rca's retained tail, there is no screen to serialize.
use crate::binary::{replay_frames, Binary};
use crate::replay::{strip_queries, trim_to_budget, Utf8Carry};
use crate::resume;
use crate::{addressed, ask, client_param, dimension, handle_of, handle_param, lines_of, parse, refusal, DEFAULT_LINES};
use agent_api_terminal::{b64, kind, ATTACH, READ};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::area::Out;
use orca_core::{Ctx, EventStream, StreamEvent, Subscription};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const POLL_WAIT: Duration = Duration::from_millis(1);
const MAX_EVENTS: usize = 256;

struct Params {
    client_id: Option<String>,
    mobile: bool,
    viewport: Option<(u64, u64)>,
    binary: bool,
    lease_only: bool,
    output_ack: bool,
    write_unavailable: bool,
    budget: Option<usize>,
    resume: Option<(String, u64)>,
}

fn literal_one(caps: &Value, key: &str) -> Result<bool, Failure> {
    match caps.get(key) {
        None => Ok(false),
        Some(v) if v.as_f64() == Some(1.0) => Ok(true),
        Some(_) => Err(Failure::invalid("Invalid input: expected 1")),
    }
}

fn whole(value: &Value, min: u64, floor: &str) -> Result<u64, Failure> {
    match value {
        Value::Number(n) => match (n.as_u64(), n.as_i64()) {
            (Some(v), _) if v >= min => Ok(v),
            (_, Some(_)) => Err(Failure::invalid(floor)),
            _ if n.is_f64() => Err(Failure::invalid("Invalid input: expected int, received number")),
            _ => Err(Failure::invalid(floor)),
        },
        _ => Err(Failure::invalid("Invalid input: expected number, received undefined")),
    }
}

impl Params {
    fn read(params: &Value) -> Result<Params, Failure> {
        handle_param(params)?;
        client_param(params, false)?;
        let client = params.get("client").filter(|c| c.is_object());
        let viewport = match params.get("viewport") {
            None => None,
            Some(Value::Object(_)) => {
                let v = &params["viewport"];
                Some((dimension(v, "cols", 1, 1000)?, dimension(v, "rows", 1, 500)?))
            }
            Some(_) => return Err(Failure::invalid("Invalid input: expected object, received undefined")),
        };
        let caps = match params.get("capabilities") {
            None => Value::Null,
            Some(c @ Value::Object(_)) => c.clone(),
            Some(_) => return Err(Failure::invalid("Invalid input: expected object, received undefined")),
        };
        let (binary, lease_only) = (literal_one(&caps, "terminalBinaryStream")?, literal_one(&caps, "mobileInputLeaseOnly")?);
        literal_one(&caps, "desktopViewportClaims")?;
        let (write_unavailable, output_ack) = (literal_one(&caps, "writeUnavailable")?, literal_one(&caps, "outputAck")?);
        let mut resume = None;
        if let Some(r) = params.get("resume") {
            let Some(token) = r.get("streamToken").and_then(Value::as_str).filter(|t| !t.is_empty()) else {
                return Err(Failure::invalid("Missing stream token"));
            };
            resume = Some((token.to_string(), whole(r.get("units").unwrap_or(&Value::Null), 0, "Too small: expected number to be >=0")?));
        }
        let budget = params.get("snapshotByteBudget").map(|b| whole(b, 1, "Too small: expected number to be >0")).transpose()?;
        Ok(Params {
            client_id: client.and_then(|c| c["id"].as_str()).map(str::to_string),
            mobile: client.is_some_and(|c| c["type"] == "mobile"),
            viewport,
            binary,
            lease_only,
            output_ack,
            write_unavailable,
            budget: budget.map(|b| b as usize),
            resume,
        })
    }
}

fn subscribed(lines: Vec<String>, truncated: bool) -> Value {
    json!({"type": "subscribed", "streamId": null, "lines": lines, "truncated": truncated})
}

/// The last lines of the retained bytes; truncated when older lines are gone, either dropped here or from rca's tail.
fn tail(bytes: &[u8], start_offset: u64) -> (Vec<String>, bool) {
    let mut lines = lines_of(bytes);
    let dropped = lines.len().saturating_sub(DEFAULT_LINES);
    lines.drain(..dropped);
    (lines, dropped > 0 || start_offset > 0)
}

struct Live {
    events: Box<dyn EventStream>,
    carry: Utf8Carry,
}

impl Live {
    /// Turns one attach event into frames; true when the terminal is over.
    fn apply(&mut self, event: StreamEvent, out: &mut Vec<Value>) -> bool {
        match event.kind.as_str() {
            kind::OUTPUT | kind::SNAPSHOT => {
                let bytes = event.data["data"].as_str().and_then(b64::decode).unwrap_or_default();
                let text = self.carry.push(&bytes).text;
                if !text.is_empty() {
                    out.push(json!({"type": "data", "chunk": text}));
                }
                false
            }
            kind::EXITED | kind::CLOSED => true,
            _ => false,
        }
    }
}

struct Stream {
    id: String,
    first: Vec<Value>,
    live: Option<Live>,
    finished: bool,
}

impl Subscription for Stream {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        let mut out = std::mem::take(&mut self.first);
        let Some(live) = self.live.as_mut() else { return out };
        let mut over = false;
        for _ in 0..MAX_EVENTS {
            match live.events.next(POLL_WAIT) {
                Ok(Some(event)) => {
                    if live.apply(event, &mut out) {
                        over = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    over = true;
                    break;
                }
            }
        }
        if over {
            self.live = None;
            self.finished = true;
            out.push(json!({"type": "end"}));
        }
        out
    }

    fn end(&mut self) -> Vec<Value> {
        self.live = None;
        if std::mem::replace(&mut self.finished, true) { Vec::new() } else { vec![json!({"type": "end"})] }
    }
}

/// Ends the stream the phone names: by the request that opened it, else by the id the real host builds (`<id>`, or `<id>:<client id>`).
pub(crate) fn unsubscribe(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    let id = params.get("subscriptionId").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing subscription ID"))?;
    if let Some(request) = params.get("requestId").and_then(Value::as_str).filter(|r| !r.is_empty()) {
        ctx.subscriptions.cancel_request(request);
        return Ok(json!({"unsubscribed": true}));
    }
    let client = params.get("client").and_then(|c| c["id"].as_str()).filter(|c| !c.is_empty());
    let full = match client {
        Some(client) => format!("terminal:{}:{id}:{client}", ctx.connection_id),
        None => format!("terminal:{}:{id}", ctx.connection_id),
    };
    ctx.subscriptions.cancel(&full);
    Ok(json!({"unsubscribed": true}))
}

pub(crate) fn open(ctx: &mut Ctx, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
    let p = Params::read(params)?;
    if p.mobile && !p.binary {
        return Err(Failure::new(RUNTIME_ERROR, "binary_terminal_stream_required"));
    }
    let (workspace, terminal) = parse(params)?;
    let agent = ctx.agent_handle.clone();
    let read = ask(agent.as_ref(), READ, addressed(&workspace, &terminal))?;
    let bytes = read["data"].as_str().and_then(b64::decode).unwrap_or_default();
    let (start, end) = (read["start_offset"].as_u64().unwrap_or(0), read["offset"].as_u64().unwrap_or(0));
    let handle = handle_of(&workspace, &terminal);
    let claimed = p.resume.as_ref().filter(|_| p.binary).and_then(|(token, units)| {
        resume::registry().lock().unwrap_or_else(|e| e.into_inner()).claim(token, ctx.device_id, &workspace, &terminal, *units, Instant::now())
    });
    let id = match &p.client_id {
        Some(client) => format!("terminal:{}:{handle}:{client}", ctx.connection_id),
        None => format!("terminal:{}:{handle}", ctx.connection_id),
    };
    let stream = |first: Vec<Value>, live: Option<Live>, finished: bool| -> Box<dyn Subscription> { Box::new(Stream { id: id.clone(), first, live, finished }) };

    if read["status"] != "running" {
        let (lines, truncated) = tail(&bytes, start);
        let done = stream(vec![subscribed(lines, truncated), json!({"type": "end"})], None, true);
        ctx.subscriptions.cancel(&id);
        return Ok(done);
    }
    if p.mobile && p.lease_only && p.client_id.is_some() {
        let lease = stream(vec![subscribed(Vec::new(), false)], None, false);
        ctx.subscriptions.cancel(&id);
        return Ok(lease);
    }
    if p.binary {
        let missed = claimed.and_then(|from| {
            let mut request = addressed(&workspace, &terminal);
            request["after_offset"] = json!(from);
            ask(agent.as_ref(), READ, request).ok().filter(|r| r["reset"] != true).map(|r| (from, r))
        });
        let (follow_from, read) = match &missed {
            Some((_, r)) => (r["offset"].as_u64().unwrap_or(0), r),
            None => (end, &read),
        };
        let (cols, rows) = (read["cols"].as_u64().or(p.viewport.map(|v| v.0)), read["rows"].as_u64().or(p.viewport.map(|v| v.1)));
        let attach = json!({"workspace": workspace, "terminal": terminal, "after_offset": follow_from});
        let (_, events) = agent.follow(ATTACH.name(), &format!("terminal/{workspace}/{terminal}"), attach, "after_offset").map_err(refusal)?;
        let number = ctx.stream_number();
        let token = p.output_ack.then(|| resume::registry().lock().unwrap_or_else(|e| e.into_inner()).register(ctx.connection_id, ctx.device_id, &workspace, &terminal));
        let mut opened = json!({"type": "subscribed", "streamId": number, "lines": [], "truncated": false, "cols": cols, "rows": rows, "displayMode": "auto"});
        if missed.is_some() {
            opened["resumed"] = json!(true);
        }
        let cursor = missed.as_ref().map_or(end, |(from, _)| *from);
        if let Some(t) = &token {
            opened["outputAck"] = json!(true);
            opened["streamToken"] = json!(t.id);
            resume::registry().lock().unwrap_or_else(|e| e.into_inner()).record(&t.id, 0, cursor);
        }
        let mut carry = Utf8Carry::default();
        let mut first = vec![Out::Text(opened.to_string())];
        if missed.is_none() {
            first.extend(replay_frames(number, &mut carry, &bytes, start, end, p.budget, (cols, rows)));
        }
        let mut binary = Binary { id: id.clone(), number, agent: agent.clone(), target: (workspace, terminal), first: Vec::new(), events, carry, token, write_unavailable: p.write_unavailable, budget: p.budget, size: (cols, rows), finished: false, units: 0, cursor, paused: false, exited: false };
        if missed.is_some() {
            binary.catch_up(&mut first);
        }
        binary.first = first;
        ctx.subscriptions.cancel(&id);
        return Ok(Box::new(binary));
    }

    let mut carry = Utf8Carry::default();
    let replay = strip_queries(&carry.push(&bytes).text);
    let (serialized, trimmed) = match p.budget {
        Some(budget) => {
            let (kept, trimmed) = trim_to_budget(&replay, budget, 0);
            (kept.to_string(), trimmed)
        }
        None => (replay, false),
    };
    let (lines, truncated) = tail(&bytes, start);
    let cols = read["cols"].as_u64().or(p.viewport.map(|v| v.0));
    let rows = read["rows"].as_u64().or(p.viewport.map(|v| v.1));
    let scrollback = json!({"type": "scrollback", "lines": lines, "truncated": truncated || trimmed, "serialized": serialized, "cols": cols, "rows": rows, "displayMode": "auto"});
    let attach = json!({"workspace": workspace, "terminal": terminal, "after_offset": end});
    let (_, events) = agent.follow(ATTACH.name(), &format!("terminal/{workspace}/{terminal}"), attach, "after_offset").map_err(refusal)?;
    let live = stream(vec![scrollback], Some(Live { events, carry }), false);
    ctx.subscriptions.cancel(&id);
    Ok(live)
}
