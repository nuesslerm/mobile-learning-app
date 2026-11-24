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
    pub(super) fn environment_refusal(&self, route: &'static str, error: EnvironmentError) -> Refusal {
        let trace_id = opaque_secret(&self.0.ids.random_bytes(16));
        self.log("orchestration.http.refused", &[("route", route.to_string()), ("status", error.status().to_string()), ("traceId", trace_id.clone())]);
        Refusal { status: error.status(), body: error.body(&trace_id) }
    }

    pub(super) fn environment_gate(&self, route: &'static str, request: &EnvironmentRequest) -> Result<(), Refusal> {
        match request.protocol {
            Some("2") => {}
            None => {
                self.log("orchestration.http.refused", &[("route", route.to_string()), ("status", "400".into())]);
                return Err(Refusal::bare(400));
            }
            Some(_) => {
                self.log("orchestration.http.refused", &[("route", route.to_string()), ("status", "426".into())]);
                return Err(Refusal { status: 426, body: wire::protocol_incompatible() });
            }
        }
        self.read_gate(route, request.authorization)
    }

    /// The bearer (401) and `orchestration:read` (403) of a read route, which the protocol header does not gate.
    pub(super) fn read_gate(&self, route: &'static str, authorization: Option<&str>) -> Result<(), Refusal> {
        self.scope_gate(route, authorization, READ_SCOPE)
    }

    /// The bearer (401) and the one `scope` (403) a route asks of it.
    pub(super) fn scope_gate(&self, route: &'static str, authorization: Option<&str>, scope: &'static str) -> Result<(), Refusal> {
        if !authorization.is_some_and(|h| h.starts_with("Bearer ")) {
            return Err(self.environment_refusal(route, EnvironmentError::CredentialMissing));
        }
        let principal = match self.authenticate(authorization) {
            Ok(principal) => principal,
            Err(refused) if refused.status == 401 => return Err(self.environment_refusal(route, EnvironmentError::CredentialInvalid)),
            Err(refused) => return Err(refused),
        };
        if !principal.scopes.iter().any(|s| s == scope) {
            return Err(self.environment_refusal(route, EnvironmentError::ScopeRequired(scope)));
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
