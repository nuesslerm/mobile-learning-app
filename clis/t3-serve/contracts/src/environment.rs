//! The error bodies of the environment HTTP API (`environmentHttp.ts` of the pin): a tagged error with a fixed `code`, a `reason`
//! from a closed set and a trace id, sent with the status the error class declares.
use serde_json::{json, Value};

/// The header every orchestration HTTP route requires, carrying the protocol version as text.
pub const ORCHESTRATION_PROTOCOL_HEADER: &str = "x-t3-orchestration-protocol";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvironmentError {
    /// No `Authorization: Bearer` credential at all.
    CredentialMissing,
    /// A bearer the server does not know, or one that expired.
    CredentialInvalid,
    /// The session lacks this permission.
    ScopeRequired(&'static str),
    InvalidHistoryCursor,
    TranscriptTooLarge,
    ThreadNotFound,
}

impl EnvironmentError {
    pub fn status(self) -> u16 {
        match self {
            EnvironmentError::CredentialMissing | EnvironmentError::CredentialInvalid => 401,
            EnvironmentError::ScopeRequired(_) => 403,
            EnvironmentError::InvalidHistoryCursor | EnvironmentError::TranscriptTooLarge => 400,
            EnvironmentError::ThreadNotFound => 404,
        }
    }

    pub fn body(self, trace_id: &str) -> Value {
        match self {
            EnvironmentError::CredentialMissing => json!({"_tag": "EnvironmentAuthInvalidError", "code": "auth_invalid", "reason": "missing_credential", "traceId": trace_id}),
            EnvironmentError::CredentialInvalid => json!({"_tag": "EnvironmentAuthInvalidError", "code": "auth_invalid", "reason": "invalid_credential", "traceId": trace_id}),
            EnvironmentError::ScopeRequired(permission) => json!({
                "_tag": "EnvironmentScopeRequiredError",
                "code": "insufficient_scope",
                "requiredScope": crate::scopes::legacy_parent(permission),
                "requiredPermission": permission,
                "traceId": trace_id,
            }),
            EnvironmentError::InvalidHistoryCursor => json!({"_tag": "EnvironmentRequestInvalidError", "code": "invalid_request", "reason": "invalid_history_cursor", "traceId": trace_id}),
            EnvironmentError::TranscriptTooLarge => json!({"_tag": "EnvironmentRequestInvalidError", "code": "invalid_request", "reason": "thread_transcript_too_large", "traceId": trace_id}),
            EnvironmentError::ThreadNotFound => json!({"_tag": "EnvironmentResourceNotFoundError", "code": "not_found", "reason": "thread_not_found", "traceId": trace_id}),
        }
    }
}

