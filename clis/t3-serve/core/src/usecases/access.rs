//! The routes that take access back or refuse a browser: `POST /api/auth/browser-session`, `clients/revoke`, `clients/revoke-others`
//! and `pairing-links/revoke`. The last three need `access:write`; a body the schema refuses is a bare 400 as the pin answers it.
use super::service::{refuse, Refusal, Service};
use crate::domain::auth::opaque_secret;
use serde_json::{json, Value};
use t3_contracts::wire;

impl Service {
    /// This server holds no browser session to open: the credential a browser brings is never one it issued.
    pub fn browser_session(&self, body: &Value) -> Result<Value, Refusal> {
        if !body.get("credential").is_some_and(Value::is_string) {
            return Err(Refusal::bare(400));
        }
        self.log("browser.session.refused", &[]);
        Err(refuse(401, wire::auth_invalid(&opaque_secret(&self.0.ids.random_bytes(16)))))
    }

    pub fn revoke_client(&self, header: Option<&str>, body: &Value) -> Result<Value, Refusal> {
        self.access_writer(header)?;
        let id = body.get("sessionId").and_then(Value::as_str).ok_or_else(|| Refusal::bare(400))?;
        let revoked = self.0.auth.revoke_token(id).map_err(|e| self.store_failed("client.revoke.failed", e.0))?;
        self.log("client.revoked", &[("revoked", revoked.to_string())]);
        Ok(json!({"revoked": revoked}))
    }

    pub fn revoke_pairing_link(&self, header: Option<&str>, body: &Value) -> Result<Value, Refusal> {
        self.access_writer(header)?;
        let id = body.get("id").and_then(Value::as_str).ok_or_else(|| Refusal::bare(400))?;
        // The pin matches the whole id: a shorter string is no link's id, though it prefixes some hash
        let revoked = id.chars().count() == crate::domain::auth::ID_LENGTH && self.0.auth.revoke_grant(id).map_err(|e| self.store_failed("pairing.revoke.failed", e.0))?;
        self.log("pairing.link.revoked", &[("revoked", revoked.to_string())]);
        Ok(json!({"revoked": revoked}))
    }

    /// Every bearer but the caller's own.
    pub fn revoke_other_clients(&self, header: Option<&str>) -> Result<Value, Refusal> {
        let principal = self.access_writer(header)?;
        let count = self.0.auth.revoke_other_tokens(&principal.token_id, self.0.clock.now_ms()).map_err(|e| self.store_failed("clients.revoke_others.failed", e.0))?;
        self.log("clients.revoked_others", &[("count", count.to_string())]);
        Ok(json!({"revokedCount": count}))
    }
}
