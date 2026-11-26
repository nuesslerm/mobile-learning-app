//! A cassette answers a client: a request comes in, the recorded reply or a mismatch goes out. Nothing here can reach a server,
//! so a request the recording does not have is never forwarded.
use crate::domain::interactions::{route, CHANNEL_HTTP, CHANNEL_OPEN, OP_HTTP, OP_WS};
use crate::domain::matching::{pick, Candidate, Key, Keys, Tolerance};
use crate::domain::mismatch::{nearest, Mismatch};
use crate::domain::ordering::{Out, Payload, Script, Session};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mimic_core::cassette::{Dir, Interaction};
use mimic_core::ports::CassetteSet;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Headers the replaying server sets for itself.
const OWN_HEADERS: &[&str] = &["connection", "keep-alive", "transfer-encoding", "content-length"];

#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum Answer {
    Reply(Reply),
    Miss(Mismatch),
}

#[derive(Debug, PartialEq)]
pub enum Upgrade {
    Accept(u64),
    Refuse(Reply),
    Miss(Mismatch),
}

struct Exchange {
    interaction: usize,
    id: String,
    method: String,
    upgrade: bool,
    key: Key,
    reply: Reply,
    used: u32,
}

struct Socket {
    script: usize,
    route: String,
    key: Key,
    used: u32,
}

pub struct Replayer {
    keys: Keys,
    exchanges: Vec<Exchange>,
    scripts: Vec<Script>,
    sockets: Vec<Socket>,
    sessions: BTreeMap<u64, Session>,
    opened: u64,
    settle_ms: u64,
    mismatches: Vec<Mismatch>,
    http: Counts,
    ws: Counts,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    matched: u32,
    reused: u32,
}

fn headers_of(value: &Value) -> Vec<(String, String)> {
    match value {
        Value::Array(pairs) => pairs.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string()))).collect(),
        Value::Object(fields) => fields.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect(),
        _ => Vec::new(),
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v.as_str())
}

