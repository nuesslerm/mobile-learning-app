//! The M11 rows through the real router over the agent double: the pull request reads and actions, the source control sign-in, the clones and
//! the thread links. The double answers the neutral shape the executor's `hosted_*` actions produce (`rca-exec` `hosted::shape`); what a request
//! becomes (the `Op::Hosted` the agent API is asked, its method and params) and what the phone is told are both asserted. The chain
//! (`tests/integration/hosted_m11.rs`) runs the same calls through the real service and executor with fake `gh` and `glab`.
use super::support::*;
use std::sync::Mutex;
use t3_core::ports::{AgentError, AgentEventKind, Finish, Op};

const ROOT: &str = "/workspace/demo";
const REPO: &str = "octo/hello";

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let mut rpc = connect_with(&h, all);
    seed(&mut rpc);
    (h, rpc)
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

fn hosted(h: &Harness) -> Vec<(String, Value)> {
    h.agent.ops.lock().unwrap().iter().filter_map(|op| if let Op::Hosted { method, root, params } = op { assert!(root == ROOT || root.is_empty(), "{root}"); Some((method.to_string(), params.clone())) } else { None }).collect()
}

fn pr_ref(number: u64) -> Value {
    json!({"projectId": "p1", "repository": REPO, "number": number})
}

fn with(mut base: Value, extra: Value) -> Value {
    base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    base
}

/// A pull request as the executor's `hosted_pr_view` answers it.
fn neutral_pr() -> Value {
    json!({"pr": {
        "number": 12, "title": "Retry flaky fetch", "state": "open", "draft": false, "author": "octocat", "head_ref": "feat/retry", "base_ref": "main", "labels": ["bug"],
        "url": "https://github.com/octo/hello/pull/12", "created_at_ms": 1_700_000_000_000_i64, "updated_at_ms": 1_700_000_100_000_i64, "review_decision": "changes_requested",
        "body": "Retries twice", "merged_at_ms": null, "closed_at_ms": null, "mergeable": "mergeable", "head_sha": "abc1234", "additions": 10, "deletions": 2, "changed_files": 3, "reviewers": ["hubot"], "assignees": [],
    }})
}

fn refused(code: &str, message: &str) -> Result<Value, AgentError> {
    Err(AgentError::Refused { code: code.into(), message: message.into() })
}

#[test]
fn a_pull_request_is_read_through_the_hosted_group_and_told_to_the_phone_as_a_summary_a_preview_a_detail_and_its_checks() {
    // Given a project, the executor's view of pull request 12 and its checks
    let (h, mut rpc) = started();
    reply(&h, "pr.view", Ok(neutral_pr()));
    reply(&h, "pr.checks", Ok(json!({"pass": 1, "fail": 1, "pending": 0, "checks": [
        {"name": "build", "state": "pass", "workflow": "CI", "url": "https://x/1", "started_at_ms": null, "completed_at_ms": null},
        {"name": "lint", "state": "fail", "workflow": null, "url": null, "started_at_ms": null, "completed_at_ms": null}]})));
    // When the phone reads the summary, the preview, the detail and the checks
    let summary = rpc.call("pullRequests.summary", pr_ref(12));
    let preview = rpc.call("pullRequests.preview", pr_ref(12));
    let detail = rpc.call("pullRequests.detail", pr_ref(12));
    let checks = rpc.call("pullRequests.checks", pr_ref(12));
    // Then each is what the executor said, in T3's words
    let (summary, preview, detail, checks) = (success(&summary), success(&preview), success(&detail), success(&checks));
    assert_eq!((summary["title"].clone(), summary["state"].clone(), summary["headBranch"].clone(), summary["reviewDecision"].clone(), summary["updatedAt"].clone()), (json!("Retry flaky fetch"), json!("open"), json!("feat/retry"), json!("changes-requested"), json!("2023-11-14T22:15:00.000Z")));
    assert_eq!((preview["author"]["login"].clone(), preview["createdAt"].clone(), preview["repository"].clone()), (json!("octocat"), json!("2023-11-14T22:13:20.000Z"), json!(REPO)));
    assert_eq!((detail["body"].clone(), detail["headSha"].clone(), detail["changedFiles"].clone(), detail["reviewers"][0]["login"].clone(), detail["labels"][0]["name"].clone(), detail["workspaceRoot"].clone()), (json!("Retries twice"), json!("abc1234"), json!(3), json!("hubot"), json!("bug"), json!(ROOT)));
    let statuses: Vec<&str> = checks["checks"].as_array().unwrap().iter().map(|c| c["status"].as_str().unwrap()).collect();
    assert_eq!((statuses, checks["state"].clone(), detail["checks"][1]["name"].clone()), (vec!["success", "failure"], json!("open"), json!("lint")));
    // And what the phone may offer is what the executor does: no inline comments, no close
    assert_eq!((detail["capabilities"]["review"]["inlineComment"].clone(), detail["capabilities"]["actions"].as_array().unwrap().iter().any(|a| a == "close")), (json!(false), false));
    // And every request was the hosted group's, for this repository and number, with the GitHub provider
    let methods: Vec<(String, Value)> = hosted(&h);
    assert!(methods.iter().all(|(_, p)| p["provider"] == "github" && p["repo"] == REPO && p["number"] == 12), "{methods:?}");
    assert_eq!(methods.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), ["pr.view", "pr.view", "pr.view", "pr.checks", "pr.view", "pr.checks"]);
}

#[test]
fn the_list_names_each_projects_repository_by_asking_the_host_and_a_project_the_host_refuses_is_an_error_row_not_a_failed_list() {
    // Given an executor signed in as octocat that lists one pull request for the project's remote
    let (h, mut rpc) = started();
    reply(&h, "sourceControl.lookupRepository", Ok(json!({"repo": {"slug": REPO}})));
    reply(&h, "pr.list", Ok(json!({"truncated": false, "prs": [neutral_pr()["pr"].clone()]})));
    reply(&h, "sourceControl.discover", Ok(json!({"providers": [{"provider": "github", "installed": true, "signed_in": true, "host": "github.com", "account": "octocat", "scopes": []}]})));
    // When the phone lists open pull requests
    let listed = rpc.call("pullRequests.list", json!({"state": "open", "filters": {"author": "octocat", "labels": [["bug"]]}}));
    // Then the entry is the pull request of that repository with the project, the viewer is the account, and the list asked with the filters
    let listed = success(&listed);
    let entry = &listed["entries"][0];
    assert_eq!((entry["repository"].clone(), entry["projectId"].clone(), entry["number"].clone(), entry["labels"][0]["name"].clone(), entry["host"].clone(), entry["additions"].clone()), (json!(REPO), json!("p1"), json!(12), json!("bug"), json!("github.com"), json!(0)));
    assert_eq!((listed["viewers"]["github.com"].clone(), listed["errors"].clone(), listed["providers"][0]["projectCount"].clone()), (json!("octocat"), json!([]), json!(1)));
    let list = hosted(&h).into_iter().find(|(m, _)| m == "pr.list").unwrap().1;
    assert_eq!((list["state"].clone(), list["author"].clone(), list["label"].clone(), list["repo"].clone()), (json!("open"), json!("octocat"), json!("bug"), json!(REPO)));
    // When the executor's machine has no gh
    reply(&h, "sourceControl.lookupRepository", refused("CLI_NOT_INSTALLED", "gh is not installed on the executor machine"));
    let none = rpc.call("pullRequests.list", json!({"state": "open"}));
    // Then the list answers, empty, with the project's error row saying why
    let none = success(&none);
    assert_eq!((none["entries"].clone(), none["errors"][0]["projectId"].clone(), none["errors"][0]["message"].clone()), (json!([]), json!("p1"), json!("gh is not installed on the executor machine")));
}

