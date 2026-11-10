//! The phone's hosted rows through the chain: from `orca-serve` through the real rca-service to the real rca-exec, whose `gh` and `glab` are the scripted
//! doubles of exec-testkit (what each prints is set here, and no real token or account is involved). A row counts only when the agent methods it maps
//! to were answered in its call's window and the executor ran the hosted actions; what each call did is then read from what the CLI was started with
//! (its arguments and stdin, never an option built from the phone's text) and, for a worktree start, from the repository itself.
use crate::rig::*;
use exec_testkit::{Case, Rule};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

const SELECTOR: &str = "id:w1::";

fn with_repo(mut params: Value) -> Value {
    params["repo"] = json!(SELECTOR);
    params
}

fn rule(program: &str, args: &[&str], stdout: &str) -> Rule {
    Rule { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), stdout: stdout.into(), prefix: true, ..Rule::default() }
}

fn failing(program: &str, args: &[&str], stderr: &str) -> Rule {
    Rule { stderr: stderr.into(), exit_code: 1, ..rule(program, args, "") }
}

/// One pull request as `gh pr list` and `gh pr view` print it.
fn gh_pr(number: u64, title: &str, state: &str, draft: bool, head: &str, base: &str) -> Value {
    json!({
        "number": number, "title": title, "state": state, "isDraft": draft, "author": {"login": "octocat"}, "headRefName": head, "baseRefName": base, "labels": [],
        "createdAt": "2026-10-01T09:30:00Z", "updatedAt": "2026-10-02T10:15:30Z", "url": format!("https://github.com/octo/hello/pull/{number}"), "reviewDecision": "APPROVED",
        "body": "Retries the upload.", "mergedAt": null, "closedAt": null, "mergeable": "MERGEABLE", "additions": 4, "deletions": 1, "changedFiles": 2, "reviewRequests": [], "assignees": [],
        "headRefOid": "0123456789abcdef0123456789abcdef01234567",
    })
}

fn checks_json() -> String {
    json!([
        {"name": "build", "state": "SUCCESS", "bucket": "pass", "workflow": "CI", "link": "https://github.com/octo/hello/actions/runs/1/job/2", "startedAt": "2026-10-02T10:00:00Z", "completedAt": "2026-10-02T10:05:00Z"},
        {"name": "lint", "state": "FAILURE", "bucket": "fail", "workflow": "CI", "link": "https://github.com/octo/hello/actions/runs/1/job/3", "startedAt": "2026-10-02T10:00:00Z", "completedAt": "2026-10-02T10:04:00Z"},
    ])
    .to_string()
}

/// One call of the trace, served, with every agent method answered and every executor action run.
fn served(case: &mut Case, phone: &mut Phone, chain: &Chain, method: &str, params: Value, agent: &[&str], actions: &[&str]) -> Value {
    let reply = phone.call(case, chain, &[&format!("rpc:{method}")], method, with_repo(params));
    for m in agent {
        answered(case, m, true);
    }
    for a in actions {
        assert!(ran(case, a), "{method} did not run {a}: {}", chain.evidence());
    }
    reply["result"].clone()
}

/// The starts of `program` whose arguments begin with `prefix`.
fn starts(chain: &Chain, program: &str, prefix: &[&str]) -> Vec<exec_testkit::hosted::Start> {
    chain.clis().starts(program).into_iter().filter(|s| s.argv.iter().map(String::as_str).take(prefix.len()).eq(prefix.iter().copied())).collect()
}

/// The arguments of each start.
fn argvs(started: &[exec_testkit::hosted::Start]) -> Vec<Vec<&str>> {
    started.iter().map(|s| s.argv.iter().map(String::as_str).collect()).collect()
}

