//! Who may call: a rotating token (`hex(HMAC-SHA256(seed, UTC date))` of today or yesterday) with an Ed25519 signature over the
//! request, or the legacy raw seed unless signed auth is required. A rotating token never falls back to the legacy check.
use crate::domain::js;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const MAX_SIGNATURE_CLOCK_SKEW_MS: f64 = 60_000.0;
pub const TOKEN_GRACE_DAYS: i64 = 1;
const MS_PER_DAY: i64 = 86_400_000;

#[derive(Clone)]
pub struct AuthConfig {
    token: SecretString,
    signing_key: Option<VerifyingKey>,
    signing_key_given: bool,
    require_signed_auth: bool,
}

impl AuthConfig {
    /// `public_key_pem` is the SPKI PEM of the signing key; none, or one that is no Ed25519 key, fails every rotating token.
    pub fn new(token: SecretString, public_key_pem: &str, require_signed_auth: bool) -> Self {
        let signing_key = read_public_key(public_key_pem);
        Self { token, signing_key, signing_key_given: !public_key_pem.trim().is_empty(), require_signed_auth }
    }

    /// A key was configured and none could be read from it, so every rotating token fails.
    pub fn signing_key_unreadable(&self) -> bool {
        self.signing_key_given && self.signing_key.is_none()
    }
}

const PEM_BEGIN: &str = "-----BEGIN PUBLIC KEY-----";
const PEM_END: &str = "-----END PUBLIC KEY-----";

/// The key of a PEM as OpenSSL, which Node hands it to, reads one: the lines between the markers joined however they are wrapped,
/// white space ignored. A strict RFC 7468 reader refuses trailing spaces and any width but 64.
fn read_public_key(pem: &str) -> Option<VerifyingKey> {
    let mut lines = pem.lines().map(str::trim_end).skip_while(|line| *line != PEM_BEGIN).skip(1);
    let mut body = String::new();
    loop {
        match lines.next()? {
            PEM_END => break,
            line => body.extend(line.chars().filter(|c| !c.is_whitespace())),
        }
    }
    if !body.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'=')) {
        return None;
    }
    VerifyingKey::from_public_key_der(&lenient_base64(&body)).ok()
}

/// The request line the signature covers: the method and the pathname and search the service derived from the target.
pub struct RequestLine<'a> {
    pub method: &'a str,
    pub path_and_query: &'a str,
}

/// The header texts of the request, as received (trimmed of surrounding white space by the HTTP layer).
#[derive(Default)]
pub struct Credentials<'a> {
    pub authorization: Option<&'a str>,
    pub signature: Option<&'a str>,
    pub timestamp: Option<&'a str>,
}

