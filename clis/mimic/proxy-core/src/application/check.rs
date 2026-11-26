//! A candidate is sent what a recorded client sent, and what it answers is compared with the recording under the set's rules.
//! The first thing that differs is named by its route or RPC method and its place in the cassette.
use super::ports::Candidate;
use crate::domain::bindings::Bindings;
use crate::domain::interactions::{ret, CHANNEL_CLOSE, CHANNEL_HTTP, CHANNEL_OPEN, CHANNEL_WS, OP_WS};
use crate::domain::matching::{envelopes, method, request_id};
use crate::domain::mismatch::location;
use crate::domain::ordering::Payload;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mimic_core::allowlist::Allowlist;
use mimic_core::cassette::{Dir, Interaction, Kind, Outcome, Step, StepKind};
use mimic_core::differ::{diff_interaction, Diff, DiffOptions, InteractionResult, Label, Severity, Verdict};
use mimic_core::json::canonical;
use mimic_core::ports::CassetteSet;
use mimic_core::rules::{Rule, RuleSet};
use mimic_core::wire::rid;
use serde::Serialize;
use serde_json::{json, Value};

/// How long the candidate may take to send the replies the recording has after a client message.
const REPLY_WAIT_MS: u64 = 5000;
const OWN_HEADERS: &[&str] = &["host", "connection", "content-length", "transfer-encoding"];

/// The first place the candidate's behaviour is not the recorded one.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct First {
    pub interaction: String,
    /// The HTTP route or the RPC method the difference belongs to.
    pub what: String,
    /// `interactions.jsonl:<line>`.
    pub cassette: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether the recording is of a pinned reference (`observed`) or a fixture someone made (`authored`).
    pub evidence: &'static str,
}

#[derive(Debug)]
pub struct Checked {
    pub verdict: Verdict,
    pub first: Option<First>,
}

fn pairs(value: &Value) -> Vec<(String, String)> {
    value.as_array().into_iter().flatten().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_str()?.to_string()))).collect()
}

