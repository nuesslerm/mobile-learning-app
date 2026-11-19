//! The hosted rows through the `Host` dispatcher with pull and merge requests in memory as rca's `hosted.pr` group: each reply is read the way the
//! phone reads it, and a change is read back from the request itself. Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_hosted::testing::{Extras, Hosting};
use orca_map_hosted::HostedArea;
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    hosting: Hosting,
    ws: Workspaces,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let ws = Workspaces::default();
        ws.workspace("main", "root", Some("/srv/main")).install(&agent);
        let hosting = Hosting::default();
        hosting.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(HostedArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, hosting, ws, conn: host.connection("dev", "tok", vec![]), n: 0 }
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
        self.agent.requests().into_iter().map(|(m, _)| m).filter(|m| m.starts_with("pr.") || m.starts_with("mr.") || m.starts_with("git.") || m.starts_with("sourceControl.")).collect()
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
fn given_a_repo_no_workspace_owns_when_a_start_point_is_resolved_then_the_answer_is_the_hosts_repo_not_found_result() {
    // Given a rig, When the phone resolves a pull request and a merge request of a repo the host does not hold
    let mut rig = Rig::new();
    let pr = rig.ok("worktree.resolvePrBase", json!({"repo": "nowhere", "prNumber": 1}));
    let mr = rig.ok("worktree.resolveMrBase", json!({"repo": "nowhere", "mrIid": 1}));
    // Then each is the host's result with its error code, and rca is asked nothing hosted
    assert_eq!((pr, mr), (json!({"error": "Repo not found", "errorCode": "repoNotFound"}), json!({"error": "Repo not found", "errorCode": "repoNotFound"})));
    assert_eq!(rig.asked_hosted(), Vec::<String>::new());
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

#[test]
fn given_a_repository_with_labels_when_they_are_listed_then_the_names_come_back_in_order_and_an_unreadable_repository_is_an_empty_list() {
    let mut rig = Rig::new();
    // Given labels on the repository
    rig.hosting.labels(&[("bug", "d73a4a"), ("docs", "0075ca"), ("", "ffffff")]);
    // When the phone lists them for the worktree's repository, and for one it names
    let own = rig.ok("github.listLabels", json!({}));
    let named = rig.ok("github.listLabels", json!({"ownerRepo": {"owner": "acme", "repo": "app"}}));
    // Then it reads the names only, an empty name dropped, and rca was asked for the named repository
    assert_eq!((own, named.clone()), (json!(["bug", "docs"]), json!(["bug", "docs"])));
    let asked = rig.agent.requests().into_iter().filter(|(m, _)| m == "pr.labelCandidates").map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!((asked[0]["repo"].clone(), asked[1]["repo"].clone(), asked[1]["limit"].clone()), (Value::Null, json!("acme/app"), json!(200)));
    // And a repository rca cannot read answers an empty list, as the host does, never an error
    let mut quiet = Rig::new();
    assert_eq!(quiet.ok("github.listLabels", json!({})), json!([]));
    // And a missing selector, a bad ownerRepo and an unknown workspace are the only refusals
    assert_eq!(rig.err("github.listLabels", json!({"repo": ""})).0, "invalid_argument");
    assert_eq!(rig.err("github.listLabels", json!({"ownerRepo": null})).0, "invalid_argument");
    assert_eq!(rig.err("github.listLabels", json!({"repo": "nowhere"})).0, "repo_not_found");
    // And an account expectation that is not {host, user} is refused before the repository is looked up
    let (code, message) = rig.err("github.listLabels", json!({"repo": "nowhere", "expectedGhAccount": "a"}));
    assert_eq!((code.as_str(), message.as_str()), ("invalid_argument", "Invalid GitHub account expectation"));
    assert_eq!(rig.ok("github.listLabels", json!({"expectedGhAccount": {"host": "GitHub.com", "user": "octocat"}})), json!(["bug", "docs"]));
}

#[test]
fn given_a_repository_with_assignable_accounts_when_they_are_listed_then_each_is_login_name_and_avatar_and_an_unreadable_repository_is_an_empty_list() {
    let mut rig = Rig::new();
    // Given two accounts the repository can assign, one without an avatar
    rig.hosting.assignees(&[("octocat", "https://avatars.example/1"), ("hubot", "")]);
    // When the phone lists them for the worktree's repository, and for one it names
    let own = rig.ok("github.listAssignableUsers", json!({}));
    let named = rig.ok("github.listAssignableUsers", json!({"ownerRepo": {"owner": "acme", "repo": "app"}}));
    // Then each is the host's {login, name, avatarUrl}, a missing avatar the empty string, and rca was asked for the named repository
    let expected = json!([{"login": "octocat", "name": null, "avatarUrl": "https://avatars.example/1"}, {"login": "hubot", "name": null, "avatarUrl": ""}]);
    assert_eq!((own, named), (expected.clone(), expected));
    let asked = rig.agent.requests().into_iter().filter(|(m, _)| m == "pr.reviewerCandidates").map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!((asked[0]["repo"].clone(), asked[1]["repo"].clone(), asked[1]["limit"].clone()), (Value::Null, json!("acme/app"), json!(100)));
    // And a repository rca cannot read answers an empty list, as the host does
    assert_eq!(Rig::new().ok("github.listAssignableUsers", json!({})), json!([]));
    // And the selector checks are those of the label list
    assert_eq!(rig.err("github.listAssignableUsers", json!({"ownerRepo": null})).0, "invalid_argument");
    assert_eq!(rig.err("github.listAssignableUsers", json!({"repo": "nowhere"})).0, "repo_not_found");
    assert_eq!(rig.err("github.listAssignableUsers", json!({"expectedGhAccount": "a"})).1, "Invalid GitHub account expectation");
}

#[test]
fn given_a_review_comment_when_the_phone_replies_to_it_by_its_numeric_id_then_the_answer_is_the_new_comment_as_the_host_renders_it() {
    let mut rig = Rig::new();
    // When the phone replies to review comment 9003 of pull request 12, naming the thread and the line it is on
    let r = rig.ok("github.addPRReviewCommentReply", json!({"prNumber": 12, "commentId": 9003, "body": "Fixed", "threadId": "PRRT_1", "path": "src/lib.rs", "line": 7}));
    // Then the new comment is the host's row: its numeric id, its node id for reactions, and the phone's path, line and thread where GitHub gave none
    assert_eq!(r, json!({"ok": true, "comment": {"id": 9004, "author": "octocat", "authorAvatarUrl": "https://avatars.example/1", "body": "Fixed", "createdAt": "2026-11-19T02:00:00Z", "url": "https://github.com/acme/app/pull/12#discussion_r9004", "isBot": false, "reactionSubjectId": "PRRC_kwDOAB", "path": "src/lib.rs", "line": 7, "threadId": "PRRT_1"}}));
    // And rca was asked by the comment's id, not a thread
    let asked = rig.agent.requests().into_iter().find(|(m, _)| m == "pr.reply").unwrap().1;
    assert_eq!((asked["number"].clone(), asked["comment_id"].clone(), asked.get("thread").cloned()), (json!(12), json!(9003), None));
    // And a comment GitHub does not know is the host's classified failure, not an error
    assert_eq!(rig.ok("github.addPRReviewCommentReply", json!({"prNumber": 12, "commentId": 1, "body": "x"})), json!({"ok": false, "error": "Issue not found — it may have been deleted.", "errorCode": "issueNotFound"}));
    // And a missing selector, comment id or body is refused
    assert_eq!(rig.err("github.addPRReviewCommentReply", json!({"repo": "", "prNumber": 12, "commentId": 1, "body": "x"})).1, "Missing repo selector");
    assert_eq!(rig.err("github.addPRReviewCommentReply", json!({"prNumber": 12, "body": "x"})).0, "invalid_argument");
    assert_eq!(rig.err("github.addPRReviewCommentReply", json!({"prNumber": 12, "commentId": 1})).0, "invalid_argument");
}

#[test]
fn given_a_line_of_a_pull_request_file_when_the_phone_comments_on_it_then_the_answer_is_the_new_review_comment_as_the_host_renders_it() {
    // Given pull request 12 of a GitHub folder whose head commit is abc1234
    let mut rig = Rig::new();
    let comment = |extra: Value| {
        let mut p = json!({"prNumber": 12, "commitId": "abc1234", "path": "src/lib.rs", "line": 9, "body": "Why?"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    // When the phone comments on lines 7 to 9 of src/lib.rs at the head commit of pull request 12
    let r = rig.ok("github.addPRReviewComment", comment(json!({"startLine": 7})));
    // Then the answer is the host's new comment row: the numeric id, the node id for reactions, the path, the line and the range's start
    assert_eq!(r, json!({"ok": true, "comment": {"id": 9100, "author": "octocat", "authorAvatarUrl": "https://avatars.example/1", "body": "Why?", "createdAt": "2026-11-19T03:00:00Z", "url": "https://github.com/acme/app/pull/12#discussion_r9100", "isBot": false, "reactionSubjectId": "PRRC_kwDOAC", "path": "src/lib.rs", "line": 9, "startLine": 7}}));
    // And rca was asked for a comment on that range of that file at that commit
    let asked = |rig: &Rig| {
        let mut p = rig.agent.requests().into_iter().rfind(|(m, _)| m == "pr.reviewComment").unwrap().1;
        p.as_object_mut().unwrap().remove("workspace");
        p
    };
    assert_eq!(asked(&rig), json!({"number": 12, "commit_id": "abc1234", "path": "src/lib.rs", "line": 9, "body": "Why?", "start_line": 7}));

    // When the range starts on its own line
    let one = rig.ok("github.addPRReviewComment", comment(json!({"startLine": 9})));
    // Then rca is asked for a one-line comment, and the answer still carries the start the phone gave
    assert_eq!(asked(&rig), json!({"number": 12, "commit_id": "abc1234", "path": "src/lib.rs", "line": 9, "body": "Why?"}));
    assert_eq!(one, json!({"ok": true, "comment": {"id": 9100, "author": "octocat", "authorAvatarUrl": "https://avatars.example/1", "body": "Why?", "createdAt": "2026-11-19T03:00:00Z", "url": "https://github.com/acme/app/pull/12#discussion_r9100", "isBot": false, "reactionSubjectId": "PRRC_kwDOAC", "path": "src/lib.rs", "line": 9, "startLine": 9}}));

    // When GitHub refuses the line, or the folder has no GitHub remote, or GitHub answers no usable id
    // Then each is the host's failure answer, not an error
    assert_eq!(rig.ok("github.addPRReviewComment", comment(json!({"commitId": "def0"}))), json!({"ok": false, "error": "Invalid update — gh: Validation Failed (HTTP 422)", "errorCode": "invalidUpdate", "errorDetail": "gh: Validation Failed (HTTP 422)"}));
    assert_eq!(rig.ok("github.addPRReviewComment", comment(json!({"commitId": "dead"}))), json!({"ok": false, "error": "Could not resolve GitHub owner/repo for this repository", "errorCode": "github.repoUnresolved"}));
    assert_eq!(rig.ok("github.addPRReviewComment", comment(json!({"commitId": "0"}))), json!({"ok": false, "error": "Unexpected response from GitHub", "errorCode": "github.unexpectedResponse"}));

    // When a field breaks the host's schema, Then the refusal is zod's first issue in the schema's order, before the selector is resolved
    let refused = |rig: &mut Rig, p: Value| rig.err("github.addPRReviewComment", p);
    assert_eq!(refused(&mut rig, comment(json!({"repo": ""}))), ("invalid_argument".into(), "Missing repo selector".into()));
    assert_eq!(refused(&mut rig, comment(json!({"repo": "nope"}))), ("repo_not_found".into(), "repo_not_found".into()));
    let mut no_commit = comment(json!({"repo": "nope"}));
    no_commit.as_object_mut().unwrap().remove("commitId");
    let invalid = |m: &str| ("invalid_argument".to_string(), m.to_string());
    assert_eq!(refused(&mut rig, no_commit), invalid("Missing PR head SHA"));
    assert_eq!(refused(&mut rig, comment(json!({"path": ""}))), invalid("File path required"));
    assert_eq!(refused(&mut rig, comment(json!({"line": 0}))), invalid("Too small: expected number to be >0"));
    assert_eq!(refused(&mut rig, comment(json!({"line": 1.5}))), invalid("Invalid input: expected int, received number"));
    assert_eq!(refused(&mut rig, comment(json!({"startLine": null}))), invalid("Invalid input: expected number, received null"));
    assert_eq!(refused(&mut rig, comment(json!({"body": 7}))), invalid("Comment body required"));
    assert_eq!(refused(&mut rig, comment(json!({"prNumber": "1", "body": ""}))), invalid("Invalid input: expected number, received string"));
    assert_eq!(refused(&mut rig, comment(json!({"prRepo": {"owner": "acme"}}))), invalid("Missing repo"));
    // And a number beyond JavaScript's safe integers is refused by the bound zod's int() carries, before its sign is checked
    assert_eq!(refused(&mut rig, comment(json!({"prNumber": 9007199254740992u64}))), invalid("Too big: expected int to be <=9007199254740991"));
    assert_eq!(refused(&mut rig, comment(json!({"line": -1e20}))), invalid("Too small: expected int to be >=-9007199254740991"));
    assert_eq!(refused(&mut rig, comment(json!({"startLine": 1e20}))), invalid("Too big: expected int to be <=9007199254740991"));
    // And a key outside the schema is stripped, never refused
    assert_eq!(rig.ok("github.addPRReviewComment", comment(json!({"ownerRepo": 7}))), json!({"ok": true, "comment": {"id": 9100, "author": "octocat", "authorAvatarUrl": "https://avatars.example/1", "body": "Why?", "createdAt": "2026-11-19T03:00:00Z", "url": "https://github.com/acme/app/pull/12#discussion_r9100", "isBot": false, "reactionSubjectId": "PRRC_kwDOAC", "path": "src/lib.rs", "line": 9}}));
}

#[test]
fn given_a_pull_and_a_merge_request_when_the_phone_closes_or_reopens_them_then_rca_sets_the_state_and_failures_are_the_hosts() {
    let mut rig = Rig::new();
    // When the phone closes pull request 12 and reopens merge request 12
    let closed = rig.ok("github.updatePRState", json!({"prNumber": 12, "updates": {"state": "closed"}}));
    let reopened = rig.ok("gitlab.updateMRState", json!({"iid": 12, "state": "opened"}));
    // Then both are done, and rca was asked for the closed and the open state
    assert_eq!((closed, reopened), (json!({"ok": true}), json!({"ok": true})));
    let asked = |m: &str| rig.agent.requests().into_iter().find(|(x, _)| x == m).unwrap().1;
    assert_eq!((asked("pr.setState")["state"].clone(), asked("mr.setState")["state"].clone()), (json!("closed"), json!("open")));
    // And an unknown pull request is the host's not-found failure, a folder with no GitLab project its unresolved one
    assert_eq!(rig.ok("github.updatePRState", json!({"prNumber": 5, "updates": {"state": "open"}})), json!({"ok": false, "error": "Pull request not found — it may have been deleted.", "errorCode": "github.pullRequestNotFound"}));
    assert_eq!(rig.ok("gitlab.updateMRState", json!({"iid": 99, "state": "closed"})), json!({"ok": false, "error": "Could not resolve GitLab project for this repository", "errorCode": "gitlab.projectUnresolved"}));
    // And a state the host does not know is refused
    assert_eq!(rig.err("github.updatePRState", json!({"prNumber": 12, "updates": {"state": "merged"}})).0, "invalid_argument");
    assert_eq!(rig.err("gitlab.updateMRState", json!({"iid": 12, "state": "open"})).0, "invalid_argument");
}

#[test]
fn given_a_merge_request_when_the_phone_comments_on_it_then_the_answer_is_the_new_note_as_the_host_renders_it() {
    // Given a workspace whose merge request 12 GitLab can take a note on
    let mut rig = Rig::new();
    // When the phone comments on it, and again for a project it names on another GitLab host
    let posted = rig.ok("gitlab.addMRComment", json!({"iid": 12, "body": "Looks good"}));
    rig.ok("gitlab.addMRComment", json!({"iid": 12, "body": "Again", "projectRef": {"host": "gitlab.example.com", "path": "grp/proj"}}));
    // Then the answer is the whole new note as the host renders it
    assert_eq!(posted, json!({"ok": true, "comment": {"id": 88, "author": "tanuki", "authorAvatarUrl": "https://gitlab.example/a.png", "body": "Looks good", "createdAt": "2026-11-19T02:00:00.000Z", "url": "", "isBot": false}}));
    // And rca was asked for the note on that number with the body, the second for the named project and host
    let asked = rig.agent.requests().into_iter().filter(|(m, _)| m == "mr.comment").map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!(asked[0], json!({"workspace": "main", "number": 12, "body": "Looks good"}));
    assert_eq!(asked[1], json!({"workspace": "main", "number": 12, "body": "Again", "repo": "grp/proj", "host": "gitlab.example.com"}));
    // And a folder with no GitLab project is the host's unresolved failure, any other glab failure its words
    assert_eq!(rig.ok("gitlab.addMRComment", json!({"iid": 99, "body": "x"})), json!({"ok": false, "error": "Could not resolve GitLab project for this repository", "errorCode": "gitlab.projectUnresolved"}));
    assert_eq!(rig.ok("gitlab.addMRComment", json!({"iid": 5, "body": "x"})), json!({"ok": false, "error": "glab: 404 Not Found"}));
    // And the host's schema refuses a bad request in key order, before the repository is resolved
    let refused = |rig: &mut Rig, params: Value| rig.err("gitlab.addMRComment", params);
    assert_eq!(refused(&mut rig, json!({"repo": "", "iid": 12, "body": "x"})), ("invalid_argument".into(), "Missing repo selector".into()));
    assert_eq!(refused(&mut rig, json!({"repo": "nope", "iid": 12, "body": "x"})), ("repo_not_found".into(), "repo_not_found".into()));
    assert_eq!(refused(&mut rig, json!({"iid": 0, "body": "x"})), ("invalid_argument".into(), "Too small: expected number to be >0".into()));
    assert_eq!(refused(&mut rig, json!({"iid": "3", "body": "x"})), ("invalid_argument".into(), "Invalid input: expected number, received string".into()));
    assert_eq!(refused(&mut rig, json!({"iid": 12})), ("invalid_argument".into(), "Comment body is required".into()));
    assert_eq!(refused(&mut rig, json!({"iid": 12, "body": "x", "projectRef": {"host": "h"}})), ("invalid_argument".into(), "Missing GitLab project path".into()));
    assert_eq!(refused(&mut rig, json!({"repo": "nope", "iid": 0, "body": "x"})), ("invalid_argument".into(), "Too small: expected number to be >0".into()));
    assert_eq!(rig.agent.requests().into_iter().filter(|(m, _)| m == "mr.comment").count(), 4);
}

#[test]
fn given_review_threads_when_one_is_resolved_or_reopened_then_the_answer_is_whether_github_did_and_a_failure_is_false() {
    let mut rig = Rig::new();
    // When the phone resolves a thread by its node id, and reopens it for a named repository
    let resolved = rig.ok("github.resolveReviewThread", json!({"threadId": "PRRT_1", "resolve": true}));
    let reopened = rig.ok("github.resolveReviewThread", json!({"threadId": "PRRT_1", "resolve": false, "prRepo": {"owner": "acme", "repo": "app"}}));
    // Then both are true, and rca was asked with the thread alone (no number), the second for the named repository
    assert_eq!((resolved, reopened), (json!(true), json!(true)));
    let asked = rig.agent.requests().into_iter().filter(|(m, _)| m == "pr.resolveThread").map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!((asked[0]["thread"].clone(), asked[0].get("number").cloned(), asked[1]["resolved"].clone(), asked[1]["repo"].clone()), (json!("PRRT_1"), None, json!(false), json!("acme/app")));
    // And a thread GitHub cannot resolve answers false, as the host does
    assert_eq!(rig.ok("github.resolveReviewThread", json!({"threadId": "nope", "resolve": true})), json!(false));
    // And a missing selector or thread is refused, an unknown repository is repo_not_found
    assert_eq!(rig.err("github.resolveReviewThread", json!({"repo": "", "threadId": "PRRT_1"})).1, "Missing repo selector");
    assert_eq!(rig.err("github.resolveReviewThread", json!({"resolve": true})).0, "invalid_argument");
    assert_eq!(rig.err("github.resolveReviewThread", json!({"repo": "nowhere", "threadId": "PRRT_1"})).0, "repo_not_found");
}

#[test]
fn given_a_pull_request_file_when_it_is_marked_or_unmarked_viewed_then_the_answer_is_whether_github_did_and_a_failure_is_false() {
    // Given a workspace whose pull request GitHub names by its node id
    let mut rig = Rig::new();
    // When the phone marks a file viewed, and unmarks it for a repository it names on an Enterprise host
    let marked = rig.ok("github.setPRFileViewed", json!({"pullRequestId": "PR_1", "path": "src/a b.rs", "viewed": true}));
    let unmarked = rig.ok("github.setPRFileViewed", json!({"pullRequestId": "PR_1", "path": "src/a b.rs", "viewed": false, "prRepo": {"owner": "acme", "repo": "app", "host": "git.acme.dev"}, "ownerRepo": "ignored"}));
    // Then both answer true, and rca was asked by the node id and path, the second for the named repository and host
    assert_eq!((marked, unmarked), (json!(true), json!(true)));
    let asked = rig.agent.requests().into_iter().filter(|(m, _)| m == "pr.fileViewed").map(|(_, p)| p).collect::<Vec<_>>();
    assert_eq!(asked, vec![json!({"pull_request": "PR_1", "path": "src/a b.rs", "viewed": true, "workspace": "main"}), json!({"pull_request": "PR_1", "path": "src/a b.rs", "viewed": false, "repo": "acme/app", "host": "git.acme.dev", "workspace": "main"})]);
    // And a pull request GitHub cannot resolve answers false, as the host does
    assert_eq!(rig.ok("github.setPRFileViewed", json!({"pullRequestId": "nope", "path": "a", "viewed": true})), json!(false));
    // When a field is missing or of the wrong kind, Then it is refused with the host's words in its schema order, before rca is asked
    let before = rig.asked_hosted().len();
    let refused = |rig: &mut Rig, params: Value| rig.err("github.setPRFileViewed", params);
    let invalid = |m: &str| ("invalid_argument".to_string(), m.to_string());
    assert_eq!(refused(&mut rig, json!({"repo": "", "pullRequestId": "PR_1", "path": "a", "viewed": true})), invalid("Missing repo selector"));
    assert_eq!(refused(&mut rig, json!({"repo": 7, "pullRequestId": "PR_1", "path": "a", "viewed": true})), invalid("Missing repo selector"));
    assert_eq!(refused(&mut rig, json!({"expectedGhAccount": "a", "prRepo": 1, "pullRequestId": "PR_1", "path": "a", "viewed": true})), invalid("Invalid GitHub account expectation"));
    assert_eq!(refused(&mut rig, json!({"prRepo": "acme/app", "pullRequestId": "PR_1", "path": "a", "viewed": true})), invalid("Invalid input: expected object, received string"));
    assert_eq!(refused(&mut rig, json!({"prRepo": {"repo": "app"}, "pullRequestId": "PR_1", "path": "a", "viewed": true})), invalid("Missing owner"));
    assert_eq!(refused(&mut rig, json!({"prRepo": {"owner": "acme"}, "pullRequestId": "PR_1", "path": "a", "viewed": true})), invalid("Missing repo"));
    assert_eq!(refused(&mut rig, json!({"path": "a", "viewed": true})), invalid("Missing pull request ID"));
    assert_eq!(refused(&mut rig, json!({"pullRequestId": "PR_1", "path": "", "viewed": true})), invalid("Missing file path"));
    assert_eq!(refused(&mut rig, json!({"pullRequestId": "PR_1", "path": "a"})), invalid("Invalid input: expected boolean, received undefined"));
    assert_eq!(refused(&mut rig, json!({"pullRequestId": "PR_1", "path": "a", "viewed": "yes"})), invalid("Invalid input: expected boolean, received string"));
    assert_eq!(refused(&mut rig, json!({"repo": "nowhere", "path": "a", "viewed": true})), invalid("Missing pull request ID"));
    // And valid fields for a repository no workspace owns are the host's repo_not_found
    assert_eq!(refused(&mut rig, json!({"repo": "nowhere", "pullRequestId": "PR_1", "path": "a", "viewed": true})), ("repo_not_found".to_string(), "repo_not_found".to_string()));
    assert_eq!(rig.asked_hosted().len(), before);
}

#[test]
fn given_a_branch_with_an_open_and_an_older_pull_request_when_its_review_is_read_then_it_is_the_open_one_in_the_neutral_shape_with_the_outcome_when_asked() {
    // Given an open pull request that passed its checks and was approved, and an older merged one of the same branch
    let mut rig = Rig::new();
    rig.hosting.pull_request(7, "Older attempt", "feat/retry", "main").state("github", 7, "merged");
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main").checks("github", 12, &[("build", "pass")]).approved("github", 12);
    // When the phone reads the branch's review
    let review = rig.ok("hostedReview.forBranch", json!({"branch": "refs/heads/feat/retry"}));
    // Then it is the open one, as the provider-neutral summary
    let head = rig.hosting.view("github", 12)["head_sha"].clone();
    assert_eq!(
        review,
        json!({"provider": "github", "number": 12, "title": "Add retry", "state": "open", "url": "https://example.test/github/12", "status": "success", "updatedAt": "2026-10-02T10:15:30.012Z", "mergeable": "MERGEABLE", "headSha": head, "baseRefName": "main", "reviewDecision": "APPROVED"})
    );
    // And with the outcome asked for it is wrapped as a read of the provider, and a branch with no review is null in the same wrapper
    let wrapped = rig.ok("hostedReview.forBranch", json!({"branch": "feat/retry", "reportReadOutcome": true}));
    assert_eq!((wrapped["review"]["number"].clone(), wrapped["readOutcome"].clone()), (json!(12), json!("provider")));
    assert_eq!(rig.ok("hostedReview.forBranch", json!({"branch": "other"})), Value::Null);
    assert_eq!(rig.ok("hostedReview.forBranch", json!({"branch": "other", "reportReadOutcome": true})), json!({"review": null, "readOutcome": "provider"}));
}

#[test]
fn given_a_linked_number_when_the_review_is_read_then_that_pull_request_is_read_without_listing_and_a_gitlab_link_is_refused() {
    // Given two pull requests and a phone that links the second to the branch
    let mut rig = Rig::new();
    rig.hosting.pull_request(3, "Linked", "somewhere/else", "main").draft("github", 3);
    rig.hosting.pull_request(4, "By branch", "feat/x", "main");
    // When it reads the branch's review with the link, and with a fallback that does not exist
    let linked = rig.ok("hostedReview.forBranch", json!({"branch": "feat/x", "linkedGitHubPR": 3}));
    let fallback = rig.ok("hostedReview.forBranch", json!({"branch": "feat/x", "fallbackGitHubPR": 99}));
    // Then the link is read as it is (a draft), without a listing, and a fallback that cannot be read gives way to the branch's own pull request
    assert_eq!((linked["number"].clone(), linked["state"].clone(), fallback["number"].clone()), (json!(3), json!("draft"), json!(4)));
    assert_eq!(rig.asked_hosted().iter().filter(|m| *m == "pr.list").count(), 1);
    // And a link to a GitLab merge request is refused, not answered from a provider no account serves
    let (code, message) = rig.err("hostedReview.forBranch", json!({"branch": "feat/x", "linkedGitLabMR": 5}));
    assert_eq!((code.as_str(), message.as_str()), ("runtime_error", "GitLab reviews are not offered by this host"));
}

#[test]
fn given_conversation_and_review_thread_comments_when_the_comments_are_read_then_each_is_a_comment_of_the_phone_oldest_first_with_its_thread() {
    // Given a conversation comment, and a resolved thread on a line with a reply, and an outdated thread with no line
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main");
    rig.hosting
        .conversation_comment(12, "IC_kwDOAbc", "octocat", "Looks good", 1_790_936_200_000, "https://example.test/pull/12#issuecomment-5001")
        .review_thread(12, "PRRT_1", "src/retry.rs", Some(14), (true, false), &[(6001, "hubot", "Use a constant", 1_790_936_100_000), (6002, "octocat", "Done", 1_790_936_300_000)])
        .review_thread(12, "PRRT_2", "src/old.rs", None, (false, true), &[(6003, "hubot", "Stale", 1_790_936_150_000)]);
    rig.hosting.pull_request(13, "Other", "feat/other", "main").conversation_comment(13, "IC_x", "someone", "Not this one", 1, "https://example.test/pull/13#issuecomment-9");
    // When the phone reads the comments of the pull request
    let comments = rig.ok("github.prComments", json!({"prNumber": 12}));
    // Then they are the comments of that pull request in time order, the thread ones with their path, thread and state
    assert_eq!(
        comments,
        json!([
            {"id": 6001, "author": "hubot", "authorAvatarUrl": "", "isBot": false, "body": "Use a constant", "createdAt": "2026-10-02T10:15:00.000Z", "url": "https://example.test/pull/12#discussion_r6001", "path": "src/retry.rs", "threadId": "PRRT_1", "isResolved": true, "isOutdated": false, "line": 14},
            {"id": 6003, "author": "hubot", "authorAvatarUrl": "", "isBot": false, "body": "Stale", "createdAt": "2026-10-02T10:15:50.000Z", "url": "https://example.test/pull/12#discussion_r6003", "path": "src/old.rs", "threadId": "PRRT_2", "isResolved": false, "isOutdated": true},
            {"id": 5001, "author": "octocat", "authorAvatarUrl": "", "isBot": false, "body": "Looks good", "createdAt": "2026-10-02T10:16:40.000Z", "url": "https://example.test/pull/12#issuecomment-5001"},
            {"id": 6002, "author": "octocat", "authorAvatarUrl": "", "isBot": false, "body": "Done", "createdAt": "2026-10-02T10:18:20.000Z", "url": "https://example.test/pull/12#discussion_r6002", "path": "src/retry.rs", "threadId": "PRRT_1", "isResolved": true, "isOutdated": false, "line": 14},
        ])
    );
    // And a pull request with no comments reads as an empty list
    rig.hosting.pull_request(14, "Quiet", "feat/quiet", "main");
    assert_eq!(rig.ok("github.prComments", json!({"prNumber": 14})), json!([]));
}

#[test]
fn given_rich_conversation_review_and_thread_comments_when_the_comments_are_read_then_each_carries_what_the_host_answers() {
    // Given a bot's conversation comment with reactions, a review with a visible body and one with only an HTML comment, and a left-side outdated thread
    // and a right-side thread with a long hunk and a range
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main");
    let bot = Extras { database_id: Some(5001), avatar_url: Some("https://avatars.test/bot.png"), is_bot: true, reaction_subject_id: Some("IC_kwDOBot"), reactions: &[("+1", 2, true), ("eyes", 1, false)], ..Extras::default() };
    let me = Extras { database_id: Some(7001), avatar_url: Some("https://avatars.test/me.png"), viewer_did_author: true, reaction_subject_id: Some("PRR_kept"), ..Extras::default() };
    let hidden = Extras { database_id: Some(7002), ..Extras::default() };
    let root = Extras { database_id: Some(6001), avatar_url: Some("https://avatars.test/hubot.png"), reaction_subject_id: Some("PRRC_1"), ..Extras::default() };
    let hunk = "@@ -1,12 +1,12 @@ fn main\n l1\n l2\n l3\n l4\n l5\n l6\n l7\n l8\n-l9\n+l9b\n l10";
    rig.hosting
        .conversation_comment(12, "IC_kwDOBot", "ci-bot", "Coverage 91%", 1_790_936_200_000, "https://example.test/pull/12#issuecomment-5001")
        .conversation_comment_extras(&bot)
        .review(12, ("PRR_kept", "octocat", "APPROVED", "Ship it"), Some(1_790_936_400_000), Some(&me))
        .review(12, ("PRR_hidden", "octocat", "COMMENTED", "<!-- generated -->\n"), Some(1_790_936_450_000), Some(&hidden))
        .review_thread(12, "PRRT_1", "src/retry.rs", Some(14), (false, false), &[(6001, "hubot", "Use a constant", 1_790_936_100_000)])
        .thread_comment_extras(0, &root)
        .thread_hunk((Some(12), Some(14), Some(12)), "RIGHT", hunk)
        .review_thread(12, "PRRT_2", "src/old.rs", None, (false, true), &[(6003, "hubot", "Stale", 1_790_936_150_000)])
        .thread_hunk((None, Some(3), Some(2)), "LEFT", "@@ -1,3 +1,3 @@\n-old");
    // When the phone reads the comments of the pull request
    let comments = rig.ok("github.prComments", json!({"prNumber": 12}));
    // Then every comment answers the host's fields in time order: the bot mark, avatar, reactions with the viewer's own, the viewer's authorship only when true,
    // the review summary stamped at its submission, the hidden review dropped, the hunk tail on the thread root of the right side only, and an outdated
    // left-side thread keeps its original lines without a hunk
    assert_eq!(
        comments,
        json!([
            {"id": 6001, "author": "hubot", "authorAvatarUrl": "https://avatars.test/hubot.png", "isBot": false, "body": "Use a constant", "createdAt": "2026-10-02T10:15:00.000Z", "url": "https://example.test/pull/12#discussion_r6001", "reactionSubjectId": "PRRC_1", "path": "src/retry.rs", "threadId": "PRRT_1", "isResolved": false, "isOutdated": false, "line": 14, "startLine": 12, "diffHunk": " l4\n l5\n l6\n l7\n l8\n-l9\n+l9b\n l10"},
            {"id": 6003, "author": "hubot", "authorAvatarUrl": "", "isBot": false, "body": "Stale", "createdAt": "2026-10-02T10:15:50.000Z", "url": "https://example.test/pull/12#discussion_r6003", "path": "src/old.rs", "threadId": "PRRT_2", "isResolved": false, "isOutdated": true, "line": 3, "startLine": 2},
            {"id": 5001, "author": "ci-bot", "authorAvatarUrl": "https://avatars.test/bot.png", "isBot": true, "body": "Coverage 91%", "createdAt": "2026-10-02T10:16:40.000Z", "url": "https://example.test/pull/12#issuecomment-5001", "reactionSubjectId": "IC_kwDOBot", "reactions": [{"content": "+1", "count": 2, "viewerHasReacted": true}, {"content": "eyes", "count": 1, "viewerHasReacted": false}]},
            {"id": 7001, "author": "octocat", "authorAvatarUrl": "https://avatars.test/me.png", "isBot": false, "body": "Ship it", "createdAt": "2026-10-02T10:20:00.000Z", "url": "", "viewerDidAuthor": true, "reactionSubjectId": "PRR_kept"},
        ])
    );
}

#[test]
fn given_a_modified_added_removed_binary_and_huge_file_when_the_contents_are_read_then_each_side_is_what_the_base_and_head_commit_hold() {
    // Given a pull request whose head and base commits hold a text file, a new file, a binary and a huge file
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "main").remote_branch("main", &"b".repeat(40));
    let (base, head) = ("a".repeat(40), "c".repeat(40));
    rig.hosting
        .blob(&base, "src/a.rs", b"old\n")
        .blob(&head, "src/a.rs", b"new\n")
        .blob(&head, "src/new.rs", b"fresh\n")
        .blob(&base, "src/gone.rs", b"bye\n")
        .blob(&base, "img.png", &[0x89, b'P', 0, 1])
        .blob(&head, "img.png", &[0x89, b'P', 0, 2])
        .blob(&head, "big.bin", &vec![b'x'; (8 << 20) + 1])
        .blob(&base, "src/before.rs", b"renamed from\n")
        .blob(&head, "src/after.rs", b"renamed to\n");
    let read = |rig: &mut Rig, path: &str, status: &str, old: Option<&str>| {
        let mut params = json!({"prNumber": 12, "path": path, "status": status, "headSha": head, "baseSha": base});
        if let Some(old) = old {
            params["oldPath"] = json!(old);
        }
        rig.ok("github.prFileContents", params)
    };
    // When the phone reads each of them
    let modified = read(&mut rig, "src/a.rs", "modified", None);
    let added = read(&mut rig, "src/new.rs", "added", None);
    let removed = read(&mut rig, "src/gone.rs", "removed", None);
    let binary = read(&mut rig, "img.png", "modified", None);
    let huge = read(&mut rig, "big.bin", "added", None);
    let renamed = read(&mut rig, "src/after.rs", "renamed", Some("src/before.rs"));
    // Then the sides are the file at the base and at the head, an added file has no original and a removed one no modified side
    assert_eq!(modified, json!({"original": "old\n", "modified": "new\n", "originalIsBinary": false, "modifiedIsBinary": false}));
    assert_eq!(added, json!({"original": "", "modified": "fresh\n", "originalIsBinary": false, "modifiedIsBinary": false}));
    assert_eq!(removed, json!({"original": "bye\n", "modified": "", "originalIsBinary": false, "modifiedIsBinary": false}));
    // And a binary is withheld as binary on both sides, a file over the limit is withheld as too large, and a rename reads the original from its old path
    assert_eq!(binary, json!({"original": "", "modified": "", "originalIsBinary": true, "modifiedIsBinary": true}));
    assert_eq!(huge, json!({"original": "", "modified": "", "originalIsBinary": false, "modifiedIsBinary": false, "modifiedTooLarge": true}));
    assert_eq!(renamed, json!({"original": "renamed from\n", "modified": "renamed to\n", "originalIsBinary": false, "modifiedIsBinary": false}));
    // And the pull request's head was fetched into a ref of its own before any blob was read
    assert_eq!(rig.hosting.local_ref("refs/rca/pr/12/head"), Some(rig.hosting.view("github", 12)["head_sha"].as_str().unwrap().to_string()));
}

#[test]
fn given_a_pull_request_that_cannot_be_fetched_or_a_path_the_commit_lacks_when_the_contents_are_read_then_the_sides_are_empty_and_a_malformed_call_is_refused() {
    // Given a pull request whose target branch the remote lost
    let mut rig = Rig::new();
    rig.hosting.pull_request(12, "Add retry", "feat/retry", "gone");
    let params = |path: &str| json!({"prNumber": 12, "path": path, "status": "modified", "headSha": "c".repeat(40), "baseSha": "a".repeat(40)});
    let nothing = json!({"original": "", "modified": "", "originalIsBinary": false, "modifiedIsBinary": false});
    // When the contents are read, Then the failed fetch is empty sides as the host answers a read it could not make
    assert_eq!(rig.ok("github.prFileContents", params("src/a.rs")), nothing);
    // And a path no commit holds, once the fetch works, is empty as well
    rig.hosting.remote_branch("gone", &"b".repeat(40));
    assert_eq!(rig.ok("github.prFileContents", params("src/missing.rs")), nothing);
    // And a call that lacks a field or names an unknown status is refused before rca is asked
    let before = rig.asked_hosted().len();
    for (bad, message) in [(json!({"prNumber": 12, "status": "modified", "headSha": "c", "baseSha": "a"}), "Missing file path"), (json!({"prNumber": 12, "path": "p", "status": "modified", "baseSha": "a"}), "Missing head SHA"), (json!({"prNumber": 12, "path": "p", "status": "modified", "headSha": "c"}), "Missing base SHA")] {
        let (code, said) = rig.err("github.prFileContents", bad);
        assert_eq!((code.as_str(), said.as_str()), ("invalid_argument", message));
    }
    let (code, _) = rig.err("github.prFileContents", json!({"prNumber": 12, "path": "p", "status": "weird", "headSha": "c", "baseSha": "a"}));
    assert_eq!((code.as_str(), rig.asked_hosted().len()), ("invalid_argument", before));
}

fn remotes_asked(rig: &Rig) -> Vec<Value> {
    rig.agent.requests().into_iter().filter(|(m, _)| m == "pr.remotes").map(|(_, p)| p).collect()
}

#[test]
fn given_a_workspace_whose_origin_names_a_github_repository_when_its_slug_is_read_then_it_is_the_owner_repo_and_github_host_and_rca_was_asked_its_remotes() {
    // Given an origin on github.com in its own casing, and an upstream
    let mut rig = Rig::new();
    rig.hosting.remote("origin", "git@github.com:Octo/Hello.git").remote("upstream", "https://github.com/acme/hello");
    // When the phone reads the repository's slug
    let slug = rig.ok("github.repoSlug", json!({}));
    // Then it is origin's owner and repository on github.com, read from the remotes rca listed for the workspace
    assert_eq!(slug, json!({"owner": "Octo", "repo": "Hello", "host": "github.com"}));
    let asked = remotes_asked(&rig);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].as_object().unwrap().keys().collect::<Vec<_>>(), vec!["workspace"]);
    // And keys the host strips are never refused
    assert_eq!(rig.ok("github.repoSlug", json!({"ownerRepo": 5, "expectedGhAccount": "x", "extra": 1})), json!({"owner": "Octo", "repo": "Hello", "host": "github.com"}));
}

#[test]
fn given_remotes_that_name_no_github_origin_when_the_slug_is_read_then_it_is_null_or_the_stand_in_remote() {
    let read = |remotes: &[(&str, &str)]| {
        let mut rig = Rig::new();
        for (name, url) in remotes {
            rig.hosting.remote(name, url);
        }
        rig.ok("github.repoSlug", json!({}))
    };
    // Given no remote, an origin on another forge, a local origin alone or an Enterprise origin, when the slug is read, then there is none
    assert_eq!(read(&[]), Value::Null);
    assert_eq!(read(&[("origin", "git@gitlab.com:grp/proj.git"), ("fork", "https://github.com/me/app.git")]), Value::Null);
    assert_eq!(read(&[("origin", "/srv/local.git")]), Value::Null);
    assert_eq!(read(&[("origin", "https://ghe.example.com/a/b")]), Value::Null);
    // Given SSH over port 443, then it is github.com
    assert_eq!(read(&[("origin", "ssh://git@ssh.github.com:443/acme/app.git")]), json!({"owner": "acme", "repo": "app", "host": "github.com"}));
    // Given a local origin or none, then the remote standing in for it answers, never upstream
    assert_eq!(read(&[("origin", "/srv/local.git"), ("fork", "https://github.com/me/app.git")]), json!({"owner": "me", "repo": "app", "host": "github.com"}));
    // Given an origin cloned from a file URL, with or without localhost, then it is local too and the fork stands in
    assert_eq!(read(&[("origin", "file:///srv/local.git"), ("fork", "https://github.com/me/app")]), json!({"owner": "me", "repo": "app", "host": "github.com"}));
    assert_eq!(read(&[("origin", "file://localhost/srv/local.git"), ("fork", "https://github.com/me/app")]), json!({"owner": "me", "repo": "app", "host": "github.com"}));
    // Given an origin the host's URL parser refuses, then it is not a network remote either
    assert_eq!(read(&[("origin", "https://git hub.com/a/b"), ("fork", "https://github.com/me/app")]), json!({"owner": "me", "repo": "app", "host": "github.com"}));
    assert_eq!(read(&[("upstream", "https://github.com/acme/app"), ("zeta", "git@github.com:z/app.git")]), json!({"owner": "z", "repo": "app", "host": "github.com"}));
    assert_eq!(read(&[("upstream", "https://github.com/acme/app")]), Value::Null);
    // Given git refusing to list them, then the host's swallowed error is null too
    let mut rig = Rig::new();
    rig.hosting.remotes_fail("git exited 128: fatal: not a git repository (or any of the parent directories): .git");
    assert_eq!(rig.ok("github.repoSlug", json!({})), Value::Null);
}

#[test]
fn given_a_bad_selector_when_the_slug_is_read_then_it_is_the_hosts_refusal() {
    let mut rig = Rig::new();
    rig.hosting.remote("origin", "git@github.com:octo/hello.git");
    // Given an empty, a non-string, a null or a missing selector, when the slug is read, then the host's selector refusal comes back
    let missing = ("invalid_argument".to_string(), "Missing repo selector".to_string());
    for params in [json!({"repo": ""}), json!({"repo": 7}), json!({"repo": null})] {
        assert_eq!(rig.err("github.repoSlug", params.clone()), missing, "{params}");
    }
    for params in [Value::Null, json!({})] {
        let frame = json!({"id": "x", "method": "github.repoSlug", "params": params}).to_string();
        let r: Value = serde_json::from_str(&rig.conn.on_message(&frame)[0]).unwrap();
        assert_eq!((r["ok"].clone(), r["error"]["code"].clone(), r["error"]["message"].clone()), (json!(false), json!("invalid_argument"), json!("Missing repo selector")), "{params}");
    }
    // And a selector that names no workspace is not found
    assert_eq!(rig.err("github.repoSlug", json!({"repo": "nowhere"})), ("repo_not_found".to_string(), "repo_not_found".to_string()));
    // And rca was asked nothing of the remotes
    assert!(remotes_asked(&rig).is_empty());
}

const ELIGIBLE: &str = "hostedReview.getCreationEligibility";

/// A clean, published branch `feat/x` of a github.com origin whose HEAD rca reports as `origin/main`.
fn github_rig() -> Rig {
    let rig = Rig::new();
    rig.hosting.remote("origin", "https://github.com/acme/app.git").origin_head("origin/main");
    rig.ws.refs("main", &[("main", "branch"), ("origin/main", "remote"), ("origin/develop", "remote"), ("feat/x", "branch")]);
    rig
}

fn ready() -> Value {
    json!({"branch": "feat/x", "hasUncommittedChanges": false, "hasUpstream": true, "ahead": 0, "behind": 0})
}

fn with(mut params: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        params[k] = v.clone();
    }
    params
}

