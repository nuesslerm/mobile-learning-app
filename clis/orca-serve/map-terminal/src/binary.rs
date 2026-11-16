//! The phone's binary terminal stream: the 16-byte frame codec, the snapshot frames and the stream object that sends live output from rca's
//! attach and takes the phone's input, resize and unsubscribe frames.
use crate::replay::{strip_queries, trim_to_budget, units, Utf8Carry};
use crate::resume::{self, Token};
use crate::{addressed, ask, viewport_param};
use agent_api_terminal::{b64, kind, READ, RESIZE, WRITE};
use orca_core::area::Out;
use orca_core::{AgentApi, EventStream, StreamEvent, Subscription};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const KIND: u8 = 0x74;
const VERSION: u8 = 1;
pub const HEADER: usize = 16;
pub const CHUNK_BYTES: usize = 16 * 1024;
const POLL_WAIT: Duration = Duration::from_millis(1);
const MAX_EVENTS: usize = 256;
const MAX_DISCARD: usize = 4096;

pub mod op {
    pub const OUTPUT: u8 = 1;
    pub const SNAPSHOT_START: u8 = 2;
    pub const SNAPSHOT_CHUNK: u8 = 3;
    pub const SNAPSHOT_END: u8 = 4;
    pub const RESIZED: u8 = 5;
    pub const INPUT: u8 = 7;
    pub const RESIZE: u8 = 8;
    pub const UNSUBSCRIBE: u8 = 10;
    pub const WRITE_UNAVAILABLE: u8 = 17;
}

#[derive(Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub opcode: u8,
    pub stream: u32,
    pub seq: u64,
    pub payload: &'a [u8],
}

pub fn encode(opcode: u8, stream: u32, seq: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + payload.len());
    out.extend_from_slice(&[KIND, VERSION, opcode, 0]);
    out.extend_from_slice(&stream.to_le_bytes());
    out.extend_from_slice(&((seq >> 32) as u32).to_le_bytes());
    out.extend_from_slice(&(seq as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn decode(bytes: &[u8]) -> Option<Frame<'_>> {
    if bytes.len() < HEADER || bytes[0] != KIND || bytes[1] != VERSION {
        return None;
    }
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some(Frame { opcode: bytes[2], stream: word(4), seq: (u64::from(word(8)) << 32) | u64::from(word(12)), payload: &bytes[HEADER..] })
}

/// What a snapshot says of itself: `seq` is the rca byte offset the text ends at, so live output after it orders correctly.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub cols: Option<u64>,
    pub rows: Option<u64>,
    pub seq: u64,
    pub truncated: bool,
    pub by_budget: bool,
}

fn chunks(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = rest.len().min(CHUNK_BYTES);
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        let (head, tail) = rest.split_at(cut);
        parts.push(head);
        rest = tail;
    }
    parts
}

/// `SnapshotStart` with the meta, one chunk per 16 KiB of text, `SnapshotEnd`.
pub fn snapshot_frames(stream: u32, shape: Shape, text: &str) -> Vec<Out> {
    let meta = json!({"kind": "scrollback", "cols": shape.cols, "rows": shape.rows, "displayMode": "auto", "seq": shape.seq, "truncated": shape.truncated, "truncatedByByteBudget": shape.by_budget, "source": "headless"});
    let mut out = vec![Out::Binary(encode(op::SNAPSHOT_START, stream, shape.seq, meta.to_string().as_bytes()))];
    out.extend(chunks(text).into_iter().map(|c| Out::Binary(encode(op::SNAPSHOT_CHUNK, stream, shape.seq, c.as_bytes()))));
    out.push(Out::Binary(encode(op::SNAPSHOT_END, stream, shape.seq, &[])));
    out
}

/// A tail replayed as a snapshot: queries stripped so the phone's terminal never answers one into the shell, then cut to the byte budget.
pub fn replay_frames(stream: u32, carry: &mut Utf8Carry, bytes: &[u8], start: u64, end: u64, budget: Option<usize>, size: (Option<u64>, Option<u64>)) -> Vec<Out> {
    let replay = strip_queries(&carry.push(bytes).text);
    let (text, by_budget) = match budget {
        Some(b) => {
            let (kept, trimmed) = trim_to_budget(&replay, b, 0);
            (kept.to_string(), trimmed)
        }
        None => (replay, false),
    };
    let shape = Shape { cols: size.0, rows: size.1, seq: end - carry.held() as u64, truncated: start > 0, by_budget };
    snapshot_frames(stream, shape, &text)
}

