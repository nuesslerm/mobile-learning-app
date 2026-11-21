//! Orca's accounts rows over rca's accounts group. Orca keeps any number of logins per provider; rca keeps one per provider CLI, so each signed-in
//! provider is the one account of its kind: its id is the provider's name (`claudeAgent`, `codex`), and it is the active one when it is the provider
//! rca's turns use (`provider.select`). rca keeps no email, no time of a login and no rate-limit windows, so the account is labelled by its provider's
//! name, `updatedAt` is 0 and the limits are the host's "unavailable" status. `accounts.subscribe` opens with the snapshot and sends a new one when the list
//! of providers changes (rca has no push of provider changes, so the stream lists them at most every `POLL`). The limits are the provider's own
//! account's, read at the workspace host (`usage.summary`, kept for a minute); the credit spend and the host sign-in rows (a desktop's) go through `provider.resetCredit`
//! and `provider.auth.*` (`signin.rs`). The Claude sign-in rows (`beginClaudeSignIn` and its two) are the `orca` CLI's, neither the phone's nor the desktop's, and are not registered.
use agent_api_accounts::{AUTH_CANCEL, AUTH_COMPLETE, AUTH_RESPOND, AUTH_START, PROVIDER_LIST, PROVIDER_SELECT, RESET_CREDIT, USAGE_SUMMARY};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{Area, AgentApi, AgentError, Ctx, Kind, Method, Subscription};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod signin;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod usage;

use signin::SignIns;
use usage::UsageCache;

pub const ACCOUNTS_LIST: &str = "accounts.list";
pub const ACCOUNTS_SELECT_CLAUDE: &str = "accounts.selectClaude";
pub const ACCOUNTS_SELECT_CODEX: &str = "accounts.selectCodex";
pub const ACCOUNTS_SELECT_CODEX_FOR_TARGET: &str = "accounts.selectCodexForTarget";
pub const ACCOUNTS_SUBSCRIBE: &str = "accounts.subscribe";
pub const ACCOUNTS_UNSUBSCRIBE: &str = "accounts.unsubscribe";
pub const ACCOUNTS_CONSUME_CODEX_RESET_CREDIT: &str = "accounts.consumeCodexResetCredit";
pub const ACCOUNTS_HOST_SIGN_IN_START: &str = "accounts.hostSignInStart";
pub const ACCOUNTS_HOST_SIGN_IN_WAIT: &str = "accounts.hostSignInWait";
pub const ACCOUNTS_HOST_SIGN_IN_SUBMIT_CODE: &str = "accounts.hostSignInSubmitCode";
pub const ACCOUNTS_HOST_SIGN_IN_CANCEL: &str = "accounts.hostSignInCancel";

const POLL: Duration = Duration::from_millis(500);
const STREAM_PREFIX: &str = "accounts-";

/// The agent methods this crate sends: a flow test holds the list to the methods really sent.
pub const USES: &[AgentMethod] = &[PROVIDER_LIST, PROVIDER_SELECT, USAGE_SUMMARY, RESET_CREDIT, AUTH_START, AUTH_RESPOND, AUTH_COMPLETE, AUTH_CANCEL];

const METHODS: &[Method] = &[
    Method { name: ACCOUNTS_LIST, kind: Kind::Unary },
    Method { name: ACCOUNTS_SELECT_CLAUDE, kind: Kind::Unary },
    Method { name: ACCOUNTS_SELECT_CODEX, kind: Kind::Unary },
    Method { name: ACCOUNTS_SELECT_CODEX_FOR_TARGET, kind: Kind::Unary },
    Method { name: ACCOUNTS_SUBSCRIBE, kind: Kind::Streaming },
    Method { name: ACCOUNTS_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: ACCOUNTS_CONSUME_CODEX_RESET_CREDIT, kind: Kind::Unary },
    Method { name: ACCOUNTS_HOST_SIGN_IN_START, kind: Kind::Unary },
    Method { name: ACCOUNTS_HOST_SIGN_IN_WAIT, kind: Kind::Unary },
    Method { name: ACCOUNTS_HOST_SIGN_IN_SUBMIT_CODE, kind: Kind::Unary },
    Method { name: ACCOUNTS_HOST_SIGN_IN_CANCEL, kind: Kind::Unary },
];

const CLAUDE: &str = "claudeAgent";
const CODEX: &str = "codex";

#[derive(Default)]
pub struct AccountsArea {
    usage: std::sync::Arc<UsageCache>,
    sign_ins: SignIns,
}