#[test]
fn given_a_clean_published_branch_of_a_github_repository_without_a_review_when_eligibility_is_read_then_a_review_can_be_created() {
    // Given a github.com origin, origin's HEAD on main and no pull request of the branch
    let mut rig = github_rig();
    // When the phone asks whether a review can be created
    let answer = rig.ok(ELIGIBLE, ready());
    // Then it can, against origin/main, with stacked creation offered
    assert_eq!(
        answer,
        json!({"provider": "github", "review": null, "reviewLookupOutcome": "not_found", "defaultBaseRef": "origin/main", "head": "feat/x", "stackedCreationSupported": true, "canCreate": true, "blockedReason": null, "nextAction": null})
    );
    // And rca was asked the remotes, origin's HEAD, the branch's pull requests and the sign-in, in that order
    assert_eq!(rig.asked_hosted(), vec!["pr.remotes", "git.upstreamStatus", "pr.list", "sourceControl.discover"]);
    let asked = |m: &str| rig.agent.requests().into_iter().find(|(x, _)| x == m).unwrap().1;
    assert_eq!(asked("pr.list"), json!({"head": "feat/x", "state": "all", "limit": 20, "workspace": "main"}));
    assert_eq!(asked("sourceControl.discover"), json!({"provider": "github", "host": "github.com", "workspace": "main"}));
}