fn end_event() -> Out {
    Out::Text(json!({"type": "end"}).to_string())
}

pub(crate) struct Binary {
    pub id: String,
    pub number: u32,
    pub agent: Arc<dyn AgentApi>,
    pub target: (String, String),
    pub first: Vec<Out>,
    pub events: Box<dyn EventStream>,
    pub carry: Utf8Carry,
    pub token: Option<Token>,
    pub write_unavailable: bool,
    pub budget: Option<usize>,
    pub size: (Option<u64>, Option<u64>),
    pub finished: bool,
    pub units: u64,
    /// The rca byte offset fed to `carry`: events at or before it are already sent.
    pub cursor: u64,
    /// The credit window is full: the attach is discarded and `terminal.read` refills from `cursor` once the phone acknowledges.
    pub paused: bool,
    pub exited: bool,
}

impl Binary {
    fn record(&self, offset: u64) {
        if let Some(token) = &self.token {
            resume::registry().lock().unwrap_or_else(|e| e.into_inner()).record(&token.id, self.units, offset);
        }
    }

    fn finish(&mut self, out: &mut Vec<Out>) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        if let Some(token) = &self.token {
            resume::registry().lock().unwrap_or_else(|e| e.into_inner()).release(&token.id, Instant::now());
        }
        out.push(end_event());
    }

    /// Sends `bytes`, which end at rca offset `end`, as one Output frame and counts its units against the window.
    fn feed(&mut self, bytes: &[u8], end: u64, out: &mut Vec<Out>) {
        let text = self.carry.push(bytes).text;
        self.cursor = end;
        if text.is_empty() {
            return;
        }
        let offset = end - self.carry.held() as u64;
        let sent = units(&text) as u64;
        self.units += sent;
        if let Some(token) = &self.token {
            token.credit.sent(sent);
        }
        self.record(offset);
        out.push(Out::Binary(encode(op::OUTPUT, self.number, offset, text.as_bytes())));
    }

    fn open_window(&self) -> bool {
        self.token.as_ref().is_none_or(|t| t.credit.can_send())
    }

    /// Reads what the stream has not sent from `cursor`: bytes while the window has room, or a recovery snapshot when rca's tail moved past it.
    /// True when the terminal is over.
    pub(crate) fn catch_up(&mut self, out: &mut Vec<Out>) -> bool {
        let mut request = addressed(&self.target.0, &self.target.1);
        request["after_offset"] = json!(self.cursor);
        let Ok(read) = ask(self.agent.as_ref(), READ, request) else { return true };
        let bytes = read["data"].as_str().and_then(b64::decode).unwrap_or_default();
        let (start, end) = (read["start_offset"].as_u64().unwrap_or(0), read["offset"].as_u64().unwrap_or(0));
        if read["reset"] == true {
            self.carry = Utf8Carry::default();
            self.cursor = end;
            out.extend(replay_frames(self.number, &mut self.carry, &bytes, start, end, self.budget, self.size));
            self.record(end - self.carry.held() as u64);
        } else {
            let mut at = 0;
            while at < bytes.len() && self.open_window() {
                let cut = (at + CHUNK_BYTES).min(bytes.len());
                self.feed(&bytes[at..cut], start + cut as u64, out);
                at = cut;
            }
        }
        self.paused = self.cursor < end;
        if read["status"] != "running" && !self.paused {
            self.exited = true;
        }
        self.exited && !self.paused
    }

    /// While paused: drops what the attach queued (the read gets it again) and refills once the phone has acknowledged.
    fn refill(&mut self, out: &mut Vec<Out>) -> bool {
        for _ in 0..MAX_DISCARD {
            match self.events.next(POLL_WAIT) {
                Ok(Some(event)) => self.exited |= matches!(event.kind.as_str(), kind::EXITED | kind::CLOSED),
                Ok(None) => break,
                Err(_) => {
                    self.exited = true;
                    break;
                }
            }
        }
        if self.open_window() {
            return self.catch_up(out);
        }
        false
    }

    /// Turns one attach event into frames; true when the terminal is over.
    fn apply(&mut self, event: StreamEvent, out: &mut Vec<Out>) -> bool {
        match event.kind.as_str() {
            kind::OUTPUT => {
                let bytes = event.data["data"].as_str().and_then(b64::decode).unwrap_or_default();
                let start = event.seq.saturating_sub(bytes.len() as u64);
                if event.seq > self.cursor {
                    let skip = (self.cursor.saturating_sub(start) as usize).min(bytes.len());
                    self.feed(&bytes[skip..], event.seq, out);
                }
                false
            }
            kind::SNAPSHOT => {
                let bytes = event.data["data"].as_str().and_then(b64::decode).unwrap_or_default();
                let start = event.data["start_offset"].as_u64().unwrap_or(0);
                self.carry = Utf8Carry::default();
                self.cursor = event.seq;
                out.extend(replay_frames(self.number, &mut self.carry, &bytes, start, event.seq, self.budget, self.size));
                self.record(event.seq - self.carry.held() as u64);
                false
            }
            kind::CLEARED => {
                self.carry = Utf8Carry::default();
                self.cursor = event.seq;
                out.extend(snapshot_frames(self.number, Shape { cols: self.size.0, rows: self.size.1, seq: event.seq, truncated: false, by_budget: false }, ""));
                self.record(event.seq);
                false
            }
            kind::EXITED | kind::CLOSED => true,
            _ => false,
        }
    }

    fn rejected(&self, out: &mut Vec<Out>) {
        if self.write_unavailable {
            out.push(Out::Binary(encode(op::WRITE_UNAVAILABLE, self.number, 0, &[])));
        }
    }
}

