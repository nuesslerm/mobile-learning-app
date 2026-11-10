//! The pull request rows M11 left, through the chain (`t3-serve serve` -> the real rca-service -> the real rca-exec): the thread links kept in rca's
//! state and read back by a restarted server, a reaction added and taken back, a comment edited with its text on stdin, the native stack read
//! through `gh api`, and the two rows that read a pull request's head from the repository itself, which these tests give a real remote that keeps
//! `refs/pull/12/head` the way a host does (so the fetch, the merge base, the blobs and the worktree are git's own). `gh` is the replay double.
use crate::chain::{last_window, said};
use crate::hosted_m11::{all_scopes, argvs, asked, exchange, ran, reference, result, rule, script, went, with};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::{Script, Turn};
use std::path::PathBuf;

/// A gh that views the pull request of the recorded exchange as asked from the repository's own directory (no `--repo`, as the executor runs it
/// for a project it knows no slug of).
fn script_view_from_the_checkout(chain: &Chain) {
    let view = exchange("hosted_pr_view", "github pull request");
    let rules: Vec<Value> = view.cli.iter().map(|r| {
        let mut rule = r.clone();
        rule["args"] = json!(r["args"].as_array().unwrap().iter().filter(|a| !a.as_str().unwrap().starts_with("--repo=")).collect::<Vec<_>>());
        rule
    }).collect();
    chain.clis().replay(&rules, &[]);
}

fn blob(chain: &Chain, rev_path: &str) -> String {
    chain.repo.git(&["show", rev_path])
}

#[test]
fn a_threads_pull_request_links_are_kept_in_rcas_state_and_the_watch_is_read_back_by_a_restarted_server() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread, and a phone that links pull requests 12 and 13 to it and watches 12
        let mut chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let key = |number: u64| json!({"host": "github.com", "repository": "octo/hello", "number": number});
        let url = |number: u64| format!("https://github.com/octo/hello/pull/{number}");
        let command = |kind: &str, id: &str, number: u64, extra: Value| with(with(json!({"type": kind, "commandId": id, "threadId": "t1"}), key(number)), extra);
        phone.command(case, &chain, command("thread.pull-request.link", "c-l12", 12, json!({"url": url(12), "source": "manual"})));
        phone.command(case, &chain, command("thread.pull-request.link", "c-l13", 13, json!({"url": url(13), "source": "agent"})));
        phone.command(case, &chain, command("thread.pull-request.watch", "c-w12", 12, json!({"watching": true})));
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "state.set"), "{agent:?}");
        // Then rca's state holds both links (12 watched), the link's source, and for each pull request the threads that link it
        let mut rca = chain.direct();
        let held = |rca: &mut Direct, key: &str| rca.call("state", "state.get", json!({"scope": "t3", "key": key})).unwrap()["value"].clone();
        let entry = |number: u64, source: &str, watching: bool, url: String| json!({"host": "github.com", "repository": "octo/hello", "number": number, "url": url, "source": source, "watching": watching});
        assert_eq!(held(&mut rca, "thread.pullRequests:t1"), json!([entry(12, "manual", true, url(12)), entry(13, "agent", false, url(13))]));
        assert_eq!((held(&mut rca, "pr.threads:octo/hello:12"), held(&mut rca, "pr.threads:octo/hello:13")), (json!(["t1"]), json!(["t1"])));
        // When the server is restarted, a phone connects again and asks which threads link pull request 12
        drop(phone);
        chain.restart_serve();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let linked = phone.call(case, &chain, &["rpc:pullRequests.linkedThreads"], "pullRequests.linkedThreads", reference(12));
        // Then the thread is listed, read from rca's state by the new process
        assert_eq!((success(&linked)["threads"][0]["id"].clone(), success(&linked)["threads"][0]["title"].clone()), (json!("t1"), json!("Notes")));
        // When the restarted server links 12 again with another address, then stops watching it, then unlinks it
        phone.command(case, &chain, command("thread.pull-request.link", "c-r12", 12, json!({"url": format!("{}/files", url(12)), "source": "manual"})));
        // Then the watch it had before the restart is still there: the server read it back before writing the link
        let twelve = |rca: &mut Direct| held(rca, "thread.pullRequests:t1").as_array().unwrap().iter().find(|e| e["number"] == 12).cloned().expect("12 is linked");
        assert_eq!(twelve(&mut rca), entry(12, "manual", true, format!("{}/files", url(12))));
        phone.command(case, &chain, command("thread.pull-request.watch", "c-u12", 12, json!({"watching": false})));
        assert_eq!(twelve(&mut rca)["watching"], json!(false));
        phone.command(case, &chain, command("thread.pull-request.unlink", "c-x12", 12, json!({})));
        // Then only 13 is left in the thread's links, and no thread links 12
        assert_eq!(held(&mut rca, "thread.pullRequests:t1"), json!([entry(13, "agent", false, url(13))]));
        assert_eq!(held(&mut rca, "pr.threads:octo/hello:12"), json!([]));
        let after = phone.call(case, &chain, &["rpc:pullRequests.linkedThreads"], "pullRequests.linkedThreads", reference(12));
        assert_eq!(success(&after)["threads"], json!([]));
    });
}

