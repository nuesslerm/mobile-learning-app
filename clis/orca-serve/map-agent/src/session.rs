//! The session rows over `agent.threads`: a session is a thread (the session id is the thread id), a send is a turn, a cancel a forced stop.
use crate::agent::{ask, refused_code};
use crate::fold::{fold, Item};
use crate::journal::{self, cursor, load, page, view, EPOCH, FENCE};
use crate::ops::{lookup, record, refused, Envelope};
use agent_api_agent::lifecycle::THREAD_FORCE_STOP;
use agent_api_agent::{code as thread_code, queue, requests, THREAD_CREATE, TURN_START};
use orca_core::wire::Failure;
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

pub(crate) fn text<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn session_id(params: &Value) -> Result<&str, Failure> {
    text(params, "sessionId").ok_or_else(|| Failure::invalid("Invalid session id"))
}

/// The Orca agent ids this host runs, and the rca driver each one is.
pub fn driver_of(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" => Some("claudeAgent"),
        "codex" => Some("codex"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

/// The Orca agent id of an rca driver.
pub fn agent_of(driver: &str) -> Option<&'static str> {
    ["claude", "codex", "opencode"].into_iter().find(|a| driver_of(a) == Some(driver))
}

pub(crate) fn unsupported() -> Failure {
    Failure::new(orca_core::wire::RUNTIME_ERROR, "structured_agent_session_unsupported")
}

/// The text of a send body and the attachment ids of its `image-ref` blocks, in order.
pub(crate) fn body_parts(body: &Value) -> Result<(String, Vec<String>), Failure> {
    let blocks = body["blocks"].as_array().ok_or_else(|| Failure::invalid("Invalid message body"))?;
    let mut texts = Vec::new();
    let mut images = Vec::new();
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => texts.push(b["text"].as_str().unwrap_or_default()),
            Some("image-ref") => images.push(b),
            _ => return Err(Failure::invalid("Invalid message body")),
        }
    }
    Ok((texts.join("\n"), crate::attach::image_ids(&images)?))
}

pub(crate) fn submission(client_message_id: &str, fingerprint: &str) -> Value {
    json!({"clientMessageId": client_message_id, "fence": FENCE, "payloadFingerprint": fingerprint, "dispatchState": "accepted", "providerItemId": null, "reason": null, "submittedAt": now_ms(), "resolvedAt": null})
}

pub(crate) fn accepted(agent: &dyn AgentApi, thread: &str, value: Value) -> Result<Value, Failure> {
    let head = view(agent, thread)?["last_seq"].as_u64().unwrap_or(0);
    Ok(json!({"ok": true, "replayed": false, "fence": FENCE, "cursor": cursor(head), "value": value}))
}

/// Records a success so a retry replays it; a refusal is not recorded (the phone may fix it and send again under the same id).
pub(crate) fn finish(agent: &dyn AgentApi, env: &Envelope, answer: Value) -> Result<Value, Failure> {
    if answer["ok"] == true {
        record(agent, env, &answer)?;
    }
    Ok(answer)
}

/// What a start or steer of a turn answers: the turn, or the refusal the phone shows.
enum Started {
    Turn,
    Refused(Value),
}

fn start_turn(agent: &dyn AgentApi, thread: &str, prompt: &str, attachments: &[String], model: Option<&str>, view: &Value) -> Result<Started, Failure> {
    let (Some(workspace), Some(driver)) = (view["workspace"].as_str(), view["driver"].as_str()) else {
        return Ok(Started::Refused(refused("agent_session_identity_required", "the session has no workspace or agent to run in")));
    };
    let mut params = json!({"thread": thread, "workspace": workspace, "driver": driver, "prompt": prompt});
    let picked = crate::catalog::stored_model(agent, thread)?;
    if let Some(m) = model.or(picked.as_deref()).or(view["model"].as_str()) {
        params["model"] = m.into();
    }
    if !attachments.is_empty() {
        params["attachments"] = json!(attachments);
    }
    match agent.request(TURN_START.name(), params) {
        Ok(_) => Ok(Started::Turn),
        Err(e) if refused_code(&e) == Some(thread_code::TURN_RUNNING) => Ok(Started::Refused(refused("agent_session_conflict", "the session already has a running turn"))),
        Err(e) => Err(crate::agent::refusal(e)),
    }
}

