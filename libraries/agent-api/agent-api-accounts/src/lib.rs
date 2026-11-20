//! The `accounts` group of the agent API: the provider CLIs (Claude Code, Codex, opencode) the service runs, which of them hold a login, and how
//! one is signed in. Each provider keeps its login in a directory of its own on the service's volume and its CLI writes the tokens there itself,
//! so nothing in this group carries a token: a mapper sees a status, the address to open and the code to enter, never a credential.
//!
//! A login is an authentication session: `provider.auth.start` starts the CLI's login and names the session (`auth`); the session's prompt (an
//! address and a code) and its end arrive as `provider.auth.updated` events of `provider.auth.subscribe`; `provider.auth.respond` answers a flow
//! that asks the person to paste a code; `provider.auth.complete` waits for the end; `provider.auth.cancel` stops it.
//! Times are milliseconds, fields are snake_case. Provider names are the drivers' (`claudeAgent`, `codex`, `opencode`).
use agent_api_proto::{text, Event, GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[cfg(feature = "testing")]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("accounts");

pub const PROVIDER_LIST: Method = Method::new(GROUP, "provider.list");
pub const PROVIDER_REFRESH: Method = Method::new(GROUP, "provider.refresh");
pub const PROVIDER_SELECT: Method = Method::new(GROUP, "provider.select");
pub const AUTH_START: Method = Method::new(GROUP, "provider.auth.start");
pub const AUTH_RESPOND: Method = Method::new(GROUP, "provider.auth.respond");
pub const AUTH_COMPLETE: Method = Method::new(GROUP, "provider.auth.complete");
pub const AUTH_CANCEL: Method = Method::new(GROUP, "provider.auth.cancel");
pub const AUTH_SUBSCRIBE: Method = Method::new(GROUP, "provider.auth.subscribe");
pub const PROVIDER_LOGOUT: Method = Method::new(GROUP, "provider.logout");
pub const USAGE_SUMMARY: Method = Method::new(GROUP, "usage.summary");
pub const RESET_CREDIT: Method = Method::new(GROUP, "provider.resetCredit");
/// The stream of the provider list: its whole state on opening and after each change (a login ended, a provider selected or signed out).
pub const PROVIDER_SUBSCRIBE: Method = Method::new(GROUP, "provider.subscribe");
/// Runs the provider CLI's own self-update at the workspace host.
pub const PROVIDER_UPDATE: Method = Method::new(GROUP, "provider.update");

/// Every method of the group.
pub const METHODS: &[Method] = &[PROVIDER_LIST, PROVIDER_REFRESH, PROVIDER_SELECT, AUTH_START, AUTH_RESPOND, AUTH_COMPLETE, AUTH_CANCEL, AUTH_SUBSCRIBE, PROVIDER_LOGOUT, USAGE_SUMMARY, RESET_CREDIT, PROVIDER_SUBSCRIBE, PROVIDER_UPDATE];

/// The `type` of the event of `provider.auth.subscribe`: the session's whole state after a change.
pub const AUTH_UPDATED: &str = "provider.auth.updated";

/// The `type` of the event of `provider.subscribe`: the whole provider list after a change.
pub const PROVIDERS_UPDATED: &str = "provider.updated";

/// The codes of this group's refusals.
pub mod code {
    pub const UNKNOWN_PROVIDER: &str = "UNKNOWN_PROVIDER";
    pub const UNKNOWN_AUTH: &str = "UNKNOWN_AUTH";
    pub const UNSUPPORTED_METHOD: &str = "UNSUPPORTED_METHOD";
    pub const AUTH_IN_PROGRESS: &str = "AUTH_IN_PROGRESS";
    pub const AUTH_ENDED: &str = "AUTH_ENDED";
    pub const SIGNED_OUT: &str = "SIGNED_OUT";
    pub const LOGIN_UNAVAILABLE: &str = "LOGIN_UNAVAILABLE";
    pub const NOT_SUPPORTED: &str = "NOT_SUPPORTED";
    pub const MODEL_API_UNAVAILABLE: &str = "MODEL_API_UNAVAILABLE";
    /// The workspace host could not ask the provider's account (no executor, or the executor or the account refused).
    pub const ACCOUNT_UNAVAILABLE: &str = "ACCOUNT_UNAVAILABLE";
    pub const DIRECTORY_UNAVAILABLE: &str = "DIRECTORY_UNAVAILABLE";
}

/// How far a provider's login stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    SignedOut,
    SignedIn,
    /// A login session of the provider is running.
    Authenticating,
}