#[test]
fn a_reaction_is_added_then_taken_back_by_deleting_the_accounts_own_reaction_and_a_comment_is_edited_with_its_text_on_stdin() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh signed in as octocat that has a +1 of hubot's and one of octocat's on the pull request, and accepts a comment edit
        let chain = Chain::start_hosted();
        chain.clis().rules(
            &[
                rule(&["api", "--method=POST"], "{\"id\": 77, \"content\": \"+1\"}"),
                rule(&["api", "user"], "{\"login\": \"octocat\"}"),
                rule(&["api", "repos/octo/hello/issues/12/reactions?content=%2B1&per_page=100"], "[{\"id\": 5, \"content\": \"+1\", \"user\": {\"login\": \"hubot\"}}, {\"id\": 77, \"content\": \"+1\", \"user\": {\"login\": \"octocat\"}}]"),
                rule(&["api", "--method=DELETE", "repos/octo/hello/issues/12/reactions/77"], ""),
                rule(&["api", "graphql", "--input=-"], "{\"data\":{\"updateIssueComment\":{\"issueComment\":{\"id\":\"IC_kwDOAA\"}}}}"),
                rule(&["api", "--method=PATCH", "repos/octo/hello/pulls/comments/9001"], "{\"id\": 9001}"),
            ],
            &[],
        );
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let body = "Fixed the **typo**\n- not a flag\n--and not an option either";
        // When the phone reacts +1, takes the reaction back, and edits a conversation comment and a review comment
        let add = phone.call(case, &chain, &["rpc:pullRequests.setReaction"], "pullRequests.setReaction", with(reference(12), json!({"content": "thumbs-up", "reacted": true})));
        went(case, "pr.react", "hosted_pr_react");
        let back = phone.call(case, &chain, &["rpc:pullRequests.setReaction"], "pullRequests.setReaction", with(reference(12), json!({"content": "thumbs-up", "reacted": false})));
        went(case, "pr.react", "hosted_pr_react");
        let issue = phone.call(case, &chain, &["rpc:pullRequests.updateComment"], "pullRequests.updateComment", with(reference(12), json!({"commentId": "IC_kwDOAA", "kind": "issue-comment", "body": body})));
        went(case, "pr.editComment", "hosted_pr_comment_edit");
        let review = phone.call(case, &chain, &["rpc:pullRequests.updateComment"], "pullRequests.updateComment", with(reference(12), json!({"commentId": "9001", "kind": "review-comment", "body": body})));
        // Then each was accepted
        for exit in [&add, &back, &issue, &review] {
            assert_eq!(exit["_tag"], "Success", "{exit}");
        }
        // And gh was asked to add the reaction, to find octocat's own (not hubot's) and to delete exactly that one
        let argv = argvs(&chain);
        let reaction_calls: Vec<&Vec<String>> = argv.iter().filter(|a| a.iter().any(|x| x.contains("reactions"))).collect();
        assert_eq!(reaction_calls.iter().map(|a| a.last().unwrap().as_str()).collect::<Vec<_>>(), ["-f=content=+1", "repos/octo/hello/issues/12/reactions?content=%2B1&per_page=100", "repos/octo/hello/issues/12/reactions/77"]);
        assert!(argv.contains(&vec!["api".to_string(), "user".to_string()]));
        // And each edit sent its text as JSON on stdin and never in an argument, the conversation comment through graphql by its id, the review comment by its number
        let starts = chain.clis().starts("gh");
        let edits: Vec<_> = starts.iter().filter(|s| s.argv.contains(&"--input=-".to_string())).collect();
        assert_eq!(edits.len(), 2, "{starts:?}");
        let stdin_of = |s: &exec_testkit::hosted::Start| serde_json::from_str::<Value>(&s.stdin).unwrap();
        assert_eq!((edits[0].argv[..2].to_vec(), stdin_of(edits[0])["variables"].clone()), (vec!["api".to_string(), "graphql".to_string()], json!({"id": "IC_kwDOAA", "body": body})));
        assert_eq!((edits[1].argv[..3].to_vec(), edits[1].argv[3].clone(), stdin_of(edits[1])), (vec!["api".to_string(), "--method=PATCH".to_string(), "repos/octo/hello/pulls/comments/9001".to_string()], "--input=-".to_string(), json!({"body": body})));
        assert!(!starts.iter().any(|s| s.argv.iter().any(|a| a.contains("typo"))), "the text reached gh as an argument");
        assert!(starts.iter().all(|s| s.matched), "a start no rule answered: {starts:?}");
    });
}

