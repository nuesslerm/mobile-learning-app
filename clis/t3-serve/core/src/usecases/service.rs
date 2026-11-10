//! The application facade the driving adapter calls: the descriptor, the token exchange, tickets, the socket gate and the config.
use super::engine::{Dispatched, Engine, Failure};
use super::forward::{is_forwarded, Forwarded};
use super::pairing::{mint, PairingRequest};
use super::projects::FORWARDED as FORWARDED_PROJECTS;
use super::runner::Runner;
use crate::domain::command::Command;
use crate::domain::command::Family;
use crate::domain::decider::Effect;
use crate::domain::auth::{hash_secret, opaque_secret, token_id};
use crate::ports::{AgentApi, AuthStore, Clock, HostProbe, Ids, Log, MetaStore, ModelCatalog, StorageProbe, TicketRecord, TokenRecord};
use super::settings::Held;
use crate::rpc::Connection;
use serde_json::{json, Value};
use std::sync::Arc;
use t3_contracts::scopes::legacy_subset;
use t3_contracts::time::iso;
use t3_contracts::wire::{self, ConfigInput, Environment, Model, BEARER_TTL_SECONDS, TICKET_TTL_SECONDS, TOKEN_EXCHANGE_GRANT};
use t3_contracts::ORCHESTRATION_PROTOCOL_PARAM;

#[derive(Debug, Clone)]
pub struct EnvironmentInfo {
    pub id: String,
    pub label: String,
    pub os: String,
    pub arch: String,
    pub server_version: String,
    pub cwd: String,
}

/// An HTTP answer the core decided on instead of a result.
#[derive(Debug)]
pub struct Refusal {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone)]
pub struct Principal {
    pub token_id: String,
    pub scopes: Vec<String>,
    pub expires_at: i64,
}

pub struct Parts {
    pub env: EnvironmentInfo,
    pub auth: Arc<dyn AuthStore>,
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn Ids>,
    pub log: Arc<dyn Log>,
    pub catalog: Arc<dyn ModelCatalog>,
    pub engine: Arc<Engine>,
    pub agent: Arc<dyn AgentApi>,
    /// What links a thread's rows to its log in the agent API: the turn of each run and how far the log is folded.
    pub meta: Arc<dyn MetaStore>,
    /// The machine `server.getHostResources` reads.
    pub host: Arc<dyn HostProbe>,
    /// The database `server.getDatabaseStorage` measures.
    pub storage: Arc<dyn StorageProbe>,
}

#[derive(Clone)]
pub struct Service(pub(crate) Arc<Parts>, Arc<Runner>, pub(crate) Arc<Held>);

fn refuse(status: u16, body: Value) -> Refusal {
    Refusal { status, body }
}

impl Service {
    pub fn new(parts: Parts) -> Service {
        let runner = Arc::new(Runner::new(parts.agent.clone(), parts.engine.clone(), parts.meta.clone(), parts.log.clone()));
        Service(Arc::new(parts), runner, Arc::new(Held::default()))
    }

    pub(crate) fn turn_of_run(&self, run: &str) -> Option<String> {
        self.1.turn_of_run(run)
    }

    pub fn agent(&self) -> &dyn AgentApi {
        self.1.agent()
    }

    /// Follows the runs a restart left running; called once, when the server starts.
    pub fn resume_runs(&self) {
        self.1.resume();
    }

    /// Catches a thread's rows up with its log in the agent API before they are read.
    pub fn sync_thread(&self, thread: &str) {
        self.1.sync_thread(thread);
    }

    /// Decides and stores a command, then starts or stops the run it asked for.
    pub fn dispatch(&self, family: Family, raw: &Value) -> Result<Dispatched, Failure> {
        self.dispatch_as(family, raw, true)
    }

