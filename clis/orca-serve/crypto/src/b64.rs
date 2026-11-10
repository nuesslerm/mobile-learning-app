use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;

pub fn encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

pub fn encode_url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Standard base64 that round-trips to the same text: no alias spellings, no stray padding bits.
pub fn decode_canonical(text: &str) -> Option<Vec<u8>> {
    let bytes = STANDARD.decode(text).ok()?;
    (STANDARD.encode(&bytes) == text).then_some(bytes)
}

pub fn decode_canonical_exact<const N: usize>(text: &str) -> Option<[u8; N]> {
    decode_canonical(text)?.try_into().ok()
}

/// The phone's lenient pairing-code reader: url-safe or standard alphabet, padding optional.
pub fn decode_pairing_code(code: &str) -> Option<Vec<u8>> {
    if code.is_empty() || !code.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/_-=".contains(&b)) {
        return None;
    }
    let trimmed = code.trim_end_matches('=');
    if trimmed.len() % 4 == 1 || trimmed.contains('=') {
        return None;
    }
    let standard: String = trimmed
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    let mut padded = standard;
    while !padded.len().is_multiple_of(4) {
        padded.push('=');
    }
    // Unused trailing bits are tolerated, as the reference does.
    let lenient = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    );
    lenient.decode(padded).ok()
}
