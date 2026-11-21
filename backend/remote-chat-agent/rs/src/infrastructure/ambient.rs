//! What the composition root draws from the machine for the ports behind it: random ids and the wall clock. The service core asks its `Clock`
//! and `IdSource` ports and never these.

use std::time::{SystemTime, UNIX_EPOCH};

/// A random (version 4) UUID, as `crypto.randomUUID`.
pub fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `n` random bytes (at most 16) as lowercase hex.
pub fn random_hex(n: usize) -> String {
    let mut hex = uuid::Uuid::new_v4().simple().to_string();
    hex.truncate(2 * n.min(16));
    hex
}

/// Milliseconds since the Unix epoch, from the system clock.
pub fn wall_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