/// How an authentication session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    Pending,
    Succeeded,
    Failed,
    Cancelled,
}

impl AuthStatus {
    pub fn ended(self) -> bool {
        self != AuthStatus::Pending
    }
}

/// One provider as `provider.list` shows it: its status and whether it is the one a new turn uses by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderView {
    pub provider: String,
    pub status: ProviderStatus,
    pub selected: bool,
    /// The login methods the provider's CLI offers, the first being the default.
    pub auth_methods: Vec<String>,
}

pub fn list_result(providers: &[ProviderView]) -> Value {
    json!({ "providers": providers, "selected": providers.iter().find(|p| p.selected).map(|p| p.provider.clone()) })
}

/// `provider.refresh` and `provider.logout` and `provider.select` name a provider; `provider.refresh` may name none (every provider).
pub fn provider_param(params: &Value) -> Result<String, ParamError> {
    text(params, "provider").map(str::to_string).ok_or_else(|| ParamError("provider is required".into()))
}

pub fn optional_provider(params: &Value) -> Result<Option<String>, ParamError> {
    match params.get("provider") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => provider_param(params).map(Some),
    }
}

/// The session a `provider.auth.*` request names.
pub fn auth_param(params: &Value) -> Result<String, ParamError> {
    text(params, "auth").map(str::to_string).ok_or_else(|| ParamError("auth is required".into()))
}

/// The params of `provider.auth.start`: the provider and, when the caller cares, the method (`device_code` or `paste`).
#[derive(Debug, Clone, PartialEq)]
pub struct StartParams {
    pub provider: String,
    pub method: Option<String>,
}

impl StartParams {
    pub fn parse(params: &Value) -> Result<StartParams, ParamError> {
        let provider = provider_param(params)?;
        let method = match params.get("method") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_str().filter(|m| !m.is_empty()).ok_or_else(|| ParamError("method must be a non-empty string".into()))?.to_string()),
        };
        Ok(StartParams { provider, method })
    }
}

/// The params of `provider.auth.respond`: the session and the text the person pasted.
#[derive(Debug, Clone, PartialEq)]
pub struct RespondParams {
    pub auth: String,
    pub text: String,
}

impl RespondParams {
    pub fn parse(params: &Value) -> Result<RespondParams, ParamError> {
        let auth = auth_param(params)?;
        let text = text(params, "text").ok_or_else(|| ParamError("text is required".into()))?.to_string();
        Ok(RespondParams { auth, text })
    }
}

/// The longest `provider.auth.complete` waits, and how long it waits when the caller names no time.
pub const COMPLETE_MAX_MS: u64 = 120_000;
pub const COMPLETE_DEFAULT_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq)]
pub struct CompleteParams {
    pub auth: String,
    pub timeout_ms: u64,
}