fn git_in(dir: &Path, env: &[(String, String)], args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).env_clear().envs(env.iter().cloned()).env("PATH", std::env::var_os("PATH").unwrap_or_default()).output().expect("git starts");
    assert!(out.status.success(), "git {args:?} in {}: {}", dir.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

#[test]
fn a_branchs_pull_request_and_its_checks_are_read_through_gh_and_each_pull_request_change_is_what_gh_was_asked_to_do() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository whose gh knows an open pull request for the branch, an older merged one and a draft, and a paired phone
        let chain = Chain::start_hosted();
        let mut phone = Phone::connect(case, &chain);
        let list = json!([gh_pr(12, "Add retry to uploader", "OPEN", false, "feat/retry", "main"), gh_pr(7, "Older attempt", "MERGED", false, "feat/retry", "main")]).to_string();
        chain.clis().rules(
            &[
                rule("gh", &["pr", "list"], &list),
                rule("gh", &["pr", "view", "12"], &gh_pr(12, "Add retry to uploader", "OPEN", false, "feat/retry", "main").to_string()),
                Rule { exit_code: 0, ..rule("gh", &["pr", "checks", "12"], &checks_json()) },
                rule("gh", &["pr", "merge", "12"], ""),
                rule("gh", &["pr", "edit", "12"], "https://github.com/octo/hello/pull/12\n"),
                rule("gh", &["pr", "ready", "12"], ""),
            ],
            &[],
        );
        // When the phone asks for the branch's pull request
        let info = served(case, &mut phone, &chain, "github.prForBranch", json!({"branch": "feat/retry"}), &["pr.list", "pr.view", "pr.checks"], &["hosted_pr_list", "hosted_pr_view", "hosted_pr_checks"]);
        // Then it is the open one with what gh printed of it: its state, its failing check rolled up, its approval and its time
        assert_eq!((info["number"].clone(), info["state"].clone(), info["checksStatus"].clone(), info["reviewDecision"].clone()), (json!(12), json!("open"), json!("failure"), json!("APPROVED")));
        assert_eq!((info["headSha"].clone(), info["baseRefName"].clone(), info["updatedAt"].clone()), (json!("0123456789abcdef0123456789abcdef01234567"), json!("main"), json!("2026-10-02T10:15:30.000Z")));
        // And gh was asked for the branch's pull requests of every state, by the head the phone named
        let listed = starts(&chain, "gh", &["pr", "list"]);
        assert!(listed[0].argv.iter().any(|a| a == "--head=feat/retry") && listed[0].argv.iter().any(|a| a == "--state=all"), "{:?}", listed[0].argv);
        // When the phone reads the checks
        let rows = served(case, &mut phone, &chain, "github.prChecks", json!({"prNumber": 12}), &["pr.checks"], &["hosted_pr_checks"]);
        // Then they are the runs gh printed, the failed one failed
        let shown: Vec<(String, String, Value)> = rows.as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().into(), c["status"].as_str().unwrap().into(), c["conclusion"].clone())).collect();
        assert_eq!(shown, vec![("build".to_string(), "completed".to_string(), json!("success")), ("lint".to_string(), "completed".to_string(), json!("failure"))]);
        // When it merges, retitles, updates the title and the body, marks it ready and back to a draft, and asks for and drops reviewers, each call is answered with the host's ok
        // and the one gh start it caused is what it was asked to do
        let mut call = |method: &str, params: Value, agent: &[&str], actions: &[&str]| -> (Value, Vec<exec_testkit::hosted::Start>) {
            let before = chain.clis().starts("gh").len();
            let reply = served(case, &mut phone, &chain, method, params, agent, actions);
            (reply, chain.clis().starts("gh").split_off(before))
        };
        let (merged, started) = call("github.mergePR", json!({"prNumber": 12}), &["pr.merge"], &["hosted_pr_merge"]);
        assert_eq!((merged, argvs(&started)), (json!({"ok": true}), vec![vec!["pr", "merge", "12", "--squash"]]));
        let (retitled, started) = call("github.updatePRTitle", json!({"prNumber": 12, "title": "--web retitled"}), &["pr.edit"], &["hosted_pr_edit"]);
        assert_eq!(retitled, json!(true));
        assert!(started.len() == 1 && started[0].argv.contains(&"--title=--web retitled".to_string()), "{started:?}");
        let body = "Retries the upload.\n--delete-branch";
        let (updated, started) = call("github.updatePR", json!({"prNumber": 12, "updates": {"title": "Retry uploads", "body": body}}), &["pr.edit"], &["hosted_pr_edit"]);
        assert_eq!(updated, json!({"ok": true}));
        // Then the title is one option value, never an option of its own, and the body went on stdin
        assert!(started.len() == 1 && started[0].argv.contains(&"--title=Retry uploads".to_string()) && started[0].argv.contains(&"--body-file=-".to_string()), "{started:?}");
        assert_eq!(started[0].stdin, body);
        assert!(!started[0].argv.iter().any(|a| a.contains("delete-branch")));
        // And the ready mark and its undo differ only by `--undo`, and the reviewers added and removed by their option
        let (ready, started) = call("github.markPRReadyForReview", json!({"prNumber": 12}), &["pr.ready"], &["hosted_pr_ready"]);
        assert_eq!((ready, argvs(&started)), (json!({"ok": true}), vec![vec!["pr", "ready", "12"]]));
        let (draft, started) = call("github.markPRDraft", json!({"prNumber": 12}), &["pr.ready"], &["hosted_pr_ready"]);
        assert_eq!((draft, argvs(&started)), (json!({"ok": true}), vec![vec!["pr", "ready", "12", "--undo"]]));
        let (added, started) = call("github.requestPRReviewers", json!({"prNumber": 12, "reviewers": ["hubot", "org/core"]}), &["pr.requestReviewers"], &["hosted_pr_request_reviewers"]);
        assert_eq!(added, json!({"ok": true}));
        assert!(started.len() == 1 && started[0].argv.contains(&"--add-reviewer=hubot".to_string()) && started[0].argv.contains(&"--add-reviewer=org/core".to_string()) && !started[0].argv.iter().any(|a| a.starts_with("--remove-reviewer")), "{started:?}");
        let (removed, started) = call("github.removePRReviewers", json!({"prNumber": 12, "reviewers": ["octocat"]}), &["pr.requestReviewers"], &["hosted_pr_request_reviewers"]);
        assert_eq!(removed, json!({"ok": true}));
        assert!(started.len() == 1 && started[0].argv.contains(&"--remove-reviewer=octocat".to_string()) && !started[0].argv.iter().any(|a| a.starts_with("--add-reviewer")), "{started:?}");
    });
}