#[test]
fn an_entry_says_whether_the_viewer_is_asked_to_review_it_and_a_pull_request_without_a_url_is_an_error_row_not_an_empty_url() {
    // Given an executor signed in as octocat that lists a pull request octocat is asked to review, one hubot is, and one the host gave no url
    let (h, mut rpc) = started();
    let (mine, theirs) = (with(neutral_pr()["pr"].clone(), json!({"number": 1, "reviewers": ["Octocat"]})), neutral_pr()["pr"].clone());
    let mut urlless = with(neutral_pr()["pr"].clone(), json!({"number": 3}));
    urlless.as_object_mut().unwrap().remove("url");
    reply(&h, "sourceControl.lookupRepository", Ok(json!({"repo": {"slug": REPO}})));
    reply(&h, "pr.list", Ok(json!({"truncated": false, "prs": [mine, theirs, urlless]})));
    reply(&h, "sourceControl.discover", Ok(json!({"providers": [{"provider": "github", "installed": true, "signed_in": true, "host": "github.com", "account": "octocat", "scopes": []}]})));
    // When the phone lists open pull requests
    let listed = rpc.call("pullRequests.list", json!({"state": "open"}));
    // Then each entry carries what its own reviewers say, and the one with no url is an error row
    let listed = success(&listed);
    let flags: Vec<(Value, Value)> = listed["entries"].as_array().unwrap().iter().map(|e| (e["number"].clone(), e["viewerReviewRequested"].clone())).collect();
    assert_eq!(flags, vec![(json!(1), json!(true)), (json!(12), json!(false))]);
    assert_eq!((listed["errors"].as_array().unwrap().len(), listed["errors"][0]["projectId"].clone()), (1, json!("p1")));
}

#[test]
fn a_missing_cli_and_a_missing_sign_in_are_the_unavailable_error_with_their_reasons_and_a_missing_pull_request_is_not_found() {
    // Given an executor that refuses in each of the three ways
    let (h, mut rpc) = started();
    for (code, expected) in [("CLI_NOT_INSTALLED", ("PullRequestUnavailableError", Some("cli-missing"))), ("PROVIDER_NOT_SIGNED_IN", ("PullRequestUnavailableError", Some("cli-unauthenticated"))), ("NOT_FOUND", ("PullRequestOperationError", Some("not-found")))] {
        reply(&h, "pr.view", refused(code, &format!("{code}: said so")));
        // When the phone asks for the summary
        let exit = rpc.call("pullRequests.summary", pr_ref(12));
        // Then it is the declared error with the executor's words
        let error = failure(&exit);
        assert_eq!((error["_tag"].as_str().unwrap(), error["reason"].as_str()), expected, "{code}");
        assert!(error["requirement"].as_str().or(error["detail"].as_str()).unwrap().contains("said so"));
    }
    // And a project the server does not have is not found, with nothing asked of the agent API
    let before = hosted(&h).len();
    let exit = rpc.call("pullRequests.summary", with(pr_ref(12), json!({"projectId": "nope"})));
    assert_eq!((failure(&exit)["reason"].clone(), hosted(&h).len()), (json!("not-found"), before));
}

#[test]
fn each_action_of_the_phone_is_one_request_of_the_hosted_group_with_its_arguments_and_what_the_cli_cannot_do_is_refused_without_asking() {
    // Given a project
    let (h, mut rpc) = started();
    let done = || Ok(json!({"done": true}));
    for method in ["pr.comment", "pr.reply", "pr.resolveThread", "pr.react", "pr.editComment", "pr.requestReviewers", "pr.setLabels", "pr.review", "pr.edit", "pr.merge", "pr.ready", "pr.updateBranch"] {
        reply(&h, method, done());
    }
    // When the phone comments, replies, resolves, reacts, asks for a review, labels, reviews, edits, merges, makes ready and updates the branch
    let sent = [
        ("pullRequests.comment", with(pr_ref(12), json!({"body": "looks **good**"}))),
        ("pullRequests.replyToThread", with(pr_ref(12), json!({"threadId": "PRRT_1", "body": "done"}))),
        ("pullRequests.setThreadResolution", with(pr_ref(12), json!({"threadId": "PRRT_1", "resolved": true}))),
        ("pullRequests.setReaction", with(pr_ref(12), json!({"content": "thumbs-up", "reacted": true}))),
        ("pullRequests.setReaction", with(pr_ref(12), json!({"content": "heart", "reacted": false, "subjectId": "9001"}))),
        ("pullRequests.updateComment", with(pr_ref(12), json!({"commentId": "IC_kwDOAA", "kind": "issue-comment", "body": "fixed *typo*"}))),
        ("pullRequests.updateComment", with(pr_ref(12), json!({"commentId": "9001", "kind": "review-comment", "body": "and this"}))),
        ("pullRequests.requestReviewers", with(pr_ref(12), json!({"reviewers": [{"id": "hubot", "kind": "user"}], "requested": true}))),
        ("pullRequests.setLabels", with(pr_ref(12), json!({"labels": ["bug"], "applied": false}))),
        ("pullRequests.submitReview", with(pr_ref(12), json!({"verdict": "request-changes", "body": "see comments", "comments": []}))),
        ("pullRequests.update", with(pr_ref(12), json!({"title": "Retry twice"}))),
        ("pullRequests.runAction", with(pr_ref(12), json!({"action": "merge", "mergeMethod": "squash"}))),
        ("pullRequests.runAction", with(pr_ref(12), json!({"action": "draft"}))),
        ("pullRequests.runAction", with(pr_ref(12), json!({"action": "update-branch", "updateMethod": "rebase"}))),
    ];
    for (tag, payload) in sent {
        let exit = rpc.call(tag, payload);
        assert_eq!(exit["_tag"], "Success", "{tag}: {exit}");
    }
    // Then each was one request with the executor's names for its arguments
    let asked: Vec<(String, Value)> = hosted(&h).into_iter().map(|(m, mut p)| {
        p.as_object_mut().unwrap().retain(|k, _| !["provider", "repo", "number"].contains(&k.as_str()));
        (m, p)
    }).collect();
    assert_eq!(asked, [
        ("pr.comment".to_string(), json!({"body": "looks **good**"})),
        ("pr.reply".to_string(), json!({"thread": "PRRT_1", "body": "done"})),
        ("pr.resolveThread".to_string(), json!({"thread": "PRRT_1", "resolved": true})),
        ("pr.react".to_string(), json!({"target": "pr", "content": "+1"})),
        ("pr.react".to_string(), json!({"target": "comment", "comment_id": 9001, "content": "heart", "undo": true})),
        ("pr.editComment".to_string(), json!({"comment": "IC_kwDOAA", "kind": "issue", "body": "fixed *typo*"})),
        ("pr.editComment".to_string(), json!({"comment": "9001", "kind": "review", "body": "and this"})),
        ("pr.requestReviewers".to_string(), json!({"add": ["hubot"]})),
        ("pr.setLabels".to_string(), json!({"remove": ["bug"]})),
        ("pr.review".to_string(), json!({"verdict": "request_changes", "body": "see comments"})),
        ("pr.edit".to_string(), json!({"title": "Retry twice"})),
        ("pr.merge".to_string(), json!({"method": "squash"})),
        ("pr.ready".to_string(), json!({"undo": true})),
        ("pr.updateBranch".to_string(), json!({"rebase": true})),
    ]);
    // When the phone asks for what the CLIs do not do
    let before = hosted(&h).len();
    let refusals = [
        rpc.call("pullRequests.submitReview", with(pr_ref(12), json!({"verdict": "comment", "body": "x", "comments": [{"path": "a", "position": {"kind": "added", "newLine": 1}, "body": "y"}]}))),
        rpc.call("pullRequests.runAction", with(pr_ref(12), json!({"action": "close"}))),
        rpc.call("pullRequests.update", with(pr_ref(12), json!({"maintainerCanModify": true}))),
    ];
    // Then each is the operation error that says so, and the agent API was asked nothing
    for exit in &refusals {
        assert_eq!(failure(exit)["_tag"], "PullRequestOperationError", "{exit}");
    }
    assert_eq!(hosted(&h).len(), before);
}