/// A first turn for a session just created: its own thread view says where and with what it runs.
pub(crate) fn start_turn_text(agent: &dyn AgentApi, thread: &str, prompt: &str, model: Option<&str>) -> Result<(), Failure> {
    match start_turn(agent, thread, prompt, &[], model, &view(agent, thread)?)? {
        Started::Turn => Ok(()),
        Started::Refused(r) => Err(Failure::new(orca_core::wire::RUNTIME_ERROR, r["refusal"]["code"].as_str().unwrap_or("agent_session_conflict"))),
    }
}

/// `agentSession.send`: a turn on the session, steered into the running turn, or (queue-if-active) queued behind it.
pub(crate) fn send(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let (prompt, images) = body_parts(&params["body"])?;
    let thread = view(agent, &env.session)?;
    let running = thread["status"] == "running";
    let model = params["options"]["model"].as_str().or_else(|| params["selection"]["model"].as_str());
    if running && !images.is_empty() {
        return Err(Failure::invalid("Images can only be sent while the chat is idle on this host"));
    }
    let value = if running && params["delivery"] == "queue-if-active" {
        let added = ask(agent, queue::QUEUE_ADD.name(), json!({"thread": env.session, "prompt": prompt}))?;
        json!({"clientMessageId": env.op, "queued": {"messageId": added["item"]["item"], "position": added["position"], "state": "waiting"}})
    } else if running {
        ask(agent, queue::TURN_STEER.name(), json!({"thread": env.session, "prompt": prompt}))?;
        json!({"clientMessageId": env.op, "submission": submission(&env.op, &env.fingerprint)})
    } else {
        match start_turn(agent, &env.session, &prompt, &images, model, &thread)? {
            Started::Turn => json!({"clientMessageId": env.op, "submission": submission(&env.op, &env.fingerprint)}),
            Started::Refused(r) => return Ok(r),
        }
    };
    let answer = accepted(agent, &env.session, value)?;
    finish(agent, &env, answer)
}

/// `agentSession.create` for a new chat: the thread, and the first message as its first turn.
pub(crate) fn create(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    if params.get("providerHandle").is_some() {
        return Err(unsupported());
    }
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let agent_id = text(params, "agent").ok_or_else(|| Failure::invalid("Invalid agent"))?;
    let driver = driver_of(agent_id).ok_or_else(unsupported)?;
    if !crate::ids::is_session_id_for(agent_id, &env.session) {
        return Err(Failure::invalid("Session id must be named for its agent"));
    }
    let target = orca_map_workspace::resolve(agent, text(params, "worktree").ok_or_else(|| Failure::invalid("Missing worktree selector"))?)?;
    let model = params["options"]["model"].as_str();
    let mut create = json!({"thread": env.session, "workspace": target.workspace, "driver": driver});
    if let Some(m) = model {
        create["model"] = m.into();
    }
    let thread = ask(agent, THREAD_CREATE.name(), create)?["thread"].clone();
    let mut first = None;
    if let Some(message) = params.get("firstMessage").filter(|m| m.is_object()) {
        let id = text(message, "clientMessageId").ok_or_else(|| Failure::invalid("Invalid client message id"))?;
        let (first_text, first_images) = body_parts(&message["body"])?;
        match start_turn(agent, &env.session, &first_text, &first_images, model, &thread)? {
            Started::Turn => first = Some(submission(id, &env.fingerprint)),
            Started::Refused(r) => return Ok(r),
        }
    }
    let loaded = load(agent, &env.session)?;
    let mut shown = page(&env.session, &loaded.folded, "tail", None, journal::DEFAULT_LIMIT);
    crate::queued::publish(agent, &env.session, &mut shown)?;
    let mut value = json!({"sessionId": env.session, "fence": FENCE, "page": shown, "unconfirmedClientMessageIds": []});
    if let Some(f) = first {
        value["firstMessage"] = f;
    }
    let answer = accepted(agent, &env.session, value)?;
    finish(agent, &env, answer)
}

