//! `e2ee_auth`: the first sealed message of a socket. The one parser accepts the token shapes and nothing mixed; device-key proofs
//! need an ed25519 verifier this host does not carry, so they are refused as `bad_auth`, the reference's answer for a host without keys.
use crate::ports::{Device, DeviceRegistry};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthFailure {
    BadAuth,
    Unauthorized,
}

impl AuthFailure {
    pub fn code(self) -> &'static str {
        match self {
            Self::BadAuth => "bad_auth",
            Self::Unauthorized => "unauthorized",
        }
    }

    /// The error sealed back to the phone before the socket closes.
    pub fn sealed_error(self) -> Value {
        json!({"type": "e2ee_error", "error": {"code": self.code()}})
    }

    pub fn close_reason(self) -> &'static str {
        match self {
            Self::BadAuth => "Invalid e2ee_auth",
            Self::Unauthorized => "Unauthorized",
        }
    }
}

pub struct Authenticated {
    pub device: Device,
    pub client_capabilities: Option<Value>,
}

/// `transcript_hash_b64` is the v2 session's, or None on legacy framing.
pub fn authenticate(plaintext: &str, transcript_hash_b64: Option<&str>, registry: &dyn DeviceRegistry) -> Result<Authenticated, AuthFailure> {
    let (token, client_capabilities) = classify(plaintext, transcript_hash_b64).ok_or(AuthFailure::BadAuth)?;
    let device = registry.find_by_token(&token).ok_or(AuthFailure::Unauthorized)?;
    if !bool::from(device.token.as_bytes().ct_eq(token.as_bytes())) {
        return Err(AuthFailure::Unauthorized);
    }
    Ok(Authenticated { device, client_capabilities })
}

fn classify(plaintext: &str, transcript_hash_b64: Option<&str>) -> Option<(String, Option<Value>)> {
    let value: Value = serde_json::from_str(plaintext).ok()?;
    let auth = value.as_object()?;
    if auth.get("type")? != "e2ee_auth" {
        return None;
    }
    let token = auth.get("deviceToken").and_then(Value::as_str).filter(|t| !t.is_empty())?;
    match transcript_hash_b64 {
        None => {
            let extra = ["v", "transcriptHashB64", "deviceId", "deviceProofB64"];
            (!extra.iter().any(|k| auth.contains_key(*k))).then(|| (token.to_string(), auth.get("clientCapabilities").cloned()))
        }
        Some(hash) => {
            // v2 token auth is an exact key set: capabilities ride only on key auth.
            let exact = auth.len() == 4 && auth.get("v")? == 2 && auth.get("transcriptHashB64")?.as_str() == Some(hash);
            exact.then(|| (token.to_string(), None))
        }
    }
}