#[test]
fn the_files_of_a_pull_request_give_its_stats_its_change_breakdown_and_the_candidates_know_what_the_pull_request_already_has() {
    // Given the executor's files, labels and reviewers
    let (h, mut rpc) = started();
    reply(&h, "pr.view", Ok(neutral_pr()));
    reply(&h, "pr.files", Ok(json!({"files": [{"path": "src/a.rs", "additions": 6, "deletions": 1, "status": "modified"}, {"path": "src/a.test.ts", "additions": 4, "deletions": 1, "status": "added"}]})));
    reply(&h, "pr.labelCandidates", Ok(json!({"labels": [{"name": "bug", "color": "d73a4a", "description": "Broken"}, {"name": "docs", "color": null, "description": null}]})));
    reply(&h, "pr.reviewerCandidates", Ok(json!({"users": [{"login": "hubot", "name": "Hubot"}, {"login": "octocat", "name": null}]})));
    // When the phone asks for the stats, the breakdown and both candidate lists
    let stats = rpc.call("pullRequests.listStats", json!({"refs": [pr_ref(12)]}));
    let breakdown = rpc.call("pullRequests.changeBreakdown", pr_ref(12));
    let labels = rpc.call("pullRequests.labelCandidates", pr_ref(12));
    let reviewers = rpc.call("pullRequests.reviewerCandidates", pr_ref(12));
    let options = rpc.call("pullRequests.filterOptions", json!({"kind": "labels", "projectId": "p1"}));
    // Then the stats are the files' sums, the breakdown splits code from tests, applied labels and requested reviewers are marked
    assert_eq!((success(&stats)["stats"][0]["additions"].clone(), success(&stats)["stats"][0]["deletions"].clone()), (json!(10), json!(2)));
    assert_eq!((success(&breakdown)["code"]["additions"].clone(), success(&breakdown)["tests"]["additions"].clone()), (json!(6), json!(4)));
    let applied: Vec<(String, bool)> = success(&labels)["candidates"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_string(), c["isApplied"].as_bool().unwrap())).collect();
    assert_eq!(applied, [("bug".to_string(), true), ("docs".to_string(), false)]);
    let requested: Vec<(String, bool)> = success(&reviewers)["candidates"].as_array().unwrap().iter().map(|c| (c["id"].as_str().unwrap().to_string(), c["isRequested"].as_bool().unwrap())).collect();
    assert_eq!(requested, [("hubot".to_string(), true), ("octocat".to_string(), false)]);
    assert_eq!(success(&options)["labels"][0]["name"], "bug");
}

#[test]
fn the_activity_joins_the_conversation_the_reviews_and_the_review_threads_and_a_thread_is_read_by_its_id() {
    // Given the executor's comments, reviews and threads (one thread without a file)
    let (h, mut rpc) = started();
    reply(&h, "pr.comments", Ok(json!({"comments": [{"id": "1", "author": "octocat", "body": "hi", "created_at_ms": 1_700_000_000_000_i64, "url": "https://x/c1"}]})));
    reply(&h, "pr.reviews", Ok(json!({"reviews": [{"id": "9", "author": "hubot", "state": "approved", "body": "", "submitted_at_ms": 1_700_000_050_000_i64}]})));
    reply(&h, "pr.threads", Ok(json!({"threads": [
        {"id": "PRRT_1", "resolved": false, "outdated": false, "path": "src/a.rs", "line": 7, "comments": [{"id": "5", "author": "hubot", "body": "nit", "created_at_ms": 1_700_000_060_000_i64, "url": null}]},
        {"id": "PRRT_2", "resolved": true, "outdated": true, "path": null, "line": null, "comments": []}]})));
    // When the phone reads the activity and the comments of a thread, and of one that does not exist
    let activity = rpc.call("pullRequests.activity", pr_ref(12));
    let thread = rpc.call("pullRequests.threadComments", with(pr_ref(12), json!({"threadId": "PRRT_1", "cursor": "c"})));
    let missing = rpc.call("pullRequests.threadComments", with(pr_ref(12), json!({"threadId": "PRRT_404", "cursor": "c"})));
    // Then the conversation holds the comment and the review (with its state), only the thread with a file is a review thread, and a missing thread is not found
    let activity = success(&activity);
    let kinds: Vec<(&str, &str)> = activity["comments"].as_array().unwrap().iter().map(|c| (c["kind"].as_str().unwrap(), c["author"]["login"].as_str().unwrap())).collect();
    assert_eq!(kinds, [("issue-comment", "octocat"), ("review", "hubot")]);
    assert_eq!((activity["comments"][1]["reviewState"].clone(), activity["reviewThreads"].as_array().unwrap().len(), activity["reviewThreads"][0]["line"].clone(), activity["commentCount"].clone()), (json!("approved"), 1, json!(7), json!(2)));
    assert_eq!((success(&thread)["comments"][0]["body"].clone(), failure(&missing)["reason"].clone()), (json!("nit"), json!("not-found")));
}

