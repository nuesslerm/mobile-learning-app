//! The hosted rows through the `Host` dispatcher with pull and merge requests in memory as rca's `hosted.pr` group: each reply is read the way the
//! phone reads it, and a change is read back from the request itself. Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_hosted::testing::Hosting;
use orca_map_hosted::HostedArea;
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    hosting: Hosting,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        Workspaces::default().workspace("main", "root", Some("/srv/main")).install(&agent);
        let hosting = Hosting::default();
        hosting.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(HostedArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, hosting, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn call(&mut self, method: &str, mut params: Value) -> Value {
        self.n += 1;
        if params.get("repo").is_none() {
            params["repo"] = json!("main");
        }
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        serde_json::from_str(&self.conn.on_message(&frame)[0]).unwrap()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> (String, String) {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }

    /// What rca was asked of the hosted and git groups since the rig was made.
    fn asked_hosted(&self) -> Vec<String> {
        self.agent.requests().into_iter().map(|(m, _)| m).filter(|m| m.starts_with("pr.") || m.starts_with("mr.") || m.starts_with("git.")).collect()
    }
}

#[test]
fn given_a_pull_request_with_every_kind_of_check_when_its_checks_are_read_then_each_is_the_run_the_phone_shows() {
    // Given checks that passed, failed, are running, were skipped and were cancelled
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main").checks("github", 12, &[("build", "pass"), ("lint", "fail"), ("e2e", "pending"), ("docs", "skipped"), ("deploy", "cancel")]);
    // When the phone reads them
    let rows = rig.ok("github.prChecks", json!({"prNumber": 12}));
    // Then each is a check with the status and conclusion of its state, and the running one has no conclusion yet
    let shown: Vec<(String, String, Value)> = rows.as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_string(), c["status"].as_str().unwrap().to_string(), c["conclusion"].clone())).collect();
    assert_eq!(
        shown,
        vec![
            ("build".into(), "completed".into(), json!("success")),
            ("lint".into(), "completed".into(), json!("failure")),
            ("e2e".into(), "in_progress".into(), Value::Null),
            ("docs".into(), "completed".into(), json!("skipped")),
            ("deploy".into(), "completed".into(), json!("cancelled")),
        ]
    );
    assert_eq!(rows[0]["url"], json!("https://example.test/checks/build"));
}

#[test]
fn given_a_check_in_a_state_rca_does_not_report_when_the_checks_are_read_then_the_call_is_refused_not_shown_as_pending() {
    // Given rca answering a check in a state outside its contract
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main").checks("github", 12, &[("build", "exploded")]);
    // When the phone reads the checks
    let (code, message) = rig.err("github.prChecks", json!({"prNumber": 12}));
    // Then the call names the check and the state
    assert_eq!(code, "runtime_error");
    assert!(message.contains("build") && message.contains("exploded"), "{message}");
}

#[test]
fn given_the_branch_of_a_pull_request_when_the_phone_asks_for_its_pull_request_then_it_is_the_open_one_with_its_state_checks_and_mergeability() {
    // Given an open pull request for the branch with a failing check, a conflict and an approval, an older merged one, and a draft on another branch
    let mut rig = Rig::new();
    rig.hosting.pull_request(7, "Older attempt", "feat/retry", "main").state("github", 7, "merged");
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main").checks("github", 12, &[("build", "pass"), ("lint", "fail")]).conflicting("github", 12).approved("github", 12);
    rig.hosting.pull_request(9, "Rework cache", "wip/cache", "main").draft("github", 9);
    // When the phone asks for the branch's pull request
    let info = rig.ok("github.prForBranch", json!({"branch": "feat/retry"}));
    // Then it is the open one, with what the phone's badge shows
    assert_eq!((info["number"].clone(), info["title"].clone(), info["state"].clone()), (json!(12), json!("Add retry"), json!("open")));
    assert_eq!((info["checksStatus"].clone(), info["mergeable"].clone(), info["reviewDecision"].clone()), (json!("failure"), json!("CONFLICTING"), json!("APPROVED")));
    assert_eq!((info["baseRefName"].clone(), info["headRefName"].clone(), info["headSha"].clone()), (json!("main"), json!("feat/retry"), rig.hosting.view("github", 12)["head_sha"].clone()));
    assert_eq!(info["updatedAt"], json!("2026-10-02T10:15:30.012Z"));
    // And a draft is a draft, a branch with only a merged pull request shows it merged, and a branch with none has none
    assert_eq!(rig.ok("github.prForBranch", json!({"branch": "wip/cache"}))["state"], json!("draft"));
    rig.hosting.pull_request(5, "Old", "old/one", "main").state("github", 5, "merged");
    assert_eq!(rig.ok("github.prForBranch", json!({"branch": "old/one"}))["state"], json!("merged"));
    assert_eq!(rig.ok("github.prForBranch", json!({"branch": "nowhere"})), Value::Null);
}

