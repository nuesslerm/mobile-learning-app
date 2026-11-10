//! The M11 rows through the chain: each call goes from the phone through `t3-serve serve` to the real rca-service and on to the real rca-exec, whose
//! `gh` is the replay double of exec-testkit (the recorded exchanges of `exec-hosted` give what it prints and what the executor must answer).
//! A call counts for its row only when it was answered and both the agent API method and the executor action ran in its window; each test also
//! reads what the double was started with, since that, not the answer, is what the CLI was asked to do. No real token or account is involved.
use crate::chain::{hit, last_window};
use crate::rig::*;
use crate::support::*;
use exec_hosted::testing::{fixtures, Exchange};
use exec_testkit::Rule;

/// The first recorded exchange of `action` whose name begins with `name`.
pub fn exchange(action: &str, name: &str) -> Exchange {
    let found = fixtures().into_iter().find(|f| f.action == action).and_then(|f| f.exchanges.into_iter().find(|e| e.name.starts_with(name)));
    found.unwrap_or_else(|| panic!("no recorded {action} exchange named {name:?}"))
}

/// What the double prints from now on: the rules the exchanges recorded (with the repository's root where they name it) and `extra`.
pub fn script(chain: &Chain, exchanges: &[&Exchange], extra: &[Rule]) {
    let root = chain.repo.path().display().to_string();
    let mut rules: Vec<Value> = exchanges.iter().flat_map(|e| e.cli.iter().map(|r| serde_json::from_str(&r.to_string().replace("{root}", &root)).unwrap())).collect();
    rules.extend(extra.iter().map(|r| serde_json::to_value(r).unwrap()));
    chain.clis().replay(&rules, &[]);
}

pub fn rule(args: &[&str], stdout: &str) -> Rule {
    Rule { program: "gh".into(), args: args.iter().map(|a| a.to_string()).collect(), stdout: stdout.into(), prefix: true, ..Rule::default() }
}

pub fn result(e: &Exchange) -> &Value {
    &e.response["result"]
}

pub fn ran(executor: &Value, action: &str) -> bool {
    hit(executor, "tool", action).is_some_and(|h| h["ok"] == true)
}

pub fn asked(agent: &[(String, bool)], method: &str) -> bool {
    agent.iter().any(|(m, ok)| m == method && *ok)
}

/// The call just made went agent API method `method`, then executor action `action`, both answered.
pub fn went(case: &exec_testkit::Case, method: &str, action: &str) {
    let (agent, executor) = last_window(case);
    assert!(asked(&agent, method) && ran(&executor, action), "{method} / {action} did not both run: {agent:?} {executor}");
}

/// Every scope the pairing may grant: the actions of these rows are write scopes the default pairing lacks.
pub fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

pub fn reference(number: u64) -> Value {
    json!({"projectId": "p1", "repository": "octo/hello", "number": number})
}

pub fn with(mut base: Value, extra: Value) -> Value {
    base.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    base
}

/// The argv `gh` was started with, in order, without the program.
pub fn argvs(chain: &Chain) -> Vec<Vec<String>> {
    chain.clis().starts("gh").into_iter().map(|s| s.argv).collect()
}