#[test]
fn the_native_stack_of_a_pull_request_is_read_through_gh_api_and_a_pull_request_in_none_is_null() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh whose stacks endpoint knows pull request 12 in a stack of two (a merged layer below it)
        let chain = Chain::start_hosted();
        let stacked = exchange("hosted_pr_stack", "github stack with its layers");
        script(&chain, &[&stacked], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone asks for the stack
        let stack = phone.call(case, &chain, &["rpc:pullRequests.stack"], "pullRequests.stack", reference(12));
        went(case, "pr.stack", "hosted_pr_stack");
        // Then it is gh's stack: its id, base and address, and each layer's number, branch, head and state
        let (stack, said) = (success(&stack), &result(&stacked)["stack"]);
        assert_eq!((stack["id"].clone(), stack["number"].clone(), stack["base"].clone(), stack["url"].clone()), (said["id"].clone(), said["number"].clone(), said["base"].clone(), said["url"].clone()));
        let layers = stack["layers"].as_array().unwrap();
        assert_eq!(layers.len(), said["layers"].as_array().unwrap().len());
        for (shown, told) in layers.iter().zip(said["layers"].as_array().unwrap()) {
            assert_eq!((shown["number"].clone(), shown["headBranch"].clone(), shown["headSha"].clone(), shown["state"].clone()), (told["number"].clone(), told["head_ref"].clone(), told["head_sha"].clone(), told["state"].clone()));
        }
        assert_eq!(layers.iter().map(|l| l["state"].as_str().unwrap()).collect::<Vec<_>>(), ["merged", "open"]);
        // When gh says the pull request is in no stack
        script(&chain, &[&exchange("hosted_pr_stack", "github pull request in no stack")], &[]);
        let none = phone.call(case, &chain, &["rpc:pullRequests.stack"], "pullRequests.stack", reference(12));
        // Then the answer is null, and it was gh that was asked
        went(case, "pr.stack", "hosted_pr_stack");
        assert_eq!(success(&none), &Value::Null);
        assert!(argvs(&chain).iter().any(|a| a.last().map(String::as_str) == Some("repos/octo/hello/stacks?pull_request=12")), "{:?}", argvs(&chain));
    });
}