pub fn derive_rotating_token(seed: &str, date: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(seed.as_bytes()).expect("hmac accepts a key of any length");
    mac.update(date.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Compares without short-circuiting on length, so timing cannot leak a partial match.
pub(crate) fn equal_in_constant_time(expected: &str, actual: &str) -> bool {
    let (expected, actual) = (expected.as_bytes(), actual.as_bytes());
    if expected.len() != actual.len() {
        let _ = expected.ct_eq(expected);
        return false;
    }
    expected.ct_eq(actual).into()
}

/// The token after `Bearer ` (one space, then at least one character), or none.
pub(crate) fn bearer(header: &str) -> Option<&str> {
    header.strip_prefix("Bearer ").filter(|candidate| !candidate.is_empty())
}

/// The decoding of `Buffer.from(text, 'base64')`: both alphabets, characters outside them skipped, `=` ends the text, and a
/// trailing partial group yields the bytes it completes.
pub fn lenient_base64(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut count) = (0u32, 0u32);
    for c in text.bytes() {
        let sextet = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(sextet);
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    bytes
}

fn is_rotating_token(config: &AuthConfig, now_ms: i64, candidate: &str) -> bool {
    let mut matched = false;
    for days_ago in 0..=TOKEN_GRACE_DAYS {
        let expected = derive_rotating_token(config.token.expose_secret(), &js::utc_date(now_ms - days_ago * MS_PER_DAY));
        matched |= equal_in_constant_time(&expected, candidate);
    }
    matched
}

fn has_valid_signature(config: &AuthConfig, now_ms: i64, credentials: &Credentials, request: &RequestLine, body: &[u8]) -> bool {
    let Some(key) = &config.signing_key else { return false };
    let (Some(signature), Some(timestamp)) = (credentials.signature, credentials.timestamp) else { return false };
    if signature.is_empty() || timestamp.is_empty() {
        return false;
    }
    let sent_at = js::to_number(timestamp);
    if !sent_at.is_finite() || (now_ms as f64 - sent_at).abs() > MAX_SIGNATURE_CLOCK_SKEW_MS {
        return false;
    }
    let body_hash = hex::encode(Sha256::digest(String::from_utf8_lossy(body).as_bytes()));
    let canonical = format!("{}\n{}\n{timestamp}\n{body_hash}", request.method, request.path_and_query);
    let Ok(signature) = Signature::from_slice(&lenient_base64(signature)) else { return false };
    key.verify(canonical.as_bytes(), &signature).is_ok()
}

/// A rotating token must also carry a valid signature; only the legacy raw token skips it.
pub fn is_authorized(config: &AuthConfig, now_ms: i64, credentials: &Credentials, request: &RequestLine, body: &[u8]) -> bool {
    let Some(candidate) = credentials.authorization.and_then(bearer) else { return false };
    if is_rotating_token(config, now_ms, candidate) {
        return has_valid_signature(config, now_ms, credentials, request, body);
    }
    !config.require_signed_auth && equal_in_constant_time(config.token.expose_secret(), candidate)
}

/// The signed rotating token only, for a route that must never take the legacy raw token.
pub fn is_signed(config: &AuthConfig, now_ms: i64, credentials: &Credentials, request: &RequestLine, body: &[u8]) -> bool {
    let Some(candidate) = credentials.authorization.and_then(bearer) else { return false };
    is_rotating_token(config, now_ms, candidate) && has_valid_signature(config, now_ms, credentials, request, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
    use ed25519_dalek::pkcs8::EncodePublicKey;
    use ed25519_dalek::{Signer, SigningKey};

    /// 2026-10-04T12:00:00Z
    const NOW: i64 = 1_791_115_200_000;
    const SEED: &str = "seed";

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn config(require_signed_auth: bool) -> AuthConfig {
        let pem = signing_key().verifying_key().to_public_key_pem(LineEnding::LF).expect("the test key encodes");
        AuthConfig::new(SecretString::from(SEED), &pem, require_signed_auth)
    }

    fn signature_over(method: &str, path_and_query: &str, timestamp: &str, body: &str) -> String {
        let body_hash = hex::encode(Sha256::digest(body.as_bytes()));
        let canonical = format!("{method}\n{path_and_query}\n{timestamp}\n{body_hash}");
        STANDARD.encode(signing_key().sign(canonical.as_bytes()).to_bytes())
    }

    struct Sent {
        authorization: Option<String>,
        signature: Option<String>,
        timestamp: Option<String>,
    }

    fn bearer_of(token: &str) -> Option<String> {
        Some(format!("Bearer {token}"))
    }

    fn signed(token: &str, method: &str, path: &str, body: &str, at: i64) -> Sent {
        let timestamp = at.to_string();
        Sent { authorization: bearer_of(token), signature: Some(signature_over(method, path, &timestamp, body)), timestamp: Some(timestamp) }
    }

    fn unsigned(token: &str) -> Sent {
        Sent { authorization: bearer_of(token), signature: None, timestamp: None }
    }

    fn allows(config: &AuthConfig, sent: &Sent, method: &str, path: &str, body: &str) -> bool {
        let credentials = Credentials { authorization: sent.authorization.as_deref(), signature: sent.signature.as_deref(), timestamp: sent.timestamp.as_deref() };
        is_authorized(config, NOW, &credentials, &RequestLine { method, path_and_query: path }, body.as_bytes())
    }

    #[test]
    fn given_the_default_migration_posture_when_the_raw_seed_is_presented_then_it_passes() {
        assert!(allows(&config(false), &unsigned(SEED), "GET", "/health", ""));
    }

    #[test]
    fn given_signed_auth_is_required_when_the_raw_seed_is_presented_then_it_is_refused() {
        assert!(!allows(&config(true), &unsigned(SEED), "GET", "/health", ""));
    }

    #[test]
    fn given_rotating_tokens_for_today_and_the_grace_day_when_signed_then_they_pass_and_unsigned_they_do_not() {
        let config = config(true);
        let body = r#"{"a":1}"#;
        for day in ["2026-10-04", "2026-10-03"] {
            let token = derive_rotating_token(SEED, day);
            assert!(allows(&config, &signed(&token, "POST", "/x?y=1", body, NOW), "POST", "/x?y=1", body), "the token of {day}, signed");
            assert!(!allows(&config, &unsigned(&token), "POST", "/x?y=1", body), "the token of {day}, unsigned");
        }
    }

    #[test]
    fn given_the_default_posture_when_a_rotating_token_is_unsigned_then_it_never_falls_back_to_the_legacy_check() {
        let token = derive_rotating_token(SEED, "2026-10-04");
        assert!(!allows(&config(false), &unsigned(&token), "GET", "/health", ""));
    }

    #[test]
    fn given_a_two_day_old_token_a_skewed_signature_or_a_tampered_body_when_presented_then_none_passes() {
        let config = config(false);
        let today = derive_rotating_token(SEED, "2026-10-04");
        let stale = signed(&derive_rotating_token(SEED, "2026-10-02"), "GET", "/health", "", NOW);
        let skewed = signed(&today, "GET", "/health", "", NOW - 61_000);
        let tampered = signed(&today, "POST", "/x", "a", NOW);
        assert!(!allows(&config, &stale, "GET", "/health", ""));
        assert!(!allows(&config, &skewed, "GET", "/health", ""));
        assert!(!allows(&config, &tampered, "POST", "/x", "b"));
    }

    #[test]
    fn given_the_skew_bound_when_a_signature_is_59_or_61_seconds_off_then_59_passes_and_61_does_not() {
        let config = config(false);
        let today = derive_rotating_token(SEED, "2026-10-04");
        for (offset, want) in [(-59_000, true), (59_000, true), (-61_000, false), (61_000, false)] {
            let sent = signed(&today, "GET", "/health", "", NOW + offset);
            assert_eq!(allows(&config, &sent, "GET", "/health", ""), want, "a timestamp {offset} ms off");
        }
    }

    #[test]
    fn given_no_signing_key_when_a_signed_rotating_token_arrives_then_it_fails_closed() {
        let config = AuthConfig::new(SecretString::from(SEED), "", false);
        let token = derive_rotating_token(SEED, "2026-10-04");
        assert!(!allows(&config, &signed(&token, "GET", "/health", "", NOW), "GET", "/health", ""));
    }

    #[test]
    fn given_a_key_that_is_no_pem_when_a_signed_rotating_token_arrives_then_it_fails_closed() {
        let config = AuthConfig::new(SecretString::from(SEED), "not a key", false);
        let token = derive_rotating_token(SEED, "2026-10-04");
        assert!(!allows(&config, &signed(&token, "GET", "/health", "", NOW), "GET", "/health", ""));
        assert!(config.signing_key_unreadable());
    }

    /// The key as pem-rfc7468 writes it, and its base64 body on one line.
    fn canonical_pem() -> (String, String) {
        let pem = signing_key().verifying_key().to_public_key_pem(LineEnding::LF).expect("the test key encodes");
        let body = pem.lines().skip(1).take_while(|line| *line != PEM_END).collect();
        (pem, body)
    }

    fn passes_with_key(pem: &str) -> bool {
        let config = AuthConfig::new(SecretString::from(SEED), pem, false);
        let token = derive_rotating_token(SEED, "2026-10-04");
        allows(&config, &signed(&token, "GET", "/health", "", NOW), "GET", "/health", "")
    }

    #[test]
    fn given_a_pem_serialised_the_way_openssl_reads_it_when_a_signed_token_arrives_then_it_verifies() {
        // Given the forms Node accepts (each checked against crypto.verify) of the same key
        let (canonical, body) = canonical_pem();
        let wrapped = |width: usize| body.as_bytes().chunks(width).map(|c| std::str::from_utf8(c).expect("base64 is ASCII")).collect::<Vec<_>>().join("\n");
        let forms = [
            ("canonical", canonical.clone()),
            ("trailing spaces on every line", canonical.lines().map(|l| format!("{l}  ")).collect::<Vec<_>>().join("\n") + "\n"),
            ("a trailing tab on every line", canonical.lines().map(|l| format!("{l}\t")).collect::<Vec<_>>().join("\n") + "\n"),
            ("the body wrapped at 32 columns", format!("{PEM_BEGIN}\n{}\n{PEM_END}\n", wrapped(32))),
            ("the body on one line", format!("{PEM_BEGIN}\n{body}\n{PEM_END}\n")),
            ("CRLF line ends", canonical.replace('\n', "\r\n")),
            ("text before the begin marker", format!("a note\n{canonical}")),
            ("no newline after the end marker", canonical.trim_end().to_string()),
            ("blank lines after the end marker", format!("{canonical}\n\n")),
            ("a space inside the body", format!("{PEM_BEGIN}\n{} {}\n{PEM_END}\n", &body[..30], &body[30..])),
        ];
        // When a rotating token signed by that key arrives, Then each form is read
        for (what, pem) in forms {
            assert!(passes_with_key(&pem), "{what}");
        }
    }

    #[test]
    fn given_a_pem_openssl_refuses_when_a_signed_token_arrives_then_it_fails_closed_and_the_key_is_reported_unreadable() {
        // Given texts Node's crypto.verify throws on
        let (canonical, body) = canonical_pem();
        let forms = [
            ("another label", canonical.replace("PUBLIC KEY", "PRIVATE KEY")),
            ("no end marker", format!("{PEM_BEGIN}\n{body}\n")),
            ("no begin marker", format!("{body}\n{PEM_END}\n")),
            ("text after the begin marker", format!("{PEM_BEGIN} x\n{body}\n{PEM_END}\n")),
            ("a character outside base64", format!("{PEM_BEGIN}\n{}*{}\n{PEM_END}\n", &body[..10], &body[10..])),
            ("a body cut short", format!("{PEM_BEGIN}\n{}\n{PEM_END}\n", &body[..30])),
        ];
        // When a rotating token signed by the real key arrives, Then it is refused
        for (what, pem) in forms {
            assert!(!passes_with_key(&pem), "{what}");
            assert!(AuthConfig::new(SecretString::from(SEED), &pem, false).signing_key_unreadable(), "{what}");
        }
    }

    #[test]
    fn given_a_key_that_is_unset_or_readable_when_asked_then_it_is_not_reported_unreadable() {
        let (canonical, _) = canonical_pem();
        for pem in ["", "  \n ", canonical.as_str()] {
            assert!(!AuthConfig::new(SecretString::from(SEED), pem, false).signing_key_unreadable(), "{pem:?}");
        }
    }

    #[test]
    fn given_a_missing_or_non_bearer_header_when_presented_then_it_is_refused() {
        let config = config(false);
        let none = Sent { authorization: None, signature: None, timestamp: None };
        let basic = Sent { authorization: Some("Basic seed".to_string()), signature: None, timestamp: None };
        let lowercase = Sent { authorization: Some(format!("bearer {SEED}")), signature: None, timestamp: None };
        let empty = Sent { authorization: Some("Bearer ".to_string()), signature: None, timestamp: None };
        for (what, sent) in [("none", none), ("Basic", basic), ("a lowercase scheme", lowercase), ("an empty token", empty)] {
            assert!(!allows(&config, &sent, "GET", "/health", ""), "{what}");
        }
    }

    #[test]
    fn given_a_timestamp_in_a_form_number_reads_when_signed_over_that_text_then_it_passes() {
        let config = config(false);
        let today = derive_rotating_token(SEED, "2026-10-04");
        for form in [format!("{NOW}e0"), format!("{NOW}.0"), format!("+{NOW}"), format!("0x{NOW:x}")] {
            let sent = Sent { authorization: bearer_of(&today), signature: Some(signature_over("GET", "/health", &form, "")), timestamp: Some(form.clone()) };
            assert!(allows(&config, &sent, "GET", "/health", ""), "the timestamp header {form:?}");
        }
        let not_a_number = format!("{NOW}x");
        let sent = Sent { authorization: bearer_of(&today), signature: Some(signature_over("GET", "/health", &not_a_number, "")), timestamp: Some(not_a_number) };
        assert!(!allows(&config, &sent, "GET", "/health", ""));
    }

    #[test]
    fn given_a_signature_in_the_url_safe_alphabet_without_padding_when_presented_then_it_passes() {
        let config = config(false);
        let today = derive_rotating_token(SEED, "2026-10-04");
        let (timestamp, standard) = (0..1_000)
            .map(|back| (NOW - back).to_string())
            .map(|ts| {
                let signature = signature_over("GET", "/health", &ts, "");
                (ts, signature)
            })
            .find(|(_, signature)| signature.contains(['+', '/']))
            .expect("a signature with + or / among 1000 timestamps");
        let url_safe = standard.replace('+', "-").replace('/', "_").trim_end_matches('=').to_string();
        let sent = Sent { authorization: bearer_of(&today), signature: Some(url_safe), timestamp: Some(timestamp) };
        assert!(allows(&config, &sent, "GET", "/health", ""));
    }

    #[test]
    fn given_a_signature_over_another_query_when_presented_then_it_is_refused() {
        let config = config(false);
        let today = derive_rotating_token(SEED, "2026-10-04");
        let sent = signed(&today, "GET", "/health?a=1", "", NOW);
        assert!(allows(&config, &sent, "GET", "/health?a=1", ""));
        assert!(!allows(&config, &sent, "GET", "/health?a=2", ""));
    }

    #[test]
    fn given_a_body_that_is_not_utf8_when_signed_over_its_decoding_then_the_signature_holds() {
        let config = config(false);
        let today = derive_rotating_token(SEED, "2026-10-04");
        let sent = signed(&today, "POST", "/x", "a\u{fffd}b", NOW);
        let credentials = Credentials { authorization: sent.authorization.as_deref(), signature: sent.signature.as_deref(), timestamp: sent.timestamp.as_deref() };
        let request = RequestLine { method: "POST", path_and_query: "/x" };
        assert!(is_authorized(&config, NOW, &credentials, &request, b"a\xffb"));
    }

    #[test]
    fn given_the_seed_and_a_date_when_the_rotating_token_is_derived_then_it_is_the_hex_hmac_node_computes() {
        assert_eq!(derive_rotating_token("seed", "2026-10-04"), "451e3d1e016662625dc52bdb029dda21ff90fe2f3871ed9231916dc6c2b42c55");
        assert_eq!(derive_rotating_token("seed", "2026-10-03"), "0baf9338d9096f9b64a8a61aaed09e6af6e57a6d3f50cf9efbec09d67e53079b");
    }

    #[test]
    fn given_base64_texts_when_decoded_leniently_then_they_decode_as_node_decodes_them() {
        let cases = [
            ("aGVsbG8=", "68656c6c6f"),
            ("aGVsbG8", "68656c6c6f"),
            ("-_-_", "fbffbf"),
            ("+/+/", "fbffbf"),
            ("a G V\nsbG8=", "68656c6c6f"),
            ("aGVs*bG8=", "68656c6c6f"),
            ("aGVs=bG8=", "68656c"),
            ("a", ""),
            ("ab", "69"),
            ("abc", "69b7"),
            ("abcde", "69b71d"),
            ("====", ""),
            ("ab==cd", "69"),
            ("", ""),
            ("!!!!", ""),
            ("__8", "ffff"),
            ("-_8=", "fbff"),
            ("  YQ  ", "61"),
            ("YWJj\r\nZGVm", "616263646566"),
        ];
        for (text, hex) in cases {
            assert_eq!(hex::encode(lenient_base64(text)), hex, "Buffer.from({text:?}, 'base64')");
        }
    }

    #[test]
    fn given_texts_of_equal_and_different_length_when_compared_then_only_the_same_text_is_equal() {
        assert!(equal_in_constant_time("abc", "abc"));
        assert!(!equal_in_constant_time("abc", "abd"));
        assert!(!equal_in_constant_time("abc", "abcd"));
        assert!(!equal_in_constant_time("abcd", "abc"));
        assert!(!equal_in_constant_time("abc", ""));
    }
}