/// `agentSession.cancel`: stops the running turn and holds the session's queue until the phone resumes it (the host's stop marks do the same), or closes the prompt it names (a question is dismissed, an approval declined).
pub(crate) fn cancel(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    if params.get("scope").is_some() || params.get("taskId").is_some() {
        return Err(Failure::new(orca_core::wire::RUNTIME_ERROR, "agent_session_background_tasks_unsupported"));
    }
    let turn = text(params, "turnId");
    let value = match params.get("prompt").filter(|p| p.is_object()) {
        Some(prompt) => {
            let loaded = load(agent, &env.session)?;
            let id = prompt["itemId"].as_str().unwrap_or_default();
            let Some(item) = loaded.folded.items.iter().find(|i| i.id == id) else { return Ok(refused("agent_session_item_revision_stale", "the prompt is not in the session")) };
            if !item.is_pending_prompt() {
                return Ok(refused("agent_session_already_resolved", "the prompt was answered already"));
            }
            if prompt["expectedRevision"].as_u64() != Some(item.revision) {
                return Ok(refused("agent_session_item_revision_stale", "the prompt changed since it was shown"));
            }
            match id.split_once(':') {
                Some(("approval", approval)) => ask(agent, requests::APPROVAL_ANSWER.name(), json!({"thread": env.session, "approval": approval, "decision": "decline"}))?,
                Some((_, request)) => ask(agent, requests::INPUT_DISMISS.name(), json!({"thread": env.session, "request": request}))?,
                None => return Err(Failure::invalid("Invalid item id")),
            };
            json!({"turnId": turn, "cancelled": true})
        }
        None => {
            let mut p = json!({"thread": env.session});
            if let Some(t) = turn {
                p["turn"] = t.into();
            }
            match agent.request(THREAD_FORCE_STOP.name(), p) {
                Ok(r) => json!({"turnId": r["turn"], "cancelled": true}),
                Err(AgentError::Refused { code, .. }) if code == thread_code::NO_TURN => json!({"turnId": turn, "cancelled": false}),
                Err(e) => return Err(crate::agent::refusal(e)),
            }
        }
    };
    let answer = accepted(agent, &env.session, value)?;
    finish(agent, &env, answer)
}

/// `agentSession.conversationCommand`: the command typed as a prompt, which is what a text-driven agent receives.
pub(crate) fn conversation_command(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let env = Envelope::read(params)?;
    if let Some(done) = lookup(agent, &env)? {
        return Ok(done);
    }
    let command = params["command"].as_str().filter(|c| matches!(*c, "clear" | "compact")).ok_or_else(|| Failure::invalid("Invalid command"))?;
    let prompt = match (command, text(params, "instructions")) {
        ("compact", Some(i)) => format!("/compact {i}"),
        _ => format!("/{command}"),
    };
    let thread = view(agent, &env.session)?;
    let value = if thread["status"] == "running" {
        if params["delivery"] != "queue-if-active" {
            return Ok(refused("agent_session_conflict", "the session is working: wait for it, or send the command queued"));
        }
        let added = ask(agent, queue::QUEUE_ADD.name(), json!({"thread": env.session, "prompt": prompt}))?;
        json!({"command": command, "state": "completed", "queued": {"messageId": added["item"]["item"], "position": added["position"], "state": "waiting"}})
    } else {
        match start_turn(agent, &env.session, &prompt, &[], None, &thread)? {
            Started::Turn => json!({"command": command, "state": "completed"}),
            Started::Refused(r) => return Ok(r),
        }
    };
    let answer = accepted(agent, &env.session, value)?;
    finish(agent, &env, answer)
}