#[test]
fn a_pull_request_is_read_from_gh_through_rca_and_the_phone_gets_its_summary_preview_detail_checks_and_diff() {
    exec_testkit::case!(SUITE, |case| {
        // Given a project on the repository, a gh that knows pull request 12 and its checks, and a paired phone
        let chain = Chain::start_hosted();
        let (view, checks, diff) = (exchange("hosted_pr_view", "github pull request"), exchange("hosted_pr_checks", "github checks, one"), exchange("hosted_pr_diff", "github diff"));
        script(&chain, &[&view, &checks, &diff], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone reads the summary, the preview, the detail, the checks and the diff
        let summary = phone.call(case, &chain, &["rpc:pullRequests.summary"], "pullRequests.summary", reference(12));
        went(case, "pr.view", "hosted_pr_view");
        let preview = phone.call(case, &chain, &["rpc:pullRequests.preview"], "pullRequests.preview", reference(12));
        let detail = phone.call(case, &chain, &["rpc:pullRequests.detail"], "pullRequests.detail", reference(12));
        let (agent, executor) = last_window(case);
        let checked = phone.call(case, &chain, &["rpc:pullRequests.checks"], "pullRequests.checks", reference(12));
        went(case, "pr.checks", "hosted_pr_checks");
        let headers = [("Authorization", phone.bearer.as_str()), ("x-t3-orchestration-protocol", "2"), ("Content-Type", "application/json")];
        let patch = http_call(case, &chain, &["http:pullRequests.diff"], ("POST", "/api/pull-requests/diff", &headers, &reference(12).to_string()), 200);
        went(case, "pr.diff", "hosted_pr_diff");
        // Then each is what gh printed, as the executor read it: the title, the branches, the body, the checks and the patch
        let pr = &result(&view)["pr"];
        let (summary, preview, detail, checked) = (success(&summary), success(&preview), success(&detail), success(&checked));
        assert_eq!((summary["title"].clone(), summary["headBranch"].clone(), summary["baseBranch"].clone(), summary["state"].clone()), (pr["title"].clone(), pr["head_ref"].clone(), pr["base_ref"].clone(), pr["state"].clone()));
        assert_eq!((preview["author"]["login"].clone(), preview["number"].clone()), (pr["author"].clone(), json!(12)));
        assert_eq!((detail["body"].clone(), detail["headSha"].clone(), detail["changedFiles"].clone()), (pr["body"].clone(), pr["head_sha"].clone(), pr["changed_files"].clone()));
        assert!(asked(&agent, "pr.view") && asked(&agent, "pr.checks") && ran(&executor, "hosted_pr_view") && ran(&executor, "hosted_pr_checks"), "{agent:?} {executor}");
        assert_eq!(checked["checks"].as_array().unwrap().len(), result(&checks)["checks"].as_array().unwrap().len());
        let shown = |name: &str| checked["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["status"].clone());
        for c in result(&checks)["checks"].as_array().unwrap() {
            let wanted = match c["state"].as_str().unwrap_or("") {
                "pass" => "success",
                "fail" => "failure",
                "cancel" => "cancelled",
                "skipped" => "skipped",
                _ => "pending",
            };
            assert_eq!(shown(c["name"].as_str().unwrap()), Some(json!(wanted)), "{} in {checked}", c["name"]);
        }
        assert_eq!(patch.body["patch"], result(&diff)["diff"]);
        // And gh was started for pull request 12 of octo/hello and nothing else
        assert!(argvs(&chain).iter().all(|a| a.iter().any(|x| x == "--repo=octo/hello" || x == "12")), "{:?}", argvs(&chain));
    });
}

#[test]
fn the_list_stats_and_filters_and_who_gh_is_signed_in_as_come_from_the_repositorys_own_remote_and_the_hosts_answers() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that names the repository's remote, lists its pull requests, files, labels and reviewers, and is signed in
        let chain = Chain::start_hosted();
        let (own, list, files, labels, people, status) = (
            exchange("hosted_repo_view", "the workspace's own remote"),
            exchange("hosted_pr_list", "github open pull requests"),
            exchange("hosted_pr_files", "github files"),
            exchange("hosted_repo_labels", "github"),
            exchange("hosted_repo_reviewers", "github"),
            exchange("hosted_auth_status", "github signed in"),
        );
        script(&chain, &[&own, &list, &files, &labels, &people, &status], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone lists the open pull requests, asks their stats, the label filter options and who it is routed as
        let listed = phone.call(case, &chain, &["rpc:pullRequests.list"], "pullRequests.list", json!({"state": "open"}));
        let (agent, executor) = last_window(case);
        assert!(asked(&agent, "sourceControl.lookupRepository") && asked(&agent, "pr.list") && ran(&executor, "hosted_repo_view") && ran(&executor, "hosted_pr_list"), "{agent:?} {executor}");
        let stats = phone.call(case, &chain, &["rpc:pullRequests.listStats"], "pullRequests.listStats", json!({"refs": [reference(12)]}));
        went(case, "pr.files", "hosted_pr_files");
        let options = phone.call(case, &chain, &["rpc:pullRequests.filterOptions"], "pullRequests.filterOptions", json!({"kind": "labels", "projectId": "p1"}));
        went(case, "pr.labelCandidates", "hosted_repo_labels");
        let routing = phone.call(case, &chain, &["rpc:pullRequests.routing"], "pullRequests.routing", reference(12));
        went(case, "sourceControl.discover", "hosted_auth_status");
        let identity = phone.call(case, &chain, &["rpc:pullRequests.routingIdentity"], "pullRequests.routingIdentity", json!({"host": "github.com"}));
        // Then the entries are the repository's pull requests, the stats are the files' sums, the options its labels, and the routing the signed-in account
        let slug = result(&own)["repo"]["slug"].clone();
        let prs = result(&list)["prs"].as_array().unwrap();
        let listed = success(&listed);
        assert_eq!(listed["entries"].as_array().unwrap().iter().map(|e| e["number"].clone()).collect::<Vec<_>>(), prs.iter().map(|p| p["number"].clone()).collect::<Vec<_>>());
        assert!(listed["entries"].as_array().unwrap().iter().all(|e| e["repository"] == slug && e["projectId"] == "p1"), "{listed}");
        assert_eq!(listed["errors"], json!([]));
        let sum = |key: &str| result(&files)["files"].as_array().unwrap().iter().map(|f| f[key].as_u64().unwrap()).sum::<u64>();
        assert_eq!((success(&stats)["stats"][0]["additions"].clone(), success(&stats)["stats"][0]["deletions"].clone()), (json!(sum("additions")), json!(sum("deletions"))));
        assert_eq!(success(&options)["labels"].as_array().unwrap().iter().map(|l| l["name"].clone()).collect::<Vec<_>>(), result(&labels)["labels"].as_array().unwrap().iter().map(|l| l["name"].clone()).collect::<Vec<_>>());
        let account = result(&status)["account"].clone();
        assert_eq!((success(&routing)["viewer"].clone(), success(&routing)["workspaceRoot"].clone(), success(&identity)["accountId"].clone(), listed["viewers"]["github.com"].clone()), (account.clone(), json!(chain.repo.path().display().to_string()), account.clone(), account));
    });
}

