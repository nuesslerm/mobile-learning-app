//! When a recorded reply may go out. A socket whose replies name the request they answer is replayed by request: a live request
//! takes a recorded one and gets that one's replies, in recorded order, under the live id. Any other socket is a strict script.
use super::interactions::{CHANNEL_CLOSE, CHANNEL_WS};
use super::matching::{envelopes, id_text, method, pick, reply_id, request_id, tag, without_id, Candidate, Key, Keys};
use super::mismatch::{nearest, Mismatch};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mimic_core::cassette::{Dir, Interaction, StepKind};
use serde_json::{json, Value};
use std::collections::HashSet;

const PING: &str = "Ping";
const PONG: &str = "Pong";
const INTERRUPT: &str = "Interrupt";

#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Json(Value),
    Text(String),
    Binary(Vec<u8>),
}

pub struct Asked {
    pub method: String,
    pub key: Key,
}

/// One recorded message, or the part of one that belongs to a single request.
pub struct Unit {
    pub step: usize,
    pub dir: Dir,
    pub payload: Payload,
    pub rid: Option<String>,
    pub asked: Option<Asked>,
    /// The forms of a client message that asks under no id.
    pub key: Option<Key>,
    pub tag: String,
    pub close: Option<Option<u16>>,
}

pub struct Script {
    pub interaction: usize,
    pub id: String,
    pub correlated: bool,
    pub units: Vec<Unit>,
}

fn unit(step: usize, dir: Dir, payload: Payload) -> Unit {
    Unit { step, dir, payload, rid: None, asked: None, key: None, tag: String::new(), close: None }
}

impl Script {
    pub fn of(interaction: usize, it: &Interaction, keys: &Keys) -> Script {
        let frames: Vec<_> = it.steps.iter().enumerate().filter(|(_, s)| matches!(s.channel.as_deref(), Some(CHANNEL_WS) | Some(CHANNEL_CLOSE))).collect();
        let json_of = |dir: Dir| frames.iter().filter(move |(_, s)| s.dir == dir && s.kind == StepKind::Json && s.channel.as_deref() == Some(CHANNEL_WS)).flat_map(|(_, s)| envelopes(&s.payload));
        let asked: HashSet<String> = json_of(Dir::C2s).filter_map(request_id).collect();
        let correlated = json_of(Dir::S2c).filter_map(reply_id).any(|id| asked.contains(&id));
        let mut units = Vec::new();
        for (n, step) in frames {
            if step.channel.as_deref() == Some(CHANNEL_CLOSE) {
                if step.payload["tcp"] != true {
                    let code = step.payload["code"].as_u64().and_then(|c| u16::try_from(c).ok());
                    units.push(Unit { close: Some(code), ..unit(n, step.dir, Payload::Text(String::new())) });
                }
                continue;
            }
            match step.kind {
                StepKind::Binary => units.push(unit(n, step.dir, Payload::Binary(STANDARD.decode(step.payload.as_str().unwrap_or_default()).unwrap_or_default()))),
                StepKind::Json if correlated && step.dir == Dir::C2s => units.extend(envelopes(&step.payload).into_iter().filter(|e| tag(e) != PING).map(|e| asking(n, e, keys))),
                StepKind::Json if correlated => units.extend(answering(n, &step.payload, &asked)),
                StepKind::Json => units.push(Unit { key: Some(keys.frame(&step.payload)), ..unit(n, step.dir, Payload::Json(step.payload.clone())) }),
                _ => units.push(unit(n, step.dir, Payload::Text(step.payload.as_str().unwrap_or_default().to_string()))),
            }
        }
        Script { interaction, id: it.id.clone(), correlated, units }
    }
}

fn asking(step: usize, envelope: &Value, keys: &Keys) -> Unit {
    let base = Unit { tag: tag(envelope).to_string(), ..unit(step, Dir::C2s, Payload::Json(envelope.clone())) };
    if let Some(id) = request_id(envelope) {
        return Unit { rid: Some(id), asked: Some(Asked { method: method(envelope), key: keys.frame(&without_id(envelope)) }), ..base };
    }
    match reply_id(envelope) {
        Some(id) => Unit { rid: Some(id), ..base },
        None => Unit { key: Some(keys.frame(envelope)), ..base },
    }
}