impl Area for AccountsArea {
    fn name(&self) -> &'static str {
        "accounts"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            ACCOUNTS_LIST => {
                for key in ["refreshUsage", "refreshStaleUsage"] {
                    if let Some(v) = params.get(key).filter(|v| !v.is_boolean()) {
                        return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", orca_core::wire::received(v))));
                    }
                }
                snapshot(ctx.agent, &self.usage, params.get("refreshUsage").and_then(Value::as_bool).unwrap_or(true))
            }
            ACCOUNTS_SELECT_CLAUDE => select(ctx.agent, &self.usage, CLAUDE, params),
            ACCOUNTS_SELECT_CODEX => select(ctx.agent, &self.usage, CODEX, params),
            ACCOUNTS_SELECT_CODEX_FOR_TARGET => {
                if !matches!(params.get("accountId"), Some(Value::String(_) | Value::Null)) {
                    return Err(Failure::invalid("Invalid input"));
                }
                let target = &params["target"];
                if target["runtime"] == "host" && target.get("wslDistro").is_some_and(|d| !d.is_null()) {
                    return Err(Failure::invalid(format!("Invalid input: expected null, received {}", orca_core::wire::received(&target["wslDistro"]))));
                }
                if target["runtime"] != "host" && params["accountId"].as_str().is_some_and(|id| id != CODEX) {
                    return Err(gone(CODEX));
                }
                if target["runtime"] != "host" {
                    return Err(Failure::invalid("rca runs no WSL: only the host target can be selected"));
                }
                select(ctx.agent, &self.usage, CODEX, params)
            }
            ACCOUNTS_CONSUME_CODEX_RESET_CREDIT => consume_reset_credit(ctx.agent, &self.usage, params),
            ACCOUNTS_HOST_SIGN_IN_START => {
                let provider = match params.get("provider").and_then(Value::as_str) {
                    Some(p @ ("claude" | "codex")) => p,
                    _ => return Err(Failure::invalid("Invalid option: expected one of \"claude\"|\"codex\"")),
                };
                self.sign_ins.start(ctx.agent, ctx.device_id, provider, &|| account_lists(ctx.agent, &self.usage))
            }
            ACCOUNTS_HOST_SIGN_IN_WAIT => {
                let (flow, after) = (flow_id(params)?, after_revision(params)?);
                self.sign_ins.wait(ctx.agent, ctx.device_id, flow, after, &|| account_lists(ctx.agent, &self.usage))
            }
            ACCOUNTS_HOST_SIGN_IN_SUBMIT_CODE => {
                let flow = flow_id(params)?;
                let code = params.get("code").and_then(Value::as_str).ok_or_else(|| Failure::invalid(format!("Invalid input: expected string, received {}", received_of(params.get("code")))))?;
                self.sign_ins.submit_code(ctx.agent, ctx.device_id, flow, code, &|| account_lists(ctx.agent, &self.usage))
            }
            ACCOUNTS_HOST_SIGN_IN_CANCEL => {
                let flow = flow_id(params)?;
                self.sign_ins.cancel(ctx.agent, ctx.device_id, flow, &|| account_lists(ctx.agent, &self.usage))
            }
            ACCOUNTS_UNSUBSCRIBE => {
                let id = params.get("subscriptionId").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing subscriptionId"))?;
                // the host cleans up whatever id it is given and answers the same: an id of no stream of this connection ends nothing
                ctx.subscriptions.cancel(id);
                Ok(json!({"unsubscribed": true}))
            }
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }

    fn open(&self, ctx: &mut Ctx, method: &str, _params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        match method {
            ACCOUNTS_SUBSCRIBE => {
                let first = snapshot(ctx.agent, &self.usage, false)?;
                let id = ctx.subscription_id(STREAM_PREFIX);
                let ready = json!({"type": "ready", "subscriptionId": id, "snapshot": first});
                Ok(Box::new(Watch { id, agent: ctx.agent_handle.clone(), usage: self.usage.clone(), ready: Some(ready), last: first, asked: Instant::now() }))
            }
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

struct Watch {
    id: String,
    agent: Arc<dyn AgentApi>,
    usage: Arc<UsageCache>,
    ready: Option<Value>,
    last: Value,
    asked: Instant,
}

impl Subscription for Watch {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        if let Some(ready) = self.ready.take() {
            return vec![ready];
        }
        if self.asked.elapsed() < POLL {
            return Vec::new();
        }
        self.asked = Instant::now();
        // a list that cannot be read now is read again at the next poll: the phone keeps the last snapshot it has
        match snapshot(self.agent.as_ref(), &self.usage, false) {
            Ok(now) if now != self.last => {
                self.last = now.clone();
                vec![json!({"type": "snapshot", "snapshot": now})]
            }
            _ => Vec::new(),
        }
    }

    fn end(&mut self) -> Vec<Value> {
        vec![json!({"type": "end"})]
    }
}

fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, message } if code == "UNKNOWN_PROVIDER" || code == "INVALID_PARAMS" => Failure::invalid(message),
        AgentError::Refused { message, .. } => Failure::new(RUNTIME_ERROR, message),
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