fn bytes_of(payload: &Value) -> Vec<u8> {
    match (payload["body"].as_str(), payload["body_b64"].as_str()) {
        (Some(text), _) => text.as_bytes().to_vec(),
        (None, Some(coded)) => STANDARD.decode(coded).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Sorted keys, also inside a JSON body: the rules number the ids they mask in the order they meet them.
fn sorted(step: &Step) -> Step {
    let mut payload = canonical(&step.payload);
    let body = payload.get("body").and_then(Value::as_str).and_then(|text| serde_json::from_str::<Value>(text).ok());
    if let (Some(body), Some(fields)) = (body, payload.as_object_mut()) {
        fields.insert("body".into(), json!(canonical(&body).to_string()));
    }
    Step { payload, ..step.clone() }
}

fn exchange(expected: &Interaction, candidate: &mut dyn Candidate, bound: &mut Bindings) -> Result<Vec<Step>, String> {
    let call = expected.steps.iter().find(|s| s.dir == Dir::Call && s.channel.as_deref() == Some(CHANNEL_HTTP)).ok_or("the recording has no request")?;
    let asked = bound.value(&call.payload);
    let (verb, target) = (asked["method"].as_str().unwrap_or("GET"), asked["path"].as_str().unwrap_or("/"));
    let headers: Vec<(String, String)> = pairs(&asked["headers"]).into_iter().filter(|(name, _)| !OWN_HEADERS.iter().any(|own| name.eq_ignore_ascii_case(own))).collect();
    let answer = candidate.http(verb, target, &headers, &bytes_of(&asked))?;
    let got = ret(answer.status, &answer.headers, &answer.body);
    if let Some(recorded) = expected.steps.iter().find(|s| s.dir == Dir::Ret) {
        bound.learn(&recorded.payload, &got.payload);
    }
    Ok(vec![call.clone(), got])
}

fn frame(dir: Dir, message: Payload) -> Step {
    let step = |kind, rid, len, payload| Step { dir, channel: Some(CHANNEL_WS.into()), kind, rid, len, payload };
    match message {
        Payload::Json(value) => step(StepKind::Json, rid(&value), None, value),
        Payload::Text(text) => step(StepKind::Text, None, None, json!(text)),
        Payload::Binary(bytes) => step(StepKind::Binary, None, Some(bytes.len() as u64), json!(STANDARD.encode(&bytes))),
    }
}

fn message(step: &Step) -> Payload {
    match step.kind {
        StepKind::Binary => Payload::Binary(STANDARD.decode(step.payload.as_str().unwrap_or_default()).unwrap_or_default()),
        StepKind::Text => Payload::Text(step.payload.as_str().unwrap_or_default().to_string()),
        _ => Payload::Json(step.payload.clone()),
    }
}

/// Takes what the candidate sends until `wanted` messages came, or it is quiet for `settle_ms`, or the wait is spent.
fn replies(candidate: &mut dyn Candidate, wanted: usize, settle_ms: u64, out: &mut Vec<Step>) -> Result<(), String> {
    let (mut got, mut waited) = (0, 0);
    while waited < REPLY_WAIT_MS {
        match candidate.receive(settle_ms)? {
            Some(reply) => {
                out.push(frame(Dir::S2c, reply));
                got += 1;
            }
            None if got >= wanted => break,
            None => waited += settle_ms.max(1),
        }
    }
    Ok(())
}

fn socket(expected: &Interaction, candidate: &mut dyn Candidate, bound: &Bindings, settle_ms: u64) -> Result<Vec<Step>, String> {
    let open = expected.steps.iter().find(|s| s.channel.as_deref() == Some(CHANNEL_OPEN)).ok_or("the recording has no socket")?;
    let target = bound.text(open.payload["path"].as_str().unwrap_or("/"));
    let status = candidate.open(&target)?;
    let mut out = vec![Step { payload: json!({"path": open.payload["path"], "status": status}), ..open.clone() }];
    if status != 101 {
        return Ok(out);
    }
    let mut owed = 0;
    for step in expected.steps.iter().filter(|s| s.channel.as_deref() == Some(CHANNEL_WS)) {
        if step.dir == Dir::S2c {
            owed += 1;
            continue;
        }
        if owed > 0 {
            replies(candidate, owed, settle_ms, &mut out)?;
            owed = 0;
        }
        let sent = match message(step) {
            Payload::Json(value) => Payload::Json(bound.value(&value)),
            other => other,
        };
        candidate.send(&sent)?;
        out.push(step.clone());
    }
    replies(candidate, owed, settle_ms, &mut out)?;
    candidate.close();
    out.extend(expected.steps.iter().filter(|s| s.channel.as_deref() == Some(CHANNEL_CLOSE)).cloned());
    Ok(out)
}

/// The RPC method a difference belongs to: the recorded request whose id the differing step is filed under.
fn method_of(expected: &Interaction, path: &str) -> Option<String> {
    let address = path.strip_prefix("steps.")?.split('.').next()?;
    let wanted = address.rsplit(':').next()?;
    let asked = expected.steps.iter().filter(|s| s.dir == Dir::C2s).flat_map(|s| envelopes(&s.payload));
    asked.into_iter().find(|e| request_id(e).as_deref() == Some(wanted)).map(method).filter(|m| !m.is_empty())
}

fn first(set: &CassetteSet, verdict: &Verdict) -> Option<First> {
    let result = verdict.results.iter().find(|r| !matches!(r.label, Label::Clean | Label::Allowlisted))?;
    let (line, expected) = set.cassette.interactions.iter().enumerate().find(|(_, i)| i.id == result.id)?;
    let diff: Option<&Diff> = result.diffs.iter().find(|d| d.severity != Severity::None);
    let row = result.row.clone().unwrap_or_default();
    let named = row.strip_prefix("http:").or_else(|| row.strip_prefix("ws:")).unwrap_or(&row).to_string();
    let what = diff.and_then(|d| method_of(expected, &d.path)).unwrap_or(named);
    Some(First {
        interaction: result.id.clone(),
        what,
        cassette: location(line + 1),
        path: diff.map(|d| d.path.clone()).unwrap_or_default(),
        expected: diff.and_then(|d| d.expected.clone()),
        actual: diff.and_then(|d| d.actual.clone()),
        error: result.error.clone(),
        evidence: if set.cassette.header.provenance.kind == Kind::Recorded { "observed" } else { "authored" },
    })
}

/// Plays every interaction of the set to the candidate, in recorded order. `known` binds fixture values the candidate cannot
/// hand out by itself (a pairing grant minted for this run).
pub fn check(set: &CassetteSet, candidate: &mut dyn Candidate, secret_keys: &[String], opts: &DiffOptions, allow: &Allowlist, known: Vec<(String, String)>, settle_ms: u64) -> Result<Checked, String> {
    let secret = Rule::SecretKey { keys: secret_keys.iter().map(|k| (k.clone(), "<secret>".to_string())).collect() };
    let rules = RuleSet::new(set.rules.rules.iter().cloned().chain([secret]).collect()).compile().map_err(|e| e.to_string())?;
    let mut bound = Bindings::new(known);
    let mut results = Vec::new();
    for expected in &set.cassette.interactions {
        let played = if expected.op.as_deref() == Some(OP_WS) { socket(expected, candidate, &bound, settle_ms) } else { exchange(expected, candidate, &mut bound) };
        results.push(match played {
            Ok(steps) => {
                let masked = |steps: &[Step]| rules.interaction(&Interaction { steps: steps.iter().map(sorted).collect(), effects: Vec::new(), outcome: Outcome::default(), ..expected.clone() });
                InteractionResult::of(&expected.id, expected.row.clone(), diff_interaction(&masked(&expected.steps), &masked(&steps), allow, opts))
            }
            Err(error) => InteractionResult::failed(&expected.id, expected.row.clone(), error),
        });
    }
    let verdict = Verdict::of(results, allow);
    Ok(Checked { first: first(set, &verdict), verdict })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::record::{default_rules, record, Meta};
    use crate::application::replay::Replayer;
    use crate::domain::matching::Tolerance;
    use crate::domain::sanitize::{Policy, SECRET_KEYS};
    use crate::domain::traffic::Event;
    use crate::testing::ReplayCandidate;
    use mimic_core::cassette::ToolInfo;

    fn socket_frame(direction: Dir, value: Value) -> Event {
        Event::WsFrame { connection: 1, direction, opcode: 1, data: value.to_string().into_bytes() }
    }

    /// A journey whose server says `reply` to the one command.
    fn journey(reply: &str) -> CassetteSet {
        let events = vec![
            Event::HttpRequest { exchange: 1, method: "GET".into(), target: "/api/session".into(), headers: vec![("authorization".into(), "Bearer LIVE-TOKEN-OF-THE-RUN".into())], body: Vec::new() },
            Event::HttpResponse { exchange: 1, status: 200, headers: vec![("content-type".into(), "application/json".into())], body: br#"{"user":"fixture user"}"#.to_vec() },
            Event::WsOpen { connection: 1, target: "/ws".into(), status: 101 },
            socket_frame(Dir::C2s, json!({"_tag": "Request", "id": "1", "tag": "thread.send", "payload": {"text": "hello"}})),
            socket_frame(Dir::S2c, json!({"_tag": "Chunk", "requestId": "1", "values": [{"text": reply}]})),
            socket_frame(Dir::S2c, json!({"_tag": "Exit", "requestId": "1"})),
        ];
        let meta = Meta { kind: Kind::Authored, target: None, tool: ToolInfo { mimic: "0".into(), git: None }, recorded_at: None, recorded_on: None, source: None, note: None };
        record(&events, default_rules(), &Policy::default(), meta).expect("recorded").set
    }

    fn checked(recording: &CassetteSet, served: &CassetteSet) -> Checked {
        let secret: Vec<String> = SECRET_KEYS.iter().map(|s| s.to_string()).collect();
        let mut candidate = ReplayCandidate::new(Replayer::new(served, &secret, Tolerance::default(), 0).expect("a replayer"));
        check(recording, &mut candidate, &secret, &DiffOptions::default(), &Allowlist::default(), Vec::new(), 1).expect("checked")
    }

    #[test]
    fn given_a_candidate_that_behaves_as_recorded_when_checked_then_nothing_differs() {
        // Given
        let recording = journey("hello back");

        // When
        let checked = checked(&recording, &recording);

        // Then
        assert_eq!((checked.verdict.summary.clean, checked.verdict.summary.total, checked.first), (2, 2, None));
    }

    #[test]
    fn given_a_candidate_whose_one_answer_differs_when_checked_then_the_first_difference_names_the_method_and_the_cassette_line() {
        // Given
        let (recording, changed) = (journey("hello back"), journey("something else"));

        // When
        let checked = checked(&recording, &changed);

        // Then
        let first = checked.first.expect("a first difference");
        assert_eq!((first.interaction.as_str(), first.what.as_str(), first.cassette.as_str(), first.evidence), ("c01", "thread.send", "interactions.jsonl:2", "authored"));
        assert_eq!((first.expected, first.actual), (Some(json!("hello back")), Some(json!("something else"))));
        assert!(first.path.starts_with("steps.ws:s2c:1."), "{}", first.path);
    }
}