impl CompleteParams {
    pub fn parse(params: &Value) -> Result<CompleteParams, ParamError> {
        let auth = auth_param(params)?;
        let timeout_ms = match params.get("timeout_ms") {
            None | Some(Value::Null) => COMPLETE_DEFAULT_MS,
            Some(v) => v.as_u64().ok_or_else(|| ParamError("timeout_ms must be a non-negative integer".into()))?.min(COMPLETE_MAX_MS),
        };
        Ok(CompleteParams { auth, timeout_ms })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubscribeParams {
    pub auth: String,
    pub after_seq: u64,
}

impl SubscribeParams {
    pub fn parse(params: &Value) -> Result<SubscribeParams, ParamError> {
        let auth = auth_param(params)?;
        let after_seq = match params.get("after_seq") {
            None | Some(Value::Null) => 0,
            Some(v) => v.as_u64().ok_or_else(|| ParamError("after_seq must be a non-negative integer".into()))?,
        };
        Ok(SubscribeParams { auth, after_seq })
    }
}

/// One authentication session as every answer and event shows it. It holds the prompt the CLI printed and the reason a login failed, and
/// nothing the CLI wrote to its directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthView {
    pub auth: String,
    pub provider: String,
    pub method: String,
    pub status: AuthStatus,
    /// The address the person opens.
    pub url: Option<String>,
    /// The code the person enters there.
    pub code: Option<String>,
    /// Why a login failed, from the CLI's own error output.
    pub error: Option<String>,
}

impl AuthView {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// The result of the requests that name a session (`start`, `respond`, `complete`, `cancel`).
pub fn auth_result(view: &AuthView) -> Value {
    json!({ "auth": view.to_value() })
}

/// The event of a session's change, numbered `seq` in the session's own count (from 1).
pub fn auth_event(seq: u64, view: &AuthView) -> Event {
    let mut data = Map::new();
    data.insert("auth".into(), view.to_value());
    Event { seq, thread: None, turn: None, kind: AUTH_UPDATED.to_string(), data }
}

/// The answer to `provider.auth.subscribe`: where the session's numbering stands; the events that follow are numbered after it.
pub fn subscribe_result(auth: &str, seq: u64) -> Value {
    json!({ "auth": auth, "topic": format!("provider.auth/{auth}"), "seq": seq })
}

/// `provider.subscribe` takes no params (null and `{}` are the same).
pub fn check_no_params(params: &Value) -> Result<(), ParamError> {
    match params {
        Value::Null => Ok(()),
        Value::Object(map) if map.is_empty() => Ok(()),
        _ => Err(ParamError("provider.subscribe takes no params".into())),
    }
}

/// The event of a change of the provider list, numbered `seq` in the subscription's own count (from 1): the whole list as `provider.list` shows it.
pub fn providers_event(seq: u64, providers: &[ProviderView]) -> Event {
    let Value::Object(data) = list_result(providers) else { unreachable!("a list result is an object") };
    Event { seq, thread: None, turn: None, kind: PROVIDERS_UPDATED.to_string(), data }
}

/// The answer to `provider.subscribe`: the topic and where the numbering stands (0: the first event is the list as it is now).
pub fn providers_subscribe_result() -> Value {
    json!({ "topic": "provider", "seq": 0 })
}

/// One rate-limit window of a provider's account: the share of it used, its length and when it resets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used_percent: f64,
    pub window_minutes: u64,
    pub resets_at_ms: Option<u64>,
}

/// The reset credits an account holds (Codex).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResetCredits {
    pub available: u64,
    pub total_earned: Option<u64>,
    pub next_expires_ms: Option<u64>,
}

/// What the provider's own account says of its limits, read at the workspace host under the login its CLI keeps there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub session: Option<Window>,
    pub weekly: Option<Window>,
    pub plan: Option<String>,
    pub credits: Option<ResetCredits>,
    pub updated_ms: u64,
}

/// One row of `usage.summary`: the provider, whether it holds a login, its limits when its account answered, and why it did not when it was
/// asked and could not answer. Both are null for a provider nothing reports on (opencode) and for one that is signed out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRow {
    pub provider: String,
    pub status: ProviderStatus,
    pub limits: Option<Limits>,
    pub limits_error: Option<String>,
}

pub fn usage_result(rows: &[UsageRow]) -> Value {
    json!({ "providers": rows })
}

/// How the account answered a spend of a reset credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}

