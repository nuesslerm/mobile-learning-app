//! Strings that look like a credential, found in a file a release would publish: the shapes the `wf` release scan refuses
//! (`.github/workflows/pipeline.yml`, the `clis` job) and the wider ones below, without a regex engine (a prefix, then a run of
//! characters of one kind), plus any value the vault holds.

fn run(text: &str, prefix: &str, min: usize, ok: impl Fn(char) -> bool) -> bool {
    text.match_indices(prefix).any(|(at, _)| text[at + prefix.len()..].chars().take_while(|c| ok(*c)).count() >= min)
}

fn alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn b64url(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// The first credential shape in `text`, by name.
pub fn find(text: &str) -> Option<&'static str> {
    if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----") {
        return Some("a private key");
    }
    if ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"].iter().any(|p| run(text, p, 30, alnum)) || run(text, "github_pat_", 30, |c| alnum(c) || c == '_') {
        return Some("a GitHub token");
    }
    if run(text, "AKIA", 16, |c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        return Some("an AWS access key id");
    }
    if text.match_indices("eyJ").any(|(at, _)| {
        let rest = &text[at + 3..];
        let head = rest.chars().take_while(|c| b64url(*c)).count();
        head >= 27 && rest[head..].strip_prefix('.').is_some_and(|after| after.chars().take_while(|c| b64url(*c)).count() >= 20)
    }) {
        return Some("a JSON web token");
    }
    if ["xoxb-", "xoxp-", "xoxa-"].iter().any(|p| run(text, p, 10, |c| alnum(c) || c == '-')) {
        return Some("a Slack token");
    }
    if run(text, "sk-", 32, alnum) {
        return Some("an API secret key");
    }
    if text.match_indices("ocid1.").any(|(at, _)| text[at..].chars().take_while(|c| alnum(*c) || *c == '.' || *c == '_' || *c == '-').collect::<String>().contains(".oc1.")) {
        return Some("an Oracle resource id");
    }
    if ["tskey-", "sk-ant-"].iter().any(|p| run(text, p, 24, b64url)) || run(text, "AIza", 35, b64url) {
        return Some("a provider API key");
    }
    if text.match_indices("Bearer ").any(|(at, _)| text[at + 7..].chars().take_while(|c| alnum(*c) || "._~+/=-".contains(*c)).count() >= 20) {
        return Some("a bearer token");
    }
    if text.match_indices("/p/").any(|(at, _)| text[at + 3..].chars().take_while(|c| b64url(*c)).count() >= 40) || run(text, "X-Amz-Signature=", 32, alnum) {
        return Some("a pre-authenticated or signed link");
    }
    if text.lines().any(named_literal) {
        return Some("a secret-named value assigned a literal");
    }
    None
}

const NAMES: [&str; 6] = ["token", "secret", "password", "passwd", "api_key", "apikey"];
const PLACEHOLDERS: [&str; 7] = ["test", "example", "fake", "dummy", "placeholder", "not-a-secret", "changeme"];

/// `name = "literal"` or `name: "literal"` where the name says secret and the literal is a long run of token characters.
fn named_literal(line: &str) -> bool {
    line.match_indices('"').any(|(open, _)| {
        let Some(len) = line[open + 1..].find('"') else { return false };
        let literal = &line[open + 1..open + 1 + len];
        let before = line[..open].to_lowercase();
        let assigned = line[..open].trim_end().ends_with(['=', ':', '>']);
        let lower = literal.to_lowercase();
        assigned
            && NAMES.iter().any(|n| before.contains(n))
            && literal.len() >= 16
            && literal.chars().all(|c| alnum(c) || "+/=_.-".contains(c))
            && literal.chars().any(|c| c.is_ascii_digit())
            && literal.chars().any(|c| c.is_ascii_alphabetic())
            && !PLACEHOLDERS.iter().any(|p| lower.contains(p))
    })
}

/// Whether `text` holds a value the vault holds. Values under 16 characters are skipped: too short to tell from ordinary text.
pub fn holds_value(text: &str, values: &[String]) -> bool {
    values.iter().any(|v| v.len() >= 16 && text.contains(v.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_text_with_a_credential_shape_when_scanned_then_it_is_named() {
        let cases = [
            ("-----BEGIN OPENSSH PRIVATE KEY-----\nabc", "a private key"),
            ("token = ghp_0123456789abcdefghijklmnopqrstuvwxyz", "a GitHub token"),
            ("github_pat_11AAAAAAA0zzzzzzzzzzzz_secretsecretsecret", "a GitHub token"),
            ("AKIAABCDEFGHIJKLMNOP", "an AWS access key id"),
            ("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnopqrstuv", "a JSON web token"),
            ("xoxb-1234567890-abcdef", "a Slack token"),
            ("sk-0123456789abcdef0123456789abcdef0123", "an API secret key"),
            ("ocid1.instance.oc1.eu-frankfurt-1.abcdefg", "an Oracle resource id"),
            ("tskey-auth-kABCDEFGHIJKLMNOPQRSTUV", "a provider API key"),
            ("sk-ant-api03-abcdefghijklmnopqrstuvwx", "a provider API key"),
            ("AIzaSyA-abcdefghijklmnopqrstuvwxyz012345", "a provider API key"),
            ("Authorization: Bearer abcdef0123456789abcdef", "a bearer token"),
            ("https://objectstorage.eu-frankfurt-1.oraclecloud.com/p/abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH/n/ns/b/bucket/o/x", "a pre-authenticated or signed link"),
            ("https://h/x?X-Amz-Signature=0123456789abcdef0123456789abcdef", "a pre-authenticated or signed link"),
            ("let api_token = \"abc123def456ghi789\";", "a secret-named value assigned a literal"),
            ("\"password\": \"Zk3j9xQ2m7Lp0vWb\"", "a secret-named value assigned a literal"),
        ];
        for (text, what) in cases {
            assert_eq!(find(text), Some(what), "{text}");
        }
    }

    #[test]
    fn given_ordinary_source_when_scanned_then_nothing_is_found() {
        for text in ["fn main() { println!(\"hello\"); }", "let ghp = 1; // ghp_ short", "AKIA short", "eyJ not a token", "task-run", "ask-me-anything-about-it-now-please-ok", "let token = \"test-token-not-a-secret\";", "let token = \"short\";", "let name = \"abc123def456ghi789jkl\";", "Bearer {token}", "Bearer token", "\"envVar\": \"SECRET_ENV_VARIABLE_NAME\""] {
            assert_eq!(find(text), None, "{text}");
        }
    }

    #[test]
    fn given_a_value_the_vault_holds_when_scanned_then_it_is_found_unless_it_is_short() {
        let vault = vec!["correct-horse-battery-staple".to_string(), "short".to_string()];

        assert!(holds_value("host = correct-horse-battery-staple", &vault));
        assert!(!holds_value("a short value", &vault));
        assert!(!holds_value("nothing here", &vault));
    }
}