#[test]
fn given_an_open_pull_request_of_the_branch_when_eligibility_is_read_then_it_is_the_existing_review_and_no_sign_in_is_asked() {
    // Given pull request 12 open on refs/heads/feat/x
    let mut rig = github_rig();
    rig.hosting.pull_request(12, "Add retry", "feat/x", "main");
    // When eligibility is read for the branch by its full ref
    let answer = rig.ok(ELIGIBLE, with(ready(), json!({"branch": "refs/heads/feat/x"})));
    // Then the review is offered to open instead
    assert_eq!(
        answer,
        json!({"provider": "github", "review": {"number": 12, "url": "https://example.test/github/12"}, "reviewLookupOutcome": "found", "defaultBaseRef": "origin/main", "head": "feat/x", "stackedCreationSupported": true, "canCreate": false, "blockedReason": "existing_review", "nextAction": "open_existing_review"})
    );
    assert!(!rig.asked_hosted().contains(&"sourceControl.discover".to_string()));
    // And a linked number is read directly, not by the branch
    let mut rig = github_rig();
    rig.hosting.pull_request(7, "Other", "elsewhere", "main");
    assert_eq!(rig.ok(ELIGIBLE, with(ready(), json!({"linkedGitHubPR": 7})))["review"], json!({"number": 7, "url": "https://example.test/github/7"}));
    assert!(!rig.asked_hosted().contains(&"pr.list".to_string()));
}