impl ResetOutcome {
    /// The outcome a provider's answer names (`reset`, `nothing_to_reset`, `no_credit`, `already_redeemed`; the camel-case spellings too).
    pub fn parse(name: &str) -> Option<ResetOutcome> {
        match name {
            "reset" => Some(ResetOutcome::Reset),
            "nothing_to_reset" | "nothingToReset" => Some(ResetOutcome::NothingToReset),
            "no_credit" | "noCredit" => Some(ResetOutcome::NoCredit),
            "already_redeemed" | "alreadyRedeemed" => Some(ResetOutcome::AlreadyRedeemed),
            _ => None,
        }
    }
}

/// The params of `provider.resetCredit`: the provider and the key a retry shares, so one credit is never spent twice (the service makes one
/// when the caller names none).
#[derive(Debug, Clone, PartialEq)]
pub struct ResetParams {
    pub provider: String,
    pub idempotency_key: Option<String>,
}

impl ResetParams {
    pub fn parse(params: &Value) -> Result<ResetParams, ParamError> {
        let provider = provider_param(params)?;
        let idempotency_key = match params.get("idempotency_key") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_str().filter(|k| !k.is_empty() && k.len() <= 128).ok_or_else(|| ParamError("idempotency_key must be a non-empty string of at most 128 bytes".into()))?.to_string()),
        };
        Ok(ResetParams { provider, idempotency_key })
    }
}

pub fn reset_credit_result(provider: &str, outcome: ResetOutcome) -> Value {
    json!({ "provider": provider, "outcome": outcome })
}

/// What `provider.update` did: whether the CLI's update exited 0, its exit code, the tail of what it printed and the version the CLI reports after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Updated {
    pub ok: bool,
    pub exit_code: i32,
    pub output: String,
    pub version: Option<String>,
}