    /// `dispatch`, where `announce` says whether a thread it makes is made in the agent API at once. A launch that starts a run as it makes the
    /// thread leaves that to the run.
    pub(super) fn dispatch_as(&self, family: Family, raw: &Value, announce: bool) -> Result<Dispatched, Failure> {
        let parsed = Command::parse(raw, family).ok();
        let fresh = |cmd: &Command| !self.0.engine.accepted(&cmd.command_id).unwrap_or(false);
        let forwarded = match &parsed {
            Some(cmd) if family == Family::Thread && is_forwarded(&cmd.kind) && fresh(cmd) => {
                let outcome = self.forward(cmd).map_err(|message| Failure::Refused { command_id: cmd.command_id.clone(), command_type: cmd.kind.clone(), message })?;
                Some((cmd.clone(), outcome))
            }
            _ => None,
        };
        if let Some((cmd, Forwarded::Kept)) = &forwarded {
            let sequence = self.0.engine.read(|state| state.last_sequence);
            self.log("command.kept", &[("type", cmd.kind.clone())]);
            return Ok(Dispatched { sequence, row: None, effect: None, replayed: false });
        }
        let project = parsed.as_ref().filter(|cmd| family == Family::Project && FORWARDED_PROJECTS.contains(&cmd.kind.as_str()) && fresh(cmd));
        if let Some(cmd) = project {
            self.project_before(cmd).map_err(|message| Failure::Refused { command_id: cmd.command_id.clone(), command_type: cmd.kind.clone(), message })?;
        }
        let dispatched = self.0.engine.dispatch(family, raw)?;
        if let Some((cmd, _)) = &forwarded {
            self.mirror(cmd);
        }
        if !dispatched.replayed {
            if let Some(cmd) = project {
                self.project_after(cmd);
            }
            if let Some(cmd) = parsed.as_ref().filter(|cmd| announce && family == Family::Thread && cmd.kind == "thread.create") {
                self.thread_created(cmd);
            }
        }
        match &dispatched.effect {
            Some(Effect::Launch(launch)) => self.1.launch(launch.clone()),
            Some(Effect::Interrupt { thread_id }) => self.1.interrupt(thread_id),
            None => {}
        }
        Ok(dispatched)
    }

    pub fn env(&self) -> &EnvironmentInfo {
        &self.0.env
    }

    pub fn engine(&self) -> &Engine {
        &self.0.engine
    }

    pub fn now_ms(&self) -> i64 {
        self.0.clock.now_ms()
    }