/// A server message as one unit when all of it answers one request, else one unit per envelope.
fn answering(step: usize, message: &Value, asked: &HashSet<String>) -> Vec<Unit> {
    let kept: Vec<&Value> = envelopes(message).into_iter().filter(|e| tag(e) != PONG).collect();
    let rid = |e: &Value| reply_id(e).filter(|id| asked.contains(id));
    let Some(first) = kept.first() else { return Vec::new() };
    let whole = kept.len() == envelopes(message).len();
    if whole && kept.iter().all(|e| rid(e) == rid(first)) {
        return vec![Unit { rid: rid(first), tag: tag(first).to_string(), ..unit(step, Dir::S2c, Payload::Json(message.clone())) }];
    }
    kept.iter().map(|e| Unit { rid: rid(e), tag: tag(e).to_string(), ..unit(step, Dir::S2c, Payload::Json((*e).clone())) }).collect()
}

fn rebind(payload: &Payload, recorded: &str, live: &Value) -> Payload {
    let Payload::Json(value) = payload else { return payload.clone() };
    let mut value = value.clone();
    let fix = |envelope: &mut Value| {
        if let Some(fields) = envelope.as_object_mut() {
            for name in ["requestId", "id"] {
                if fields.get(name).and_then(id_text).as_deref() == Some(recorded) {
                    fields.insert(name.to_string(), live.clone());
                }
            }
        }
    };
    match &mut value {
        Value::Array(items) => items.iter_mut().for_each(fix),
        other => fix(other),
    }
    Payload::Json(value)
}

#[derive(Debug, PartialEq)]
pub enum Out {
    Send(Payload),
    Close(Option<u16>),
    Miss(Mismatch),
}

/// A recorded reply that could go out next: recordings of this socket before those borrowed from a peer, then recorded order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Next {
    foreign: bool,
    unit: usize,
    binding: Option<usize>,
}

struct Binding {
    script: usize,
    rid: String,
    live: Value,
    asked_at: usize,
    units: Vec<usize>,
    next: usize,
}

/// One live socket: which recording it plays and how far each of its requests has been answered.
pub struct Session {
    pub script: usize,
    /// Other recordings of the same route, searched when this one has no such request.
    pub peers: Vec<usize>,
    pub label: String,
    pub matched: u32,
    pub reused: u32,
    cursor: usize,
    bindings: Vec<Binding>,
    satisfied: HashSet<usize>,
    used: HashSet<(usize, usize)>,
    pushed: HashSet<usize>,
    last_client_ms: u64,
}

impl Session {
    pub fn new(script: usize, peers: Vec<usize>, label: String, now_ms: u64) -> Session {
        Session { script, peers, label, matched: 0, reused: 0, cursor: 0, bindings: Vec::new(), satisfied: HashSet::new(), used: HashSet::new(), pushed: HashSet::new(), last_client_ms: now_ms }
    }

    fn miss(&self, what: &str, reason: &str, received: &Value, near: Option<(&Script, &Unit, &Value)>) -> Out {
        let near = near.map(|(script, unit, expected)| nearest(&script.id, script.interaction + 1, Some(unit.step), expected, received));
        Out::Miss(Mismatch::new("ws", what, Some(self.label.clone()), reason, received, near))
    }

    /// What a client message does: binds its requests to recorded ones, or reports the one the recording does not have.
    pub fn client(&mut self, scripts: &[Script], message: &Payload, keys: &Keys, now_ms: u64) -> Vec<Out> {
        self.last_client_ms = now_ms;
        let own = &scripts[self.script];
        if !own.correlated {
            return self.scripted(own, message, keys);
        }
        let Payload::Json(value) = message else {
            let known = own.units.iter().any(|u| u.dir == Dir::C2s && u.payload == *message);
            return if known { Vec::new() } else { vec![self.miss("<frame>", "the recording has no such frame", &json!({"kind": "not json"}), None)] };
        };
        let mut out = Vec::new();
        for envelope in envelopes(value) {
            if tag(envelope) == PING {
                out.push(Out::Send(Payload::Json(json!({"_tag": PONG}))));
            } else if request_id(envelope).is_some() {
                out.extend(self.ask(scripts, envelope, keys));
            } else if envelope.get("requestId").is_some() {
                if tag(envelope) == INTERRUPT {
                    self.interrupt(scripts, &envelope["requestId"]);
                }
            } else {
                let key = keys.frame(envelope);
                let known = own.units.iter().filter_map(|u| u.key.as_ref()).any(|have| have.strict == key.strict || have.loose == key.loose);
                if !known {
                    out.push(self.miss(tag(envelope), "the recording has no such message", &key.shown, None));
                }
            }
        }
        out
    }