fn body_of(payload: &Value) -> Vec<u8> {
    match (payload["body"].as_str(), payload["body_b64"].as_str()) {
        (Some(text), _) => text.as_bytes().to_vec(),
        (None, Some(coded)) => STANDARD.decode(coded).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// A body as the matcher reads it: its text, or its base64 when it is not text.
fn body_text(body: &[u8]) -> String {
    std::str::from_utf8(body).map_or_else(|_| STANDARD.encode(body), String::from)
}

fn exchange(at: usize, it: &Interaction, keys: &Keys) -> Option<Exchange> {
    let part = |dir: Dir| it.steps.iter().find(|s| s.dir == dir && s.channel.as_deref() == Some(CHANNEL_HTTP)).map(|s| &s.payload);
    let (asked, answered) = (part(Dir::Call)?, part(Dir::Ret)?);
    let (method, target) = (asked["method"].as_str()?, asked["path"].as_str()?);
    let content_type = header(&headers_of(&asked["headers"]), "content-type").to_string();
    let headers = headers_of(&answered["headers"]).into_iter().filter(|(k, _)| !OWN_HEADERS.iter().any(|own| k.eq_ignore_ascii_case(own))).collect();
    Some(Exchange {
        interaction: at,
        id: it.id.clone(),
        method: method.to_uppercase(),
        upgrade: it.input["upgrade"] == true,
        key: keys.http(method, target, &body_text(&body_of(asked)), &content_type),
        reply: Reply { status: answered["status"].as_u64().and_then(|s| u16::try_from(s).ok())?, headers, body: body_of(answered) },
        used: 0,
    })
}

impl Replayer {
    pub fn new(set: &CassetteSet, secret_keys: &[String], tolerance: Tolerance, settle_ms: u64) -> Result<Replayer, String> {
        let keys = Keys::new(&set.rules, secret_keys, tolerance).map_err(|e| e.to_string())?;
        let (mut exchanges, mut scripts, mut sockets) = (Vec::new(), Vec::new(), Vec::new());
        for (at, it) in set.cassette.interactions.iter().enumerate() {
            match it.op.as_deref() {
                Some(OP_HTTP) => exchanges.extend(exchange(at, it, &keys)),
                Some(OP_WS) => {
                    let Some(target) = it.steps.iter().find(|s| s.channel.as_deref() == Some(CHANNEL_OPEN)).and_then(|s| s.payload["path"].as_str()) else { continue };
                    sockets.push(Socket { script: scripts.len(), route: route(target).to_string(), key: keys.target(target), used: 0 });
                    scripts.push(Script::of(at, it, &keys));
                }
                _ => {}
            }
        }
        Ok(Replayer { keys, exchanges, scripts, sockets, sessions: BTreeMap::new(), opened: 0, settle_ms, mismatches: Vec::new(), http: Counts::default(), ws: Counts::default() })
    }

    fn missed(&mut self, mismatch: Mismatch) -> Mismatch {
        self.mismatches.push(mismatch.clone());
        mismatch
    }

    fn take(&mut self, upgrade: bool, method: &str, key: &Key) -> Option<Reply> {
        let fits = |e: &Exchange| e.upgrade == upgrade && e.method.eq_ignore_ascii_case(method);
        let candidates: Vec<Candidate> = self.exchanges.iter().enumerate().filter(|(_, e)| fits(e)).map(|(at, e)| Candidate { at, key: &e.key, used: e.used > 0 }).collect();
        let (at, reused) = pick(&candidates, key)?;
        self.exchanges[at].used += 1;
        self.http.matched += 1;
        self.http.reused += u32::from(reused);
        Some(self.exchanges[at].reply.clone())
    }

    pub fn http(&mut self, method: &str, target: &str, headers: &[(String, String)], body: &[u8]) -> Answer {
        let key = self.keys.http(method, target, &body_text(body), header(headers, "content-type"));
        if let Some(reply) = self.take(false, method, &key) {
            return Answer::Reply(reply);
        }
        let shown_route = |k: &Key| k.shown["path"].as_str().map(|p| route(p).to_string()).unwrap_or_default();
        let wanted = shown_route(&key);
        let near = self.exchanges.iter().find(|e| !e.upgrade && e.method.eq_ignore_ascii_case(method) && shown_route(&e.key) == wanted);
        let what = format!("{} {wanted}", method.to_uppercase());
        let mismatch = match near {
            Some(e) => Mismatch::new("http", what, None, "no recorded request of this route has this query and body", &key.shown, Some(nearest(&e.id, e.interaction + 1, Some(0), &e.key.shown, &key.shown))),
            None => Mismatch::new("http", what, None, "the recording has no request of this route", &key.shown, None),
        };
        Answer::Miss(self.missed(mismatch))
    }

    /// A client asks for a socket: the next recording of that target plays, or the refusal the recording has.
    pub fn upgrade(&mut self, target: &str, now_ms: u64) -> Upgrade {
        let key = self.keys.target(target);
        let candidates: Vec<Candidate> = self.sockets.iter().enumerate().map(|(at, s)| Candidate { at, key: &s.key, used: s.used > 0 }).collect();
        if let Some((at, reused)) = pick(&candidates, &key) {
            self.sockets[at].used += 1;
            self.ws.reused += u32::from(reused);
            self.opened += 1;
            let (script, wanted) = (self.sockets[at].script, self.sockets[at].route.clone());
            let peers = self.sockets.iter().filter(|s| s.script != script && s.route == wanted).map(|s| s.script).collect();
            self.sessions.insert(self.opened, Session::new(script, peers, format!("c{}", self.opened), now_ms));
            return Upgrade::Accept(self.opened);
        }
        let refusal = self.keys.http("GET", target, "", "");
        if let Some(reply) = self.take(true, "GET", &refusal) {
            return Upgrade::Refuse(reply);
        }
        let what = format!("socket {}", route(key.shown.as_str().unwrap_or_default()));
        let mismatch = Mismatch::new("ws", what, None, "the recording has no socket of this route and query", &key.shown, None);
        Upgrade::Miss(self.missed(mismatch))
    }

    fn collect(&mut self, out: Vec<Out>) -> Vec<Out> {
        for mismatch in out.iter().filter_map(|o| if let Out::Miss(m) = o { Some(m) } else { None }) {
            self.mismatches.push(mismatch.clone());
        }
        out
    }

    /// A message of the client on an accepted socket, and what may go out because of it.
    pub fn frame(&mut self, session: u64, message: &Payload, now_ms: u64) -> Vec<Out> {
        let Some(live) = self.sessions.get_mut(&session) else { return Vec::new() };
        let mut out = live.client(&self.scripts, message, &self.keys, now_ms);
        out.extend(live.due(&self.scripts, now_ms, self.settle_ms));
        self.collect(out)
    }

    /// What may go out now that time has passed: called when a socket opens and while it is idle.
    pub fn tick(&mut self, session: u64, now_ms: u64) -> Vec<Out> {
        let Some(live) = self.sessions.get_mut(&session) else { return Vec::new() };
        let out = live.due(&self.scripts, now_ms, self.settle_ms);
        self.collect(out)
    }

    pub fn closed(&mut self, session: u64) {
        if let Some(live) = self.sessions.remove(&session) {
            self.ws.matched += live.matched;
            self.ws.reused += live.reused;
        }
    }

    pub fn mismatches(&self) -> &[Mismatch] {
        &self.mismatches
    }

    /// What the run did with the recording, for the report. Sockets still open count as they stand.
    pub fn summary(&self) -> Value {
        let open = self.sessions.values().fold(Counts::default(), |sum, s| Counts { matched: sum.matched + s.matched, reused: sum.reused + s.reused });
        let unused: Vec<&str> = self
            .exchanges
            .iter()
            .filter(|e| e.used == 0)
            .map(|e| e.id.as_str())
            .chain(self.sockets.iter().filter(|s| s.used == 0).map(|s| self.scripts[s.script].id.as_str()))
            .collect();
        json!({
            "http": {"matched": self.http.matched, "reused": self.http.reused},
            "ws": {"connections": self.opened, "matched": self.ws.matched + open.matched, "reused": self.ws.reused + open.reused},
            "mismatches": self.mismatches.len(),
            "unused": unused,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::record::{default_rules, record, Meta};
    use crate::domain::sanitize::{Policy, SECRET_KEYS};
    use crate::domain::traffic::Event;
    use mimic_core::cassette::{Kind, ToolInfo};

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn frame(direction: Dir, value: Value) -> Event {
        Event::WsFrame { connection: 1, direction, opcode: 1, data: value.to_string().into_bytes() }
    }

    fn replayer() -> Replayer {
        let events = vec![
            Event::HttpRequest { exchange: 1, method: "POST".into(), target: "/ticket".into(), headers: headers(&[("authorization", "Bearer RECORDED-TOKEN-1")]), body: Vec::new() },
            Event::HttpResponse { exchange: 1, status: 200, headers: headers(&[("content-type", "application/json"), ("content-length", "33")]), body: br#"{"ticket":"RECORDED-TICKET-1"}"#.to_vec() },
            Event::WsOpen { connection: 1, target: "/ws?wsTicket=RECORDED-TICKET-1".into(), status: 101 },
            frame(Dir::C2s, json!({"_tag": "Request", "id": "1", "tag": "thread.open", "payload": {"thread": "t-1"}})),
            frame(Dir::S2c, json!({"_tag": "Chunk", "requestId": "1", "values": [{"text": "hello from the fixture"}]})),
            frame(Dir::S2c, json!({"_tag": "Exit", "requestId": "1"})),
            Event::WsRefused { connection: 2, target: "/admin/ws".into(), status: 401, body: br#"{"error":"expired"}"#.to_vec() },
        ];
        let meta = Meta { kind: Kind::Authored, target: None, tool: ToolInfo { mimic: "0".into(), git: None }, recorded_at: None, recorded_on: None, source: None, note: None };
        let recorded = record(&events, default_rules(), &Policy::default(), meta).expect("recorded");
        let secret: Vec<String> = SECRET_KEYS.iter().map(|s| s.to_string()).collect();
        Replayer::new(&recorded.set, &secret, Tolerance::default(), 300).expect("a replayer")
    }

    #[test]
    fn given_a_recorded_journey_when_a_client_repeats_it_with_its_own_credentials_and_ids_then_it_gets_the_recorded_answers() {
        // Given
        let mut replayer = replayer();

        // When
        let ticket = replayer.http("POST", "/ticket", &headers(&[("authorization", "Bearer A-LIVE-TOKEN")]), b"");
        let socket = replayer.upgrade("/ws?wsTicket=fx-ticket-1", 0);
        let Upgrade::Accept(session) = socket else { panic!("{socket:?}") };
        let out = replayer.frame(session, &Payload::Json(json!({"_tag": "Request", "id": "88", "tag": "thread.open", "payload": {"thread": "t-1"}})), 1);

        // Then
        let Answer::Reply(reply) = ticket else { panic!("{ticket:?}") };
        assert_eq!((reply.status, String::from_utf8_lossy(&reply.body).into_owned()), (200, r#"{"ticket":"fx-ticket-1"}"#.to_string()));
        assert_eq!(reply.headers, headers(&[("content-type", "application/json")]));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Out::Send(Payload::Json(json!({"_tag": "Chunk", "requestId": "88", "values": [{"text": "hello from the fixture"}]}))));
        assert_eq!(out[1], Out::Send(Payload::Json(json!({"_tag": "Exit", "requestId": "88"}))));
        assert_eq!(replayer.summary()["mismatches"], 0);
    }

    #[test]
    fn given_requests_the_recording_does_not_have_when_they_arrive_then_each_is_a_mismatch_with_its_route() {
        // Given
        let mut replayer = replayer();

        // When
        let unknown = replayer.http("GET", "/nowhere?token=A-LIVE-TOKEN-VALUE", &[], b"");
        let changed = replayer.http("POST", "/ticket", &[], br#"{"scope":"other"}"#);
        let socket = replayer.upgrade("/other", 0);

        // Then
        let Answer::Miss(first) = unknown else { panic!("{unknown:?}") };
        assert_eq!((first.what.as_str(), first.nearest.is_none()), ("GET /nowhere", true));
        assert!(!serde_json::to_string(&first).unwrap().contains("A-LIVE-TOKEN-VALUE"));
        let Answer::Miss(second) = changed else { panic!("{changed:?}") };
        assert_eq!(second.nearest.as_ref().map(|n| n.cassette.as_str()), Some("interactions.jsonl:1"));
        assert!(matches!(socket, Upgrade::Miss(_)));
        assert_eq!(replayer.summary()["mismatches"], 3);
        assert_eq!(replayer.summary()["unused"], json!(["h0001", "h0002", "c01"]));
    }

    #[test]
    fn given_a_recorded_refusal_when_the_same_socket_is_asked_for_then_the_refusal_is_replayed() {
        // Given
        let mut replayer = replayer();

        // When
        let refused = replayer.upgrade("/admin/ws", 0);

        // Then
        let Upgrade::Refuse(reply) = refused else { panic!("{refused:?}") };
        assert_eq!((reply.status, reply.body), (401, br#"{"error":"expired"}"#.to_vec()));
    }
}