#[test]
fn the_sign_in_shows_the_code_and_the_address_waits_for_the_flow_it_started_and_never_carries_a_token() {
    // Given an executor that starts a flow and then reports the account
    let (h, mut rpc) = started();
    reply(&h, "sourceControl.signInStart", Ok(json!({"flow": "f1", "provider": "github", "host": "github.com", "user_code": "ABCD-1234", "verification_uri": "https://github.com/login/device"})));
    reply(&h, "sourceControl.signInWait", Ok(json!({"flow": "f1", "state": "signed_in", "account": "octocat"})));
    reply(&h, "sourceControl.signInCancel", Ok(json!({"flow": "f1", "cancelled": true})));
    let host = json!({"provider": "github", "host": "github.com"});
    // When the phone waits before starting, starts, waits, and cancels after the sign-in ended
    let early = rpc.call("sourceControl.signInWait", host.clone());
    let start = rpc.call("sourceControl.signInStart", host.clone());
    let wait = rpc.call("sourceControl.signInWait", host.clone());
    let late = rpc.call("sourceControl.signInCancel", host.clone());
    // Then the first is "not-started" with nothing asked, the prompt is the code and the address, the wait names the account and the flow is forgotten
    assert_eq!((failure(&early)["_tag"].clone(), failure(&early)["reason"].clone()), (json!("SourceControlSignInError"), json!("not-started")));
    assert_eq!((success(&start)["userCode"].clone(), success(&start)["verificationUri"].clone()), (json!("ABCD-1234"), json!("https://github.com/login/device")));
    assert!(success(&start)["expiresAt"].as_i64().unwrap() > 0);
    assert_eq!(success(&wait)["account"], "octocat");
    assert_eq!(failure(&late)["reason"], "not-started");
    // And the executor was asked to start, then to wait for the flow it named, and no frame the phone got has a token
    let asked: Vec<(String, Value)> = hosted(&h).into_iter().map(|(m, p)| (m, p["flow"].clone())).collect();
    assert_eq!(asked, [("sourceControl.signInStart".to_string(), Value::Null), ("sourceControl.signInWait".to_string(), json!("f1"))]);
    assert!(![&start, &wait].iter().any(|v| v.to_string().contains("token")));
}

#[test]
fn the_discovery_says_which_cli_is_there_and_who_it_is_signed_in_as_and_a_machine_without_gh_is_missing_not_an_error() {
    // Given an executor signed in to GitHub with no glab
    let (h, mut rpc) = started();
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        Op::Hosted { method: "sourceControl.discover", params, .. } if params["provider"] == "github" => Some(Ok(json!({"providers": [{"provider": "github", "installed": true, "signed_in": true, "host": "github.com", "account": "octocat", "scopes": ["repo"]}]}))),
        Op::Hosted { method: "sourceControl.discover", .. } => Some(Err(AgentError::Refused { code: "CLI_NOT_INSTALLED".into(), message: "glab is not installed on the executor machine".into() })),
        _ => None,
    }));
    // When the phone discovers source control
    let discovery = rpc.call("server.discoverSourceControl", json!({}));
    // Then GitHub is available and authenticated as octocat, GitLab is missing, and git is listed
    let providers = &success(&discovery)["sourceControlProviders"];
    assert_eq!((providers[0]["status"].clone(), providers[0]["auth"]["status"].clone(), providers[0]["auth"]["account"].clone(), providers[0]["executable"].clone()), (json!("available"), json!("authenticated"), json!({"_tag": "Some", "value": "octocat"}), json!("gh")));
    assert_eq!((providers[1]["status"].clone(), providers[1]["auth"]["status"].clone()), (json!("missing"), json!("unknown")));
    assert_eq!(success(&discovery)["versionControlSystems"][0]["kind"], "git");
}

#[test]
fn a_clone_makes_the_project_starts_the_executors_clone_streams_its_states_and_can_be_cancelled_and_retried() {
    // Given an executor that starts clones c1 and c2 and answers their state
    let (h, mut rpc) = started();
    let counter = Mutex::new(0);
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::Hosted { method: "clone.start", .. } => {
            let mut n = counter.lock().unwrap();
            *n += 1;
            Some(Ok(json!({"workspace": "w", "clone": format!("c{n}"), "repo": REPO, "state": "running"})))
        }
        Op::Hosted { method: "clone.status", params, .. } => Some(Ok(json!({"workspace": "w", "clone": params["clone"], "repo": REPO, "state": "running", "detail": {}, "seq": 1}))),
        Op::Hosted { method: "clone.cancel", params, .. } => Some(Ok(json!({"clone": params["clone"], "cancelled": true}))),
        _ => None,
    }));
    h.agent.clone_states.lock().unwrap().insert(String::new(), vec![json!({"clone": "c1", "state": "done", "repo": REPO, "detail": {"path": "/workspace/hello"}, "seq": 2})]);
    // When the phone starts a clone of a repository into a new project
    let start = rpc.call("projectClone.start", json!({"projectId": "p-clone", "title": "Hello", "createdAt": "2026-01-01T00:00:00.000Z", "repository": REPO, "provider": "github", "destinationPath": "/workspace/hello"}));
    // Then the project exists on the destination, and the executor was asked to clone that repository into it
    let start = success(&start);
    assert_eq!((start["projectId"].clone(), start["cwd"].clone(), start["remoteUrl"].clone(), start["repository"]["nameWithOwner"].clone()), (json!("p-clone"), json!("/workspace/hello"), json!("https://github.com/octo/hello.git"), json!(REPO)));
    let asked = hosted(&h);
    assert_eq!((asked[0].0.as_str(), asked[0].1["repo"].clone(), asked[0].1["dest"].clone()), ("clone.start", json!(REPO), json!("/workspace/hello")));
    // When the phone subscribes it sees the running clone first, and then the one the log says finished
    let stream = rpc.send("subscribeProjectClones", json!({}));
    let first = rpc.chunk(&stream);
    let snapshot = &first[0][0];
    assert_eq!((snapshot["projectId"].clone(), snapshot["phase"].clone(), snapshot["destinationPath"].clone()), (json!("p-clone"), json!("running"), json!("/workspace/hello")));
    let later = rpc.chunk(&stream);
    assert_eq!((later[0][0]["phase"].clone(), later[0][0]["percent"].clone(), later[0][0]["sequence"].clone()), (json!("done"), json!(100), json!(2)));
    // When the phone cancels and retries
    let cancel = rpc.call("projectClone.cancel", json!({"projectId": "p-clone"}));
    let retry = rpc.call("projectClone.retry", json!({"projectId": "p-clone"}));
    let unknown = rpc.call("projectClone.cancel", json!({"projectId": "p-none"}));
    // Then the cancel named the clone that started, the retry started another and an unknown project applies nothing
    assert_eq!((success(&cancel)["applied"].clone(), success(&retry)["applied"].clone(), success(&unknown)["applied"].clone()), (json!(true), json!(true), json!(false)));
    let methods: Vec<(String, Value)> = hosted(&h).into_iter().filter(|(m, _)| m == "clone.cancel" || m == "clone.start").map(|(m, p)| (m, p["clone"].clone())).collect();
    assert_eq!(methods, [("clone.start".to_string(), Value::Null), ("clone.cancel".to_string(), json!("c1")), ("clone.start".to_string(), Value::Null)]);
}

