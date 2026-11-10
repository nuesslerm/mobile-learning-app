//! A double of `agent.threads`, `agent.queue`, `agent.requests` and `attachments`: threads whose events are kept in memory in the shapes the service answers.
//! The chain runs the real service and executor against the same calls, so a shape that drifts fails there.
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
struct Thread {
    workspace: Option<String>,
    driver: Option<String>,
    model: Option<String>,
    events: Vec<Value>,
    running: Option<String>,
    turns: u32,
    queue: Vec<(String, String)>,
    next_item: u32,
    held: bool,
}

type Kept = Arc<Mutex<BTreeMap<String, (Value, Vec<u8>)>>>;

#[derive(Default, Clone)]
pub struct Threads(Arc<Mutex<BTreeMap<String, Thread>>>, Kept);

fn refused(code: &str, message: String) -> AgentError {
    AgentError::Refused { code: code.into(), message }
}

fn not_queued(item: &str) -> AgentError {
    refused("ITEM_NOT_QUEUED", format!("Run {item} is not queued."))
}

fn missing(thread: &str) -> AgentError {
    refused("THREAD_NOT_FOUND", format!("no thread has the id {thread}"))
}

impl Threads {
    /// Appends an event the way the engine would: numbered after the last one, in the turn that runs.
    pub fn push(&self, thread: &str, event: Value) -> u64 {
        let mut all = self.0.lock().unwrap();
        let t = all.get_mut(thread).expect("a thread of this double");
        let seq = t.events.len() as u64 + 1;
        let mut e = event;
        e["seq"] = seq.into();
        e["thread"] = thread.into();
        if e.get("turn").is_none() {
            if let Some(turn) = &t.running {
                e["turn"] = turn.clone().into();
            }
        }
        t.events.push(e);
        seq
    }

    pub fn events(&self, thread: &str) -> Vec<Value> {
        self.0.lock().unwrap().get(thread).map(|t| t.events.clone()).unwrap_or_default()
    }

    /// The turn that runs on the thread, if any.
    pub fn running(&self, thread: &str) -> Option<String> {
        self.0.lock().unwrap().get(thread).and_then(|t| t.running.clone())
    }

    /// Ends the running turn as the engine does when the CLI finishes.
    pub fn finish(&self, thread: &str, reason: &str) {
        self.push(thread, json!({"type": "turn.finished", "reason": reason}));
        self.0.lock().unwrap().get_mut(thread).unwrap().running = None;
    }

    /// The prompts of the queue, in the order they start.
    pub fn queued(&self, thread: &str) -> Vec<String> {
        self.0.lock().unwrap().get(thread).map(|t| t.queue.iter().map(|(_, p)| p.clone()).collect()).unwrap_or_default()
    }

    pub fn queued_ids(&self, thread: &str) -> Vec<String> {
        self.0.lock().unwrap().get(thread).map(|t| t.queue.iter().map(|(i, _)| i.clone()).collect()).unwrap_or_default()
    }

    /// Holds the queue as a forced stop does.
    pub fn hold(&self, thread: &str) {
        self.0.lock().unwrap().get_mut(thread).expect("a thread of this double").held = true;
    }

    /// Makes `agent` answer the methods of `USES` from this thread set (`repo.list` comes from the workspace double).
    pub fn install(&self, agent: &ScriptedAgent) {
        for method in ["thread.create", "thread.get", "thread.history", "turn.start", "thread.forceStop", "queue.add", "queue.list", "queue.edit", "queue.move", "queue.delete", "queue.resume", "turn.steer", "approval.answer", "input.answer", "input.dismiss", "upload.put", "attachment.get"] {
            let me = self.clone();
            agent.reply(method, move |p| me.answer(method, p));
        }
    }

    fn attachment(&self, method: &str, p: &Value) -> Result<Value, AgentError> {
        let view = |id: &str, meta: &Value, bytes: &[u8]| {
            let digest = sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
            json!({"id": id, "name": meta["name"], "mime_type": meta["mime_type"], "kind": meta["kind"], "size_bytes": bytes.len(), "sha256": digest, "created_at_ms": 0})
        };
        let mut kept = self.1.lock().unwrap();
        if method == "upload.put" {
            let id = p["id"].as_str().unwrap_or_default().to_string();
            let bytes = STANDARD.decode(p["data"].as_str().unwrap_or_default()).map_err(|_| refused("INVALID_PARAMS", "data is not base64".into()))?;
            let attachment = view(&id, p, &bytes);
            kept.insert(id, (p.clone(), bytes));
            return Ok(json!({"attachment": attachment}));
        }
        let id = p["id"].as_str().unwrap_or_default();
        let (meta, bytes) = kept.get(id).ok_or_else(|| refused("ATTACHMENT_NOT_FOUND", format!("no attachment has the id {id}")))?;
        let from = (p["offset"].as_u64().unwrap_or(0) as usize).min(bytes.len());
        let to = (from + p["length"].as_u64().unwrap_or(1 << 20) as usize).min(bytes.len());
        Ok(json!({"attachment": view(id, meta, bytes), "data": STANDARD.encode(&bytes[from..to]), "offset": from, "bytes": to - from, "eof": to == bytes.len()}))
    }