impl Subscription for Binary {
    fn id(&self) -> &str {
        &self.id
    }

    fn number(&self) -> Option<u32> {
        Some(self.number)
    }

    fn poll(&mut self) -> Vec<Value> {
        Vec::new()
    }

    fn end(&mut self) -> Vec<Value> {
        Vec::new()
    }

    fn poll_out(&mut self) -> Vec<Out> {
        let mut out = std::mem::take(&mut self.first);
        if self.finished {
            return out;
        }
        let mut over = false;
        if self.paused {
            over = self.refill(&mut out);
        }
        for _ in 0..MAX_EVENTS {
            if over || self.paused {
                break;
            }
            if !self.open_window() {
                self.paused = true;
                break;
            }
            match self.events.next(POLL_WAIT) {
                Ok(Some(event)) => {
                    if self.apply(event, &mut out) {
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
            self.finish(&mut out);
        }
        out
    }

    fn end_out(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        self.finish(&mut out);
        out
    }

    fn on_binary(&mut self, frame: &[u8]) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(frame) = decode(frame).filter(|_| !self.finished) else { return out };
        let (workspace, terminal) = (&self.target.0, &self.target.1);
        match frame.opcode {
            op::INPUT => {
                let text = String::from_utf8_lossy(frame.payload);
                if text.is_empty() {
                    return out;
                }
                let mut request = addressed(workspace, terminal);
                request["data"] = json!(text);
                if ask(self.agent.as_ref(), WRITE, request).is_err() {
                    self.rejected(&mut out);
                }
            }
            op::RESIZE => {
                let Ok(size) = serde_json::from_slice::<Value>(frame.payload) else { return out };
                let Ok((cols, rows)) = viewport_param(&json!({"viewport": size})) else { return out };
                let mut request = addressed(workspace, terminal);
                request["cols"] = json!(cols);
                request["rows"] = json!(rows);
                if ask(self.agent.as_ref(), RESIZE, request).is_ok() {
                    self.size = (Some(cols), Some(rows));
                    let body = json!({"cols": cols, "rows": rows, "displayMode": "auto", "reason": "resize"});
                    out.push(Out::Binary(encode(op::RESIZED, self.number, 0, body.to_string().as_bytes())));
                }
            }
            op::UNSUBSCRIBE => self.finish(&mut out),
            _ => {}
        }
        out
    }
}