#[test]
fn a_thread_links_watches_and_unlinks_a_pull_request_in_the_agent_apis_state_and_the_pull_request_lists_its_threads() {
    // Given a state store behind the agent double
    let (h, mut rpc) = started();
    let store = std::sync::Arc::new(Mutex::new(std::collections::HashMap::<String, Value>::new()));
    let seen = store.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::StateGet { key, .. } => Some(Ok(json!({"value": seen.lock().unwrap().get(key).cloned().unwrap_or(Value::Null)}))),
        Op::StateSet { key, value, .. } => {
            seen.lock().unwrap().insert(key.clone(), value.clone());
            Some(Ok(json!({})))
        }
        _ => None,
    }));
    let key = json!({"host": "github.com", "repository": REPO, "number": 12});
    let command = |kind: &str, id: &str, extra: Value| with(with(json!({"type": kind, "commandId": id, "threadId": "t1"}), key.clone()), extra);
    // When the thread links the pull request, watches it, and the phone asks which threads link it
    let link = rpc.call("orchestration.dispatchCommand", command("thread.pull-request.link", "c-l", json!({"url": "https://github.com/octo/hello/pull/12", "source": "manual"})));
    let watch = rpc.call("orchestration.dispatchCommand", command("thread.pull-request.watch", "c-w", json!({"watching": true})));
    let linked = rpc.call("pullRequests.linkedThreads", pr_ref(12));
    // Then both were accepted, the state holds the link as watched, and the thread is listed with its project and title
    assert_eq!((link["_tag"].clone(), watch["_tag"].clone()), (json!("Success"), json!("Success")));
    assert_eq!(store.lock().unwrap()["thread.pullRequests:t1"], json!([{"host": "github.com", "repository": REPO, "number": 12, "url": "https://github.com/octo/hello/pull/12", "source": "manual", "watching": true}]));
    assert_eq!((success(&linked)["threads"][0]["id"].clone(), success(&linked)["threads"][0]["projectId"].clone()), (json!("t1"), json!("p1")));
    // When the thread unlinks it
    let unlink = rpc.call("orchestration.dispatchCommand", command("thread.pull-request.unlink", "c-u", json!({})));
    let after = rpc.call("pullRequests.linkedThreads", pr_ref(12));
    // Then the state holds no link and no thread is listed; and a link without its address is refused
    assert_eq!((unlink["_tag"].clone(), store.lock().unwrap()["thread.pullRequests:t1"].clone(), success(&after)["threads"].clone()), (json!("Success"), json!([]), json!([])));
    let bad = rpc.call("orchestration.dispatchCommand", command("thread.pull-request.link", "c-b", json!({})));
    assert_eq!(bad["_tag"], "Failure");
}

#[test]
fn the_files_a_person_marked_viewed_are_kept_in_state_and_read_back_and_the_diff_the_phone_pages_is_the_executors_cut_diff() {
    // Given a state store and the executor's diff
    let (h, mut rpc) = started();
    let store = std::sync::Arc::new(Mutex::new(std::collections::HashMap::<String, Value>::new()));
    let seen = store.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::StateGet { key, .. } => Some(Ok(json!({"value": seen.lock().unwrap().get(key).cloned().unwrap_or(Value::Null)}))),
        Op::StateSet { key, value, .. } => {
            seen.lock().unwrap().insert(key.clone(), value.clone());
            Some(Ok(json!({})))
        }
        _ => None,
    }));
    // When the phone marks two files viewed, unmarks one and reads the marks
    for files in [json!([{"path": "a.rs", "viewed": true}, {"path": "b.rs", "viewed": true}]), json!([{"path": "a.rs", "viewed": false}])] {
        assert_eq!(rpc.call("pullRequests.setFilesViewed", with(pr_ref(12), json!({"files": files})))["_tag"], "Success");
    }
    let viewed = rpc.call("pullRequests.filesViewed", pr_ref(12));
    // Then only the file still marked is listed, as viewed, and the marks of another pull request are separate
    assert_eq!(success(&viewed)["files"], json!([{"path": "b.rs", "state": "viewed"}]));
    assert_eq!(success(&rpc.call("pullRequests.filesViewed", pr_ref(13)))["files"], json!([]));
}

fn asked(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

#[test]
fn a_pull_request_in_a_native_stack_gives_its_layers_bottom_to_top_and_one_in_none_gives_null() {
    // Given the executor's stack of pull request 12 (a merged layer below it), and later none
    let (h, mut rpc) = started();
    reply(&h, "pr.stack", Ok(json!({"stack": {"id": "501", "number": 3, "url": "https://github.com/octo/hello/stack/3", "base": "main", "layers": [
        {"number": 11, "title": "Part one", "draft": false, "head_ref": "feat-a", "head_sha": "a".repeat(40), "state": "merged"},
        {"number": 12, "title": null, "draft": null, "head_ref": "feat-b", "head_sha": null, "state": "open"}]}})));
    // When the phone asks for the stack
    let stack = rpc.call("pullRequests.stack", pr_ref(12));
    // Then it is T3's stack: the id, the base and each layer with only what the host gave
    let stack = success(&stack);
    assert_eq!((stack["id"].clone(), stack["base"].clone(), stack["url"].clone()), (json!("501"), json!("main"), json!("https://github.com/octo/hello/stack/3")));
    assert_eq!(stack["layers"][0], json!({"number": 11, "headBranch": "feat-a", "state": "merged", "title": "Part one", "headSha": "a".repeat(40), "isDraft": false}));
    assert_eq!(stack["layers"][1], json!({"number": 12, "headBranch": "feat-b", "state": "open"}));
    assert_eq!(hosted(&h)[0], ("pr.stack".to_string(), json!({"provider": "github", "repo": REPO, "number": 12})));
    // When the executor says it is in none
    reply(&h, "pr.stack", Ok(json!({"stack": null})));
    let none = rpc.call("pullRequests.stack", pr_ref(12));
    // Then the answer is null, and a refusal of the executor is the operation error
    assert_eq!(success(&none), &Value::Null);
    reply(&h, "pr.stack", refused("IO", "gh exited 1: boom"));
    assert_eq!(failure(&rpc.call("pullRequests.stack", pr_ref(12)))["_tag"], "PullRequestOperationError");
}

/// What the executor says of pull request 12 for the file contents read: its files, its head and base, the compare and each blob by revision.
fn expect_head_read(h: &Harness) {
    reply(h, "pr.view", Ok(neutral_pr()));
    reply(h, "pr.files", Ok(json!({"files": [{"path": "src/up.rs", "additions": 3, "deletions": 1, "status": "modified"}, {"path": "src/new.rs", "additions": 5, "deletions": 0, "status": "added"}]})));
    reply(h, "git.fetch", Ok(json!({"remote": "origin", "output": "", "refs": [{"ref": "refs/rca/pr/12/head", "sha": "h".repeat(40)}, {"ref": "refs/rca/pr/12/base", "sha": "b".repeat(40)}]})));
    reply(h, "git.compare", Ok(json!({"base": "b".repeat(40), "head": "h".repeat(40), "merge_base": "m".repeat(40), "ahead": 2, "behind": 0, "files": [], "truncated": false, "commits": [
        {"sha": format!("{}{}", "c1", "0".repeat(38)), "parents": ["m".repeat(40)]}, {"sha": "h".repeat(40), "parents": [format!("{}{}", "c1", "0".repeat(38))]}]})));
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        Op::GitBlob { rev, path, .. } if path == "src/gone.rs" => Some(Err(AgentError::Refused { code: "NOT_FOUND".into(), message: format!("{path} does not exist at {rev}") })),
        Op::GitBlob { rev, path, .. } => Some(Ok(json!({"text": format!("{path} at {}\n", &rev[..2]), "truncated": false}))),
        _ => None,
    }));
}