/// The repository given a remote that keeps pull request 12 as a host does: `main` with `src/up.rs` "one", and refs/pull/12/head two commits
/// above it ("two", then "three"). Returns the first change's commit and the head. The pull request's own branch is not left in the repository.
fn publish_pull_request(chain: &Chain) -> (String, String) {
    let remote = chain.base.parent().unwrap().join("remote.git");
    let repo = &chain.repo;
    repo.git(&["init", "--quiet", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
    repo.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    repo.write("src/up.rs", "one\n");
    repo.commit_all("base");
    repo.git(&["push", "--quiet", "origin", "main"]);
    repo.git(&["switch", "--quiet", "--create", "pr-12"]);
    repo.write("src/up.rs", "two\n");
    repo.commit_all("first change");
    let first = repo.head();
    repo.write("src/up.rs", "three\n");
    repo.commit_all("second change");
    let head = repo.head();
    repo.git(&["push", "--quiet", "origin", "HEAD:refs/pull/12/head"]);
    repo.git(&["switch", "--quiet", "main"]);
    repo.git(&["branch", "--quiet", "--delete", "--force", "pr-12"]);
    (first, head)
}

#[test]
fn the_contents_of_a_changed_file_are_the_repositorys_own_at_the_merge_base_and_the_fetched_head_or_at_a_commit_of_the_pull_request() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository whose remote keeps pull request 12 (two commits on src/up.rs), and a gh that views it and lists its files
        let chain = Chain::start_hosted();
        let (first, head) = publish_pull_request(&chain);
        script(&chain, &[&exchange("hosted_pr_view", "github pull request"), &exchange("hosted_pr_files", "github files")], &[]);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let input = |path: &str, extra: Value| with(reference(12), with(json!({"changeType": "change", "oldPath": path, "newPath": path}), extra));
        // When the phone asks for the changed file, an unchanged file of the pull request's list, and the file at the first commit
        let changed = phone.call(case, &chain, &["rpc:pullRequests.diffFileContents"], "pullRequests.diffFileContents", input("src/up.rs", json!({})));
        let (agent, executor) = last_window(case);
        let listed = phone.call(case, &chain, &["rpc:pullRequests.diffFileContents"], "pullRequests.diffFileContents", input("README.md", json!({})));
        let at_commit = phone.call(case, &chain, &["rpc:pullRequests.diffFileContents"], "pullRequests.diffFileContents", input("src/up.rs", json!({"commit": &first[..9]})));
        // Then the old side is the merge base's file and the new side the head's, the other file is whole on both sides, and a commit reads itself and its parent
        assert_eq!(success(&changed), &json!({"oldContents": "one\n", "newContents": "three\n"}));
        assert_eq!(success(&listed), &json!({"oldContents": "temp repo\n", "newContents": "temp repo\n"}));
        assert_eq!(success(&at_commit), &json!({"oldContents": "one\n", "newContents": "two\n"}));
        // And the call went through the agent API's git actions on the fetched head: a fetch, the compare and the blobs, each run by the executor
        for (method, action) in [("pr.view", "hosted_pr_view"), ("pr.files", "hosted_pr_files"), ("git.fetch", "git_fetch"), ("git.compare", "git_compare"), ("git.blob", "git_blob")] {
            assert!(asked(&agent, method) && ran(&executor, action), "{method} / {action}: {agent:?} {executor}");
        }
        // And the head is held in a ref the branch list does not show, no branch moved and the checkout is untouched
        assert_eq!(chain.repo.git(&["for-each-ref", "--format=%(refname) %(objectname)", "refs/rca"]).lines().next().unwrap(), format!("refs/rca/pr/12/base {}", chain.repo.git(&["rev-parse", "main"])));
        assert!(chain.repo.git(&["for-each-ref", "--format=%(refname)", "refs/rca"]).contains("refs/rca/pr/12/head"));
        assert_eq!(chain.repo.git(&["rev-parse", "refs/rca/pr/12/head"]), head);
        assert_eq!((chain.repo.git(&["branch", "--format=%(refname:short)"]), chain.repo.git(&["status", "--porcelain"])), ("main".to_string(), String::new()));
        // When the phone asks for a file the pull request does not change, and a commit that is not in it
        let refused = |exit: &Value| failure(exit)["detail"].as_str().unwrap_or_default().to_string();
        let other = phone.calls.call("pullRequests.diffFileContents", input("Cargo.toml", json!({})));
        let stranger = phone.calls.call("pullRequests.diffFileContents", input("src/up.rs", json!({"commit": head.replace(&head[..9], "deadbeef0")})));
        // Then each is refused, and nothing of the repository was read for the first
        assert_eq!((refused(&other), refused(&stranger)), ("This file is not part of the change request.".to_string(), "This commit is not part of the change request.".to_string()));
        assert!(blob(&chain, "main:README.md").starts_with("temp repo"));
    });
}

fn prepare(chain: &Chain, mode: &str, reference: &str) -> Value {
    json!({"cwd": chain.repo.path().display().to_string(), "reference": reference, "mode": mode})
}