    fn ask(&mut self, scripts: &[Script], envelope: &Value, keys: &Keys) -> Option<Out> {
        let (wanted, key) = (method(envelope), keys.frame(&without_id(envelope)));
        let order: Vec<usize> = std::iter::once(self.script).chain(self.peers.iter().copied()).collect();
        let recorded: Vec<(usize, usize, &Asked)> = order
            .iter()
            .flat_map(|s| scripts[*s].units.iter().enumerate().filter_map(move |(u, unit)| unit.asked.as_ref().map(|a| (*s, u, a))))
            .filter(|(_, _, a)| a.method == wanted)
            .collect();
        let candidates: Vec<Candidate> = recorded.iter().enumerate().map(|(at, (s, u, a))| Candidate { at, key: &a.key, used: self.used.contains(&(*s, *u)) }).collect();
        let Some((at, reused)) = pick(&candidates, &key) else {
            let what = if wanted.is_empty() { "<no method>" } else { wanted.as_str() };
            return Some(match recorded.first() {
                Some((s, u, a)) => self.miss(what, "no recorded request of this method has this payload", &key.shown, Some((&scripts[*s], &scripts[*s].units[*u], &a.key.shown))),
                None => self.miss(what, "the recording has no request of this method", &key.shown, None),
            });
        };
        let (script, asked_at, _) = recorded[at];
        let rid = scripts[script].units[asked_at].rid.clone().unwrap_or_default();
        let later = scripts[script].units.iter().enumerate().skip(asked_at + 1);
        let units = later.take_while(|(_, u)| !(u.asked.is_some() && u.rid.as_deref() == Some(rid.as_str()))).filter(|(_, u)| u.dir == Dir::S2c && u.rid.as_deref() == Some(rid.as_str())).map(|(i, _)| i).collect();
        self.used.insert((script, asked_at));
        if script == self.script {
            self.satisfied.insert(asked_at);
        }
        self.matched += 1;
        self.reused += u32::from(reused);
        self.bindings.push(Binding { script, rid, live: envelope["id"].clone(), asked_at, units, next: 0 });
        None
    }

    /// The client stopped a request: what the recording sent after its own interrupt still goes out, nothing before it.
    fn interrupt(&mut self, scripts: &[Script], live: &Value) {
        let Some(binding) = self.bindings.iter_mut().rev().find(|b| b.live == *live) else { return };
        let units = &scripts[binding.script].units;
        let recorded = units.iter().enumerate().skip(binding.asked_at + 1).find(|(_, u)| u.dir == Dir::C2s && u.tag == INTERRUPT && u.rid.as_deref() == Some(binding.rid.as_str()));
        binding.next = match recorded {
            Some((at, _)) => binding.units.iter().position(|u| *u > at).unwrap_or(binding.units.len()),
            None => binding.units.len(),
        };
    }

    fn open(&self, own: &Script, asked_at: Option<usize>, upto: usize, quiet: bool) -> bool {
        quiet || own.units[..upto].iter().enumerate().all(|(i, u)| u.asked.is_none() || asked_at.is_some_and(|a| i <= a) || self.satisfied.contains(&i))
    }

    /// The recorded replies that may go out now, in recorded order. A reply recorded after other requests waits until the client
    /// has made them too, or has been quiet for `settle_ms`.
    pub fn due(&mut self, scripts: &[Script], now_ms: u64, settle_ms: u64) -> Vec<Out> {
        let own = &scripts[self.script];
        let mut out = Vec::new();
        if !own.correlated {
            while let Some(next) = own.units.get(self.cursor).filter(|u| u.dir == Dir::S2c) {
                out.push(next.close.map_or_else(|| Out::Send(next.payload.clone()), Out::Close));
                self.cursor += 1;
            }
            return out;
        }
        let quiet = now_ms.saturating_sub(self.last_client_ms) >= settle_ms;
        loop {
            let answers = self.bindings.iter().enumerate().filter_map(|(at, binding)| {
                let unit = *binding.units.get(binding.next)?;
                let foreign = binding.script != self.script;
                (foreign || self.open(own, Some(binding.asked_at), unit, quiet)).then_some(Next { foreign, unit, binding: Some(at) })
            });
            let unasked = own.units.iter().enumerate().find(|(i, u)| u.dir == Dir::S2c && u.rid.is_none() && u.close.is_none() && !self.pushed.contains(i));
            let pushed = unasked.filter(|(unit, _)| self.open(own, None, *unit, quiet)).map(|(unit, _)| Next { foreign: false, unit, binding: None });
            // two bindings of one recording tie on the unit: the one that asked first has the lower index and goes first
            let Some(Next { unit: next, binding, .. }) = answers.chain(pushed).min() else { break };
            match binding {
                Some(at) => {
                    let binding = &mut self.bindings[at];
                    out.push(Out::Send(rebind(&scripts[binding.script].units[next].payload, &binding.rid, &binding.live)));
                    binding.next += 1;
                }
                None => {
                    self.pushed.insert(next);
                    out.push(Out::Send(own.units[next].payload.clone()));
                }
            }
        }
        out
    }