#[test]
fn the_contents_of_a_changed_file_are_read_at_the_merge_base_and_the_head_of_the_fetched_pull_request_and_a_commit_gives_its_parent_and_itself() {
    // Given the executor's pull request 12, its two changed files, the fetched head and base, and the blobs of the repository
    let (h, mut rpc) = started();
    expect_head_read(&h);
    let input = |change: &str, old: &str, new: &str, extra: Value| with(with(pr_ref(12), json!({"changeType": change, "oldPath": old, "newPath": new})), extra);
    // When the phone asks for a changed file, a new one, a deleted one, and a file at one commit of the pull request
    let changed = rpc.call("pullRequests.diffFileContents", input("change", "src/up.rs", "src/up.rs", json!({})));
    let new = rpc.call("pullRequests.diffFileContents", input("new", "src/new.rs", "src/new.rs", json!({})));
    let commit = rpc.call("pullRequests.diffFileContents", input("change", "src/up.rs", "src/up.rs", json!({"commit": "c100000"})));
    // Then the old side is the merge base's and the new side the head's, a new file has no old side, and a commit reads itself and its parent
    assert_eq!(success(&changed), &json!({"oldContents": "src/up.rs at mm\n", "newContents": "src/up.rs at hh\n"}));
    assert_eq!(success(&new), &json!({"oldContents": "", "newContents": "src/new.rs at hh\n"}));
    assert_eq!(success(&commit), &json!({"oldContents": "src/up.rs at mm\n", "newContents": "src/up.rs at c1\n"}));
    // And the head was fetched from the pull request's own ref, with the base branch, before any blob was read
    let ops = asked(&h);
    assert_eq!(ops.iter().find_map(|o| if let Op::GitFetch { refspecs, .. } = o { Some(refspecs.clone()) } else { None }), Some(vec!["+refs/pull/12/head:refs/rca/pr/12/head".to_string(), "+refs/heads/main:refs/rca/pr/12/base".to_string()]));
    let first_blob = ops.iter().position(|o| matches!(o, Op::GitBlob { .. })).unwrap();
    assert!(ops[..first_blob].iter().any(|o| matches!(o, Op::GitFetch { .. })));
}

#[test]
fn a_file_the_pull_request_does_not_change_a_commit_it_does_not_have_and_a_file_that_is_gone_are_refused_before_anything_is_returned() {
    // Given the same pull request
    let (h, mut rpc) = started();
    expect_head_read(&h);
    let input = |old: &str, new: &str, extra: Value| with(with(pr_ref(12), json!({"changeType": "change", "oldPath": old, "newPath": new})), extra);
    // When the phone asks for a file the pull request does not change, a commit that is not in it, and a changed file the head lacks
    let other = rpc.call("pullRequests.diffFileContents", input("Cargo.toml", "Cargo.toml", json!({})));
    let stranger = rpc.call("pullRequests.diffFileContents", input("src/up.rs", "src/up.rs", json!({"commit": "dead000"})));
    let gone = rpc.call("pullRequests.diffFileContents", input("src/up.rs", "src/gone.rs", json!({"commit": "c100000"})));
    // Then each is the operation error that says why, and nothing of the repository was read for the first
    assert_eq!(failure(&other)["detail"], "This file is not part of the change request.");
    assert_eq!(failure(&stranger)["detail"], "This commit is not part of the change request.");
    assert_eq!((failure(&gone)["_tag"].clone(), failure(&gone)["reason"].clone()), (json!("PullRequestOperationError"), json!("not-found")));
    let blobs_of_other = asked(&h).iter().filter(|o| matches!(o, Op::GitBlob { path, .. } if path == "Cargo.toml")).count();
    assert_eq!(blobs_of_other, 0);
}

fn prepare(mode: &str, reference: &str) -> Value {
    json!({"cwd": ROOT, "reference": reference, "mode": mode})
}

fn expect_prepare(h: &Harness) {
    reply(h, "pr.view", Ok(neutral_pr()));
    reply(h, "git.fetch", Ok(json!({"remote": "origin", "output": "", "refs": [{"ref": "refs/rca/pr/12/head", "sha": "h".repeat(40)}]})));
    reply(h, "git.status", Ok(json!({"branch": "main", "head": "m".repeat(40), "detached": false, "files": [{"path": "notes.txt", "index": "?", "worktree": "?", "conflicted": false}], "clean": false, "ahead": 0, "behind": 0, "truncated": false})));
    reply(h, "git.refs", Ok(json!({"current": "main", "refs": [{"name": "main", "sha": "m".repeat(40), "kind": "branch", "current": true, "full": "refs/heads/main"}]})));
    reply(h, "git.createRef", Ok(json!({"name": "t3code/pr-12/feat-retry"})));
    reply(h, "git.checkout", Ok(json!({"branch": "t3code/pr-12/feat-retry"})));
    reply(h, "worktree.list", Ok(json!({"worktrees": [{"path": ROOT, "head": "m".repeat(40), "branch": "main", "is_main": true}]})));
    reply(h, "worktree.create", Ok(json!({"path": "/base/wt-feat", "branch": "t3code/pr-12/feat-retry", "head": "h".repeat(40)})));
}