/// One provider of `provider.list`: a row without its name, status or selection is refused, not read as a signed-out provider.
struct Provider<'a> {
    name: &'a str,
    signed_in: bool,
    selected: bool,
}

fn providers(list: &Value) -> Result<Vec<Provider<'_>>, Failure> {
    let incomplete = |what: &str| Failure::new(RUNTIME_ERROR, format!("rca's provider.list answered {what}"));
    list["providers"]
        .as_array()
        .ok_or_else(|| incomplete("no providers list"))?
        .iter()
        .map(|p| {
            Ok(Provider {
                name: p["provider"].as_str().ok_or_else(|| incomplete("a provider with no name"))?,
                signed_in: p["status"].as_str().ok_or_else(|| incomplete("a provider with no status"))? == "signed_in",
                selected: p["selected"].as_bool().ok_or_else(|| incomplete("a provider with no selection flag"))?,
            })
        })
        .collect()
}

/// The accounts snapshot of the phone's accounts screen.
fn snapshot(agent: &dyn AgentApi, usage: &UsageCache, force: bool) -> Result<Value, Failure> {
    let list = agent.request(PROVIDER_LIST.name(), json!({})).map_err(refusal)?;
    let all = providers(&list)?;
    let account = |name: &str| all.iter().find(|p| p.name == name && p.signed_in);
    let side = |name: &str, with_time: bool| {
        let found = account(name);
        let accounts: Vec<Value> = found.map(|p| if with_time { json!({"id": p.name, "email": p.name, "updatedAt": 0}) } else { json!({"id": p.name, "email": p.name}) }).into_iter().collect();
        let active = found.filter(|p| p.selected).map(|p| p.name);
        json!({"accounts": accounts, "activeAccountId": active, "activeAccountIdsByRuntime": {"host": active, "wsl": {}}})
    };
    let none = || json!({"accounts": [], "activeAccountId": null});
    let asked = [CLAUDE, CODEX].iter().any(|name| account(name).is_some());
    let summary = if asked { usage.summary(agent, force) } else { Value::Null };
    Ok(json!({
        "claude": side(CLAUDE, false),
        "codex": side(CODEX, true),
        "devin": none(),
        "opencode": none(),
        "rateLimits": {
            "claude": limits("claude", &all, &summary),
            "codex": limits("codex", &all, &summary),
            "claudeTarget": {"runtime": "host", "wslDistro": null},
            "codexTarget": {"runtime": "host", "wslDistro": null},
            "inactiveClaudeAccounts": [],
            "inactiveCodexAccounts": []
        }
    }))
}

/// A signed-in provider's limits are its own account's, as the workspace host read them; a provider that is signed out, or whose account was not asked,
/// is the host's "unavailable" status, never an invented window.
fn limits(provider: &str, all: &[Provider<'_>], summary: &Value) -> Value {
    let name = if provider == "claude" { CLAUDE } else { CODEX };
    if all.iter().any(|p| p.name == name && p.signed_in) {
        usage::limits_of(summary, provider)
    } else {
        usage::unavailable(provider)
    }
}

/// The host's answer for an account id it does not hold.
fn gone(provider: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, if provider == CLAUDE { "That Claude account no longer exists." } else { "That Codex rate limit account no longer exists." })
}

/// Makes the one account of `provider` the active one, which is selecting the provider rca's turns use.
fn select(agent: &dyn AgentApi, usage: &UsageCache, provider: &str, params: &Value) -> Result<Value, Failure> {
    match params.get("accountId") {
        Some(Value::String(id)) if id == provider => {}
        Some(Value::String(_)) => return Err(gone(provider)),
        None => return Err(Failure::invalid("Invalid input")),
        Some(Value::Null) => return Err(Failure::invalid("rca always uses a provider's own login: name the account, there is no unmanaged default to clear to")),
        Some(_) => return Err(Failure::invalid("Invalid input")),
    }
    agent.request(PROVIDER_SELECT.name(), json!({"provider": provider})).map_err(refusal)?;
    snapshot(agent, usage, false)
}