#[test]
fn given_an_open_pull_request_when_merged_then_it_is_squashed_unless_the_phone_names_a_method_and_a_second_merge_is_the_hosts_failed_answer() {
    // Given two open pull requests
    let mut rig = Rig::new();
    rig.hosting.pull_request(1, "One", "a", "main").pull_request(2, "Two", "b", "main");
    // When the phone merges one without a method and the other with `rebase`
    let first = rig.ok("github.mergePR", json!({"prNumber": 1}));
    let second = rig.ok("github.mergePR", json!({"prNumber": 2, "method": "rebase"}));
    // Then both are merged, by the method each was asked for
    assert_eq!((first, second), (json!({"ok": true}), json!({"ok": true})));
    assert_eq!((rig.hosting.view("github", 1)["state"].clone(), rig.hosting.merged_with("github", 1)), (json!("merged"), Some("squash".to_string())));
    assert_eq!(rig.hosting.merged_with("github", 2), Some("rebase".to_string()));
    // And merging the merged one again is `{ok: false}` with the executor's words, not a success
    let again = rig.ok("github.mergePR", json!({"prNumber": 1}));
    assert_eq!(again["ok"], json!(false));
    assert!(again["error"].as_str().unwrap().contains("not mergeable"), "{again}");
}

#[test]
fn given_a_pull_request_when_its_title_and_body_are_updated_then_the_request_shows_them_and_an_empty_update_is_refused_before_rca_is_asked() {
    // Given a pull request
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main");
    // When the phone retitles it and then updates the title and the body
    assert_eq!(rig.ok("github.updatePRTitle", json!({"prNumber": 12, "title": "Add retry to uploader"})), json!(true));
    assert_eq!(rig.ok("github.updatePR", json!({"prNumber": 12, "updates": {"title": "Retry uploads", "body": "Retries the upload."}})), json!({"ok": true}));
    // Then the request has both
    let seen = rig.hosting.view("github", 12);
    assert_eq!((seen["title"].clone(), seen["body"].clone()), (json!("Retry uploads"), json!("Retries the upload.")));
    // And an update that changes nothing is refused before rca is asked, with the words gitlab.updateMR uses, and a retitle that is refused is an error
    let before = rig.asked_hosted().len();
    let (code, message) = rig.err("github.updatePR", json!({"prNumber": 12, "updates": {}}));
    assert_eq!((code.as_str(), message.as_str(), rig.asked_hosted().len()), ("invalid_argument", "No updates given", before));
    let (code, _) = rig.err("github.updatePRTitle", json!({"prNumber": 99, "title": "x"}));
    assert_eq!(code, "runtime_error");
}

#[test]
fn given_a_draft_when_marked_ready_and_back_then_the_request_follows_and_reviewers_are_requested_and_removed() {
    // Given a draft with one reviewer
    let mut rig = Rig::new();
    rig.hosting.pull_request(9, "Rework cache", "wip/cache", "main").draft("github", 9);
    rig.ok("github.requestPRReviewers", json!({"prNumber": 9, "reviewers": ["hubot"]}));
    // When it is marked ready and a second reviewer is requested
    rig.ok("github.markPRReadyForReview", json!({"prNumber": 9}));
    rig.ok("github.requestPRReviewers", json!({"prNumber": 9, "reviewers": [" octocat ", ""]}));
    // Then it is no draft and has both reviewers
    let seen = rig.hosting.view("github", 9);
    assert_eq!((seen["draft"].clone(), seen["reviewers"].clone()), (json!(false), json!(["hubot", "octocat"])));
    // When it goes back to a draft and one reviewer is removed
    rig.ok("github.markPRDraft", json!({"prNumber": 9}));
    rig.ok("github.removePRReviewers", json!({"prNumber": 9, "reviewers": ["hubot"]}));
    // Then it is a draft again with the other reviewer
    let seen = rig.hosting.view("github", 9);
    assert_eq!((seen["draft"].clone(), seen["reviewers"].clone()), (json!(true), json!(["octocat"])));
    // And a request naming no reviewer is refused before rca is asked
    let before = rig.asked_hosted().len();
    let (code, _) = rig.err("github.requestPRReviewers", json!({"prNumber": 9, "reviewers": []}));
    assert_eq!((code.as_str(), rig.asked_hosted().len()), ("invalid_argument", before));
}

