//! The device bearers of the agent API: one secret per paired device or mapper, minted once by an operator who holds the service secret, shown
//! once, and kept only as a digest. The decisions (a token's digest, whether a presented token is one a record names, what a revoke removes)
//! are here; the store that keeps the records and the randomness a token is made of are the service's adapter's.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What every token starts with, so a leaked one is recognised for what it is.
pub const TOKEN_PREFIX: &str = "rcab_";

/// A device bearer as an operator sees it: never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bearer {
    pub id: String,
    pub label: String,
    pub created_at: String,
}

/// A bearer just minted, with the token it is known by from now on: shown once, because only its digest is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minted {
    pub bearer: Bearer,
    pub token: String,
}

/// What the service needs of its device bearers. The service secret mints and revokes (an operator); a token only opens the agent API socket.
pub trait DeviceBearers: Send + Sync {
    /// A new bearer for `label` and its token.
    fn mint(&self, label: &str) -> Result<Minted, String>;
    /// The bearer a presented token belongs to, when it belongs to one that was not revoked.
    fn verify(&self, token: &str) -> Option<Bearer>;
    /// Every bearer that was not revoked, oldest first.
    fn list(&self) -> Result<Vec<Bearer>, String>;
    /// Whether a bearer had the id (and no longer works).
    fn revoke(&self, id: &str) -> Result<bool, String>;
}

/// The digest a token is kept as: SHA-256, lowercase hex. A token is 244 bits of randomness, so a digest needs no salt.
pub fn token_digest(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The token an `Authorization` header value carries: the `Bearer` scheme and something after it, nothing else.
pub fn bearer_token(authorization: Option<&str>) -> Option<&str> {
    authorization.and_then(|value| value.strip_prefix("Bearer ")).map(str::trim).filter(|token| !token.is_empty())
}

/// What is kept of one bearer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    #[serde(flatten)]
    pub bearer: Bearer,
    pub digest: String,
}

/// The records of every bearer, and the rules over them. An adapter loads it, calls one of these, and saves it back when it changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub bearers: Vec<Record>,
}

/// Compares without stopping at the first difference, so how long it took says nothing about how much of a digest matched.
fn same(left: &str, right: &str) -> bool {
    left.len() == right.len() && left.bytes().zip(right.bytes()).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

impl Book {
    /// Adds the bearer `id` for `token`, keeping the digest of the token and not the token.
    pub fn add(&mut self, bearer: Bearer, token: &str) {
        self.bearers.push(Record { bearer, digest: token_digest(token) });
    }

    pub fn verify(&self, token: &str) -> Option<Bearer> {
        let digest = token_digest(token);
        self.bearers.iter().fold(None, |found, record| if same(&record.digest, &digest) { Some(record.bearer.clone()) } else { found })
    }

    pub fn list(&self) -> Vec<Bearer> {
        self.bearers.iter().map(|record| record.bearer.clone()).collect()
    }

    /// Removes the bearer with the id; whether there was one.
    pub fn revoke(&mut self, id: &str) -> bool {
        let before = self.bearers.len();
        self.bearers.retain(|record| record.bearer.id != id);
        self.bearers.len() != before
    }
}

#[cfg(feature = "testing")]
pub use testing::MemoryBearers;

#[cfg(feature = "testing")]
mod testing {
    use super::*;
    use std::sync::Mutex;

    /// Device bearers kept in memory, with tokens and ids that count: `rcab_test_1`, `bearer-1`.
    #[derive(Default)]
    pub struct MemoryBearers {
        book: Mutex<(Book, u64)>,
    }

    impl DeviceBearers for MemoryBearers {
        fn mint(&self, label: &str) -> Result<Minted, String> {
            let mut held = self.book.lock().unwrap_or_else(|p| p.into_inner());
            held.1 += 1;
            let (token, bearer) = (format!("{TOKEN_PREFIX}test_{}", held.1), Bearer { id: format!("bearer-{}", held.1), label: label.to_string(), created_at: "2026-11-04T10:00:00Z".into() });
            held.0.add(bearer.clone(), &token);
            Ok(Minted { bearer, token })
        }

        fn verify(&self, token: &str) -> Option<Bearer> {
            self.book.lock().unwrap_or_else(|p| p.into_inner()).0.verify(token)
        }

        fn list(&self) -> Result<Vec<Bearer>, String> {
            Ok(self.book.lock().unwrap_or_else(|p| p.into_inner()).0.list())
        }

        fn revoke(&self, id: &str) -> Result<bool, String> {
            Ok(self.book.lock().unwrap_or_else(|p| p.into_inner()).0.revoke(id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bearer(id: &str) -> Bearer {
        Bearer { id: id.into(), label: format!("device {id}"), created_at: "2026-11-04T10:00:00Z".into() }
    }

    #[test]
    fn given_a_token_when_digested_then_it_is_the_sha256_in_hex_and_two_tokens_never_share_one() {
        // Given the digest of a known string and of two tokens
        // Then the first is the published SHA-256 of "abc", and different tokens have different digests of 64 hex characters
        assert_eq!(token_digest("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_ne!(token_digest("rcab_1"), token_digest("rcab_2"));
        assert!(token_digest("rcab_1").len() == 64 && token_digest("rcab_1").bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn given_bearers_minted_when_a_token_is_presented_then_only_the_token_of_a_bearer_that_is_still_there_is_known_and_the_book_keeps_no_token() {
        // Given a book with two bearers
        let mut book = Book::default();
        book.add(bearer("a"), "rcab_token_a");
        book.add(bearer("b"), "rcab_token_b");
        // Then each token names its bearer, a made-up one and a bearer's id name nobody, and the serialized book holds no token
        assert_eq!(book.verify("rcab_token_b"), Some(bearer("b")));
        assert_eq!((book.verify("rcab_token_c"), book.verify("a"), book.verify("")), (None, None, None));
        assert!(!serde_json::to_string(&book).unwrap().contains("rcab_token"));
        // When the first is revoked, Then its token is refused and the other still works, and revoking it again finds nothing
        assert!(book.revoke("a"));
        assert_eq!((book.verify("rcab_token_a"), book.verify("rcab_token_b")), (None, Some(bearer("b"))));
        assert!(!book.revoke("a"));
        assert_eq!(book.list(), vec![bearer("b")]);
    }

    #[test]
    fn given_authorization_values_when_the_token_is_read_then_only_the_bearer_scheme_with_a_token_gives_one() {
        // Given header values of the bearer scheme, other schemes, a bare token and nothing
        let read = bearer_token;
        // Then only `Bearer <token>` gives the token
        assert_eq!(read(Some("Bearer rcab_x")), Some("rcab_x"));
        assert_eq!([read(Some("Bearer ")), read(Some("Bearer")), read(Some("Basic rcab_x")), read(Some("rcab_x")), read(Some("bearer rcab_x")), read(None)], [None; 6]);
    }

    #[test]
    fn given_two_digests_when_compared_then_they_are_the_same_only_when_every_character_is() {
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab") && !same("", "a"));
    }

    #[cfg(feature = "testing")]
    #[test]
    fn given_the_in_memory_double_when_a_bearer_is_minted_then_it_behaves_as_the_book_does() {
        let bearers = MemoryBearers::default();
        let minted = bearers.mint("phone").unwrap();
        assert_eq!(bearers.verify(&minted.token), Some(minted.bearer.clone()));
        assert!(bearers.revoke(&minted.bearer.id).unwrap() && bearers.verify(&minted.token).is_none() && bearers.list().unwrap().is_empty());
    }
}