#[test]
fn a_pull_request_is_prepared_locally_as_a_branch_of_its_own_at_the_fetched_head_and_uncommitted_changes_stop_it() {
    // Given the executor's pull request 12, a repository with only an untracked file, and a fetched head
    let (h, mut rpc) = started();
    expect_prepare(&h);
    // When the phone prepares the pull request in the project itself, by its address
    let prepared = rpc.call("git.preparePullRequestThread", prepare("local", "https://github.com/octo/hello/pull/12"));
    // Then the branch is created at the head and checked out, and the answer names the pull request, the branch and no worktree
    let prepared = success(&prepared);
    assert_eq!((prepared["pullRequest"]["number"].clone(), prepared["pullRequest"]["headBranch"].clone(), prepared["pullRequest"]["baseBranch"].clone(), prepared["branch"].clone()), (json!(12), json!("feat/retry"), json!("main"), json!("t3code/pr-12/feat-retry")));
    assert_eq!((prepared["worktreePath"].clone(), prepared["isOnPullRequestHead"].clone()), (Value::Null, json!(true)));
    assert!(asked(&h).contains(&Op::GitCreateRef { root: ROOT.into(), name: "t3code/pr-12/feat-retry".into(), from: Some("h".repeat(40)), checkout: true }));
    // When the branch exists and is behind, and then a tracked file is modified
    reply(&h, "git.refs", Ok(json!({"current": "main", "refs": [{"name": "t3code/pr-12/feat-retry", "sha": "o".repeat(40), "kind": "branch", "current": false, "full": "refs/heads/t3code/pr-12/feat-retry"}]})));
    let behind = rpc.call("git.preparePullRequestThread", prepare("local", "12"));
    reply(&h, "git.status", Ok(json!({"branch": "main", "head": "m".repeat(40), "detached": false, "files": [{"path": "a.rs", "index": ".", "worktree": "M", "conflicted": false}], "clean": false, "ahead": 0, "behind": 0, "truncated": false})));
    let before = asked(&h).len();
    let dirty = rpc.call("git.preparePullRequestThread", prepare("local", "12"));
    // Then the first checks the existing branch out and says it is not on the head, and the second is refused before anything is fetched
    assert_eq!((success(&behind)["isOnPullRequestHead"].clone(), success(&behind)["branch"].clone()), (json!(false), json!("t3code/pr-12/feat-retry")));
    assert!(asked(&h).contains(&Op::GitCheckout { root: ROOT.into(), target: "t3code/pr-12/feat-retry".into(), track: false }));
    assert_eq!((failure(&dirty)["_tag"].clone(), failure(&dirty)["detail"].as_str().unwrap().starts_with("This repository has uncommitted changes.")), (json!("GitManagerError"), true));
    assert!(asked(&h)[before..].iter().all(|o| !matches!(o, Op::GitFetch { .. })), "a fetch ran for a repository with changes");
}

#[test]
fn a_pull_request_is_prepared_in_a_worktree_made_at_the_head_reused_when_the_branch_has_one_and_refused_when_the_main_checkout_holds_it() {
    // Given the executor's pull request 12 and a repository with no worktree of it
    let (h, mut rpc) = started();
    expect_prepare(&h);
    // When the phone prepares it in a worktree
    let made = rpc.call("git.preparePullRequestThread", prepare("worktree", "#12"));
    // Then a worktree is made on the branch at the fetched head, and its path is the executor's
    let made = success(&made);
    assert_eq!((made["worktreePath"].clone(), made["branch"].clone(), made["isOnPullRequestHead"].clone()), (json!("/base/wt-feat"), json!("t3code/pr-12/feat-retry"), json!(true)));
    let creates: Vec<Op> = asked(&h).into_iter().filter(|o| matches!(o, Op::WorktreeCreate { .. })).collect();
    assert!(matches!(&creates[..], [Op::WorktreeCreate { root, branch, base: Some(base), .. }] if root == ROOT && branch == "t3code/pr-12/feat-retry" && *base == "h".repeat(40)), "{creates:?}");
    // When the branch already has a worktree that is behind, and when the main checkout is on it
    reply(&h, "worktree.list", Ok(json!({"worktrees": [{"path": "/base/old", "head": "o".repeat(40), "branch": "t3code/pr-12/feat-retry", "is_main": false}]})));
    let reused = rpc.call("git.preparePullRequestThread", prepare("worktree", "12"));
    reply(&h, "worktree.list", Ok(json!({"worktrees": [{"path": ROOT, "head": "h".repeat(40), "branch": "t3code/pr-12/feat-retry", "is_main": true}]})));
    let held = rpc.call("git.preparePullRequestThread", prepare("worktree", "12"));
    // Then the worktree is reused as it is and reported behind, and the second is refused naming the main repo
    assert_eq!((success(&reused)["worktreePath"].clone(), success(&reused)["isOnPullRequestHead"].clone()), (json!("/base/old"), json!(false)));
    assert!(failure(&held)["detail"].as_str().unwrap().contains("already checked out in the main repo"));
    assert_eq!(asked(&h).iter().filter(|o| matches!(o, Op::WorktreeCreate { .. })).count(), 1);
}

#[test]
fn a_reference_that_is_no_pull_request_and_a_pull_request_the_host_does_not_have_are_manager_errors_and_nothing_is_fetched() {
    // Given an executor that does not know pull request 99
    let (h, mut rpc) = started();
    expect_prepare(&h);
    reply(&h, "pr.view", refused("NOT_FOUND", "gh: no pull request"));
    // When the phone prepares nonsense and pull request 99
    let nonsense = rpc.call("git.preparePullRequestThread", prepare("worktree", "feature-branch"));
    let missing = rpc.call("git.preparePullRequestThread", prepare("local", "99"));
    // Then both are the declared manager error, and no fetch ran
    assert_eq!((failure(&nonsense)["_tag"].clone(), failure(&missing)["_tag"].clone()), (json!("GitManagerError"), json!("GitManagerError")));
    assert!(asked(&h).iter().all(|o| !matches!(o, Op::GitFetch { .. })));
}

/// What the executor's diff of pull request 12 is: two files, the first with a hunk of lines 3-6.
const DIFF: &str = "diff --git a/src/up.rs b/src/up.rs\n--- a/src/up.rs\n+++ b/src/up.rs\n@@ -3,2 +3,4 @@ fn a() {\n keep\n-old\n+new\n+more\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-a\n+b\n";

/// A guide as a model may answer it: one stop with an excerpt that reaches past the hunk, a note inside it, and a file the change does not have.
fn model_guide() -> String {
    let guide = json!({"stops": [{"title": "Retry the upload", "summary": "Retries twice.", "attention": {"kind": "risk", "note": "a retry may repeat a write"},
        "excerpts": [{"path": "src/up.rs", "side": "new", "start": 1, "end": 40, "caption": "the retry"}, {"path": "missing.rs", "side": "new", "start": 1, "end": 2, "caption": "x"}],
        "files": [{"path": "src/up.rs"}, {"path": "missing.rs"}], "notes": [{"path": "src/up.rs", "line": 4, "title": "Why twice", "body": "see the test"}, {"path": "src/up.rs", "line": 90, "title": "Out", "body": "b"}], "questions": ["Is the write idempotent?"]}]});
    format!("Here is the tour.\n```json\n{guide}\n```")
}

type Stored = std::sync::Arc<Mutex<std::collections::HashMap<String, Value>>>;

/// The agent API's state as the guide reads and writes it: the settings the phone made (guides on, with a model) and every key written since.
fn state_with_settings(h: &Harness, settings: Value) -> Stored {
    let stored: Stored = Stored::default();
    let held = stored.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::StateGet { key, .. } if key == "server.settings" => Some(Ok(json!({"value": settings}))),
        Op::StateGet { key, .. } => Some(Ok(json!({"value": held.lock().unwrap().get(key).cloned()}))),
        Op::StateSet { key, value, .. } => {
            held.lock().unwrap().insert(key.clone(), value.clone());
            Some(Ok(json!({})))
        }
        _ => None,
    }));
    stored
}

fn guides_on() -> Value {
    json!({"reviewGuidesEnabled": true, "reviewGuideModelSelection": {"instanceId": "claudeAgent", "model": "opus"}, "reviewGuideInstructions": "Be brief."})
}