#[test]
fn given_a_pull_request_of_a_fork_when_it_is_merged_then_the_executor_acts_on_the_repository_the_phone_named() {
    // Given a pull request, and the phone naming the fork's repository and an enterprise host
    let mut rig = Rig::new();
    rig.hosting.pull_request(3, "From a fork", "fork/change", "main");
    // When it merges it
    rig.ok("github.mergePR", json!({"prNumber": 3, "prRepo": {"owner": "octo", "repo": "hello", "host": "git.example.com"}}));
    // Then the request rca got names that repository and host, and one that names none names neither
    let sent = rig.agent.requests().into_iter().find(|(m, _)| m == "pr.merge").unwrap().1;
    assert_eq!((sent["repo"].clone(), sent["host"].clone(), sent["workspace"].clone()), (json!("octo/hello"), json!("git.example.com"), json!("main")));
    rig.hosting.pull_request(4, "Plain", "plain", "main");
    rig.ok("github.mergePR", json!({"prNumber": 4}));
    let plain = rig.agent.requests().into_iter().rfind(|(m, _)| m == "pr.merge").unwrap().1;
    assert!(plain.get("repo").is_none() && plain.get("host").is_none(), "{plain}");
}

#[test]
fn given_a_merge_request_when_merged_labelled_and_marked_ready_then_it_follows_and_a_merge_by_rebase_is_the_hosts_failed_answer() {
    // Given a draft merge request labelled wip
    let mut rig = Rig::new();
    rig.hosting.merge_request(4, "Add retry", "feat/retry", "main").draft("gitlab", 4);
    rig.ok("gitlab.updateMR", json!({"iid": 4, "updates": {"addLabels": ["wip", "p2"]}}));
    // When the phone retitles it, swaps a label and marks it ready in one update
    let done = rig.ok("gitlab.updateMR", json!({"iid": 4, "updates": {"title": "Add retry to uploader", "removeLabels": ["wip"], "addLabels": ["bug"], "readyForReview": true}}));
    // Then every part is done
    assert_eq!(done, json!({"ok": true}));
    let seen = rig.hosting.view("gitlab", 4);
    assert_eq!((seen["title"].clone(), seen["labels"].clone(), seen["draft"].clone()), (json!("Add retry to uploader"), json!(["p2", "bug"]), json!(false)));
    // When it is merged by rebase, which GitLab does not do, and then without a method
    let refused = rig.ok("gitlab.mergeMR", json!({"iid": 4, "method": "rebase"}));
    assert_eq!(refused["ok"], json!(false));
    assert_eq!(rig.hosting.merged_with("gitlab", 4), None);
    assert_eq!(rig.ok("gitlab.mergeMR", json!({"iid": 4})), json!({"ok": true}));
    // Then it is merged the executor's default way
    assert_eq!(rig.hosting.merged_with("gitlab", 4), Some("merge".to_string()));
    // And an update that names nothing is refused before rca is asked
    let before = rig.asked_hosted().len();
    let (code, message) = rig.err("gitlab.updateMR", json!({"iid": 4, "updates": {}}));
    assert_eq!((code.as_str(), message.as_str(), rig.asked_hosted().len()), ("invalid_argument", "No updates given", before));
}

#[test]
fn given_a_pull_request_when_the_start_of_its_worktree_is_resolved_then_the_head_is_fetched_into_a_ref_of_its_own_and_is_the_base() {
    // Given a pull request whose head and target branch the remote holds
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main").remote_branch("main", &"b".repeat(40));
    let head = rig.hosting.view("github", 12)["head_sha"].as_str().unwrap().to_string();
    // When the phone resolves the start of a worktree for it
    let start = rig.ok("worktree.resolvePrBase", json!({"prNumber": 12, "headRefName": "ignored", "isCrossRepository": true}));
    // Then the worktree starts at the head's commit, the target branch is the compare base, and both are refs the fetch made
    assert_eq!((start["baseBranch"].clone(), start["headSha"].clone(), start["compareBaseRef"].clone()), (json!(head), json!(head), json!("refs/rca/pr/12/base")));
    assert_eq!((rig.hosting.local_ref("refs/rca/pr/12/head"), rig.hosting.local_ref("refs/rca/pr/12/base")), (Some(head), Some("b".repeat(40))));
}

#[test]
fn given_a_merge_request_when_the_start_of_its_worktree_is_resolved_then_the_head_is_fetched_from_its_merge_request_ref_and_no_head_sha_is_claimed() {
    // Given a merge request whose head and target branch the remote holds
    let mut rig = Rig::new();
    rig.hosting.merge_request(4, "Add retry", "feat/retry", "dev").remote_branch("dev", &"c".repeat(40));
    // When the phone resolves the start of a worktree for it
    let start = rig.ok("worktree.resolveMrBase", json!({"mrIid": 4}));
    // Then the base is the head's commit and the compare base the target branch's ref
    let head = rig.hosting.view("gitlab", 4)["head_sha"].clone();
    assert_eq!((start["baseBranch"].clone(), start["compareBaseRef"].clone(), start.get("headSha").cloned()), (head, json!("refs/rca/pr/4/base"), None));
    assert_eq!(rig.hosting.local_ref("refs/rca/pr/4/base"), Some("c".repeat(40)));
}