#[test]
fn given_a_repository_with_no_remote_when_eligibility_is_read_then_it_is_the_recorded_unsupported_provider_answer() {
    // Given a fresh repository on main with no remote, as the host's scenario recording made it
    let mut rig = Rig::new();
    // When eligibility is read for branch a
    let answer = rig.ok(ELIGIBLE, json!({"branch": "a"}));
    // Then it is exactly the host's recorded answer, with no stacked creation key
    assert_eq!(
        answer,
        json!({"blockedReason": "unsupported_provider", "canCreate": false, "defaultBaseRef": "main", "head": "a", "nextAction": null, "provider": "unsupported", "review": null, "reviewLookupOutcome": "not_found"})
    );
    // And no review was looked up
    assert_eq!(rig.asked_hosted(), vec!["pr.remotes", "git.upstreamStatus", "git.refs"]);
}

#[test]
fn given_each_blocking_state_of_the_branch_when_eligibility_is_read_then_the_first_rule_of_the_hosts_ladder_answers() {
    let cases: [(Value, bool, Value, Value); 8] = [
        (json!({"branch": "HEAD"}), false, json!("detached_head"), Value::Null),
        (json!({"branch": "Main"}), false, json!("default_branch"), Value::Null),
        (json!({"hasUncommittedChanges": true}), false, json!("dirty"), json!("commit")),
        (json!({"hasUpstream": false}), false, json!("no_upstream"), json!("publish")),
        (json!({"behind": 1}), false, json!("needs_sync"), json!("sync")),
        (json!({}), true, json!("auth_required"), json!("authenticate")),
        (json!({"ahead": 2}), false, json!("needs_push"), json!("push")),
        (json!({"ahead": 2, "behind": 1, "hasUncommittedChanges": true}), false, json!("dirty"), json!("commit")),
    ];
    for (extra, signed_out, reason, next) in cases {
        // Given the branch in that state
        let mut rig = github_rig();
        if signed_out {
            rig.hosting.signed_out();
        }
        // When eligibility is read
        let answer = rig.ok(ELIGIBLE, with(ready(), extra.clone()));
        // Then creation is blocked with that rule's reason and next action
        assert_eq!((answer["canCreate"].clone(), answer["blockedReason"].clone(), answer["nextAction"].clone()), (json!(false), reason, next), "{extra}");
    }
    // Given no word on the upstream, then creation is not offered and nothing blocks it by name
    let mut rig = github_rig();
    let answer = rig.ok(ELIGIBLE, json!({"branch": "feat/x"}));
    assert_eq!((answer["canCreate"].clone(), answer["blockedReason"].clone(), answer["nextAction"].clone()), (json!(false), Value::Null, Value::Null));
}

