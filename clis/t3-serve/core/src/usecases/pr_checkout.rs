//! `pullRequests.diffFileContents` and `git.preparePullRequestThread`: a pull request's head read from the project's own repository. The head is
//! fetched into a ref of the `refs/rca/` namespace (no branch list shows it, no branch or checkout moves), and the file contents, the merge base
//! and the worktree come from git actions of the agent API on it. No host CLI runs for this, so it needs only the remote's own sign-in.
use super::pr::{failure, Project, Target};
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Op};
use serde_json::{json, Value};

const OPERATION: &str = "PullRequestOperationError";
const MANAGER: &str = "GitManagerError";
/// The commits of a change read to tell which commit a request names.
const COMMITS: u32 = 500;

fn operation_error(operation: &str, detail: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": OPERATION, "operation": operation, "detail": detail, "reason": "not-found"}))
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The branch a pull request's head gets in the project: a name of its own, so no branch of the repository is taken over.
pub fn local_branch(number: u64, head_branch: &str) -> String {
    let plain: String = head_branch.chars().map(|c| if c.is_ascii_alphanumeric() || "._".contains(c) { c } else { '-' }).collect();
    let plain = plain.trim_matches(['-', '.']);
    let plain = plain.strip_suffix(".lock").unwrap_or(plain);
    let plain: String = plain.chars().take(60).collect();
    format!("t3code/pr-{number}/{}", if plain.is_empty() { "head" } else { &plain })
}

/// A pull request reference as a person gives it: `12`, `#12`, or the address of a pull request or merge request. The host is the address's.
pub fn parse_reference(reference: &str) -> Option<(u64, Option<String>)> {
    let reference = reference.trim();
    let digits = |s: &str| (!s.is_empty() && s.len() <= 9 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u64>().ok()).flatten().filter(|n| *n > 0);
    if let Some(n) = digits(reference.trim_start_matches('#')) {
        return Some((n, None));
    }
    let rest = reference.strip_prefix("https://").or_else(|| reference.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    let mut segments = path.split(['?', '#']).next()?.split('/');
    let number = segments.by_ref().skip_while(|s| *s != "pull" && *s != "merge_requests").nth(1).and_then(digits)?;
    Some((number, Some(host.to_string())))
}

fn sha_prefix(candidate: &str, sha: &str) -> bool {
    (7..=64).contains(&candidate.len()) && candidate.bytes().all(|b| b.is_ascii_hexdigit()) && sha.to_ascii_lowercase().starts_with(&candidate.to_ascii_lowercase())
}

impl Service {
    /// The pull request's head (and its base branch) fetched into `refs/rca/pr/<number>/`: the commits they name.
    fn fetch_pull_request(&self, operation: &str, target: &Target, number: u64, base_branch: Option<&str>) -> Result<(String, Option<String>), QueryFailure> {
        let source = if target.provider == "gitlab" { format!("refs/merge-requests/{number}/head") } else { format!("refs/pull/{number}/head") };
        let mut refspecs = vec![format!("+{source}:refs/rca/pr/{number}/head")];
        refspecs.extend(base_branch.map(|b| format!("+refs/heads/{b}:refs/rca/pr/{number}/base")));
        let fetched = self.agent().run(&Op::GitFetch { root: target.project.root.clone(), refspecs }).map_err(|e| failure(operation, target.provider, &e))?;
        let sha_of = |name: String| fetched["refs"].as_array().into_iter().flatten().find(|r| r["ref"] == name).and_then(|r| r["sha"].as_str()).map(str::to_string);
        let head = sha_of(format!("refs/rca/pr/{number}/head")).ok_or_else(|| operation_error(operation, "the remote gave no head for the pull request"))?;
        Ok((head, base_branch.and_then(|_| sha_of(format!("refs/rca/pr/{number}/base")))))
    }

    /// The file at `rev` as text; a file that is not there is `None`, one too big or not text is a refusal.
    fn blob_text(&self, operation: &str, provider: &str, root: &str, rev: &str, path: &str) -> Result<Option<String>, QueryFailure> {
        match self.agent().run(&Op::GitBlob { root: root.into(), rev: rev.into(), path: path.into() }) {
            Ok(blob) if blob["truncated"] == true => Err(operation_error(operation, &format!("{path} is too big to expand"))),
            Ok(blob) => Ok(Some(blob["text"].as_str().unwrap_or_default().to_string())),
            Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" => Ok(None),
            Err(e) => Err(failure(operation, provider, &e)),
        }
    }

    /// `pullRequests.diffFileContents`: a changed file as it was before the change (at the merge base, or the commit's parent) and after (at the
    /// head, or the commit). Only a file the pull request changes is read, as T3 allows.
    pub fn pr_diff_file_contents(&self, payload: &Value) -> Result<Value, QueryFailure> {
        const OP: &str = "diffFileContents";
        let target = self.target(OP, payload)?;
        let number = Self::number(payload)?;
        let (change, old_path, new_path) = (field(payload, "changeType")?, field(payload, "oldPath")?, field(payload, "newPath")?);
        let root = target.project.root.clone();
        let pr = self.view(OP, &target, number)?;
        if text(payload, "commit").is_none() {
            let files = self.ask_hosted(OP, &target, "pr.files", json!({"number": number}))?;
            if !files["files"].as_array().into_iter().flatten().any(|f| f["path"] == new_path || f["path"] == old_path) {
                return Err(operation_error(OP, "This file is not part of the change request."));
            }
        }
        let (head, base) = self.fetch_pull_request(OP, &target, number, text(&pr, "base_ref"))?;
        let base = base.ok_or_else(|| operation_error(OP, "the remote gave no base branch for the pull request"))?;
        let compare = self.agent().run(&Op::GitCompare { root: root.clone(), base: base.clone(), head: head.clone(), limit: COMMITS }).map_err(|e| failure(OP, target.provider, &e))?;
        let merge_base = text(&compare, "merge_base").unwrap_or(&base).to_string();
        let (before, after) = match text(payload, "commit") {
            None => (Some(merge_base), head),
            Some(named) => {
                let found = compare["commits"].as_array().into_iter().flatten().find(|c| c["sha"].as_str().is_some_and(|sha| sha_prefix(named, sha))).ok_or_else(|| operation_error(OP, "This commit is not part of the change request."))?;
                (found["parents"][0].as_str().map(str::to_string), found["sha"].as_str().unwrap_or(named).to_string())
            }
        };
        let old = match (change, before) {
            ("new", _) | (_, None) => String::new(),
            (_, Some(rev)) => self.blob_text(OP, target.provider, &root, &rev, old_path)?.ok_or_else(|| operation_error(OP, &format!("{old_path} was not found before the change")))?,
        };
        let new = if change == "deleted" { String::new() } else { self.blob_text(OP, target.provider, &root, &after, new_path)?.ok_or_else(|| operation_error(OP, &format!("{new_path} was not found in the change")))? };
        Ok(json!({"oldContents": old, "newContents": new}))
    }

    /// `git.preparePullRequestThread`: the pull request's head as a branch of its own, checked out in the project (`local`) or in a worktree of
    /// its own (`worktree`, reused when the branch already has one).
    pub fn git_prepare_pull_request_thread(&self, payload: &Value) -> Result<Value, QueryFailure> {
        const OP: &str = "preparePullRequestThread";
        let cwd = field(payload, "cwd")?;
        let manager = |detail: &str| QueryFailure::Error(json!({"_tag": MANAGER, "operation": OP, "cwd": cwd, "detail": detail}));
        let as_manager = |e: AgentError| manager(&e.message());
        let (number, host) = parse_reference(field(payload, "reference")?).ok_or_else(|| manager("Enter a pull request number or the address of one."))?;
        let known = self.projects_named(&[]).into_iter().find(|p| p.root == cwd);
        let project = known.unwrap_or(Project { id: String::new(), title: String::new(), root: cwd.to_string() });
        let provider = super::pr::provider_of(host.as_deref());
        let target = Target { project, provider, host, repo: None };
        let pr = self.view(OP, &target, number).map_err(|e| match e {
            QueryFailure::Error(v) => manager(v["detail"].as_str().or(v["requirement"].as_str()).unwrap_or("the host refused")),
            other => other,
        })?;
        let head_branch = text(&pr, "head_ref").ok_or_else(|| manager("The host gave a pull request without a head branch."))?;
        let resolved = json!({
            "number": number, "title": text(&pr, "title").unwrap_or("(untitled)"), "url": text(&pr, "url").unwrap_or_default(), "baseBranch": text(&pr, "base_ref").unwrap_or_default(),
            "headBranch": head_branch, "state": text(&pr, "state").unwrap_or("open"),
        });
        let mode = field(payload, "mode")?;
        let local = mode == "local";
        if local {
            let status = self.agent().run(&Op::GitStatus { root: cwd.into() }).map_err(as_manager)?;
            if status["files"].as_array().into_iter().flatten().any(|f| f["index"] != "?") {
                return Err(manager(&format!("This repository has uncommitted changes. Commit or stash them before checking out pull request #{number} here, or check it out in its own folder.")));
            }
        }
        let (head, _) = self.fetch_pull_request(OP, &target, number, None).map_err(|e| match e {
            QueryFailure::Error(v) => manager(v["detail"].as_str().unwrap_or("the remote refused")),
            other => other,
        })?;
        let branch = local_branch(number, head_branch);
        if local {
            let refs = self.agent().run(&Op::GitRefs { root: cwd.into(), kind: Some("branch"), limit: 500 }).map_err(as_manager)?;
            let existing = refs["refs"].as_array().into_iter().flatten().find(|r| r["name"] == branch.as_str()).and_then(|r| r["sha"].as_str().map(str::to_string));
            let at = match existing {
                Some(sha) => {
                    self.agent().run(&Op::GitCheckout { root: cwd.into(), target: branch.clone(), track: false }).map_err(as_manager)?;
                    sha
                }
                None => {
                    self.agent().run(&Op::GitCreateRef { root: cwd.into(), name: branch.clone(), from: Some(head.clone()), checkout: true }).map_err(as_manager)?;
                    head.clone()
                }
            };
            return Ok(json!({"pullRequest": resolved, "branch": branch, "worktreePath": null, "isOnPullRequestHead": at == head}));
        }
        let listed = self.agent().run(&Op::WorktreeList { root: cwd.into() }).map_err(as_manager)?;
        let holder = listed["worktrees"].as_array().into_iter().flatten().find(|w| w["branch"] == branch.as_str());
        if let Some(w) = holder {
            if w["is_main"] == true {
                return Err(manager("This PR branch is already checked out in the main repo. Use Local, or switch the main repo off that branch before creating a worktree thread."));
            }
            return Ok(json!({"pullRequest": resolved, "branch": branch, "worktreePath": w["path"], "isOnPullRequestHead": w["head"] == head.as_str()}));
        }
        let create = Op::WorktreeCreate { root: cwd.into(), worktree: super::workspace::worktree_name(&branch, cwd), branch: branch.clone(), base: Some(head) };
        let made = self.agent().run(&create).map_err(as_manager)?;
        Ok(json!({"pullRequest": resolved, "branch": made["branch"], "worktreePath": made["path"], "isOnPullRequestHead": true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_ways_a_person_names_a_pull_request_when_parsed_then_the_number_and_the_address_host_come_out_and_anything_else_is_none() {
        assert_eq!(parse_reference("12"), Some((12, None)));
        assert_eq!(parse_reference(" #12 "), Some((12, None)));
        assert_eq!(parse_reference("https://github.com/octo/hello/pull/12"), Some((12, Some("github.com".into()))));
        assert_eq!(parse_reference("https://gitlab.example.com/grp/sub/proj/-/merge_requests/4?diff=1#note_2"), Some((4, Some("gitlab.example.com".into()))));
        for bad in ["", "0", "#", "abc", "https://github.com/octo/hello", "https://github.com/octo/hello/pull/x", "ftp://x/pull/1", "12 13"] {
            assert_eq!(parse_reference(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn given_a_head_branch_when_it_gets_a_local_name_then_the_name_is_one_the_executor_accepts_and_never_the_branch_itself() {
        assert_eq!(local_branch(12, "feature/retry-upload"), "t3code/pr-12/feature-retry-upload");
        assert_eq!(local_branch(7, "main"), "t3code/pr-7/main");
        assert_eq!(local_branch(7, "--x; rm -rf /"), "t3code/pr-7/x--rm--rf");
        assert_eq!(local_branch(7, "..."), "t3code/pr-7/head");
        assert_eq!(local_branch(7, "x.lock"), "t3code/pr-7/x");
        assert!(local_branch(1, &"a".repeat(300)).len() < 80);
    }

    #[test]
    fn given_a_commit_a_person_names_when_matched_against_a_sha_then_only_a_hex_prefix_of_seven_or_more_matches() {
        let sha = "c6e5bb0685be41ca17f3b89783b2f2cd1ab4561e";
        assert!(sha_prefix("c6e5bb0", sha) && sha_prefix(sha, sha) && sha_prefix("C6E5BB0", sha));
        assert!(!sha_prefix("c6e5bb", sha) && !sha_prefix("c6e5bb0x", sha) && !sha_prefix("", sha) && !sha_prefix("HEAD", sha));
    }
}
