//! The orchestration HTTP routes a phone reads a thread's past through: `GET /api/orchestration/threads/:threadId/history` and
//! `.../transcript`. Each passes the protocol header (426), the bearer (401) and `orchestration:read` (403) before it reads.
use super::service::{Refusal, Service};
use crate::domain::auth::opaque_secret;
use crate::domain::history::{self, HistoryError};
use serde_json::Value;
use t3_contracts::environment::EnvironmentError;
use t3_contracts::wire;

const READ_SCOPE: &str = "orchestration:read";

/// What the routes read of the request besides the path.
pub struct EnvironmentRequest<'a> {
    pub authorization: Option<&'a str>,
    pub protocol: Option<&'a str>,
}

impl Service {
    fn environment_refusal(&self, route: &'static str, error: EnvironmentError) -> Refusal {
        let trace_id = opaque_secret(&self.0.ids.random_bytes(16));
        self.log("orchestration.http.refused", &[("route", route.to_string()), ("status", error.status().to_string()), ("traceId", trace_id.clone())]);
        Refusal { status: error.status(), body: error.body(&trace_id) }
    }

    pub(super) fn environment_gate(&self, route: &'static str, request: &EnvironmentRequest) -> Result<(), Refusal> {
        if request.protocol != Some("2") {
            self.log("orchestration.http.refused", &[("route", route.to_string()), ("status", "426".into())]);
            return Err(Refusal { status: 426, body: wire::protocol_incompatible() });
        }
        if !request.authorization.is_some_and(|h| h.starts_with("Bearer ")) {
            return Err(self.environment_refusal(route, EnvironmentError::CredentialMissing));
        }
        let principal = match self.authenticate(request.authorization) {
            Ok(principal) => principal,
            Err(refused) if refused.status == 401 => return Err(self.environment_refusal(route, EnvironmentError::CredentialInvalid)),
            Err(refused) => return Err(refused),
        };
        if !principal.scopes.iter().any(|s| s == READ_SCOPE) {
            return Err(self.environment_refusal(route, EnvironmentError::ScopeRequired(READ_SCOPE)));
        }
        Ok(())
    }

    pub(super) fn history_refusal(&self, route: &'static str, error: HistoryError) -> Refusal {
        self.environment_refusal(
            route,
            match error {
                HistoryError::ThreadNotFound => EnvironmentError::ThreadNotFound,
                HistoryError::InvalidCursor => EnvironmentError::InvalidHistoryCursor,
                HistoryError::TranscriptTooLarge => EnvironmentError::TranscriptTooLarge,
            },
        )
    }

    /// `query` is the decoded query string; `cursor` is required, `view=conversation` keeps the conversation rows only.
    pub fn thread_history_page(&self, request: &EnvironmentRequest, thread: &str, query: &[(String, String)]) -> Result<Value, Refusal> {
        const ROUTE: &str = "threadHistoryPage";
        self.environment_gate(ROUTE, request)?;
        let param = |name: &str| query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let conversation_only = param("view") == Some("conversation");
        self.sync_thread(thread);
        self.engine().read(|state| history::page(state, thread, param("cursor").unwrap_or(""), conversation_only)).map_err(|e| self.history_refusal(ROUTE, e))
    }

    pub fn thread_transcript(&self, request: &EnvironmentRequest, thread: &str) -> Result<Value, Refusal> {
        const ROUTE: &str = "threadTranscript";
        self.environment_gate(ROUTE, request)?;
        self.sync_thread(thread);
        self.engine().read(|state| history::transcript(state, thread, &self.env().id)).map_err(|e| self.history_refusal(ROUTE, e))
    }
}