#[test]
fn a_pull_request_is_prepared_in_a_worktree_at_its_fetched_head_and_the_same_worktree_is_found_again() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository whose remote keeps pull request 12, and a gh that views it
        let chain = Chain::start_hosted();
        let (_, head) = publish_pull_request(&chain);
        script_view_from_the_checkout(&chain);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When the phone prepares the pull request in a worktree, by its address
        let made = phone.call(case, &chain, &["rpc:git.preparePullRequestThread"], "git.preparePullRequestThread", prepare(&chain, "worktree", "https://github.com/octo/hello/pull/12"));
        let (agent, executor) = last_window(case);
        // Then a worktree under the executor's worktree base is on a branch of its own at the head, holding the pull request's files
        let made = success(&made);
        let path = PathBuf::from(made["worktreePath"].as_str().expect("a worktree path"));
        assert!(path.starts_with(&chain.base) && path.is_dir(), "{path:?} is no worktree under {:?}", chain.base);
        assert_eq!((made["branch"].clone(), made["isOnPullRequestHead"].clone(), made["pullRequest"]["number"].clone(), made["pullRequest"]["headBranch"].clone()), (json!("t3code/pr-12/feat-retry"), json!(true), json!(12), json!("feat/retry")));
        assert_eq!((chain.repo.git(&["-C", path.to_str().unwrap(), "rev-parse", "HEAD"]), chain.repo.git(&["-C", path.to_str().unwrap(), "rev-parse", "--abbrev-ref", "HEAD"])), (head.clone(), "t3code/pr-12/feat-retry".to_string()));
        assert_eq!(std::fs::read_to_string(path.join("src/up.rs")).unwrap(), "three\n");
        for (method, action) in [("pr.view", "hosted_pr_view"), ("git.fetch", "git_fetch"), ("worktree.list", "git_worktree_list"), ("worktree.create", "git_worktree_add")] {
            assert!(asked(&agent, method) && ran(&executor, action), "{method} / {action}: {agent:?} {executor}");
        }
        // And the main checkout stayed on main
        assert_eq!(chain.repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        // When the phone prepares it again
        let again = phone.call(case, &chain, &["rpc:git.preparePullRequestThread"], "git.preparePullRequestThread", prepare(&chain, "worktree", "12"));
        // Then the worktree is the same one, on the head, and no second worktree was made
        assert_eq!((success(&again)["worktreePath"].clone(), success(&again)["isOnPullRequestHead"].clone()), (made["worktreePath"].clone(), json!(true)));
        assert_eq!(chain.repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 2);
    });
}

#[test]
fn a_pull_request_is_prepared_in_the_project_as_a_branch_at_its_head_and_a_repository_with_uncommitted_changes_is_refused_before_a_fetch() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository whose remote keeps pull request 12, a gh that views it, and a tracked file edited
        let chain = Chain::start_hosted();
        let (_, head) = publish_pull_request(&chain);
        script_view_from_the_checkout(&chain);
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        chain.repo.write("README.md", "edited\n");
        // When the phone prepares the pull request in the project
        let refused = phone.calls.call("git.preparePullRequestThread", prepare(&chain, "local", "12"));
        // Then it is refused with the manager error and nothing was fetched or switched
        assert_eq!((failure(&refused)["_tag"].clone(), failure(&refused)["detail"].as_str().unwrap().starts_with("This repository has uncommitted changes")), (json!("GitManagerError"), true));
        assert_eq!((chain.repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), chain.repo.git(&["for-each-ref", "refs/rca"])), ("main".to_string(), String::new()));
        // When the edit is undone and the phone prepares it again
        chain.repo.git(&["checkout", "--quiet", "--", "README.md"]);
        let prepared = phone.call(case, &chain, &["rpc:git.preparePullRequestThread"], "git.preparePullRequestThread", prepare(&chain, "local", "12"));
        let (agent, executor) = last_window(case);
        // Then the project's checkout is on a branch of its own at the head, with the pull request's files
        let prepared = success(&prepared);
        assert_eq!((prepared["branch"].clone(), prepared["worktreePath"].clone(), prepared["isOnPullRequestHead"].clone()), (json!("t3code/pr-12/feat-retry"), Value::Null, json!(true)));
        assert_eq!((chain.repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), chain.repo.head()), ("t3code/pr-12/feat-retry".to_string(), head));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("src/up.rs")).unwrap(), "three\n");
        for (method, action) in [("pr.view", "hosted_pr_view"), ("git.fetch", "git_fetch"), ("git.status", "git_status"), ("git.createRef", "git_create_ref")] {
            assert!(asked(&agent, method) && ran(&executor, action), "{method} / {action}: {agent:?} {executor}");
        }
        // And main is where it was: two commits below the head
        assert_eq!(chain.repo.git(&["rev-parse", "main"]), chain.repo.git(&["rev-parse", "refs/rca/pr/12/head~2"]));
    });
}

