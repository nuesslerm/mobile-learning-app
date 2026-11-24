//! `service.logs`: the last events the service's own code wrote (`rca_service_core::application::log`), newest last. It is the service's own log and
//! nothing else: no executor, provider or client output is in it, and no event carries a credential (the module that writes them never takes one).
//! Times are milliseconds since the epoch.
use agent_api_proto::ParamError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The most lines one read returns, and how many it returns when the caller names none.
pub const LIMIT_MAX: usize = 1000;
pub const LIMIT_DEFAULT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn parse(name: &str) -> Option<Level> {
        match name {
            "info" => Some(Level::Info),
            "warn" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

/// One event: its time, level, name and the fields it carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub at_ms: u64,
    pub level: Level,
    pub event: String,
    pub meta: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogsParams {
    pub limit: usize,
    pub min_level: Level,
}

impl LogsParams {
    pub fn parse(params: &Value) -> Result<LogsParams, ParamError> {
        let limit = match params.get("limit") {
            None | Some(Value::Null) => LIMIT_DEFAULT,
            Some(v) => v.as_u64().filter(|n| (1..=LIMIT_MAX as u64).contains(n)).ok_or_else(|| ParamError(format!("limit must be an integer from 1 to {LIMIT_MAX}")))? as usize,
        };
        let min_level = match params.get("min_level") {
            None | Some(Value::Null) => Level::Info,
            Some(v) => v.as_str().and_then(Level::parse).ok_or_else(|| ParamError("min_level must be info, warn or error".into()))?,
        };
        Ok(LogsParams { limit, min_level })
    }
}

/// The answer: the lines (the last `limit` at or above the level) and how many events the service keeps in all, so a caller sees what it missed.
pub fn logs_result(lines: &[LogLine], kept: usize) -> Value {
    json!({ "lines": lines, "kept": kept })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_log_params_when_read_then_the_limit_is_held_to_its_range_the_level_defaults_to_info_and_a_bad_one_is_refused() {
        // Given none, a limit and level, and values out of range
        // Then the defaults apply, a good pair stands and each bad one is refused with what is allowed
        assert_eq!(LogsParams::parse(&json!({})).unwrap(), LogsParams { limit: LIMIT_DEFAULT, min_level: Level::Info });
        assert_eq!(LogsParams::parse(&json!({"limit": 5, "min_level": "warn"})).unwrap(), LogsParams { limit: 5, min_level: Level::Warn });
        assert_eq!(LogsParams::parse(&json!({"limit": 0})).unwrap_err().0, "limit must be an integer from 1 to 1000");
        assert_eq!(LogsParams::parse(&json!({"limit": 1001})).unwrap_err().0, "limit must be an integer from 1 to 1000");
        assert_eq!(LogsParams::parse(&json!({"min_level": "debug"})).unwrap_err().0, "min_level must be info, warn or error");
        assert!(Level::Info < Level::Warn && Level::Warn < Level::Error);
    }

    #[test]
    fn given_lines_when_the_result_is_built_then_each_has_its_fields_in_order_and_the_kept_count_is_named() {
        // Given one line
        let line = LogLine { at_ms: 7, level: Level::Warn, event: "x.failed".into(), meta: json!({"why": "no"}) };
        // Then the result carries it with the count of events the service keeps
        assert_eq!(logs_result(&[line], 9).to_string(), r#"{"lines":[{"at_ms":7,"level":"warn","event":"x.failed","meta":{"why":"no"}}],"kept":9}"#);
    }
}