#[test]
fn the_activity_a_thread_the_breakdown_and_the_candidates_are_read_from_gh_and_know_what_the_pull_request_already_has() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh with the pull request's comments, reviews, threads, files, labels and reviewers
        let chain = Chain::start_hosted();
        let (view, comments, reviews, threads, files, labels, people) = (
            exchange("hosted_pr_view", "github pull request"),
            exchange("hosted_pr_comments", "github comments"),
            exchange("hosted_pr_reviews", "github"),
            exchange("hosted_pr_threads", "github review threads through graphql"),
            exchange("hosted_pr_files", "github files"),
            exchange("hosted_repo_labels", "github"),
            exchange("hosted_repo_reviewers", "github"),
        );
        script(&chain, &[&view, &comments, &reviews, &threads, &files, &labels, &people], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone reads the activity, one thread's comments, the breakdown and both candidate lists
        let activity = phone.call(case, &chain, &["rpc:pullRequests.activity"], "pullRequests.activity", reference(12));
        let (agent, executor) = last_window(case);
        assert!(["pr.comments", "pr.reviews", "pr.threads"].iter().all(|m| asked(&agent, m)) && ["hosted_pr_comments", "hosted_pr_reviews", "hosted_pr_threads"].iter().all(|a| ran(&executor, a)), "{agent:?} {executor}");
        let thread = result(&threads)["threads"].as_array().unwrap().iter().find(|t| t["path"].is_string()).expect("a thread with a file").clone();
        let comments_of = phone.call(case, &chain, &["rpc:pullRequests.threadComments"], "pullRequests.threadComments", with(reference(12), json!({"threadId": thread["id"], "cursor": "c"})));
        went(case, "pr.threads", "hosted_pr_threads");
        let breakdown = phone.call(case, &chain, &["rpc:pullRequests.changeBreakdown"], "pullRequests.changeBreakdown", reference(12));
        went(case, "pr.files", "hosted_pr_files");
        let label_candidates = phone.call(case, &chain, &["rpc:pullRequests.labelCandidates"], "pullRequests.labelCandidates", reference(12));
        went(case, "pr.labelCandidates", "hosted_repo_labels");
        let reviewer_candidates = phone.call(case, &chain, &["rpc:pullRequests.reviewerCandidates"], "pullRequests.reviewerCandidates", reference(12));
        went(case, "pr.reviewerCandidates", "hosted_repo_reviewers");
        // Then the conversation holds gh's comments and reviews, the thread is read by its id, and the totals of the breakdown are the files'
        let activity = success(&activity);
        let conversation = activity["comments"].as_array().unwrap();
        assert_eq!(conversation.len(), result(&comments)["comments"].as_array().unwrap().len() + result(&reviews)["reviews"].as_array().unwrap().len());
        assert!(activity["reviewThreads"].as_array().unwrap().iter().any(|t| t["id"] == thread["id"] && t["path"] == thread["path"]), "{activity}");
        assert_eq!(success(&comments_of)["comments"].as_array().unwrap().len(), thread["comments"].as_array().unwrap().len());
        let total = |part: &Value| part["additions"].as_u64().unwrap() + part["deletions"].as_u64().unwrap();
        let parts: u64 = ["code", "tests", "mocks", "generated", "docs"].iter().map(|k| total(&success(&breakdown)[k])).sum();
        assert_eq!(parts, result(&files)["files"].as_array().unwrap().iter().map(|f| f["additions"].as_u64().unwrap() + f["deletions"].as_u64().unwrap()).sum::<u64>());
        let applied: Vec<&str> = result(&view)["pr"]["labels"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for candidate in success(&label_candidates)["candidates"].as_array().unwrap() {
            assert_eq!(candidate["isApplied"], json!(applied.contains(&candidate["name"].as_str().unwrap())), "{candidate}");
        }
        assert_eq!(success(&reviewer_candidates)["candidates"].as_array().unwrap().len(), result(&people)["users"].as_array().unwrap().len());
    });
}

