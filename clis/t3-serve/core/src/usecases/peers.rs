//! `peerLinks.*`: links from this environment to other T3 environments. t3 serve holds none and makes none (a link needs this server to sign in
//! at a peer's `/mcp`, which rca's agents never do), so the list is empty, an unlink removes nothing, a link is refused as disabled and no agent
//! ever asks for one, so no request exists to answer.
use super::queries::QueryFailure;
use super::service::Service;
use serde_json::{json, Value};
use t3_contracts::wire;

fn text<'a>(payload: &'a Value, key: &str) -> Result<&'a str, QueryFailure> {
    payload.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| QueryFailure::Defect(format!("Expected a non-empty string at [\"{key}\"]")))
}

impl Service {
    pub fn peer_links_list(&self) -> Value {
        json!({"links": []})
    }

    pub fn peer_links_unlink(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let environment = text(payload, "environmentId")?;
        self.log("peer.unlink.none", &[("environment", environment.to_string())]);
        Ok(json!({"removed": false}))
    }

    pub fn peer_links_link(&self, payload: &Value) -> QueryFailure {
        for key in ["url", "pairingCode"] {
            if let Err(defect) = text(payload, key) {
                return defect;
            }
        }
        if payload["access"].is_null() {
            return QueryFailure::Defect("Expected the access to link with at [\"access\"]".into());
        }
        self.log("peer.link.disabled", &[]);
        QueryFailure::Error(json!({"_tag": "PeerLinkError", "reason": "disabled", "message": "Linked environments is off on this machine. Turn it on in Settings → Labs."}))
    }

    pub fn peer_links_answer(&self, payload: &Value) -> QueryFailure {
        for key in ["threadId", "turnItemId"] {
            if let Err(defect) = text(payload, key) {
                return defect;
            }
        }
        if !matches!(payload["answer"]["type"].as_str(), Some("link" | "use-existing" | "decline")) {
            return QueryFailure::Defect("Expected the answer to be link, use-existing or decline at [\"answer\"]".into());
        }
        let thread = payload["threadId"].as_str().unwrap_or_default();
        self.log("peer.answer.unknown", &[("thread", thread.to_string())]);
        if !self.thread_is_held(thread) {
            return QueryFailure::Error(wire::thread_missing_error("PeerLinkRequestError", thread, json!({"reason": "load_failed"})));
        }
        QueryFailure::Error(json!({"_tag": "PeerLinkRequestError", "reason": "not_found", "message": "This link request no longer exists."}))
    }
}