    fn take(&self, id: &str, item: &str) -> Result<(String, String), AgentError> {
        let mut all = self.0.lock().unwrap();
        let q = &mut all.get_mut(id).unwrap().queue;
        let at = q.iter().position(|(i, _)| i == item).ok_or_else(|| not_queued(item))?;
        Ok(q.remove(at))
    }

    fn view(&self, id: &str, t: &Thread) -> Value {
        let mut v = json!({"id": id, "title": "", "status": if t.running.is_some() { "running" } else { "idle" }, "last_seq": t.events.len(), "created_at": "2026-11-04T10:00:00Z", "updated_at": "2026-11-04T10:00:00Z"});
        for (k, f) in [("workspace", &t.workspace), ("driver", &t.driver), ("model", &t.model), ("turn", &t.running)] {
            if let Some(x) = f {
                v[k] = x.clone().into();
            }
        }
        v
    }

    fn answer(&self, method: &str, p: &Value) -> Result<Value, AgentError> {
        if matches!(method, "upload.put" | "attachment.get") {
            return self.attachment(method, p);
        }
        let id = p["thread"].as_str().unwrap_or_default().to_string();
        if method == "thread.create" {
            let mut all = self.0.lock().unwrap();
            let t = all.entry(id.clone()).or_insert_with(|| Thread { workspace: p["workspace"].as_str().map(str::to_string), driver: p["driver"].as_str().map(str::to_string), model: p["model"].as_str().map(str::to_string), ..Thread::default() });
            return Ok(json!({"thread": self.view(&id, t)}));
        }
        let snapshot = self.0.lock().unwrap().get(&id).cloned().ok_or_else(|| missing(&id))?;
        match method {
            "thread.get" => Ok(json!({"thread": self.view(&id, &snapshot)})),
            "thread.history" => {
                let after = p["after_seq"].as_u64().unwrap_or(0) as usize;
                let limit = p["limit"].as_u64().unwrap_or(200) as usize;
                let rest: Vec<Value> = snapshot.events.iter().skip(after).cloned().collect();
                let page: Vec<Value> = rest.iter().take(limit).cloned().collect();
                Ok(json!({"events": page, "last_seq": snapshot.events.len(), "more": rest.len() > limit}))
            }
            "turn.start" => {
                let kept = self.1.lock().unwrap();
                if let Some(id) = p["attachments"].as_array().into_iter().flatten().filter_map(Value::as_str).find(|a| !kept.contains_key(*a)) {
                    return Err(refused("ATTACHMENT_NOT_FOUND", format!("no attachment has the id {id}")));
                }
                drop(kept);
                if let Some(turn) = snapshot.running {
                    return Err(refused("TURN_RUNNING", format!("thread {id} already has a running turn ({turn})")));
                }
                let turn = {
                    let mut all = self.0.lock().unwrap();
                    let t = all.get_mut(&id).unwrap();
                    t.turns += 1;
                    t.running = Some(format!("u{}", t.turns));
                    t.running.clone().unwrap()
                };
                self.push(&id, json!({"type": "turn.started"}));
                self.push(&id, json!({"type": "message.delta", "role": "user", "text": p["prompt"]}));
                Ok(json!({"thread": id, "turn": turn}))
            }
            "thread.forceStop" => {
                let Some(turn) = snapshot.running else { return Err(refused("NO_TURN", format!("thread {id} has no running turn"))) };
                self.finish(&id, "interrupted");
                self.hold(&id);
                Ok(json!({"thread": id, "turn": turn}))
            }
            "turn.steer" => {
                let Some(turn) = snapshot.running else { return Err(refused("NO_TURN", format!("thread {id} has no running turn"))) };
                let prompt = match p["item"].as_str() {
                    Some(item) => self.take(&id, item)?.1,
                    None => p["prompt"].as_str().unwrap_or_default().to_string(),
                };
                if let Some(item) = p["item"].as_str() {
                    self.push(&id, json!({"type": "turn.steered", "prompt": prompt, "item": item}));
                }
                let seq = self.push(&id, json!({"type": "message.delta", "role": "user", "text": prompt}));
                Ok(json!({"thread": id, "turn": turn, "seq": seq}))
            }
            "queue.add" => {
                let (item, position) = {
                    let mut all = self.0.lock().unwrap();
                    let t = all.get_mut(&id).unwrap();
                    t.next_item += 1;
                    let item = format!("q{}", t.next_item);
                    t.queue.push((item.clone(), p["prompt"].as_str().unwrap_or_default().to_string()));
                    (item, t.queue.len())
                };
                self.push(&id, json!({"type": "queue.added", "item": {"item": item, "prompt": p["prompt"]}, "position": position}));
                Ok(json!({"thread": id, "item": {"item": item, "prompt": p["prompt"]}, "position": position, "seq": snapshot.events.len()}))
            }
            "queue.list" => {
                let items: Vec<Value> = snapshot.queue.iter().map(|(i, pr)| json!({"item": i, "prompt": pr, "created_at_ms": 0})).collect();
                Ok(json!({"thread": id, "held": snapshot.held, "items": items}))
            }
            "queue.edit" => {
                let item = p["item"].as_str().unwrap_or_default();
                self.0.lock().unwrap().get_mut(&id).unwrap().queue.iter_mut().find(|(i, _)| i == item).ok_or_else(|| not_queued(item))?.1 = p["prompt"].as_str().unwrap_or_default().to_string();
                let seq = self.push(&id, json!({"type": "queue.edited", "item": item, "prompt": p["prompt"]}));
                Ok(json!({"thread": id, "item": item, "seq": seq}))
            }
            "queue.move" => {
                let item = p["item"].as_str().unwrap_or_default();
                let order = {
                    let mut all = self.0.lock().unwrap();
                    let t = all.get_mut(&id).unwrap();
                    let from = t.queue.iter().position(|(i, _)| i == item).ok_or_else(|| not_queued(item))?;
                    let moved = t.queue.remove(from);
                    let at = match p["before"].as_str() {
                        Some(b) => t.queue.iter().position(|(i, _)| i == b).ok_or_else(|| refused("ITEM_NOT_QUEUED", format!("Queue target {b} is not queued.")))?,
                        None => t.queue.len(),
                    };
                    t.queue.insert(at, moved);
                    t.queue.iter().map(|(i, _)| i.clone()).collect::<Vec<_>>()
                };
                let seq = self.push(&id, json!({"type": "queue.moved", "order": order}));
                Ok(json!({"thread": id, "order": order, "seq": seq}))
            }
            "queue.delete" => {
                let item = p["item"].as_str().unwrap_or_default();
                self.take(&id, item)?;
                let seq = self.push(&id, json!({"type": "queue.deleted", "item": item}));
                Ok(json!({"thread": id, "item": item, "seq": seq}))
            }
            "queue.resume" => {
                self.0.lock().unwrap().get_mut(&id).unwrap().held = false;
                if let Some(item) = p["item"].as_str() {
                    if snapshot.running.is_some() {
                        return Err(refused("NOT_STOPPED", "A queued user message can only be sent now while the thread is stopped.".into()));
                    }
                    let (_, prompt) = self.take(&id, item)?;
                    self.push(&id, json!({"type": "queue.started", "item": item}));
                    self.answer("turn.start", &json!({"thread": id, "prompt": prompt}))?;
                    return Ok(json!({"thread": id, "held": false, "started": item, "seq": snapshot.events.len()}));
                }
                Ok(json!({"thread": id, "held": false, "seq": snapshot.events.len()}))
            }
            "approval.answer" => {
                self.push(&id, json!({"type": "approval.answered", "approval": p["approval"], "decision": p["decision"]}));
                Ok(json!({"thread": id}))
            }
            "input.answer" => {
                self.push(&id, json!({"type": "input.answered", "request": p["request"], "answer": p["answer"]}));
                Ok(json!({"thread": id}))
            }
            "input.dismiss" => {
                self.push(&id, json!({"type": "input.dismissed", "request": p["request"]}));
                Ok(json!({"thread": id}))
            }
            other => Err(refused("UNKNOWN_METHOD", format!("{other} is not a method of this double"))),
        }
    }
}

