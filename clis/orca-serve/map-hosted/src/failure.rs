//! The host's `{ok: false, error, errorCode, errorDetail?}` answers for a failed GitHub or GitLab operation: its one English table
//! (`CODE_HOST_FAILURE_COPY`) and its classification of what gh printed (`classifyGhError`, `classifyPullRequestUpdateError`).
use serde_json::{json, Value};

fn copy(code: &str) -> &'static str {
    match code {
        "networkError" => "Network error — check your connection.",
        "invalidUpdate" => "Invalid update — {{detail}}",
        "issueNotFound" => "Issue not found — it may have been deleted.",
        "issueUpdateFailed" => "Failed to update issue: {{detail}}",
        "github.repoUnresolved" => "Could not resolve GitHub owner/repo for this repository",
        "github.unexpectedResponse" => "Unexpected response from GitHub",
        "github.rateLimited" => "GitHub rate limit hit. Try again in a few minutes.",
        "github.issuePermissionDenied" => "You don't have permission to edit this issue. Check your GitHub token scopes.",
        "github.issuesDisabled" => "Issues are disabled on this repository.",
        "github.pullRequestPermissionDenied" => "You don't have permission to edit this pull request. Check your GitHub token scopes.",
        "github.pullRequestNotFound" => "Pull request not found — it may have been deleted.",
        "github.pullRequestUpdateFailed" => "Failed to update pull request: {{detail}}",
        "gitlab.projectUnresolved" => "Could not resolve GitLab project for this repository",
        "gitlab.unexpectedResponse" => "Unexpected response from GitLab",
        _ => "{{detail}}",
    }
}

/// `codeHostFailure`: the code's text, the detail filled in and kept verbatim beside it.
pub fn code_host(code: &str, detail: Option<&str>) -> Value {
    let mut out = json!({"ok": false, "error": copy(code).replace("{{detail}}", detail.unwrap_or("")), "errorCode": code});
    if let Some(d) = detail {
        out["errorDetail"] = json!(d);
    }
    out
}

/// gh found no GitHub remote for the folder: the host checks this before it runs gh at all.
fn unresolved(lower: &str) -> bool {
    ["none of the git remotes", "no git remotes", "not a git repository"].iter().any(|w| lower.contains(w))
}

/// `classifyGhError`: the kind of failure and the code the host answers for it.
fn classify(stderr: &str) -> (&'static str, &'static str, Option<String>) {
    let s = stderr.to_lowercase();
    let trimmed = stderr.trim().to_string();
    if s.contains("rate limit") {
        ("rate_limited", "github.rateLimited", None)
    } else if s.contains("http 403") || s.contains("resource not accessible") {
        ("permission_denied", "github.issuePermissionDenied", None)
    } else if s.contains("http 404") || s.contains("could not resolve to a repository") {
        ("not_found", "issueNotFound", None)
    } else if s.contains("has disabled issues") {
        ("issues_disabled", "github.issuesDisabled", None)
    } else if s.contains("http 422") || s.contains("validation failed") {
        ("validation_error", "invalidUpdate", Some(trimmed))
    } else if ["timeout", "no such host", "network", "could not resolve host"].iter().any(|w| s.contains(w)) {
        ("network_error", "networkError", None)
    } else {
        ("unknown", "issueUpdateFailed", Some(trimmed))
    }
}

/// A failed gh call as the host answers an issue or comment operation.
pub fn gh(stderr: &str) -> Value {
    if unresolved(&stderr.to_lowercase()) {
        return code_host("github.repoUnresolved", None);
    }
    let (_, code, detail) = classify(stderr);
    code_host(code, detail.as_deref())
}

/// A failed gh call as the host answers a pull request update (`classifyPullRequestUpdateError` with its default action).
pub fn pr_update(stderr: &str) -> Value {
    if unresolved(&stderr.to_lowercase()) {
        return code_host("github.repoUnresolved", None);
    }
    let trimmed = stderr.trim();
    match classify(stderr) {
        ("permission_denied", ..) => code_host("github.pullRequestPermissionDenied", None),
        ("not_found", ..) => code_host("github.pullRequestNotFound", None),
        ("validation_error", ..) => code_host("invalidUpdate", Some(trimmed)),
        ("unknown", ..) => code_host("github.pullRequestUpdateFailed", Some(trimmed)),
        (_, code, detail) => code_host(code, detail.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_what_gh_printed_when_a_failure_is_classified_then_it_is_the_hosts_code_text_and_detail() {
        // Given gh's words for each kind of failure, When classified for a comment and for a pull request update
        // Then each is the host's code and English text, with gh's words kept as the detail where the host keeps them
        assert_eq!(gh("HTTP 404: Not Found"), json!({"ok": false, "error": "Issue not found — it may have been deleted.", "errorCode": "issueNotFound"}));
        assert_eq!(pr_update("HTTP 404: Not Found"), json!({"ok": false, "error": "Pull request not found — it may have been deleted.", "errorCode": "github.pullRequestNotFound"}));
        assert_eq!(pr_update(" boom \n"), json!({"ok": false, "error": "Failed to update pull request: boom", "errorCode": "github.pullRequestUpdateFailed", "errorDetail": "boom"}));
        assert_eq!(gh("API rate limit exceeded (HTTP 403)")["errorCode"], "github.rateLimited");
        assert_eq!(gh("none of the git remotes configured for this repository point to a known GitHub host"), json!({"ok": false, "error": "Could not resolve GitHub owner/repo for this repository", "errorCode": "github.repoUnresolved"}));
    }
}