#[test]
fn given_the_pull_request_lookup_fails_when_eligibility_is_read_then_the_review_is_unavailable_and_creation_is_not_offered() {
    // Given gh failing to list the branch's pull requests
    let mut rig = github_rig();
    rig.hosting.list_fails("gh: HTTP 502");
    // When eligibility is read
    let answer = rig.ok(ELIGIBLE, ready());
    // Then the failure is not an absence: nothing blocks by name, but creation is not offered
    assert_eq!(
        answer,
        json!({"provider": "github", "review": null, "reviewLookupOutcome": "unavailable", "defaultBaseRef": "origin/main", "head": "feat/x", "stackedCreationSupported": true, "canCreate": false, "blockedReason": null, "nextAction": null})
    );
}

#[test]
fn given_a_gitlab_origin_with_a_merge_request_of_the_branch_when_eligibility_is_read_then_it_is_the_gitlab_review() {
    // Given an SSH origin on gitlab.com and merge request 5 of the branch
    let mut rig = Rig::new();
    rig.hosting.remote("origin", "git@gitlab.com:grp/app.git").merge_request(5, "Add retry", "feat/x", "main");
    rig.ws.refs("main", &[("origin/main", "remote")]);
    // When eligibility is read
    let answer = rig.ok(ELIGIBLE, ready());
    // Then it is GitLab's review, with no stacked creation, found by the merge request list
    assert_eq!(
        answer,
        json!({"provider": "gitlab", "review": {"number": 5, "url": "https://example.test/gitlab/5"}, "reviewLookupOutcome": "found", "defaultBaseRef": "origin/main", "head": "feat/x", "canCreate": false, "blockedReason": "existing_review", "nextAction": "open_existing_review"})
    );
    assert_eq!(rig.asked_hosted(), vec!["pr.remotes", "git.upstreamStatus", "git.refs", "mr.list"]);
}