#[test]
fn each_action_of_the_phone_starts_gh_with_what_the_phone_sent_the_text_on_stdin_and_never_as_an_option() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that accepts every action (what it prints is what the real one prints for the call)
        let chain = Chain::start_hosted();
        chain.clis().rules(
            &[
                rule(&["pr", "comment"], "https://github.com/octo/hello/pull/12#issuecomment-1\n"),
                rule(&["api", "graphql", "-f=thread=PRRT_1", "-f=body=Fixed"], "{\"data\":{\"addPullRequestReviewThreadReply\":{\"comment\":{\"id\":\"PRRC_1\",\"url\":\"https://x/c\"}}}}"),
                rule(&["api", "graphql", "-f=thread=PRRT_1"], "{\"data\":{\"resolveReviewThread\":{\"thread\":{\"isResolved\":true}}}}"),
                rule(&["pr"], ""),
            ],
            &[],
        );
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let body = "Looks good\n- but `mind` the **retry**";
        // When the phone comments, replies, resolves, asks for a review, labels, reviews, edits, and merges
        let comment = phone.call(case, &chain, &["rpc:pullRequests.comment"], "pullRequests.comment", with(reference(12), json!({"body": body})));
        went(case, "pr.comment", "hosted_pr_comment");
        let reply = phone.call(case, &chain, &["rpc:pullRequests.replyToThread"], "pullRequests.replyToThread", with(reference(12), json!({"threadId": "PRRT_1", "body": "Fixed"})));
        went(case, "pr.reply", "hosted_pr_reply");
        let resolve = phone.call(case, &chain, &["rpc:pullRequests.setThreadResolution"], "pullRequests.setThreadResolution", with(reference(12), json!({"threadId": "PRRT_1", "resolved": true})));
        went(case, "pr.resolveThread", "hosted_pr_resolve_thread");
        let reviewers = phone.call(case, &chain, &["rpc:pullRequests.requestReviewers"], "pullRequests.requestReviewers", with(reference(12), json!({"reviewers": [{"id": "hubot", "kind": "user"}], "requested": true})));
        went(case, "pr.requestReviewers", "hosted_pr_request_reviewers");
        let set_labels = phone.call(case, &chain, &["rpc:pullRequests.setLabels"], "pullRequests.setLabels", with(reference(12), json!({"labels": ["bug", "good first issue"], "applied": true})));
        went(case, "pr.setLabels", "hosted_pr_labels");
        let review = phone.call(case, &chain, &["rpc:pullRequests.submitReview"], "pullRequests.submitReview", with(reference(12), json!({"verdict": "approve", "body": "ship it", "comments": []})));
        went(case, "pr.review", "hosted_pr_review");
        let update = phone.call(case, &chain, &["rpc:pullRequests.update"], "pullRequests.update", with(reference(12), json!({"title": "New title", "body": "New *body*"})));
        went(case, "pr.edit", "hosted_pr_edit");
        let merge = phone.call(case, &chain, &["rpc:pullRequests.runAction"], "pullRequests.runAction", with(reference(12), json!({"action": "merge", "mergeMethod": "squash"})));
        went(case, "pr.merge", "hosted_pr_merge");
        // Then each was accepted
        for exit in [&comment, &reply, &resolve, &reviewers, &set_labels, &review, &update, &merge] {
            assert_eq!(exit["_tag"], "Success", "{exit}");
        }
        // And gh was started with each action's own words: the comment's body on stdin (never an option), the labels, the reviewer, the verdict, the squash
        let starts = chain.clis().starts("gh");
        let argv = |first: &str| starts.iter().find(|s| s.argv.first().map(String::as_str) == Some(first) && s.argv.get(1).map(String::as_str) == Some("comment")).map(|s| (s.argv.clone(), s.stdin.clone()));
        let (comment_argv, comment_stdin) = argv("pr").expect("pr comment was started");
        assert_eq!((comment_argv.contains(&"--body-file=-".to_string()), comment_stdin.as_str()), (true, body));
        assert!(!starts.iter().any(|s| s.argv.iter().any(|a| a.contains("Looks good"))), "the body reached gh as an argument");
        let has = |flag: &str| starts.iter().any(|s| s.argv.iter().any(|a| a == flag));
        assert!(has("--add-label=bug") && has("--add-label=good first issue") && has("--add-reviewer=hubot") && has("--approve") && has("--squash") && has("--title=New title"), "{starts:?}");
        assert!(starts.iter().any(|s| s.argv.first().map(String::as_str) == Some("pr") && s.argv.get(1).map(String::as_str) == Some("review") && s.stdin == "ship it"));
        assert!(starts.iter().all(|s| s.matched), "a start no rule answered: {starts:?}");
    });
}

