//! Session ids: `<agent>_<token>`, the lane the phone's `isStructuredAgentSessionIdFor` checks (a token of letters, digits and underscores).
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn is_session_id_for(agent: &str, session: &str) -> bool {
    session.strip_prefix(agent).and_then(|r| r.strip_prefix('_')).is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

/// Any agent's session id: `<agent>_<token>`.
pub(crate) fn is_session_id(session: &str) -> bool {
    session.split_once('_').is_some_and(|(agent, _)| !agent.is_empty() && agent.bytes().all(|b| b.is_ascii_alphanumeric()) && is_session_id_for(agent, session))
}

fn parts() -> (u32, u16, u16, u16, u64) {
    let bytes = os_random().unwrap_or_else(weak_random);
    let u = u128::from_be_bytes(bytes);
    ((u >> 96) as u32, (u >> 80) as u16, (u >> 64) as u16, (u >> 48) as u16, (u & 0xffff_ffff_ffff) as u64)
}

fn os_random() -> Option<[u8; 16]> {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom").ok()?.read_exact(&mut b).ok()?;
    Some(b)
}

// Only reached where the OS source is unreadable; ids still differ, they are just not unguessable there.
fn weak_random() -> [u8; 16] {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let word = |salt: u64| {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(n ^ salt);
        h.finish()
    };
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(&word(1).to_be_bytes());
    b[8..].copy_from_slice(&word(2).to_be_bytes());
    b
}

/// A fresh id for `agent`: 128 bits from the OS random source, as the host's `uuid` with underscores for hyphens.
pub(crate) fn mint(agent: &str) -> String {
    let (a, b, c, d, e) = parts();
    format!("{agent}_{a:08x}_{b:04x}_{c:04x}_{d:04x}_{e:012x}")
}

/// A fresh upload id in the host's store shape: lowercase hex groups of 8-4-4-4-12.
pub(crate) fn mint_upload() -> String {
    let (a, b, c, d, e) = parts();
    format!("{a:08x}-{b:04x}-{c:04x}-{d:04x}-{e:012x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_minted_ids_when_checked_then_each_is_named_for_its_agent_only_and_no_two_are_alike() {
        // Given a few ids minted for claude
        let ids: Vec<String> = (0..50).map(|_| mint("claude")).collect();
        // Then each is of claude's lane and of no other agent's, and all differ
        assert!(ids.iter().all(|i| is_session_id_for("claude", i) && !is_session_id_for("codex", i)));
        assert_eq!(ids.iter().collect::<std::collections::BTreeSet<_>>().len(), ids.len());
        // And a hyphen or an empty token is not a session id
        assert!(!is_session_id_for("claude", "claude_a-b") && !is_session_id_for("claude", "claude_"));
    }
}