/// A double of `agent.catalog`: the agent CLIs one executor has and the skills of its workspaces, in the shapes the service answers (a test
/// holds them to the contract crate's types). `opus` carries an effort here so the mapping of efforts is exercised.
#[derive(Default, Clone)]
pub struct Catalog(Arc<Mutex<CatalogState>>);

#[derive(Default)]
struct CatalogState {
    programs: Vec<String>,
    skills: Vec<Value>,
    detections: Vec<Value>,
}

const EXECUTOR: &str = "m1";

impl Catalog {
    /// The programs the executor has (`claude`, `codex`, `opencode`); the others are missing.
    pub fn has(&self, programs: &[&str]) -> &Catalog {
        self.0.lock().unwrap().programs = programs.iter().map(|p| p.to_string()).collect();
        self
    }

    /// A skill the executor found: `scope` is `workspace` or `home`, `dir` its skills directory.
    pub fn skill(&self, scope: &str, dir: &str, name: &str, description: Option<&str>) -> &Catalog {
        let path = if scope == "home" { format!("~/{dir}/{name}/SKILL.md") } else { format!("{dir}/{name}/SKILL.md") };
        let mut s = json!({"name": name, "scope": scope, "dir": dir, "path": path});
        if let Some(d) = description {
            s["description"] = d.into();
        }
        self.0.lock().unwrap().skills.push(s);
        self
    }

