//! Scrubbing for everything that leaves the process as a log line: a field whose name looks like a credential is replaced
//! whole, a known secret value is replaced wherever it stands, and an email, a JWT or a card number in any text is replaced
//! too, as the TypeScript logger does.

use regex::Regex;
use std::borrow::Cow;
use std::sync::OnceLock;

pub const REDACTED: &str = "[REDACTED]";

const KEY_PATTERNS: [&str; 11] =
    ["token", "secret", "password", "passwd", "cookie", "authorization", "apikey", "api_key", "privatekey", "private_key", "credential"];

/// A secret shorter than this is not scrubbed from text: it would turn ordinary words into noise.
const MIN_SECRET_CHARS: usize = 8;

pub fn key_is_sensitive(key: &str) -> bool {
    let lower = key.to_lowercase();
    KEY_PATTERNS.iter().any(|pattern| lower.contains(pattern))
}

fn pii() -> &'static [Regex; 3] {
    static PII: OnceLock<[Regex; 3]> = OnceLock::new();
    PII.get_or_init(|| {
        [
            Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").expect("email"),
            Regex::new(r"(?-u:\b)eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}(?-u:\b)").expect("jwt"),
            Regex::new(r"(?-u:\b)[0-9](?:[ -]?[0-9]){12,18}(?-u:\b)").expect("card"),
        ]
    })
}

#[derive(Debug, Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    /// The values of the environment variables whose names look like secrets, plus `extra`.
    pub fn from_env(extra: impl IntoIterator<Item = String>) -> Self {
        Self::from_values(std::env::vars().filter(|(name, _)| key_is_sensitive(name)).map(|(_, value)| value).chain(extra))
    }

    pub fn from_values(values: impl IntoIterator<Item = String>) -> Self {
        let mut kept: Vec<String> = Vec::new();
        for value in values {
            if value.chars().count() >= MIN_SECRET_CHARS && !kept.contains(&value) {
                kept.push(value);
            }
        }
        kept.sort_by_key(|value| std::cmp::Reverse(value.chars().count()));
        Self { values: kept }
    }

    /// `text` with every known secret replaced (longest first, so a secret that contains another goes whole), then every
    /// email, JWT and card number.
    pub fn scrub<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut out = Cow::Borrowed(text);
        for secret in &self.values {
            if out.contains(secret.as_str()) {
                out = Cow::Owned(out.replace(secret.as_str(), REDACTED));
            }
        }
        for pattern in pii() {
            if pattern.is_match(&out) {
                out = Cow::Owned(pattern.replace_all(&out, REDACTED).into_owned());
            }
        }
        out
    }

    /// The value a field named `key` may carry: whole-redacted when the name looks like a credential, else scrubbed.
    pub fn field<'a>(&self, key: &str, value: &'a str) -> Cow<'a, str> {
        if key_is_sensitive(key) {
            Cow::Borrowed(REDACTED)
        } else {
            self.scrub(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_credential_like_name_when_checked_then_it_is_sensitive_whatever_its_case() {
        assert!(key_is_sensitive("sessionCookie"));
        assert!(key_is_sensitive("X_API_KEY"));
        assert!(!key_is_sensitive("event"));
    }

    #[test]
    fn given_a_known_secret_when_it_appears_inside_text_then_it_is_replaced_and_other_text_survives() {
        let redactor = Redactor::from_values(["hunter2-hunter2".to_string()]);

        assert_eq!(redactor.scrub("login hunter2-hunter2 ok"), "login [REDACTED] ok");
        assert_eq!(redactor.scrub("nothing here"), "nothing here");
    }

    #[test]
    fn given_a_secret_that_contains_another_when_scrubbed_then_the_longer_goes_whole() {
        let redactor = Redactor::from_values(["abcdefgh".to_string(), "abcdefgh-and-more".to_string()]);

        assert_eq!(redactor.scrub("x abcdefgh-and-more y"), "x [REDACTED] y");
    }

    #[test]
    fn given_a_short_value_when_registered_then_it_is_not_scrubbed() {
        assert_eq!(Redactor::from_values(["abc".to_string()]).scrub("abc"), "abc");
    }

    #[test]
    fn given_an_email_a_jwt_and_a_card_number_when_scrubbed_then_each_is_replaced() {
        let redactor = Redactor::default();

        assert_eq!(redactor.scrub("from a.b@example.com"), "from [REDACTED]");
        assert_eq!(redactor.scrub("t eyJhbGciOi.eyJzdWIiOi.abcdefghij end"), "t [REDACTED] end");
        assert_eq!(redactor.scrub("card 4111 1111 1111 1111"), "card [REDACTED]");
        assert_eq!(redactor.scrub("took 1200 ms"), "took 1200 ms");
    }

    #[test]
    fn given_a_credential_named_field_when_kept_then_its_value_goes_whole() {
        let redactor = Redactor::default();

        assert_eq!(redactor.field("cookie", "abc"), REDACTED);
        assert_eq!(redactor.field("note", "abc"), "abc");
    }
}