fn expect_guide_reads(h: &Harness) {
    reply(h, "pr.view", Ok(neutral_pr()));
    reply(h, "pr.diff", Ok(json!({"diff": DIFF, "truncated": false})));
    reply(h, "pr.threads", Ok(json!({"threads": [{"id": "T1", "path": "src/up.rs", "resolved": true, "outdated": false, "line": 3, "comments": [{"id": "1", "author": "hubot", "body": "Why retry here?", "created_at_ms": 1, "url": null}]}]})));
}

/// The guide the phone reads, once its run is over: waits for the run to end.
fn finished_guide(rpc: &mut Rpc) -> Value {
    for _ in 0..200 {
        let read = rpc.call("pullRequests.reviewGuide", pr_ref(12));
        let read = success(&read).clone();
        if read["run"].is_null() || read["run"]["_tag"] == "Failed" {
            return read;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("the review guide run never ended");
}

#[test]
fn a_review_guide_is_a_model_turn_over_the_diff_checked_against_it_kept_in_state_and_read_back_by_the_phone() {
    // Given guides turned on with a model, the executor's pull request, diff and settled thread, and a model that answers with a guide
    let (h, mut rpc) = started();
    let stored = state_with_settings(&h, guides_on());
    expect_guide_reads(&h);
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text(model_guide()), AgentEventKind::TurnFinished(Finish::Completed)];
    // When the phone reads the guide before any was written, asks for one, and reads it again once the run ended
    let before = success(&rpc.call("pullRequests.reviewGuide", pr_ref(12))).clone();
    let started = rpc.call("pullRequests.generateReviewGuide", pr_ref(12));
    let after = finished_guide(&mut rpc);
    // Then nothing was there before, the ask answered at once with a run for the head the host reports, and the guide is there after with no run
    assert_eq!((before["guide"].clone(), before["run"].clone(), before["currentHeadSha"].clone()), (Value::Null, Value::Null, json!("abc1234")));
    let started = success(&started);
    assert_eq!((started["run"]["_tag"].clone(), started["run"]["headSha"].clone(), started["started"].clone(), started["guide"].clone()), (json!("Running"), json!("abc1234"), json!(true), Value::Null));
    assert_eq!((after["run"].clone(), after["currentHeadSha"].clone()), (Value::Null, json!("abc1234")));
    let guide = &after["guide"];
    assert_eq!((guide["headSha"].clone(), guide["modelSelection"].clone()), (json!("abc1234"), json!({"instanceId": "claudeAgent", "model": "opus"})));
    // And the stops are the model's checked against the diff: the excerpt cut to the hunk, the note inside it kept, the unknown file dropped, the other changed file in a last stop
    let stops = guide["stops"].as_array().unwrap();
    assert_eq!(stops[0]["excerpts"], json!([{"path": "src/up.rs", "side": "new", "start": 3, "end": 6, "caption": "the retry"}]));
    assert_eq!((stops[0]["notes"].as_array().unwrap().len(), stops[0]["attention"]["kind"].clone(), stops[0]["questions"].clone(), stops[0]["files"].clone()), (1, json!("risk"), json!(["Is the write idempotent?"]), json!([])));
    assert_eq!((stops[1]["title"].clone(), stops[1]["files"].clone()), (json!("Other files"), json!([{"path": "README.md"}])));
    // And the model's turn was asked on a thread of its own with the pull request, the diff, the settled remark and the instructions, then the thread was deleted
    let turns = h.agent.turns.lock().unwrap().clone();
    assert_eq!((turns.len(), turns[0].instance_id.as_str(), turns[0].model.as_deref()), (1, "claudeAgent", Some("opus")));
    for part in ["Retry flaky fetch", "Base branch: main", "Retries twice", "- src/up.rs (resolved, hubot): Why retry here?", "Be brief.", DIFF] {
        assert!(turns[0].prompt.contains(part), "the prompt lacks {part:?}");
    }
    assert!(asked(&h).iter().any(|o| matches!(o, Op::ThreadDelete { thread, .. } if *thread == turns[0].thread_id)));
    assert_eq!(stored.lock().unwrap().get("pr.guide:p1:octo/hello:12").map(|g| g["stops"].as_array().unwrap().len()), Some(2));
}

#[test]
fn a_review_guide_is_refused_while_guides_are_off_or_no_model_is_set_a_second_ask_for_the_same_head_starts_nothing_and_a_failed_turn_is_a_failed_run() {
    // Given the default settings (guides off)
    let (h, mut rpc) = started();
    let _ = state_with_settings(&h, json!({}));
    expect_guide_reads(&h);
    // When the phone asks for a guide
    let off = rpc.call("pullRequests.generateReviewGuide", pr_ref(12));
    // Then it is the operation error T3 words, and no turn started
    assert_eq!(failure(&off)["detail"], "Review guides are off. Turn them on in Settings → Source Control.");
    // When guides are on and no model is set at all (the defaults name one, so it is cleared)
    let _ = state_with_settings(&h, json!({"reviewGuidesEnabled": true, "textGenerationModelSelection": null}));
    let unset = rpc.call("pullRequests.generateReviewGuide", pr_ref(12));
    assert_eq!(failure(&unset)["detail"], "Pick a model for review guides in Settings → Source Control.");
    assert!(h.agent.turns.lock().unwrap().is_empty());
    // When guides are on, the model's turn is held open, and the phone asks twice
    let _ = state_with_settings(&h, guides_on());
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    let first = rpc.call("pullRequests.generateReviewGuide", pr_ref(12));
    let second = rpc.call("pullRequests.generateReviewGuide", pr_ref(12));
    // Then the first starts a run and the second finds it and starts none
    assert_eq!((success(&first)["started"].clone(), success(&second)["started"].clone(), success(&second)["run"]["_tag"].clone()), (json!(true), json!(false), json!("Running")));
    for _ in 0..200 {
        if !h.agent.turns.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert_eq!(h.agent.turns.lock().unwrap().len(), 1);
    // When that turn fails, and a model answers a new head with something that is no guide
    h.agent.append(&h.agent.turns.lock().unwrap()[0].thread_id, Some("u1"), AgentEventKind::Failure("the model is out of credit".into()));
    h.agent.append(&h.agent.turns.lock().unwrap()[0].thread_id, Some("u1"), AgentEventKind::TurnFinished(Finish::Failed));
    let failed = finished_guide(&mut rpc);
    // Then the run is failed with the model's own words, and no guide was kept
    assert_eq!((failed["run"]["_tag"].clone(), failed["run"]["detail"].clone(), failed["guide"].clone()), (json!("Failed"), json!("the model is out of credit"), Value::Null));
    h.agent.hold.store(false, std::sync::atomic::Ordering::SeqCst);
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("I could not do that.".into()), AgentEventKind::TurnFinished(Finish::Completed)];
    rpc.call("pullRequests.generateReviewGuide", pr_ref(12));
    let nonsense = finished_guide(&mut rpc);
    assert_eq!((nonsense["run"]["_tag"].clone(), nonsense["run"]["detail"].clone()), (json!("Failed"), json!("The model's answer holds no JSON object.")));
}