pub(crate) fn history(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let session = session_id(params)?;
    let direction = params["direction"].as_str().filter(|d| matches!(*d, "tail" | "before" | "after")).ok_or_else(|| Failure::invalid("Invalid direction"))?;
    let limit = params["limit"].as_u64().map_or(journal::DEFAULT_LIMIT, |l| l.clamp(1, 200) as usize);
    let loaded = load(agent, session)?;
    let from = params.get("cursor").filter(|c| c.is_object());
    let reset = match (direction, from) {
        ("tail", _) => None,
        (_, None) => Some("epoch_changed"),
        (_, Some(c)) if c["epoch"] != EPOCH => Some("epoch_changed"),
        (_, Some(c)) if c["sequence"].as_u64().unwrap_or(0) > loaded.folded.last_seq => Some("cursor_ahead"),
        _ => None,
    };
    if let Some(reason) = reset {
        let mut shown = page(session, &loaded.folded, "tail", None, limit);
        crate::queued::publish(agent, session, &mut shown)?;
        return Ok(json!({"ok": false, "reset": reason, "fence": FENCE, "page": shown}));
    }
    let seq = from.and_then(|c| c["sequence"].as_u64());
    let mut shown = page(session, &loaded.folded, direction, seq, limit);
    crate::queued::publish(agent, session, &mut shown)?;
    Ok(json!({"ok": true, "page": shown}))
}

pub(crate) fn item(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let session = session_id(params)?;
    let id = text(params, "itemId").ok_or_else(|| Failure::invalid("Invalid item id"))?;
    let loaded = load(agent, session)?;
    Ok(json!({"item": loaded.folded.items.iter().find(|i| i.id == id).map(Item::render)}))
}

pub(crate) fn hold(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    view(agent, session_id(params)?)?;
    Ok(json!({"held": true}))
}

pub(crate) fn release() -> Value {
    json!({"released": true})
}

/// A version the phone echoes back: the same while the open requests are the same.
fn version_of(session: &str, json: &str) -> String {
    let h: Vec<u8> = Sha256::digest(format!("{session}\n{json}")).to_vec();
    let hex: String = h.iter().take(16).map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-4{}-8{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

const PIECE_BYTES: usize = 24 * 1024;

/// `agentSession.currentRequests`: the open approvals and questions as one JSON text, in pieces the phone reads by cursor.
pub(crate) fn current_requests(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let session = session_id(params)?;
    let loaded = load(agent, session)?;
    let items: Vec<Value> = loaded.folded.items.iter().filter(|i| i.is_pending_prompt()).map(Item::render).collect();
    let json = serde_json::to_string(&items).unwrap_or_default();
    let version = version_of(session, &json);
    let marker = json!({"contract": "agent-session.current-requests.v1", "sessionId": session, "epoch": EPOCH, "publisher": "rca", "revision": loaded.folded.last_seq, "owner": format!("rca:{session}"), "version": version});
    if params["version"] != version.as_str() {
        return Ok(json!({"marker": marker, "json": "", "nextCursor": 0, "restart": true}));
    }
    let start = params["cursor"].as_u64().unwrap_or(0) as usize;
    if start >= json.len() || !json.is_char_boundary(start) {
        return Err(Failure::new(orca_core::wire::RUNTIME_ERROR, "Invalid current-request cursor"));
    }
    let mut end = (start + PIECE_BYTES).min(json.len());
    while !json.is_char_boundary(end) {
        end -= 1;
    }
    Ok(json!({"marker": marker, "json": &json[start..end], "nextCursor": if end == json.len() { Value::Null } else { end.into() }}))
}

/// `nativeChat.readSession`: the transcript as the phone's chat rows, newest window first, paged by the row offset (the event's `seq`).
pub(crate) fn read_session(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let session = session_id(params)?;
    let limit = params["limit"].as_u64().map_or(journal::DEFAULT_LIMIT, |l| l.max(1) as usize);
    let (events, _) = journal::events_after(agent, session, 0)?;
    view(agent, session)?;
    let folded = fold(&events);
    let all: Vec<&Item> = folded.items.iter().filter(|i| i.body["kind"] == "message" && params["beforeOffset"].as_u64().is_none_or(|b| i.sequence < b)).collect();
    let start = all.len().saturating_sub(limit);
    let messages: Vec<Value> = all[start..].iter().map(|i| json!({"id": i.id, "role": i.body["role"], "blocks": i.body["blocks"], "timestamp": null, "source": "transcript", "transcriptOffset": i.sequence})).collect();
    let before = all.get(start).map_or_else(|| params["beforeOffset"].as_u64().unwrap_or(0), |i| i.sequence);
    Ok(json!({"messages": messages, "hasMore": start > 0, "beforeOffset": before}))
}