#[test]
fn a_gh_that_is_not_installed_or_signed_out_is_the_phones_unavailable_error_and_a_hostile_label_never_starts_gh() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that says the machine is signed out for the view
        let chain = Chain::start_hosted();
        let signed_out = exchange("hosted_repo_view", "a machine that is not signed in says so");
        script(&chain, &[&signed_out], &[Rule { program: "gh".into(), args: vec!["pr".into(), "view".into(), "12".into(), "--repo=octo/hello".into()], prefix: true, stderr: "To get started with GitHub CLI, please run:  gh auth login\n".into(), exit_code: 4, ..Rule::default() }]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone reads the summary, and sets a label that would be an option
        let summary = case.call(&chain.tap, &rows(&["rpc:pullRequests.summary"]), || {
            let exit = phone.calls.call("pullRequests.summary", reference(12));
            (exit["_tag"] == "Failure", exit)
        });
        let before = chain.clis().starts("gh").len();
        let hostile = case.call(&chain.tap, &rows(&["rpc:pullRequests.setLabels"]), || {
            let exit = phone.calls.call("pullRequests.setLabels", with(reference(12), json!({"labels": ["--remove-label=x"], "applied": true})));
            (exit["_tag"] == "Failure", exit)
        });
        // Then the first is the unavailable error with the reason T3 names, the second the operation error from the executor's own validation, and gh was not started for it
        let error = failure(&summary);
        assert_eq!((error["_tag"].as_str(), error["reason"].as_str()), (Some("PullRequestUnavailableError"), Some("cli-unauthenticated")), "{error}");
        assert_eq!(failure(&hostile)["_tag"], "PullRequestOperationError");
        assert_eq!(chain.clis().starts("gh").len(), before, "gh was started for a label that is an option");
    });
}

