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
        "github.autoMergeAlreadyMergeable" => "This pull request can already be merged. Use Merge instead of auto-merge.",
        "github.autoMergeEnableFailed" => "Failed to enable auto-merge for pull request: {{detail}}",
        "github.autoMergeDisableFailed" => "Failed to disable auto-merge for pull request: {{detail}}",
        "github.noFailedChecks" => "No failed GitHub Actions checks to rerun.",
        "github.noRerunnableChecks" => "No rerunnable checks found.",
        "github.rerunPermissionDenied" => "You don't have permission to rerun checks on this repository. Check your GitHub token scopes.",
        "github.rerunTargetNotFound" => "GitHub resource to rerun was not found — it may have expired or been deleted.",
        "github.rerunRejected" => "Could not rerun checks — {{detail}}",
        "github.rerunFailed" => "Failed to rerun checks: {{detail}}",
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
pub(crate) fn unresolved(lower: &str) -> bool {
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
    pr_update_as(stderr, "github.pullRequestUpdateFailed")
}

/// `classifyPullRequestUpdateError` for the action whose failure code is `action`.
fn pr_update_as(stderr: &str, action: &str) -> Value {
    if unresolved(&stderr.to_lowercase()) {
        return code_host("github.repoUnresolved", None);
    }
    let trimmed = stderr.trim();
    match classify(stderr) {
        ("permission_denied", ..) => code_host("github.pullRequestPermissionDenied", None),
        ("not_found", ..) => code_host("github.pullRequestNotFound", None),
        ("validation_error", ..) => code_host("invalidUpdate", Some(trimmed)),
        ("unknown", ..) => code_host(action, Some(trimmed)),
        (_, code, detail) => code_host(code, detail.as_deref()),
    }
}

/// `classifySetAutoMergeError`: GitHub refuses auto-merge for a pull request that is clean, which the host words on its own.
pub fn auto_merge(stderr: &str, enabled: bool) -> Value {
    if enabled && stderr.to_lowercase().contains("in clean status") {
        return code_host("github.autoMergeAlreadyMergeable", None);
    }
    pr_update_as(stderr, if enabled { "github.autoMergeEnableFailed" } else { "github.autoMergeDisableFailed" })
}

/// `classifyRerunChecksError`: a rerun that failed after `started` runs were started names them, since a rerun is not idempotent; with none started it is the
/// classified failure itself.
pub fn rerun(stderr: &str, started: usize) -> Value {
    if unresolved(&stderr.to_lowercase()) {
        return code_host("github.repoUnresolved", None);
    }
    let trimmed = stderr.trim();
    let failure = match classify(stderr) {
        ("permission_denied", ..) => code_host("github.rerunPermissionDenied", None),
        ("not_found", ..) => code_host("github.rerunTargetNotFound", None),
        ("validation_error", ..) => code_host("github.rerunRejected", Some(trimmed)),
        ("issues_disabled" | "unknown", ..) => code_host("github.rerunFailed", Some(trimmed)),
        (_, code, detail) => code_host(code, detail.as_deref()),
    };
    if started == 0 {
        return failure;
    }
    json!({"ok": false, "error": format!("{} ({started} rerun{} already started)", failure["error"].as_str().unwrap_or_default(), if started == 1 { "" } else { "s" })})
}

/// A failed glab call as the host answers a merge request update: an unresolved project by its code, anything else by glab's words.
pub fn glab(stderr: &str) -> Value {
    if unresolved(&stderr.to_lowercase()) || stderr.to_lowercase().contains("no gitlab remote") {
        return code_host("gitlab.projectUnresolved", None);
    }
    json!({"ok": false, "error": stderr.trim()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_failed_rerun_or_auto_merge_when_classified_then_each_is_the_hosts_code_and_a_partial_rerun_counts_what_started() {
        // Given gh's words for a clean pull request, a token that may not rerun, and a rerun that failed after one started
        // Then each is the host's code and text, and the partial rerun is the host's sentence with no code
        assert_eq!(auto_merge("gh exited 1: GraphQL: Pull request Pull request is in clean status (enablePullRequestAutoMerge)", true)["errorCode"], "github.autoMergeAlreadyMergeable");
        assert_eq!(auto_merge("gh exited 1: boom", true)["errorCode"], "github.autoMergeEnableFailed");
        assert_eq!(auto_merge("gh exited 1: boom", false)["errorCode"], "github.autoMergeDisableFailed");
        assert_eq!(rerun("HTTP 403: Resource not accessible", 0)["errorCode"], "github.rerunPermissionDenied");
        assert_eq!(rerun("HTTP 403: Resource not accessible", 2), json!({"ok": false, "error": "You don't have permission to rerun checks on this repository. Check your GitHub token scopes. (2 reruns already started)"}));
        assert_eq!(rerun("boom", 1)["error"], "Failed to rerun checks: boom (1 rerun already started)");
    }

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
