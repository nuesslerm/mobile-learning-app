//! The `service.activity` sub-group: `service.clientActivity`, a lease a client renews while a person is using it (visible, focused, recently
//! touched) and for which scopes, so the service can tell whether anyone is looking before it does optional work. Neutral of any app: snake_case
//! fields, times in milliseconds.
use agent_api_proto::{GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const GROUP: GroupName = GroupName::new("service.activity");

pub const CLIENT_ACTIVITY: Method = Method::new(GROUP, "service.clientActivity");

/// Every method of the group.
pub const METHODS: &[Method] = &[CLIENT_ACTIVITY];

pub const DEFAULT_TTL_MS: u64 = 45_000;
pub const MIN_TTL_MS: u64 = 1_000;
pub const MAX_TTL_MS: u64 = 120_000;
pub const MAX_CLIENT_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    Web,
    DesktopRenderer,
    Mobile,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    Active,
    Inactive,
    Background,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tri {
    True,
    False,
    Unknown,
}

/// What a client reports. `scopes` name what it is looking at (`server-config`, `thread:<id>`, ...), opaque to the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientActivityParams {
    pub client: String,
    pub kind: ClientKind,
    pub visible: bool,
    pub focused: bool,
    pub recently_interacted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_state: Option<AppState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_power_mode: Option<Tri>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    pub observed_at_ms: u64,
}

impl ClientActivityParams {
    pub fn parse(params: &Value) -> Result<ClientActivityParams, ParamError> {
        let parsed: ClientActivityParams = serde_json::from_value(params.clone()).map_err(|e| ParamError(e.to_string()))?;
        if parsed.client.trim().is_empty() || parsed.client.chars().count() > MAX_CLIENT_LEN {
            return Err(ParamError(format!("client is a name of 1 to {MAX_CLIENT_LEN} characters")));
        }
        Ok(parsed)
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// How long the lease lasts: the asked time inside the limits, or the default.
    pub fn ttl_ms(&self) -> u64 {
        self.ttl_ms.unwrap_or(DEFAULT_TTL_MS).clamp(MIN_TTL_MS, MAX_TTL_MS)
    }
}

/// The result: when the lease the report made ends.
pub fn result(expires_at_ms: u64) -> Value {
    json!({ "expires_at_ms": expires_at_ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Value {
        json!({"client": "phone-1", "kind": "mobile", "visible": true, "focused": true, "recently_interacted": false, "scopes": ["server-config"], "observed_at_ms": 5})
    }

    #[test]
    fn given_a_report_when_parsed_and_encoded_then_it_reads_back_and_a_ttl_outside_the_limits_is_clamped() {
        // Given a report with no ttl, and the same with a ttl below and above the limits
        let base = ClientActivityParams::parse(&report()).unwrap();
        let with = |ttl: u64| ClientActivityParams { ttl_ms: Some(ttl), ..base.clone() };
        // Then it reads back as sent, the default applies, and the limits hold
        assert_eq!(base.to_value()["kind"], json!("mobile"));
        assert_eq!(ClientActivityParams::parse(&base.to_value()).unwrap(), base);
        assert_eq!((base.ttl_ms(), with(1).ttl_ms(), with(10_000_000).ttl_ms(), with(60_000).ttl_ms()), (DEFAULT_TTL_MS, MIN_TTL_MS, MAX_TTL_MS, 60_000));
    }

    #[test]
    fn given_reports_that_break_the_contract_when_parsed_then_each_is_refused() {
        let broken = |patch: Value| {
            let mut v = report();
            v.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            ClientActivityParams::parse(&v).is_err()
        };
        // Given an empty client, a client over the limit, a kind the contract lacks, and a field it does not have
        // Then each is refused
        assert!(broken(json!({"client": " "})));
        assert!(broken(json!({"client": "x".repeat(MAX_CLIENT_LEN + 1)})));
        assert!(broken(json!({"kind": "tv"})));
        assert!(broken(json!({"extra": 1})));
        assert!(ClientActivityParams::parse(&json!({"client": "a"})).is_err());
    }
}
