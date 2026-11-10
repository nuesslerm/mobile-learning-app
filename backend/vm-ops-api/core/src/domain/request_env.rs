//! The env variables a redeploy request body may set, for the apps that declare them. The body is KEY=VALUE lines; every rule is
//! an allow-list and none of the refusals carries any part of the body.
use crate::domain::apps::request_env_names;

pub const MAX_BODY_BYTES: usize = 1024;
pub const MAX_VALUE_CHARS: usize = 256;
const AUTH_KEY_NAME: &str = "TS_CI_RUNNER_AUTH_KEY";
const AUTH_KEY_PREFIX: &str = "tskey-auth-";
const AUTH_KEY_MAX_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvBodyError {
    TooLarge,
    NotUtf8,
    NoneAccepted,
    NotNameValue,
    NameNotAllowed,
    DuplicateName,
    BadValue,
}

impl EnvBodyError {
    /// The rule the request broke, as the 400 says it.
    pub fn rule(self) -> &'static str {
        match self {
            EnvBodyError::TooLarge => "env body exceeds 1024 bytes",
            EnvBodyError::NotUtf8 => "env body is not UTF-8 text",
            EnvBodyError::NoneAccepted => "this app accepts no env body",
            EnvBodyError::NotNameValue => "env body must be NAME=VALUE lines, each ended by a newline",
            EnvBodyError::NameNotAllowed => "env name is not on this app's allow-list",
            EnvBodyError::DuplicateName => "env name is set twice",
            EnvBodyError::BadValue => "env value must be 1 to 256 characters of [A-Za-z0-9_-] (a tskey-auth-* key for TS_CI_RUNNER_AUTH_KEY, at most 160)",
        }
    }
}

fn is_valid_value(name: &str, value: &str) -> bool {
    let plain = (1..=MAX_VALUE_CHARS).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    plain && (name != AUTH_KEY_NAME || (value.starts_with(AUTH_KEY_PREFIX) && value.len() > AUTH_KEY_PREFIX.len() && value.len() <= AUTH_KEY_MAX_CHARS))
}

/// The `(name, value)` pairs of `body` for `app`, in order. An empty body is no pairs for any app.
pub fn parse_env_body(app: &str, body: &[u8]) -> Result<Vec<(String, String)>, EnvBodyError> {
    if body.is_empty() {
        return Ok(Vec::new());
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(EnvBodyError::TooLarge);
    }
    let allowed = request_env_names(app);
    if allowed.is_empty() {
        return Err(EnvBodyError::NoneAccepted);
    }
    let text = std::str::from_utf8(body).map_err(|_| EnvBodyError::NotUtf8)?;
    let lines = text.strip_suffix('\n').unwrap_or(text);
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in lines.split('\n') {
        let (name, value) = line.split_once('=').ok_or(EnvBodyError::NotNameValue)?;
        if !allowed.contains(&name) {
            return Err(EnvBodyError::NameNotAllowed);
        }
        if pairs.iter().any(|(seen, _)| seen == name) {
            return Err(EnvBodyError::DuplicateName);
        }
        if !is_valid_value(name, value) {
            return Err(EnvBodyError::BadValue);
        }
        pairs.push((name.to_string(), value.to_string()));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_ci_runner_when_a_runner_token_line_is_parsed_then_it_is_the_pair_with_or_without_the_final_newline() {
        let want = Ok(vec![("RUNNER_TOKEN".to_string(), "AbC-123_x".to_string())]);
        assert_eq!(parse_env_body("ci-runner", b"RUNNER_TOKEN=AbC-123_x"), want);
        assert_eq!(parse_env_body("ci-runner", b"RUNNER_TOKEN=AbC-123_x\n"), want);
    }

    #[test]
    fn given_the_ci_runner_when_both_declared_names_are_sent_then_both_pairs_come_back_and_nothing_else_is_declared() {
        let body = b"RUNNER_TOKEN=AbC\nTS_CI_RUNNER_AUTH_KEY=tskey-auth-kAbC123CNTRL-xyz\n";
        let want = Ok(vec![("RUNNER_TOKEN".to_string(), "AbC".to_string()), ("TS_CI_RUNNER_AUTH_KEY".to_string(), "tskey-auth-kAbC123CNTRL-xyz".to_string())]);
        assert_eq!(parse_env_body("ci-runner", body), want);
        assert_eq!(request_env_names("ci-runner"), ["RUNNER_TOKEN", "TS_CI_RUNNER_AUTH_KEY"]);
    }

    #[test]
    fn given_a_runner_auth_key_that_is_not_a_tskey_auth_or_too_long_when_parsed_then_it_is_a_bad_value() {
        let long = format!("TS_CI_RUNNER_AUTH_KEY=tskey-auth-{}", "a".repeat(150));
        let edge = format!("TS_CI_RUNNER_AUTH_KEY=tskey-auth-{}", "a".repeat(149));
        for body in [&b"TS_CI_RUNNER_AUTH_KEY=abc"[..], b"TS_CI_RUNNER_AUTH_KEY=tskey-api-abc", b"TS_CI_RUNNER_AUTH_KEY=tskey-auth-", long.as_bytes()] {
            assert_eq!(parse_env_body("ci-runner", body), Err(EnvBodyError::BadValue), "{body:?}");
        }
        assert!(parse_env_body("ci-runner", edge.as_bytes()).is_ok());
    }

    #[test]
    fn given_a_body_that_breaks_a_rule_when_parsed_then_the_refusal_names_the_rule() {
        let long = format!("RUNNER_TOKEN={}", "a".repeat(257));
        let big = format!("RUNNER_TOKEN={}", "a".repeat(1100));
        let cases: [(&str, &[u8], EnvBodyError); 12] = [
            ("leetcode-backend", b"RUNNER_TOKEN=abc", EnvBodyError::NoneAccepted),
            ("ci-runner", b"OTHER=abc", EnvBodyError::NameNotAllowed),
            ("ci-runner", b"RUNNER_TOKEN=abc\nRUNNER_TOKEN=def", EnvBodyError::DuplicateName),
            ("ci-runner", b"RUNNER_TOKEN=ab c", EnvBodyError::BadValue),
            ("ci-runner", b"RUNNER_TOKEN=ab\rc", EnvBodyError::BadValue),
            ("ci-runner", b"RUNNER_TOKEN=a$(id)", EnvBodyError::BadValue),
            ("ci-runner", b"RUNNER_TOKEN=", EnvBodyError::BadValue),
            ("ci-runner", long.as_bytes(), EnvBodyError::BadValue),
            ("ci-runner", big.as_bytes(), EnvBodyError::TooLarge),
            ("ci-runner", b"\n", EnvBodyError::NotNameValue),
            ("ci-runner", b"RUNNER_TOKEN=abc\n\n", EnvBodyError::NotNameValue),
            ("ci-runner", b"RUNNER_TOKEN=\xff", EnvBodyError::NotUtf8),
        ];
        for (app, body, want) in cases {
            assert_eq!(parse_env_body(app, body), Err(want), "{app} {body:?}");
        }
    }

    #[test]
    fn given_an_empty_body_when_parsed_then_every_app_gets_no_pairs() {
        assert_eq!(parse_env_body("leetcode-backend", b""), Ok(Vec::new()));
        assert_eq!(parse_env_body("ci-runner", b""), Ok(Vec::new()));
    }
}