#[test]
fn given_a_base_when_eligibility_is_read_then_it_is_the_default_only_when_a_remote_tracks_it() {
    // Given develop on origin and a branch that only exists locally
    let mut rig = github_rig();
    // When eligibility names develop, then it is the base
    let answer = rig.ok(ELIGIBLE, with(ready(), json!({"base": " develop "})));
    assert_eq!((answer["defaultBaseRef"].clone(), answer["canCreate"].clone()), (json!("develop"), json!(true)));
    let asked = rig.agent.requests().into_iter().find(|(m, _)| m == "git.refs").unwrap().1;
    assert_eq!(asked, json!({"kind": "remote", "query": "develop", "workspace": "main"}));
    // When it names a local-only branch, then the repository's default replaces it
    let mut rig = github_rig();
    assert_eq!(rig.ok(ELIGIBLE, with(ready(), json!({"base": "local-only"})))["defaultBaseRef"], json!("origin/main"));
    // When the base is the branch itself under its remote name, then it is the default branch
    let mut rig = github_rig();
    rig.ws.refs("main", &[("origin/feat/x", "remote")]);
    assert_eq!(rig.ok(ELIGIBLE, with(ready(), json!({"base": "origin/feat/x"})))["blockedReason"], json!("default_branch"));
}

#[test]
fn given_a_worktree_selector_when_eligibility_is_read_then_one_of_another_repository_is_refused() {
    // Given a second repository, and a worktree of main named by its id
    let mut rig = github_rig();
    rig.ws.workspace("other", "root", Some("/srv/other"));
    // When the worktree belongs to the repository, then it answers
    assert_eq!(rig.ok(ELIGIBLE, with(ready(), json!({"worktree": "id:main::/srv/main"})))["canCreate"], json!(true));
    // When it names another repository, then access is denied before rca is asked anything hosted
    let before = rig.asked_hosted().len();
    assert_eq!(rig.err(ELIGIBLE, with(ready(), json!({"worktree": "id:other::/srv/other"}))), ("runtime_error".to_string(), "Access denied: worktree does not belong to repository".to_string()));
    assert_eq!(rig.err(ELIGIBLE, with(ready(), json!({"worktree": "other"}))).1, "Access denied: worktree does not belong to repository");
    // When it names nothing, then it is the selector's failure
    assert_eq!(rig.err(ELIGIBLE, with(ready(), json!({"worktree": "nowhere"}))).0, "selector_not_found");
    assert_eq!(rig.asked_hosted().len(), before);
}