#[test]
fn a_merge_gh_refuses_is_the_hosts_failed_answer_with_gh_s_words_after_the_request_reached_the_executor() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that refuses the merge
        let chain = Chain::start_hosted();
        let mut phone = Phone::connect(case, &chain);
        chain.clis().rules(&[failing("gh", &["pr", "merge", "12"], "GraphQL: Pull request is not mergeable (mergePullRequest)")], &[]);
        // When the phone merges, Then the answer is `{ok: false}` with gh's words, from a merge the executor ran
        let reply = phone.refused(case, &chain, &["rpc:github.mergePR"], "github.mergePR", with_repo(json!({"prNumber": 12, "method": "rebase"})));
        assert_eq!(reply["result"]["ok"], json!(false), "{reply}");
        assert!(reply["result"]["error"].as_str().unwrap().contains("not mergeable"), "{reply}");
        answered(case, "pr.merge", false);
        assert_eq!(starts(&chain, "gh", &["pr", "merge"])[0].argv, vec!["pr", "merge", "12", "--rebase"]);
    });
}

#[test]
fn a_merge_requests_merge_and_update_are_what_glab_was_asked_to_do_and_a_rebase_merge_is_refused_before_glab_starts() {
    exec_testkit::case!(SUITE, |case| {
        // Given a glab that takes the merge and the update
        let chain = Chain::start_hosted();
        let mut phone = Phone::connect(case, &chain);
        chain.clis().rules(&[rule("glab", &["api", "--method=PUT"], "{\"iid\": 4, \"state\": \"merged\"}"), rule("glab", &["mr", "update", "4"], "- Updating merge request !4\n")], &[]);
        // When the phone updates the merge request's title, labels and ready mark, and then merges it
        let done = served(case, &mut phone, &chain, "gitlab.updateMR", json!({"iid": 4, "updates": {"title": "Retry uploads", "addLabels": ["bug"], "removeLabels": ["wip"], "readyForReview": true}}), &["mr.edit", "mr.setLabels", "mr.ready"], &["hosted_pr_edit", "hosted_pr_labels", "hosted_pr_ready"]);
        assert_eq!(done, json!({"ok": true}));
        assert_eq!(served(case, &mut phone, &chain, "gitlab.mergeMR", json!({"iid": 4, "method": "squash"}), &["mr.merge"], &["hosted_pr_merge"]), json!({"ok": true}));
        // Then glab was started to update the title, to relabel and to mark ready, each as options of `mr update`
        let updates: Vec<Vec<String>> = starts(&chain, "glab", &["mr", "update"]).into_iter().map(|s| s.argv).collect();
        assert!(updates.iter().any(|a| a.contains(&"--title=Retry uploads".to_string())), "{updates:?}");
        assert!(updates.iter().any(|a| a.contains(&"--label=bug".to_string()) && a.contains(&"--unlabel=wip".to_string())), "{updates:?}");
        assert!(updates.iter().any(|a| a.contains(&"--ready".to_string())), "{updates:?}");
        // And the merge was a squash through the REST call, its options in the JSON on stdin
        let merge = starts(&chain, "glab", &["api"]);
        assert_eq!(merge.len(), 1, "{merge:?}");
        let sent: Value = serde_json::from_str(&merge[0].stdin).unwrap();
        assert_eq!(sent["squash"], json!(true));
        // When the phone merges by rebase, which GitLab does not do
        let before = chain.clis().starts("glab").len();
        let refused = phone.refused(case, &chain, &["rpc:gitlab.mergeMR"], "gitlab.mergeMR", with_repo(json!({"iid": 4, "method": "rebase"})));
        // Then the answer is `{ok: false}` and glab was not started again
        assert_eq!(refused["result"]["ok"], json!(false), "{refused}");
        assert_eq!(chain.clis().starts("glab").len(), before);
    });
}