#[test]
fn the_sign_in_shows_the_cli_code_and_address_waits_for_the_account_cancels_another_and_discovery_says_who_is_signed_in_with_no_token_in_any_answer() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that prints a code and an address for its web login and then shows the account (its status text holds a token line)
        let chain = Chain::start_hosted();
        let login = Rule { stderr: "! First copy your one-time code: ABCD-1234\nOpen this URL to continue in your web browser: https://github.com/login/device\n".into(), sleep_ms: 600, ..rule(&["auth", "login", "--web", "--hostname=github.com", "--git-protocol=https"], "") };
        let status = Rule { stdout: "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Token: gho_SECRETTOKEN0000\n  - Token scopes: 'repo'\n".into(), ..rule(&["auth", "status"], "") };
        chain.clis().rules(&[Rule { prefix: false, ..login }, Rule { prefix: false, ..status }], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let host = json!({"provider": "github", "host": "github.com"});
        // When the phone starts the sign-in, waits for it, and discovers source control
        let start = phone.call(case, &chain, &["rpc:sourceControl.signInStart"], "sourceControl.signInStart", host.clone());
        went(case, "sourceControl.signInStart", "hosted_auth_login_start");
        let wait = phone.call(case, &chain, &["rpc:sourceControl.signInWait"], "sourceControl.signInWait", host.clone());
        went(case, "sourceControl.signInWait", "hosted_auth_login_wait");
        let discovery = phone.call(case, &chain, &["rpc:server.discoverSourceControl"], "server.discoverSourceControl", json!({}));
        went(case, "sourceControl.discover", "hosted_auth_status");
        // Then the code and the address are gh's, the wait names the account, the discovery lists gh as authenticated, and the token is in no answer
        assert_eq!((success(&start)["userCode"].clone(), success(&start)["verificationUri"].clone()), (json!("ABCD-1234"), json!("https://github.com/login/device")));
        assert_eq!(success(&wait)["account"], "octocat");
        let github = &success(&discovery)["sourceControlProviders"][0];
        assert_eq!((github["kind"].clone(), github["auth"]["status"].clone(), github["auth"]["account"].clone()), (json!("github"), json!("authenticated"), json!({"_tag": "Some", "value": "octocat"})));
        for answer in [&start, &wait, &discovery] {
            assert!(!answer.to_string().contains("SECRETTOKEN"), "{answer}");
        }
        // When the phone starts a login that stays open and cancels it
        chain.clis().rules(&[Rule { stderr: "! First copy your one-time code: WXYZ-9999\nOpen this URL to continue in your web browser: https://github.com/login/device\n".into(), hang: true, prefix: false, ..rule(&["auth", "login", "--web", "--hostname=github.com", "--git-protocol=https"], "") }], &[]);
        success(&phone.call(case, &chain, &["rpc:sourceControl.signInStart"], "sourceControl.signInStart", host.clone()));
        let cancel = phone.call(case, &chain, &["rpc:sourceControl.signInCancel"], "sourceControl.signInCancel", host.clone());
        // Then the executor stopped it, and waiting for it now says no sign-in is under way
        went(case, "sourceControl.signInCancel", "hosted_auth_login_cancel");
        assert_eq!(cancel["_tag"], "Success");
        let after = phone.calls.call("sourceControl.signInWait", host);
        assert_eq!(failure(&after)["reason"], "not-started");
    });
}