pub fn update_result(provider: &str, updated: &Updated) -> Value {
    json!({ "provider": provider, "update": updated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_params_missing_a_part_when_read_then_each_is_refused_with_what_is_required() {
        // Given requests with no provider, no session, an empty paste and a bad timeout
        // Then each is refused with what it needs
        assert_eq!(StartParams::parse(&json!({})).unwrap_err().0, "provider is required");
        assert_eq!(StartParams::parse(&json!({"provider": "codex", "method": 3})).unwrap_err().0, "method must be a non-empty string");
        assert_eq!(StartParams::parse(&json!({"provider": "codex"})).unwrap(), StartParams { provider: "codex".into(), method: None });
        assert_eq!(RespondParams::parse(&json!({"auth": "a"})).unwrap_err().0, "text is required");
        assert_eq!(RespondParams::parse(&json!({"text": "x"})).unwrap_err().0, "auth is required");
        assert_eq!(CompleteParams::parse(&json!({"auth": "a", "timeout_ms": -1})).unwrap_err().0, "timeout_ms must be a non-negative integer");
        assert_eq!(SubscribeParams::parse(&json!({"auth": "a", "after_seq": "1"})).unwrap_err().0, "after_seq must be a non-negative integer");
    }

    #[test]
    fn given_a_wait_longer_than_the_most_or_none_when_complete_is_read_then_the_wait_is_bounded_and_defaulted() {
        // Given a timeout above the bound, one below it and none
        // Then the bound caps it, a small one stands and none gets the default
        assert_eq!(CompleteParams::parse(&json!({"auth": "a", "timeout_ms": 10_000_000})).unwrap().timeout_ms, COMPLETE_MAX_MS);
        assert_eq!(CompleteParams::parse(&json!({"auth": "a", "timeout_ms": 5})).unwrap().timeout_ms, 5);
        assert_eq!(CompleteParams::parse(&json!({"auth": "a"})).unwrap().timeout_ms, COMPLETE_DEFAULT_MS);
        assert_eq!(optional_provider(&json!({})).unwrap(), None);
        assert_eq!(optional_provider(&json!({"provider": null})).unwrap(), None);
        assert_eq!(optional_provider(&json!({"provider": ""})).unwrap_err().0, "provider is required");
    }

    #[test]
    fn given_a_session_when_its_view_and_event_are_built_then_the_frames_name_the_state_and_carry_no_credential_field() {
        // Given a pending session with a prompt
        let view = AuthView { auth: "auth-1".into(), provider: "codex".into(), method: "device_code".into(), status: AuthStatus::Pending, url: Some("https://x.test/d".into()), code: Some("ABCD-1234".into()), error: None };
        // Then the event frame is numbered, typed and holds the whole session
        assert_eq!(auth_event(2, &view).to_value(), json!({"event": {"seq": 2, "type": "provider.auth.updated", "auth": {"auth": "auth-1", "provider": "codex", "method": "device_code", "status": "pending", "url": "https://x.test/d", "code": "ABCD-1234", "error": null}}}));
        // And the view has exactly the fields a mapper may see
        let value = view.to_value();
        let fields: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(fields, ["auth", "provider", "method", "status", "url", "code", "error"]);
        assert!(AuthStatus::Succeeded.ended() && AuthStatus::Failed.ended() && AuthStatus::Cancelled.ended() && !AuthStatus::Pending.ended());
        assert_eq!(subscribe_result("auth-1", 0), json!({"auth": "auth-1", "topic": "provider.auth/auth-1", "seq": 0}));
    }

    #[test]
    fn given_providers_when_the_list_result_is_built_then_it_names_the_selected_one_and_the_methods_are_the_contracts() {
        // Given two providers, the second selected
        let rows = [
            ProviderView { provider: "codex".into(), status: ProviderStatus::SignedOut, selected: false, auth_methods: vec!["device_code".into()] },
            ProviderView { provider: "claudeAgent".into(), status: ProviderStatus::SignedIn, selected: true, auth_methods: vec!["device_code".into(), "paste".into()] },
        ];
        // Then the result lists both and names the selected
        let result = list_result(&rows);
        assert_eq!(result["selected"], json!("claudeAgent"));
        assert_eq!(result["providers"][0]["status"], json!("signed_out"));
        assert_eq!(list_result(&[])["selected"], Value::Null);
        assert_eq!(METHODS.len(), 13);
        assert_eq!(check_no_params(&json!({"x": 1})).unwrap_err().0, "provider.subscribe takes no params");
        assert!(check_no_params(&Value::Null).is_ok() && check_no_params(&json!({})).is_ok());
        assert_eq!(providers_event(1, &rows).to_value()["event"]["selected"], json!("claudeAgent"));
        assert!(METHODS.iter().all(|m| m.group() == GROUP));
    }

    #[test]
    fn given_reset_params_and_outcomes_when_read_then_a_missing_key_is_allowed_a_bad_one_refused_and_both_spellings_of_an_outcome_are_known() {
        // Given params with a key, with none and with a number, and the spellings of the outcomes
        // Then the key is optional, a number is refused and each outcome is read from its snake and camel names
        assert_eq!(ResetParams::parse(&json!({"provider": "codex", "idempotency_key": "k"})).unwrap().idempotency_key.as_deref(), Some("k"));
        assert_eq!(ResetParams::parse(&json!({"provider": "codex"})).unwrap().idempotency_key, None);
        assert_eq!(ResetParams::parse(&json!({"provider": "codex", "idempotency_key": 3})).unwrap_err().0, "idempotency_key must be a non-empty string of at most 128 bytes");
        assert_eq!(ResetParams::parse(&json!({})).unwrap_err().0, "provider is required");
        assert_eq!(["reset", "nothing_to_reset", "nothingToReset", "no_credit", "noCredit", "already_redeemed", "alreadyRedeemed", "x"].map(ResetOutcome::parse), [Some(ResetOutcome::Reset), Some(ResetOutcome::NothingToReset), Some(ResetOutcome::NothingToReset), Some(ResetOutcome::NoCredit), Some(ResetOutcome::NoCredit), Some(ResetOutcome::AlreadyRedeemed), Some(ResetOutcome::AlreadyRedeemed), None]);
        assert_eq!(reset_credit_result("codex", ResetOutcome::NoCredit), json!({"provider": "codex", "outcome": "no_credit"}));
    }
}
