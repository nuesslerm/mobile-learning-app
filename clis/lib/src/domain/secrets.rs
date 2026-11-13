//! Strings that look like a credential, found in a file a release would publish. The same shapes the `wf` release scan refuses
//! (`.github/workflows/pipeline.yml`, the `clis` job), without a regex engine: a prefix, then a run of characters of one kind.

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
    None
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
        ];
        for (text, what) in cases {
            assert_eq!(find(text), Some(what), "{text}");
        }
    }

    #[test]
    fn given_ordinary_source_when_scanned_then_nothing_is_found() {
        for text in ["fn main() { println!(\"hello\"); }", "let ghp = 1; // ghp_ short", "AKIA short", "eyJ not a token", "task-run", "ask-me-anything-about-it-now-please-ok"] {
            assert_eq!(find(text), None, "{text}");
        }
    }
}