#[test]
fn given_params_the_hosts_schema_refuses_when_eligibility_is_read_then_it_is_zods_first_issue_and_rca_is_asked_nothing_hosted() {
    let mut rig = github_rig();
    let invalid = |m: &str| ("invalid_argument".to_string(), m.to_string());
    let cases = [
        (json!({"repo": 7, "branch": "a"}), "Missing repo selector"),
        (json!({}), "Missing branch"),
        (json!({"branch": ""}), "Missing branch"),
        (json!({"worktree": "", "branch": 1}), "Missing worktree selector"),
        (json!({"worktree": null, "branch": "a"}), "Invalid input: expected string, received null"),
        (json!({"branch": "a", "base": 7}), "Invalid input: expected string, received number"),
        (json!({"branch": "a", "hasUpstream": "yes"}), "Invalid input: expected boolean, received string"),
        (json!({"branch": "a", "hasUncommittedChanges": null}), "Invalid input: expected boolean, received null"),
        (json!({"branch": "a", "ahead": -1}), "Too small: expected number to be >=0"),
        (json!({"branch": "a", "behind": 1.5}), "Invalid input: expected int, received number"),
        (json!({"branch": "a", "ahead": 9007199254740992u64}), "Too big: expected int to be <=9007199254740991"),
        (json!({"branch": "a", "linkedGitHubPR": 0}), "Too small: expected number to be >0"),
        (json!({"branch": "a", "linkedGiteaPR": "1"}), "Invalid input: expected number, received string"),
    ];
    for (params, message) in cases {
        // Given params the host's schema refuses, when eligibility is read, then the refusal is zod's first issue in key order
        assert_eq!(rig.err(ELIGIBLE, params.clone()), invalid(message), "{params}");
    }
    // Given no params at all, then the repo selector is missing
    let frame = json!({"id": "x", "method": ELIGIBLE, "params": null}).to_string();
    let r: Value = serde_json::from_str(&rig.conn.on_message(&frame)[0]).unwrap();
    assert_eq!((r["error"]["code"].clone(), r["error"]["message"].clone()), (json!("invalid_argument"), json!("Missing repo selector")));
    // Given a repo no workspace owns, even with a worktree, then it is not found
    assert_eq!(rig.err(ELIGIBLE, json!({"repo": "nowhere", "branch": "a", "worktree": "a"})), ("repo_not_found".to_string(), "repo_not_found".to_string()));
    // And null links and an ignored WSL wake are taken
    assert_eq!(rig.ok(ELIGIBLE, with(ready(), json!({"linkedGitHubPR": null, "fallbackGitHubPR": null, "wakeWslDistro": "x", "extra": 1})))["canCreate"], json!(true));
    // And rca was asked nothing hosted for the refused ones
    assert_eq!(rig.asked_hosted(), vec!["pr.remotes", "git.upstreamStatus", "pr.list", "sourceControl.discover"]);
}