#[test]
fn a_repository_is_looked_up_checked_for_access_and_published_through_gh_and_the_url_is_the_one_gh_printed() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that views octo/hello, shows the signed-in account, and creates a repository
        let chain = Chain::start_hosted();
        let (view, status) = (exchange("hosted_repo_view", "github repository"), exchange("hosted_auth_status", "github signed in"));
        script(&chain, &[&view, &status], &[rule(&["repo", "create", "octo/new"], "https://github.com/octo/new\n")]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        // When the phone looks octo/hello up, asks the app access for it, and publishes the project's repository as octo/new
        let lookup = phone.call(case, &chain, &["rpc:sourceControl.lookupRepository"], "sourceControl.lookupRepository", json!({"provider": "github", "repository": "octo/hello"}));
        went(case, "sourceControl.lookupRepository", "hosted_repo_view");
        let access = phone.call(case, &chain, &["rpc:sourceControl.appAccess"], "sourceControl.appAccess", json!({"provider": "github", "host": "github.com", "repository": "octo/hello"}));
        went(case, "sourceControl.appAccess", "hosted_repo_view");
        let published = phone.call(case, &chain, &["rpc:sourceControl.publishRepository"], "sourceControl.publishRepository", json!({"cwd": chain.repo.path().display().to_string(), "provider": "github", "repository": "octo/new", "visibility": "private"}));
        went(case, "sourceControl.publishRepository", "hosted_repo_publish");
        // Then the lookup is gh's view, the access is covered, and the publish names the repository, the url gh printed and the branch the repository is on
        let repo = &result(&view)["repo"];
        assert_eq!((success(&lookup)["nameWithOwner"].clone(), success(&lookup)["url"].clone(), success(&lookup)["sshUrl"].clone()), (repo["slug"].clone(), repo["url"].clone(), repo["ssh_url"].clone()));
        assert_eq!(success(&access)["status"], "covered");
        let published = success(&published);
        assert_eq!((published["repository"]["nameWithOwner"].clone(), published["remoteUrl"].clone(), published["status"].clone()), (json!("octo/new"), json!("https://github.com/octo/new"), json!("pushed")));
        assert_eq!(published["branch"], chain.repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim());
        // And gh was asked to create the repository private, from the repository's root
        assert!(argvs(&chain).iter().any(|a| a.starts_with(&["repo".to_string(), "create".to_string(), "octo/new".to_string()]) && a.contains(&"--private".to_string())), "{:?}", argvs(&chain));
    });
}

