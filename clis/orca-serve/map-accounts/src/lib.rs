//! Orca's accounts rows over rca's accounts group. Orca keeps any number of logins per provider; rca keeps one per provider CLI, so each signed-in
//! provider is the one account of its kind: its id is the provider's name (`claudeAgent`, `codex`), and it is the active one when it is the provider
//! rca's turns use (`provider.select`). rca keeps no email, no time of a login and no rate-limit windows, so the account is labelled by its provider's
//! name, `updatedAt` is 0 and the limits are the host's "unavailable" status. Streams of account changes, the reset credit and the other rows of Orca's accounts group are
//! not mapped and stay pending.
use agent_api_accounts::{PROVIDER_LIST, PROVIDER_SELECT};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{Area, AgentApi, AgentError, Ctx, Kind, Method};
use serde_json::{json, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const ACCOUNTS_LIST: &str = "accounts.list";
pub const ACCOUNTS_SELECT_CLAUDE: &str = "accounts.selectClaude";
pub const ACCOUNTS_SELECT_CODEX: &str = "accounts.selectCodex";
pub const ACCOUNTS_SELECT_CODEX_FOR_TARGET: &str = "accounts.selectCodexForTarget";

/// The agent methods this crate sends: a flow test holds the list to the methods really sent.
pub const USES: &[AgentMethod] = &[PROVIDER_LIST, PROVIDER_SELECT];

const METHODS: &[Method] = &[
    Method { name: ACCOUNTS_LIST, kind: Kind::Unary },
    Method { name: ACCOUNTS_SELECT_CLAUDE, kind: Kind::Unary },
    Method { name: ACCOUNTS_SELECT_CODEX, kind: Kind::Unary },
    Method { name: ACCOUNTS_SELECT_CODEX_FOR_TARGET, kind: Kind::Unary },
];

const CLAUDE: &str = "claudeAgent";
const CODEX: &str = "codex";

pub struct AccountsArea;

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
                snapshot(ctx.agent)
            }
            ACCOUNTS_SELECT_CLAUDE => select(ctx.agent, CLAUDE, params),
            ACCOUNTS_SELECT_CODEX => select(ctx.agent, CODEX, params),
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
                select(ctx.agent, CODEX, params)
            }
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
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
fn snapshot(agent: &dyn AgentApi) -> Result<Value, Failure> {
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
    Ok(json!({
        "claude": side(CLAUDE, false),
        "codex": side(CODEX, true),
        "devin": none(),
        "opencode": none(),
        "rateLimits": {
            "claude": unavailable("claude"),
            "codex": unavailable("codex"),
            "claudeTarget": {"runtime": "host", "wslDistro": null},
            "codexTarget": {"runtime": "host", "wslDistro": null},
            "inactiveClaudeAccounts": [],
            "inactiveCodexAccounts": []
        }
    }))
}

/// rca reads no usage windows, so a provider's limits are the host's "unavailable" status, never an invented window.
fn unavailable(provider: &str) -> Value {
    json!({"provider": provider, "status": "unavailable", "session": null, "weekly": null, "updatedAt": 0, "error": "rca does not read usage limits"})
}

/// The host's answer for an account id it does not hold.
fn gone(provider: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, if provider == CLAUDE { "That Claude account no longer exists." } else { "That Codex rate limit account no longer exists." })
}

/// Makes the one account of `provider` the active one, which is selecting the provider rca's turns use.
fn select(agent: &dyn AgentApi, provider: &str, params: &Value) -> Result<Value, Failure> {
    match params.get("accountId") {
        Some(Value::String(id)) if id == provider => {}
        Some(Value::String(_)) => return Err(gone(provider)),
        None => return Err(Failure::invalid("Invalid input")),
        Some(Value::Null) => return Err(Failure::invalid("rca always uses a provider's own login: name the account, there is no unmanaged default to clear to")),
        Some(_) => return Err(Failure::invalid("Invalid input")),
    }
    agent.request(PROVIDER_SELECT.name(), json!({"provider": provider})).map_err(refusal)?;
    snapshot(agent)
}