#[test]
fn given_a_start_point_that_cannot_be_resolved_when_asked_then_the_answer_is_the_hosts_error_with_the_reason_after_rca_was_asked() {
    // Given a pull request whose target branch the remote lost
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "gone");
    // When the phone resolves it, Then the fetch's refusal is the host's fetchFailed answer naming the ref
    let lost = rig.ok("worktree.resolvePrBase", json!({"prNumber": 12}));
    assert_eq!(lost["errorCode"], json!("fetchFailed"));
    assert!(lost["error"].as_str().unwrap().contains("feat/retry") && lost["error"].as_str().unwrap().contains("refs/heads/gone"), "{lost}");
    assert!(rig.asked_hosted().contains(&"git.fetch".to_string()));
    // And when the provider cannot read the request, the answer carries the provider's words and no fetch is made
    rig.hosting.view_fails("gh says: could not resolve to a PullRequest");
    let before = rig.asked_hosted().iter().filter(|m| *m == "git.fetch").count();
    let unreadable = rig.ok("worktree.resolvePrBase", json!({"prNumber": 99}));
    assert_eq!(unreadable, json!({"error": "gh says: could not resolve to a PullRequest"}));
    assert_eq!(rig.asked_hosted().iter().filter(|m| *m == "git.fetch").count(), before);
}

#[test]
fn given_a_selector_no_workspace_owns_or_a_missing_number_when_a_row_is_called_then_it_is_refused_and_rca_is_asked_nothing_hosted() {
    // Given a rig
    let mut rig = Rig::new();
    for (method, params, code) in [
        ("github.mergePR", json!({"repo": "nowhere", "prNumber": 1}), "selector_not_found"),
        ("github.mergePR", json!({"prNumber": 0}), "invalid_argument"),
        ("github.prChecks", json!({}), "invalid_argument"),
        ("github.prForBranch", json!({}), "invalid_argument"),
        ("github.updatePRTitle", json!({"prNumber": 1}), "invalid_argument"),
        ("gitlab.mergeMR", json!({}), "invalid_argument"),
        ("worktree.resolvePrBase", json!({}), "invalid_argument"),
        ("worktree.resolveMrBase", json!({"prNumber": 3}), "invalid_argument"),
    ] {
        // When the phone calls it, Then it is refused as the host refuses it
        let (got, _) = rig.err(method, params);
        assert_eq!(got, code, "{method}");
    }
    assert_eq!(rig.asked_hosted(), Vec::<String>::new());
    // And a call without a repo selector at all is refused too
    let r = rig.conn.on_message(&json!({"id": "x", "method": "github.prChecks", "params": {"prNumber": 1}}).to_string());
    assert_eq!(serde_json::from_str::<Value>(&r[0]).unwrap()["error"]["message"], json!("Missing repo selector"));
}

#[test]
fn given_rca_answers_a_request_without_what_its_contract_requires_when_the_branch_pull_request_is_read_then_the_call_is_refused() {
    // Given a pull request, and rca's view answering without its head commit
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main");
    let mut view = rig.hosting.view("github", 12);
    view.as_object_mut().unwrap().remove("head_sha");
    rig.agent.reply("pr.view", move |_| Ok(json!({"pr": view.clone()})));
    // When the phone asks for the branch's pull request
    let (code, message) = rig.err("github.prForBranch", json!({"branch": "feat/retry"}));
    // Then the call names the missing field instead of answering with a blank sha
    assert_eq!(code, "runtime_error");
    assert!(message.contains("head_sha"), "{message}");
}

#[test]
fn given_pull_requests_whose_only_checks_were_cancelled_or_skipped_when_the_branch_pull_request_is_read_then_the_rollup_is_the_hosts_failure_and_success() {
    // Given one pull request with only a cancelled check, one with only a skipped check and one with a skipped and a pending check
    let mut rig = Rig::new();
    rig.hosting.pull_request(1, "Cancelled", "a", "main").checks("github", 1, &[("deploy", "cancel")]);
    rig.hosting.pull_request(2, "Skipped", "b", "main").checks("github", 2, &[("docs", "skipped")]);
    rig.hosting.pull_request(3, "Skipped and running", "c", "main").checks("github", 3, &[("docs", "skipped"), ("e2e", "pending")]);
    // When the phone reads each by its branch
    let status = |rig: &mut Rig, branch: &str| rig.ok("github.prForBranch", json!({"branch": branch}))["checksStatus"].clone();
    // Then a cancelled check fails the rollup and a skipped one passes it, as the pinned host's provider-check-summary counts them, and a running one still wins over a pass
    assert_eq!((status(&mut rig, "a"), status(&mut rig, "b"), status(&mut rig, "c")), (json!("failure"), json!("success"), json!("pending")));
}
