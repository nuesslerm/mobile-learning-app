use super::{Failure, Result};

pub const INVITE_SECONDS: i64 = 300;

pub trait CredentialStore: Send + Sync {
    fn save_invite(&self, code: &str, ttl_seconds: i64) -> Result<()>;
    fn redeem_once(&self, code: &str, token: &str) -> Result<bool>;
    fn authenticated(&self, token: &str) -> Result<bool>;
    fn revoke(&self, token: &str) -> Result<()>;
}

pub trait SecretSource: Send + Sync {
    fn generate(&self) -> String;
}

pub struct Pairing<S, G> {
    pub store: S,
    pub secrets: G,
}
impl<S: CredentialStore, G: SecretSource> Pairing<S, G> {
    pub fn invite(&self) -> Result<String> {
        let code = self.secrets.generate();
        self.store.save_invite(&code, INVITE_SECONDS)?;
        Ok(code)
    }
}
impl<S: CredentialStore, G: SecretSource> super::Devices for Pairing<S, G> {
    fn exchange(&self, code: &str) -> Result<String> {
        let token = self.secrets.generate();
        if !self.store.redeem_once(code, &token)? {
            return Err(Failure::new(
                "UNAUTHORIZED",
                "Invalid or expired pairing code",
            ));
        }
        Ok(token)
    }
    fn authenticated(&self, token: &str) -> Result<bool> {
        self.store.authenticated(token)
    }
    fn revoke(&self, token: &str) -> Result<()> {
        self.store.revoke(token)
    }
}