#[test]
fn a_pull_requests_worktree_starts_at_its_head_fetched_into_a_ref_of_its_own_and_no_branch_or_checkout_moves() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository whose remote holds main and a pull request's head ref another clone pushed, and a gh that reads the pull request
        let chain = Chain::start_hosted();
        let mut phone = Phone::connect(case, &chain);
        let repo = &chain.repo;
        let env = repo.git_env();
        let bare = tempfile::Builder::new().prefix("orca-chain-remote-").tempdir().unwrap();
        git_in(bare.path(), &env, &["init", "--quiet", "--bare", "--initial-branch=main"]);
        repo.git(&["remote", "add", "origin", bare.path().to_str().unwrap()]);
        repo.git(&["push", "--quiet", "origin", "main"]);
        let other = tempfile::Builder::new().prefix("orca-chain-other-").tempdir().unwrap();
        let work = other.path().join("work");
        git_in(other.path(), &env, &["clone", "--quiet", bare.path().to_str().unwrap(), "work"]);
        std::fs::write(work.join("retry.txt"), "the pull request's change\n").unwrap();
        git_in(&work, &env, &["add", "--all"]);
        git_in(&work, &env, &["commit", "--quiet", "--message", "retry"]);
        let head = git_in(&work, &env, &["rev-parse", "HEAD"]);
        git_in(&work, &env, &["push", "--quiet", "origin", "HEAD:refs/pull/12/head"]);
        chain.clis().rules(&[rule("gh", &["pr", "view", "12"], &gh_pr(12, "Add retry", "OPEN", false, "feat/retry", "main").to_string())], &[]);
        let (branch_before, head_before, branches_before) = (repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), repo.head(), repo.git(&["branch", "--list"]));
        // When the phone resolves where a worktree for the pull request starts
        let start = served(case, &mut phone, &chain, "worktree.resolvePrBase", json!({"prNumber": 12}), &["pr.view", "git.fetch"], &["hosted_pr_view", "git_fetch"]);
        // Then the answer is the pull request's head commit, the repository holds it under a ref of its own, and the target branch is the compare base
        assert_eq!((start["baseBranch"].clone(), start["headSha"].clone(), start["compareBaseRef"].clone()), (json!(head), json!(head), json!("refs/rca/pr/12/base")), "{start}");
        assert_eq!(repo.git(&["rev-parse", "refs/rca/pr/12/head"]), head);
        assert_eq!(repo.git(&["rev-parse", "refs/rca/pr/12/base"]), repo.git(&["rev-parse", "main"]));
        // And no branch, no checkout and no file of the repository moved
        assert_eq!((repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), repo.head(), repo.git(&["branch", "--list"])), (branch_before, head_before, branches_before));
        assert!(!repo.path().join("retry.txt").exists());
        // When the phone makes the worktree from that start
        let made = phone.call(case, &chain, &["rpc:worktree.create"], "worktree.create", json!({"repo": SELECTOR, "name": "pr 12", "baseBranch": start["baseBranch"]}));
        // Then the new worktree holds the pull request's change
        let path = made["result"]["worktree"]["path"].as_str().expect("a worktree path");
        assert_eq!(std::fs::read_to_string(Path::new(path).join("retry.txt")).unwrap(), "the pull request's change\n", "{}", chain.evidence());
        // When the phone resolves the start for a merge request whose head ref the remote also holds, through a glab that reads it
        git_in(&work, &env, &["push", "--quiet", "origin", "HEAD:refs/merge-requests/4/head"]);
        let mr = "{\"id\": 104, \"iid\": 4, \"title\": \"Add retry\", \"state\": \"opened\", \"draft\": false, \"author\": {\"username\": \"tanuki\"}, \"source_branch\": \"feat/retry\", \"target_branch\": \"main\", \"labels\": [], \"web_url\": \"https://gitlab.com/grp/proj/-/merge_requests/4\", \"created_at\": \"2026-10-01T09:30:00.000Z\", \"updated_at\": \"2026-10-02T10:15:30.500Z\", \"description\": \"\", \"merged_at\": null, \"closed_at\": null, \"sha\": \"0123456789abcdef0123456789abcdef01234567\", \"has_conflicts\": false, \"detailed_merge_status\": \"mergeable\"}";
        chain.clis().rules(&[rule("glab", &["api", "projects/:fullpath/merge_requests/4"], mr), rule("gh", &["pr", "view", "12"], &gh_pr(12, "Add retry", "OPEN", false, "feat/retry", "main").to_string())], &[]);
        let mr_start = served(case, &mut phone, &chain, "worktree.resolveMrBase", json!({"mrIid": 4}), &["mr.view", "git.fetch"], &["hosted_pr_view", "git_fetch"]);
        // Then it starts at the merge request's head commit with the target branch as the compare base, and claims no head sha of its own
        assert_eq!((mr_start["baseBranch"].clone(), mr_start["compareBaseRef"].clone(), mr_start.get("headSha").cloned()), (json!(head), json!("refs/rca/pr/4/base"), None), "{mr_start}");
        assert_eq!(repo.git(&["rev-parse", "refs/rca/pr/4/head"]), head);
        // When the pull request's head is gone from the remote, Then the answer is the host's fetchFailed error after rca was asked
        git_in(bare.path(), &env, &["update-ref", "-d", "refs/pull/12/head"]);
        let lost = phone.call(case, &chain, &["rpc:worktree.resolvePrBase"], "worktree.resolvePrBase", with_repo(json!({"prNumber": 12})));
        assert_eq!(lost["result"]["errorCode"], json!("fetchFailed"), "{lost}");
        answered(case, "git.fetch", false);
    });
}