fn received_of(v: Option<&Value>) -> &'static str {
    v.map_or("undefined", orca_core::wire::received)
}

fn flow_id(params: &Value) -> Result<&str, Failure> {
    match params.get("flowId") {
        Some(Value::String(id)) if !id.is_empty() && id.len() <= 64 => Ok(id),
        Some(Value::String(id)) if id.is_empty() => Err(Failure::invalid("Too small: expected string to have >=1 characters")),
        Some(Value::String(_)) => Err(Failure::invalid("Too big: expected string to have <=64 characters")),
        other => Err(Failure::invalid(format!("Invalid input: expected string, received {}", received_of(other)))),
    }
}

fn after_revision(params: &Value) -> Result<u64, Failure> {
    match params.get("afterRevision") {
        None | Some(Value::Null) => Ok(0),
        Some(v) => v.as_u64().ok_or_else(|| Failure::invalid(format!("Invalid input: expected number, received {}", orca_core::wire::received(v)))),
    }
}

/// The accounts of both providers as the sign-in status and the spend's snapshot show them; a list that cannot be read is empty.
fn account_lists(agent: &dyn AgentApi, usage: &UsageCache) -> (Value, Value) {
    match snapshot(agent, usage, false) {
        Ok(s) => (s["claude"].clone(), s["codex"].clone()),
        Err(_) => (json!({"accounts": [], "activeAccountId": null}), json!({"accounts": [], "activeAccountId": null})),
    }
}

const UUID_PARTS: [usize; 5] = [8, 4, 4, 4, 12];

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5 && parts.iter().zip(UUID_PARTS).all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `accounts.consumeCodexResetCredit`: spends a credit of the Codex account at the workspace host (`provider.resetCredit`, which asks the account's
/// own app-server) and answers with the outcome and the accounts as they stand. rca keeps no account or offer revision, so `expectedScope` is checked for
/// its shape and its account only; the key shared by a retry is what keeps a credit from being spent twice.
fn consume_reset_credit(agent: &dyn AgentApi, usage: &UsageCache, params: &Value) -> Result<Value, Failure> {
    let key = params.get("idempotencyKey").and_then(Value::as_str).filter(|k| is_uuid(k)).ok_or_else(|| Failure::invalid("Invalid idempotencyKey"))?;
    let scope = &params["expectedScope"];
    let invalid = |what: &str| Failure::invalid(what.to_string());
    if !scope.is_object() {
        return Err(invalid("Invalid input: expected object, received undefined"));
    }
    let account = scope["accountId"].as_str().filter(|a| !a.is_empty() && a.len() <= 512).ok_or_else(|| invalid("Missing accountId"))?;
    if scope["accountRevision"].as_u64().is_none() {
        return Err(invalid("Invalid input: expected number, received undefined"));
    }
    if !scope["offerRevision"].as_str().is_some_and(|o| o.starts_with("v1:") && o.len() <= 4096) {
        return Err(invalid("Invalid offerRevision"));
    }
    let target = &scope["target"];
    let host = target["runtime"] == "host" && target["wslDistro"].is_null();
    if !host && !(target["runtime"] == "wsl" && target["wslDistro"].as_str().is_some_and(|d| !d.trim().is_empty())) {
        return Err(invalid("Invalid input"));
    }
    if !host {
        return Err(Failure::invalid("rca runs no WSL: only the host target can be spent against"));
    }
    if account != CODEX {
        return Err(gone(CODEX));
    }
    let spent = agent.request(RESET_CREDIT.name(), json!({"provider": CODEX, "idempotency_key": key})).map_err(refusal)?;
    usage.forget();
    let outcome = match spent["outcome"].as_str() {
        Some("nothing_to_reset") => "nothingToReset",
        Some("no_credit") => "noCredit",
        Some("already_redeemed") => "alreadyRedeemed",
        _ => "reset",
    };
    let state = snapshot(agent, usage, true)?;
    Ok(json!({"outcome": outcome, "scope": scope, "snapshot": {"claude": state["claude"], "codex": state["codex"], "rateLimits": state["rateLimits"]}}))
}