#[test]
fn a_review_guide_is_written_by_a_model_turn_of_the_real_service_over_gh_s_diff_checked_against_it_and_kept_in_rcas_state() {
    exec_testkit::case!(SUITE, |case| {
        // Given a gh that views pull request 12, gives its diff and its review threads, and a model (the scripted claude) that answers with a tour
        let chain = Chain::start_hosted();
        let (view, diff, threads) = (exchange("hosted_pr_view", "github pull request"), exchange("hosted_pr_diff", "github diff"), exchange("hosted_pr_threads", "github review threads through graphql"));
        script(&chain, &[&view, &diff, &threads], &[]);
        let tour = json!({"stops": [{"title": "Retry the send", "summary": "The upload retries.", "attention": {"kind": "risk", "note": "a retry may repeat the send"},
            "excerpts": [{"path": "src/up.rs", "side": "new", "start": 1, "end": 30, "caption": "the retry"}, {"path": "src/nope.rs", "side": "new", "start": 1, "end": 3, "caption": "not in the change"}],
            "files": [{"path": "src/up.rs"}], "notes": [{"path": "src/up.rs", "line": 3, "title": "Why", "body": "see the test"}, {"path": "src/up.rs", "line": 77, "title": "Outside", "body": "b"}], "questions": ["Is send idempotent?"]}]});
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said(&format!("Here is the tour:\n{tour}"))])]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        // When guides are off and the phone asks for one, then they are turned on with a model
        let off = phone.calls.call("pullRequests.generateReviewGuide", reference(12));
        phone.call(case, &chain, &["rpc:server.updateSettings"], "server.updateSettings", json!({"patch": {"reviewGuidesEnabled": true, "reviewGuideModelSelection": {"instanceId": "claudeAgent", "model": "opus"}}}));
        let started = phone.call(case, &chain, &["rpc:pullRequests.generateReviewGuide"], "pullRequests.generateReviewGuide", reference(12));
        // Then the first is refused with T3's words and no model ran, and the second answers at once with a run for the head gh reports
        assert_eq!(failure(&off)["detail"], "Review guides are off. Turn them on in Settings → Source Control.");
        assert_eq!((success(&started)["started"].clone(), success(&started)["run"]["_tag"].clone(), success(&started)["run"]["headSha"].clone()), (json!(true), json!("Running"), result(&view)["pr"]["head_sha"].clone()));
        // When the phone reads the guide until the run is over
        let read = loop {
            let read = phone.call(case, &chain, &["rpc:pullRequests.reviewGuide"], "pullRequests.reviewGuide", reference(12));
            let read = success(&read).clone();
            if read["run"].is_null() {
                break read;
            }
            assert_ne!(read["run"]["_tag"], "Failed", "{read}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        // Then the guide is the model's, checked against gh's diff: the excerpt cut to the hunk (lines 1-4 of src/up.rs), the note in it kept, the file the change lacks dropped
        let guide = &read["guide"];
        assert_eq!((guide["headSha"].clone(), guide["modelSelection"].clone(), read["currentHeadSha"].clone()), (result(&view)["pr"]["head_sha"].clone(), json!({"instanceId": "claudeAgent", "model": "opus"}), result(&view)["pr"]["head_sha"].clone()));
        assert_eq!(guide["stops"].as_array().unwrap().len(), 1);
        let stop = &guide["stops"][0];
        assert_eq!(stop["excerpts"], json!([{"path": "src/up.rs", "side": "new", "start": 1, "end": 4, "caption": "the retry"}]));
        assert_eq!((stop["notes"].as_array().unwrap().len(), stop["notes"][0]["title"].clone(), stop["attention"]["kind"].clone(), stop["questions"].clone()), (1, json!("Why"), json!("risk"), json!(["Is send idempotent?"])));
        // And the model was started once, with the diff gh printed in what it was asked, and the diff came from the executor's gh
        let invocations = chain.cli.invocations("claude");
        assert_eq!(invocations.len(), 1, "{invocations:?}");
        let asked_of_the_model = invocations[0].argv.join("\n");
        for part in ["Add retry to uploader", "retry(send);"] {
            assert!(asked_of_the_model.contains(part), "the model was not given {part:?}: {asked_of_the_model}");
        }
        assert!(argvs(&chain).iter().any(|a| a.first().map(String::as_str) == Some("pr") && a.get(1).map(String::as_str) == Some("diff")), "{:?}", argvs(&chain));
        // And rca holds the guide in its state, and no thread of the guide is left behind
        let mut rca = chain.direct();
        let stored = rca.call("state", "state.get", json!({"scope": "t3", "key": "pr.guide:p1:octo/hello:12"})).unwrap();
        assert_eq!(stored["value"]["stops"], guide["stops"]);
        let (agent, _) = last_window(case);
        assert!(!agent.is_empty());
        let leftover = rca.call("agent.threads", "thread.list", json!({})).map(|l| l.to_string()).unwrap_or_default();
        assert!(!leftover.contains("review-guide"), "{leftover}");
    });
}