    pub fn log(&self, name: &'static str, fields: &[(&'static str, String)]) {
        self.0.log.event(name, fields);
    }

    fn environment(&self) -> Environment<'_> {
        let e = &self.0.env;
        Environment { id: &e.id, label: &e.label, os: &e.os, arch: &e.arch, server_version: &e.server_version }
    }

    pub fn descriptor(&self) -> Value {
        wire::descriptor(&self.environment())
    }

    pub fn server_config(&self) -> Value {
        let models = self.0.catalog.models();
        let models = models.iter().map(|m| Model { slug: &m.slug, name: &m.name }).collect();
        let mut config = wire::server_config(&ConfigInput { environment: self.descriptor(), cwd: &self.0.env.cwd, models, now: &iso(self.0.clock.now_ms()) });
        if let (Some(settings), Some(map)) = (self.config_settings(), config.as_object_mut()) {
            map.insert("settings".into(), settings);
        }
        config
    }

    /// `POST /oauth/token`: RFC 8693 token exchange of a pairing code for a 30-day bearer token.
    pub fn exchange(&self, form: &[(String, String)]) -> Result<Value, Refusal> {
        let field = |name: &str| form.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
        if field("grant_type") != Some(TOKEN_EXCHANGE_GRANT) {
            return Err(refuse(400, wire::bad_request("grant_type is the token-exchange grant")));
        }
        let Some(code) = field("subject_token") else {
            return Err(refuse(400, wire::bad_request("subject_token is the pairing code")));
        };
        let now = self.0.clock.now_ms();
        let grant = match self.0.auth.consume_grant(&hash_secret(code), now) {
            Ok(Ok(grant)) => grant,
            Ok(Err(reason)) => {
                self.log("oauth.exchange.refused", &[("reason", format!("{reason:?}").to_lowercase())]);
                return Err(refuse(401, wire::auth_invalid(&opaque_secret(&self.0.ids.random_bytes(16)))));
            }
            Err(e) => return Err(self.store_failed("oauth.exchange.failed", e.0)),
        };
        let secret = opaque_secret(&self.0.ids.random_bytes(32));
        let hash = hash_secret(&secret);
        let record = TokenRecord { id: token_id(&hash), scopes: grant.scopes.clone(), label: grant.label.clone(), expires_at: now + BEARER_TTL_SECONDS * 1000 };
        self.0.auth.put_token(&hash, &record).map_err(|e| self.store_failed("oauth.exchange.failed", e.0))?;
        self.log("oauth.exchange.ok", &[("tokenId", record.id.clone()), ("label", record.label)]);
        Ok(wire::token_response(&secret, &grant.scopes))
    }

    fn store_failed(&self, event: &'static str, error: String) -> Refusal {
        self.log(event, &[("error", error)]);
        refuse(500, json!({"code": "internal", "message": "The server could not reach its store."}))
    }

    /// The principal behind an `Authorization: Bearer` header value.
    pub fn authenticate(&self, header: Option<&str>) -> Result<Principal, Refusal> {
        let denied = |reason: &'static str| {
            self.log("auth.bearer.refused", &[("reason", reason.to_string())]);
            refuse(401, wire::unauthorized("A valid bearer token is required."))
        };
        let token = header.and_then(|h| h.strip_prefix("Bearer ")).map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| denied("missing"))?;
        let record = self.0.auth.find_token(&hash_secret(token)).map_err(|e| self.store_failed("auth.bearer.failed", e.0))?.ok_or_else(|| denied("unknown"))?;
        if self.0.clock.now_ms() >= record.expires_at {
            return Err(denied("expired"));
        }
        Ok(Principal { token_id: record.id, scopes: record.scopes, expires_at: record.expires_at })
    }

    pub fn session(&self, header: Option<&str>) -> Value {
        match self.authenticate(header) {
            Ok(p) => wire::session_state(Some((&p.scopes, &legacy_subset(&p.scopes), &iso(p.expires_at)))),
            Err(_) => wire::session_state(None),
        }
    }

    /// `POST /api/auth/websocket-ticket[/:use]`: a 5-minute single-use ticket for the socket (`rpc`) or media.
    pub fn ticket(&self, header: Option<&str>, use_: &str) -> Result<Value, Refusal> {
        let principal = self.authenticate(header)?;
        if use_ != "rpc" && use_ != "media" {
            return Err(refuse(400, wire::bad_request("use is rpc or media")));
        }
        let secret = opaque_secret(&self.0.ids.random_bytes(24));
        let expires_at = self.0.clock.now_ms() + TICKET_TTL_SECONDS * 1000;
        let record = TicketRecord { token_id: principal.token_id, scopes: principal.scopes, use_: use_.into() };
        self.0.auth.put_ticket(&hash_secret(&secret), &record, expires_at).map_err(|e| self.store_failed("ws.ticket.failed", e.0))?;
        Ok(wire::ticket_response(&secret, &iso(expires_at)))
    }

    /// `POST /api/auth/pairing-token`: a pairing code for another device, which a client with `access:write` mints and which carries only scopes
    /// that client holds itself, so a device can never hand out more than it has.
    pub fn pairing_credential(&self, header: Option<&str>, body: &Value) -> Result<Value, Refusal> {
        let principal = self.authenticate(header)?;
        let insufficient = |scope: &str| {
            self.log("pairing.credential.refused", &[("scope", scope.to_string()), ("tokenId", principal.token_id.clone())]);
            refuse(403, json!({"_tag": "EnvironmentScopeRequiredError", "code": "insufficient_scope", "requiredScope": scope, "traceId": opaque_secret(&self.0.ids.random_bytes(16))}))
        };
        if !principal.scopes.iter().any(|s| s == "access:write") {
            return Err(insufficient("access:write"));
        }
        let invalid = |message: &str| refuse(400, json!({"_tag": "EnvironmentHttpBadRequestError", "code": "bad_request", "reason": "invalid_scope", "message": message}));
        let label = match body.get("label") {
            None => None,
            Some(v) => Some(v.as_str().map(str::trim).filter(|l| !l.is_empty()).ok_or_else(|| refuse(400, wire::bad_request("label is a non-empty string")))?.to_string()),
        };
        let scopes: Vec<String> = match body.get("scopes") {
            None => t3_contracts::scopes::DEFAULT_PAIRING_SCOPES.iter().map(|s| s.to_string()).collect(),
            Some(v) => v.as_array().and_then(|a| a.iter().map(|s| s.as_str().map(str::to_string)).collect()).ok_or_else(|| refuse(400, wire::bad_request("scopes is a list of scope names")))?,
        };
        if scopes.is_empty() || scopes.iter().enumerate().any(|(i, s)| scopes[..i].contains(s)) {
            return Err(invalid("scopes is a non-empty list with no scope twice"));
        }
        if let Some(unknown) = scopes.iter().find(|s| !t3_contracts::scopes::is_scope(s)) {
            return Err(invalid(&format!("{unknown} is not a scope")));
        }
        if let Some(missing) = scopes.iter().find(|s| !principal.scopes.contains(s)) {
            return Err(insufficient(missing));
        }
        let request = PairingRequest { scopes, label: label.clone().unwrap_or_else(|| "pairing link".into()), ttl_seconds: wire::PAIRING_TTL_SECONDS };
        let pairing = mint(self.0.auth.as_ref(), self.0.clock.as_ref(), self.0.ids.as_ref(), request).map_err(|e| self.store_failed("pairing.credential.failed", e))?;
        self.log("pairing.credential.minted", &[("tokenId", principal.token_id), ("scopes", pairing.scopes.join(","))]);
        let mut answer = json!({"id": token_id(&hash_secret(&pairing.code)), "credential": pairing.code, "expiresAt": iso(pairing.expires_at)});
        if let Some(label) = label {
            answer["label"] = json!(label);
        }
        Ok(answer)
    }

    /// The `/ws` gate: the protocol parameter first (426, before any authentication), then the ticket (401).
    pub fn connect(&self, query: &[(String, String)]) -> Result<Connection, Refusal> {
        let param = |name: &str| query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        if param(ORCHESTRATION_PROTOCOL_PARAM) != Some("2") {
            self.log("ws.upgrade.refused", &[("reason", "protocol".into())]);
            return Err(refuse(426, wire::protocol_incompatible()));
        }
        let denied = |reason: &'static str| {
            self.log("ws.upgrade.refused", &[("reason", reason.to_string())]);
            refuse(401, wire::unauthorized("A valid websocket ticket is required."))
        };
        let ticket = param("wsTicket").filter(|t| !t.is_empty()).ok_or_else(|| denied("no_ticket"))?;
        let record = self.0.auth.consume_ticket(&hash_secret(ticket), self.0.clock.now_ms()).map_err(|e| self.store_failed("ws.upgrade.failed", e.0))?.ok_or_else(|| denied("bad_ticket"))?;
        if record.use_ != "rpc" {
            return Err(denied("wrong_use"));
        }
        self.log("ws.opened", &[("tokenId", record.token_id.clone())]);
        Ok(Connection::new(self.clone(), Principal { token_id: record.token_id, scopes: record.scopes, expires_at: 0 }))
    }
}
