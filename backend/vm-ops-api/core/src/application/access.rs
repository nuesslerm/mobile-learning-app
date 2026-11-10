//! The gate every non-public route stands behind.
use crate::application::ports::Clock;
use crate::domain::auth::{self, AuthConfig, Credentials, RequestLine};
use std::sync::Arc;

pub struct Gate {
    config: AuthConfig,
    clock: Arc<dyn Clock>,
}

impl Gate {
    pub fn new(config: AuthConfig, clock: Arc<dyn Clock>) -> Self {
        Self { config, clock }
    }

    pub fn allows(&self, credentials: &Credentials, request: &RequestLine, body: &[u8]) -> bool {
        auth::is_authorized(&self.config, self.clock.now_ms(), credentials, request, body)
    }

    /// As `allows`, but the legacy raw token is refused: a drill takes the signed token alone.
    pub fn allows_signed(&self, credentials: &Credentials, request: &RequestLine, body: &[u8]) -> bool {
        auth::is_signed(&self.config, self.clock.now_ms(), credentials, request, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::derive_rotating_token;
    use crate::testing::FakeClock;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
    use ed25519_dalek::pkcs8::EncodePublicKey;
    use ed25519_dalek::{Signer, SigningKey};
    use secrecy::SecretString;
    use sha2::{Digest, Sha256};

    /// 2026-10-04T12:00:00Z
    const NOON: i64 = 1_791_115_200_000;
    const DAY: i64 = 86_400_000;

    fn gate(clock: &Arc<FakeClock>) -> Gate {
        let key = SigningKey::from_bytes(&[7; 32]);
        let pem = key.verifying_key().to_public_key_pem(LineEnding::LF).expect("the test key encodes");
        Gate::new(AuthConfig::new(SecretString::from("seed"), &pem, false), clock.clone())
    }

    /// Whether the gate lets in a GET of /health carrying the rotating token of `date`, signed at `signed_at`.
    fn allows_rotating(gate: &Gate, date: &str, signed_at: i64) -> bool {
        let timestamp = signed_at.to_string();
        let canonical = format!("GET\n/health\n{timestamp}\n{}", hex::encode(Sha256::digest(b"")));
        let signature = STANDARD.encode(SigningKey::from_bytes(&[7; 32]).sign(canonical.as_bytes()).to_bytes());
        let authorization = format!("Bearer {}", derive_rotating_token("seed", date));
        let credentials = Credentials { authorization: Some(&authorization), signature: Some(&signature), timestamp: Some(&timestamp) };
        gate.allows(&credentials, &RequestLine { method: "GET", path_and_query: "/health" }, b"")
    }

    #[test]
    fn given_a_clock_when_the_gate_reads_a_rotating_token_then_it_judges_the_token_by_the_clocks_day() {
        // Given a gate whose clock says noon on 4 Oct
        let clock = Arc::new(FakeClock::at(NOON));
        let gate = gate(&clock);
        // When the token of 4 Oct and of 3 Oct arrive, signed at noon, Then both pass
        assert!(allows_rotating(&gate, "2026-10-04", NOON));
        assert!(allows_rotating(&gate, "2026-10-03", NOON));
        // When the clock moves a day on, Then the token of 3 Oct is no longer in its grace
        clock.set(NOON + DAY);
        assert!(allows_rotating(&gate, "2026-10-04", NOON + DAY));
        assert!(!allows_rotating(&gate, "2026-10-03", NOON + DAY));
    }

    #[test]
    fn given_the_legacy_seed_when_the_gate_is_asked_then_it_lets_it_in() {
        let clock = Arc::new(FakeClock::at(NOON));
        let gate = gate(&clock);
        let allows = |authorization: &str| {
            let credentials = Credentials { authorization: Some(authorization), ..Credentials::default() };
            gate.allows(&credentials, &RequestLine { method: "GET", path_and_query: "/health" }, b"")
        };
        assert!(allows("Bearer seed"));
        assert!(!allows("Bearer other"));
    }

    #[test]
    fn given_a_signed_only_route_when_the_gate_is_asked_then_the_legacy_seed_and_an_unsigned_rotating_token_are_refused() {
        let clock = Arc::new(FakeClock::at(NOON));
        let gate = gate(&clock);
        let token = format!("Bearer {}", derive_rotating_token("seed", "2026-10-04"));
        let line = RequestLine { method: "GET", path_and_query: "/health" };
        let ask = |authorization: &str, signature: Option<&str>, timestamp: Option<&str>| {
            gate.allows_signed(&Credentials { authorization: Some(authorization), signature, timestamp }, &line, b"")
        };
        let timestamp = NOON.to_string();
        let canonical = format!("GET\n/health\n{timestamp}\n{}", hex::encode(Sha256::digest(b"")));
        let signature = STANDARD.encode(SigningKey::from_bytes(&[7; 32]).sign(canonical.as_bytes()).to_bytes());
        assert!(!ask("Bearer seed", None, None));
        assert!(!ask(&token, None, None));
        assert!(ask(&token, Some(&signature), Some(&timestamp)));
    }
}