#[test]
fn a_clone_makes_the_project_runs_gh_in_the_executor_streams_its_end_is_cancelled_when_it_hangs_and_retries() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that clones octo/hello at once and octo/big never, and the destinations under the executor's root
        let chain = Chain::start_hosted();
        let (hello, big) = (chain.repo.path().join("clones/hello").display().to_string(), chain.repo.path().join("big").display().to_string());
        chain.clis().rules(&[Rule { program: "gh".into(), args: vec!["repo".into(), "clone".into(), "octo/hello".into(), hello.clone()], ..Rule::default() }, Rule { program: "gh".into(), args: vec!["repo".into(), "clone".into(), "octo/big".into(), big.clone()], hang: true, mkdir: Some(big.clone()), ..Rule::default() }], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let mut direct = chain.direct();
        // When the phone starts a clone of octo/hello into a new project
        let start = case.call(&chain.tap, &rows(&["rpc:projectClone.start"]), || {
            let exit = phone.calls.call("projectClone.start", json!({"projectId": "p-hello", "title": "Hello", "createdAt": "2026-01-01T00:00:00.000Z", "repository": "octo/hello", "provider": "github", "destinationPath": hello}));
            let finished = std::time::Instant::now();
            while direct.call("hosted.clone", "clone.status", json!({"workspace": "w1", "clone": "c1"})).map(|s| s["state"] != "done").unwrap_or(true) {
                assert!(finished.elapsed() < std::time::Duration::from_secs(30), "the clone never ended: {}", chain.evidence());
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            (exit["_tag"] == "Success", exit)
        });
        went(case, "clone.start", "hosted_repo_clone");
        // And subscribes
        let (_, first) = phone.stream(case, &chain, &["rpc:subscribeProjectClones"], "subscribeProjectClones", json!({}));
        // Then the project is on the destination and the snapshot says it was cloned in full
        let start = success(&start);
        assert_eq!((start["projectId"].clone(), start["cwd"].clone(), start["repository"]["nameWithOwner"].clone()), (json!("p-hello"), json!(hello), json!("octo/hello")));
        let snapshot = &first[0][0];
        assert_eq!((snapshot["projectId"].clone(), snapshot["phase"].clone(), snapshot["percent"].clone(), snapshot["destinationPath"].clone()), (json!("p-hello"), json!("done"), json!(100), json!(hello)));
        assert!(chain.clis().starts("gh").iter().any(|s| s.argv == ["repo", "clone", "octo/hello", hello.as_str()]));
        // When a clone that hangs is started and cancelled
        success(&phone.call(case, &chain, &["rpc:projectClone.start"], "projectClone.start", json!({"projectId": "p-big", "title": "Big", "createdAt": "2026-01-01T00:00:00.000Z", "repository": "octo/big", "provider": "github", "destinationPath": big})));
        let waited = std::time::Instant::now();
        while chain.clis().starts("gh").len() < 2 {
            assert!(waited.elapsed() < std::time::Duration::from_secs(30), "gh was never started for the big clone");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let cancel = phone.call(case, &chain, &["rpc:projectClone.cancel"], "projectClone.cancel", json!({"projectId": "p-big"}));
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "clone.cancel"), "{agent:?}");
        // Then it was applied, and the stream says the clone was cancelled
        assert_eq!(success(&cancel)["applied"], true);
        let frames = phone.drain_watch();
        let phases: Vec<String> = frames.iter().filter(|f| f["_tag"] == "Chunk").flat_map(|f| f["values"].as_array().unwrap().clone()).flat_map(|list| list.as_array().cloned().unwrap_or_default()).filter(|s| s["projectId"] == "p-big").map(|s| s["phase"].as_str().unwrap().to_string()).collect();
        assert!(phases.last().is_some_and(|p| p == "cancelled"), "{phases:?}");
        assert!(!std::path::Path::new(&big).exists(), "the cancelled clone left {big} behind");
        // When the finished clone is retried
        let retry = phone.call(case, &chain, &["rpc:projectClone.retry"], "projectClone.retry", json!({"projectId": "p-hello"}));
        // Then a second clone of the same repository started
        assert_eq!(success(&retry)["applied"], true);
        let retried = std::time::Instant::now();
        while chain.clis().starts("gh").iter().filter(|s| s.argv == ["repo", "clone", "octo/hello", hello.as_str()]).count() < 2 {
            assert!(retried.elapsed() < std::time::Duration::from_secs(30), "gh was not started again: {}", chain.evidence());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });
}

#[test]
fn the_files_a_person_marked_viewed_are_kept_in_rcas_state_and_read_back() {
    exec_testkit::case!(SUITE, |case| {
        // Given a project
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone marks two files viewed and unmarks one, then reads the marks
        phone.call(case, &chain, &["rpc:pullRequests.setFilesViewed"], "pullRequests.setFilesViewed", with(reference(12), json!({"files": [{"path": "a.rs", "viewed": true}, {"path": "b.rs", "viewed": true}]})));
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "state.set"), "{agent:?}");
        phone.call(case, &chain, &["rpc:pullRequests.setFilesViewed"], "pullRequests.setFilesViewed", with(reference(12), json!({"files": [{"path": "a.rs", "viewed": false}]})));
        let viewed = phone.call(case, &chain, &["rpc:pullRequests.filesViewed"], "pullRequests.filesViewed", reference(12));
        // Then rca held the marks and only the file still marked is read back
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "state.get"), "{agent:?}");
        assert_eq!(success(&viewed)["files"], json!([{"path": "b.rs", "state": "viewed"}]));
        let mut direct = chain.direct();
        let stored = direct.call("state", "state.get", json!({"scope": "t3", "key": "pr.viewed:p1:octo/hello:12"})).unwrap();
        assert_eq!(stored["value"], json!({"b.rs": "viewed"}));
    });
}