    /// The `agents.detect` requests the executor was asked: their params, in order.
    pub fn detections(&self) -> Vec<Value> {
        self.0.lock().unwrap().detections.clone()
    }

    pub fn executor(&self) -> &'static str {
        EXECUTOR
    }

    pub fn install(&self, agent: &ScriptedAgent) {
        for method in ["models.list", "agents.detect", "skills.list", "commands.list"] {
            let me = self.clone();
            agent.reply(method, move |p| me.answer(method, p));
        }
    }

    fn models(driver: &str) -> Value {
        let model = |id: &str, label: &str, is_default: bool| json!({"id": id, "label": label, "is_default": is_default, "efforts": []});
        let models = match driver {
            "claudeAgent" => {
                let mut opus = model("opus", "Opus", false);
                opus["default_effort"] = "high".into();
                opus["efforts"] = json!([{"value": "low", "label": "Low"}, {"value": "high", "label": "High", "description": "Thinks longer"}]);
                json!([model("sonnet", "Sonnet", true), opus])
            }
            "codex" => json!([model("gpt-5-codex", "GPT-5 Codex", true), model("gpt-5", "GPT-5", false)]),
            _ => json!([model("default", "Default", true)]),
        };
        json!({"driver": driver, "models": models, "runtime_modes": [], "default_runtime_mode": "full-access", "interaction_modes": [], "default_interaction_mode": "default"})
    }

    fn answer(&self, method: &str, p: &Value) -> Result<Value, AgentError> {
        let mut state = self.0.lock().unwrap();
        match method {
            "models.list" => Ok(Self::models(p["driver"].as_str().unwrap_or_default())),
            "agents.detect" => {
                if p["executor"].as_str().is_some_and(|e| e != EXECUTOR) {
                    return Err(refused("UNKNOWN_EXECUTOR", format!("no executor named {} is attached", p["executor"].as_str().unwrap_or_default())));
                }
                state.detections.push(p.clone());
                let agents: Vec<Value> = [("opencode", "opencode"), ("claudeAgent", "claude"), ("codex", "codex")]
                    .iter()
                    .map(|(driver, program)| {
                        let there = state.programs.iter().any(|x| x == program);
                        if there {
                            json!({"driver": driver, "program": program, "installed": true, "path": format!("/usr/bin/{program}"), "version": "1.0.0"})
                        } else {
                            json!({"driver": driver, "program": program, "installed": false})
                        }
                    })
                    .collect();
                Ok(json!({"executor": EXECUTOR, "agents": agents, "checked_at_ms": 1}))
            }
            "skills.list" => {
                if p["workspace"].as_str().is_none_or(|w| w.is_empty()) {
                    return Err(refused("INVALID_PARAMS", "workspace is required".into()));
                }
                Ok(json!({"skills": state.skills, "truncated": false}))
            }
            _ => {
                let driver = p["driver"].as_str().unwrap_or_default();
                let dirs = agent_api_catalog::skill_dirs(driver);
                let commands: Vec<Value> = match p["workspace"].as_str() {
                    None => Vec::new(),
                    Some(_) => state
                        .skills
                        .iter()
                        .filter(|s| dirs.contains(&s["dir"].as_str().unwrap_or_default()))
                        .map(|s| json!({"name": s["name"], "kind": "skill", "description": s["description"], "builtin": false, "path": s["path"]}))
                        .collect(),
                };
                let commands: Vec<Value> = commands.into_iter().map(|mut c| if c["description"].is_null() { c.as_object_mut().unwrap().remove("description"); c } else { c }).collect();
                Ok(json!({"driver": driver, "commands": commands}))
            }
        }
    }
}
