//! The gateway's tokens, in memory and as digests: a token is never kept, and the service starting again knows none, so a CLI that outlives a
//! restart is refused with a clear error and its next turn mints afresh.
use super::ambient::random_hex;
use rca_group_gateway::{Grant, Ids, TokenStore};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct MemoryGrants {
    grants: Mutex<HashMap<String, Grant>>,
}

impl TokenStore for MemoryGrants {
    fn put(&self, digest: String, grant: Grant) {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).insert(digest, grant);
    }

    fn get(&self, digest: &str) -> Option<Grant> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).get(digest).cloned()
    }

    fn remove_thread(&self, thread: &str) {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, grant| grant.thread != thread);
    }
}

/// 244 bits from the system's randomness.
pub struct RandomTokens;

impl Ids for RandomTokens {
    fn token(&self) -> String {
        format!("{}{}", random_hex(16), random_hex(16))
    }
}
