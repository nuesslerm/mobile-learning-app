//! Pairing codes: minted by `wf t3 pair`, redeemed once at `/oauth/token`. Only the hash of a code is stored.
use crate::domain::auth::{hash_secret, pairing_code, validate_scopes};
use crate::ports::{AuthStore, Clock, GrantRecord, Ids, Millis, StoreError};
use t3_contracts::scopes::DEFAULT_PAIRING_SCOPES;
use t3_contracts::wire::PAIRING_TTL_SECONDS;

pub struct PairingRequest {
    pub scopes: Vec<String>,
    pub label: String,
    pub ttl_seconds: i64,
}

impl Default for PairingRequest {
    fn default() -> Self {
        PairingRequest { scopes: DEFAULT_PAIRING_SCOPES.iter().map(|s| s.to_string()).collect(), label: "wf t3 pair".into(), ttl_seconds: PAIRING_TTL_SECONDS }
    }
}

pub struct Pairing {
    pub code: String,
    pub expires_at: Millis,
    pub scopes: Vec<String>,
    pub label: String,
}

pub fn mint(auth: &dyn AuthStore, clock: &dyn Clock, ids: &dyn Ids, request: PairingRequest) -> Result<Pairing, String> {
    if request.ttl_seconds <= 0 {
        return Err("the ttl is a positive number of seconds".into());
    }
    let scopes = validate_scopes(&request.scopes)?;
    let code = pairing_code(&ids.random_bytes(32));
    let expires_at = clock.now_ms() + request.ttl_seconds * 1000;
    let grant = GrantRecord { scopes: scopes.clone(), label: request.label.clone() };
    auth.put_grant(&hash_secret(&code), &grant, expires_at).map_err(|StoreError(m)| m)?;
    Ok(Pairing { code, expires_at, scopes, label: request.label })
}
