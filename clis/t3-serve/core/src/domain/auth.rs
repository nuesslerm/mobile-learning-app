//! Secrets and their hashes. A pairing code, bearer token or ticket is stored only as the SHA-256 of what the holder presents.
use sha2::{Digest, Sha256};
use t3_contracts::scopes::is_scope;
use t3_contracts::wire::{PAIRING_ALPHABET, PAIRING_CODE_LENGTH};

pub fn hash_secret(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

/// 12 characters of the 32-letter alphabet from random bytes (256 is a multiple of 32, so masking is unbiased).
pub fn pairing_code(random: &[u8]) -> String {
    let alphabet = PAIRING_ALPHABET.as_bytes();
    random.iter().take(PAIRING_CODE_LENGTH).map(|b| alphabet[usize::from(b & 31)] as char).collect()
}

/// A version 4 UUID from 16 random bytes.
pub fn uuid_v4(random: &[u8]) -> String {
    let mut b = [0u8; 16];
    for (slot, byte) in b.iter_mut().zip(random) {
        *slot = *byte;
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn opaque_secret(random: &[u8]) -> String {
    hex::encode(random)
}

/// The id a token is known by in logs and records: a prefix of its hash, which reveals nothing of the secret.
pub fn token_id(hash: &str) -> String {
    hash.chars().take(12).collect()
}

pub fn validate_scopes(requested: &[String]) -> Result<Vec<String>, String> {
    if requested.is_empty() {
        return Err("a pairing needs at least one scope".into());
    }
    let mut out: Vec<String> = Vec::new();
    for scope in requested {
        if !is_scope(scope) {
            return Err(format!("{scope} is not one of the 17 scopes"));
        }
        if !out.contains(scope) {
            out.push(scope.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pairing_code_has_twelve_characters_of_the_alphabet_whatever_the_bytes() {
        // Given every possible byte value as random input
        let bytes: Vec<u8> = (0..=255).collect();
        for chunk in bytes.chunks(12).filter(|c| c.len() == 12) {
            // When a code is made
            let code = pairing_code(chunk);
            // Then it is 12 long and uses no ambiguous character
            assert_eq!(code.len(), 12);
            assert!(code.chars().all(|c| PAIRING_ALPHABET.contains(c)), "{code}");
        }
        assert!(!PAIRING_ALPHABET.contains(['0', 'O', '1', 'I']));
    }
}