    fn scripted(&mut self, own: &Script, message: &Payload, keys: &Keys) -> Vec<Out> {
        let expected = own.units.get(self.cursor).filter(|u| u.dir == Dir::C2s && u.close.is_none());
        let (same, shown) = match (expected, message) {
            (Some(unit), Payload::Json(value)) => {
                let key = keys.frame(value);
                (unit.key.as_ref().is_some_and(|have| have.strict == key.strict || have.loose == key.loose), key.shown)
            }
            (Some(unit), other) => (unit.payload == *other, json!({"kind": "not json"})),
            (None, Payload::Json(value)) => (false, keys.frame(value).shown),
            (None, _) => (false, json!({"kind": "not json"})),
        };
        if same {
            self.cursor += 1;
            self.matched += 1;
            return Vec::new();
        }
        let near = expected.and_then(|unit| unit.key.as_ref().map(|key| (own, unit, &key.shown)));
        let reason = if expected.is_some() { "the recording expects another message here" } else { "the recording expects no message here" };
        vec![self.miss("<frame>", reason, &shown, near)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::interactions::interactions;
    use crate::domain::traffic::Event;
    use mimic_core::rules::{Rule, RuleSet};

    fn frame(direction: Dir, value: Value) -> Event {
        Event::WsFrame { connection: 1, direction, opcode: 1, data: value.to_string().into_bytes() }
    }

    fn script(frames: Vec<Event>) -> (Vec<Script>, Keys) {
        let keys = Keys::new(&RuleSet::new(vec![Rule::UuidOrdinal]), &[], Default::default()).expect("rules compile");
        let mut events = vec![Event::WsOpen { connection: 1, target: "/ws".into(), status: 101 }];
        events.extend(frames);
        let its = interactions(&events);
        (vec![Script::of(0, &its[0], &keys)], keys)
    }

    fn ask(id: &str, method: &str, payload: Value) -> Value {
        json!({"_tag": "Request", "id": id, "tag": method, "payload": payload})
    }

    fn chunk(id: &str, text: &str) -> Value {
        json!({"_tag": "Chunk", "requestId": id, "values": [text]})
    }

    fn exit(id: &str) -> Value {
        json!({"_tag": "Exit", "requestId": id})
    }

    fn sent(out: &[Out]) -> Vec<Value> {
        out.iter().filter_map(|o| if let Out::Send(Payload::Json(v)) = o { Some(v.clone()) } else { None }).collect()
    }

    fn recorded() -> (Vec<Script>, Keys) {
        script(vec![
            frame(Dir::C2s, ask("1", "subscribe", json!({"thread": "t"}))),
            frame(Dir::S2c, chunk("1", "first")),
            frame(Dir::C2s, ask("2", "send", json!({"text": "hi"}))),
            frame(Dir::S2c, exit("2")),
            frame(Dir::S2c, chunk("1", "pushed after send")),
            frame(Dir::S2c, exit("1")),
        ])
    }

    #[test]
    fn given_a_second_run_with_other_request_ids_when_it_asks_then_the_recorded_replies_come_back_under_its_own_ids() {
        // Given
        let (scripts, keys) = recorded();
        let mut session = Session::new(0, Vec::new(), "c1".into(), 0);

        // When
        let mut out = session.client(&scripts, &Payload::Json(ask("41", "subscribe", json!({"thread": "t"}))), &keys, 0);
        out.extend(session.due(&scripts, 0, 300));
        out.extend(session.client(&scripts, &Payload::Json(ask("42", "send", json!({"text": "hi"}))), &keys, 1));
        out.extend(session.due(&scripts, 1, 300));

        // Then
        assert_eq!(sent(&out), [chunk("41", "first"), exit("42"), chunk("41", "pushed after send"), exit("41")]);
    }

    #[test]
    fn given_a_reply_recorded_after_another_request_when_the_client_has_not_made_it_then_the_reply_waits_until_the_client_is_quiet() {
        // Given
        let (scripts, keys) = recorded();
        let mut session = Session::new(0, Vec::new(), "c1".into(), 0);
        session.client(&scripts, &Payload::Json(ask("7", "subscribe", json!({"thread": "t"}))), &keys, 0);

        // When
        let at_once = session.due(&scripts, 10, 300);
        let once_quiet = session.due(&scripts, 400, 300);

        // Then
        assert_eq!(sent(&at_once), [chunk("7", "first")]);
        assert_eq!(sent(&once_quiet), [chunk("7", "pushed after send"), exit("7")]);
    }

    #[test]
    fn given_a_request_the_recording_does_not_have_when_it_arrives_then_the_mismatch_names_the_method_and_the_nearest_recording() {
        // Given
        let (scripts, keys) = recorded();
        let mut session = Session::new(0, Vec::new(), "c9".into(), 0);

        // When
        let changed = session.client(&scripts, &Payload::Json(ask("1", "send", json!({"text": "bye"}))), &keys, 0);
        let unknown = session.client(&scripts, &Payload::Json(ask("2", "delete", json!({}))), &keys, 0);

        // Then
        let Out::Miss(first) = &changed[0] else { panic!("{changed:?}") };
        assert_eq!((first.what.as_str(), first.connection.as_deref()), ("send", Some("c9")));
        let near = first.nearest.as_ref().expect("a nearest recording");
        assert_eq!((near.cassette.as_str(), near.step, near.diff[0].path.as_str()), ("interactions.jsonl:1", Some(3), "payload.text"));
        let Out::Miss(second) = &unknown[0] else { panic!("{unknown:?}") };
        assert_eq!((second.reason.as_str(), second.nearest.is_none()), ("the recording has no request of this method", true));
    }

    #[test]
    fn given_the_same_request_again_when_its_recording_is_used_up_then_it_is_answered_again_and_counted_as_reused() {
        // Given
        let (scripts, keys) = recorded();
        let mut session = Session::new(0, Vec::new(), "c1".into(), 0);
        let send = |id: &str| Payload::Json(ask(id, "send", json!({"text": "hi"})));

        // When
        session.client(&scripts, &send("1"), &keys, 0);
        session.client(&scripts, &send("2"), &keys, 0);
        let out = session.due(&scripts, 1000, 300);

        // Then
        assert_eq!(sent(&out), [exit("1"), exit("2")]);
        assert_eq!((session.matched, session.reused), (2, 1));
    }

    #[test]
    fn given_a_ping_and_an_interrupt_when_they_arrive_then_the_ping_is_answered_and_the_stream_stops() {
        // Given
        let (scripts, keys) = recorded();
        let mut session = Session::new(0, Vec::new(), "c1".into(), 0);
        session.client(&scripts, &Payload::Json(ask("5", "subscribe", json!({"thread": "t"}))), &keys, 0);

        // When
        let pong = session.client(&scripts, &Payload::Json(json!({"_tag": "Ping"})), &keys, 0);
        session.client(&scripts, &Payload::Json(json!({"_tag": "Interrupt", "requestId": "5"})), &keys, 0);
        let after = session.due(&scripts, 1000, 300);

        // Then
        assert_eq!(sent(&pong), [json!({"_tag": "Pong"})]);
        assert_eq!(sent(&after), Vec::<Value>::new());
    }

    #[test]
    fn given_a_socket_whose_replies_name_no_request_when_replayed_then_it_is_a_strict_script() {
        // Given
        let (scripts, keys) = script(vec![frame(Dir::S2c, json!({"hello": 1})), frame(Dir::C2s, json!({"say": "a"})), frame(Dir::S2c, json!({"said": "a"}))]);
        let mut session = Session::new(0, Vec::new(), "c1".into(), 0);

        // When
        let hello = session.due(&scripts, 0, 300);
        let wrong = session.client(&scripts, &Payload::Json(json!({"say": "b"})), &keys, 0);
        let right = session.client(&scripts, &Payload::Json(json!({"say": "a"})), &keys, 0);
        let reply = session.due(&scripts, 0, 300);

        // Then
        assert_eq!(sent(&hello), [json!({"hello": 1})]);
        assert!(matches!(wrong[0], Out::Miss(_)) && right.is_empty());
        assert_eq!(sent(&reply), [json!({"said": "a"})]);
    }
}
