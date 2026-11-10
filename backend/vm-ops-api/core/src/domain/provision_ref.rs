//! Which git ref an app's compose file and `.env.defaults` are fetched from, and the raw URL that ref names.

pub const PROVISION_REPO: &str = "nuesslerm/mobile-learning-app";
/// The `dev` trunk: what a redeploy provisions when nothing names a ref.
pub const PROVISION_REF: &str = "dev";

/// An explicit `?ref=` beats the commit a `sha-<commit>` tag names, which beats the trunk. An empty explicit ref counts as none, and
/// a ref is used for that one redeploy only.
pub fn resolve_provision_ref(tag: &str, explicit_ref: Option<&str>) -> String {
    if let Some(explicit) = explicit_ref.filter(|r| !r.is_empty()) {
        return explicit.to_string();
    }
    if let Some(commit) = tag.strip_prefix("sha-").filter(|c| (7..=40).contains(&c.len()) && c.bytes().all(is_lower_hex)) {
        return commit.to_string();
    }
    PROVISION_REF.to_string()
}

fn is_lower_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f')
}

pub fn raw_url(path: &str, reference: &str) -> String {
    format!("https://raw.githubusercontent.com/{PROVISION_REPO}/{reference}/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::validators::is_valid_provision_ref;

    const COMMIT: &str = "47210700850be715eb63d37055270549b316c46f";

    #[test]
    fn given_nothing_more_specific_when_resolved_then_it_is_the_trunk_and_a_sha_tag_gives_its_commit() {
        assert_eq!(resolve_provision_ref("latest", None), PROVISION_REF);
        assert_eq!(resolve_provision_ref("sha-abc1234", None), "abc1234");
        assert_eq!(resolve_provision_ref(&format!("sha-{COMMIT}"), None), COMMIT);
        assert!(is_valid_provision_ref(&resolve_provision_ref("sha-19c202f", None)));
    }

    #[test]
    fn given_an_explicit_ref_when_resolved_then_it_beats_the_commit_and_an_empty_one_counts_as_none() {
        assert_eq!(resolve_provision_ref("latest", Some("other-branch")), "other-branch");
        assert_eq!(resolve_provision_ref("sha-abc1234", Some("other-branch")), "other-branch");
        assert_eq!(resolve_provision_ref("latest", Some("")), PROVISION_REF);
        assert_eq!(resolve_provision_ref("sha-abc1234", Some("")), "abc1234");
    }

    #[test]
    fn given_a_tag_that_names_no_commit_when_resolved_then_the_trunk_wins() {
        let too_long = format!("sha-{COMMIT}8");
        let tags = ["sha-abc123", "sha-", "sha-ABC1234", "sha-abc123g", "sha-abc1234\n", "sha-abc1234 ", " sha-abc1234", "SHA-abc1234", "sha_abc1234", "abc1234"];
        for tag in tags.iter().copied().chain([too_long.as_str()]) {
            assert_eq!(resolve_provision_ref(tag, None), PROVISION_REF, "{tag:?}");
        }
        assert_eq!(resolve_provision_ref("sha-0123456789abcdef0123456789abcdef01234567", None), "0123456789abcdef0123456789abcdef01234567");
    }

    #[test]
    fn given_a_path_and_a_ref_when_the_raw_url_is_built_then_it_is_the_one_github_serves() {
        let url = raw_url("infra/oracle-cloud/vm-ops-api/docker-compose.yml", "claude/some-branch");
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/nuesslerm/mobile-learning-app/claude/some-branch/infra/oracle-cloud/vm-ops-api/docker-compose.yml"
        );
    }
}
