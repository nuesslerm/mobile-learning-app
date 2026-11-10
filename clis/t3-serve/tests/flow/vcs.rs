//! The M9 rows through the real router over the agent double: the status of a repository and its stream, the refs, switching and creating one,
//! pulling, the diffs a review shows and the stacked actions with their progress. The double answers what the agent API's `git` contract
//! recorded for each method; what a request becomes (the `Op` the agent API is asked) and what the phone is told are both read from data the
//! contract owns.
use super::support::*;
use t3_core::ports::{AgentError, Op};

const ROOT: &str = "/workspace/demo";

/// A recorded result of `method` whose exchange is named like `name`.
fn recorded(method: &str, name: &str) -> Value {
    let fixtures = agent_api_git::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && e.succeeds());
    found.unwrap_or_else(|| panic!("no recorded {method} exchange named like {name}")).response["result"].clone()
}

fn refusal(method: &str, name: &str) -> AgentError {
    let fixtures = agent_api_git::testing::fixtures();
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && !e.succeeds()).unwrap_or_else(|| panic!("no recorded refusal of {method} named like {name}"));
    AgentError::Refused { code: found.response["error"]["code"].as_str().unwrap().into(), message: found.response["error"]["message"].as_str().unwrap().into() }
}

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let rpc = connect_with(&h, all);
    (h, rpc)
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

fn status_with(files: Value, ahead: u64) -> Value {
    let mut status = recorded("git.status", "reports the branch");
    status["files"] = files;
    status["ahead"] = json!(ahead);
    status
}

fn changed(path: &str) -> Value {
    json!({"path": path, "index": ".", "worktree": "M", "conflicted": false})
}

#[test]
fn a_refreshed_status_is_the_executors_status_with_the_insertions_and_deletions_counted_from_its_diff_and_the_upstream_from_its_own_answer() {
    // Given a repository the contract recorded: an edited README, an untracked file, no upstream, and the diff of the README
    let (h, mut rpc) = started();
    reply(&h, "git.status", Ok(recorded("git.status", "reports the branch")));
    reply(&h, "git.upstreamStatus", Ok(recorded("git.upstreamStatus", "no upstream")));
    reply(&h, "git.diff", Ok(recorded("git.diff", "diffs one path")));
    // When the phone refreshes it
    let status = rpc.call("vcs.refreshStatus", json!({"cwd": ROOT}));
    let status = success(&status);
    // Then it is a repository on main with changes, the README counted from the diff, the untracked file with no count, and no upstream
    assert_eq!((status["isRepo"].clone(), status["refName"].clone(), status["isDefaultRef"].clone(), status["hasWorkingTreeChanges"].clone(), status["hasPrimaryRemote"].clone()), (json!(true), json!("main"), json!(true), json!(true), json!(false)));
    assert_eq!(status["workingTree"]["files"], json!([{"path": "README.md", "insertions": 1, "deletions": 0}, {"path": "new.txt", "insertions": 0, "deletions": 0}]));
    assert_eq!((status["workingTree"]["insertions"].clone(), status["workingTree"]["rootPath"].clone()), (json!(1), json!(ROOT)));
    assert_eq!((status["hasUpstream"].clone(), status["aheadCount"].clone(), status["behindCount"].clone(), status["pr"].clone()), (json!(false), json!(0), json!(0), Value::Null));
    // And the agent API was asked for the status, the upstream and the diff against HEAD of the project's directory
    assert_eq!(ops(&h), [Op::GitStatus { root: ROOT.into() }, Op::GitUpstream { root: ROOT.into() }, Op::GitDiff { root: ROOT.into(), base: Some("HEAD".into()), head: None, path: None }]);
}

#[test]
fn a_directory_that_is_no_repository_is_a_status_that_says_so_and_any_other_refusal_is_the_declared_git_manager_error() {
    // Given an executor that says the directory is no repository, and then one that fails otherwise
    let (h, mut rpc) = started();
    let not_a_repo = || AgentError::Refused { code: "IO_ERROR".into(), message: "git status: fatal: not a git repository".into() };
    reply(&h, "git.status", Err(not_a_repo()));
    reply(&h, "git.refs", Err(not_a_repo()));
    // When the phone refreshes the status and lists the refs
    let status = rpc.call("vcs.refreshStatus", json!({"cwd": ROOT}));
    let refs = rpc.call("vcs.listRefs", json!({"cwd": ROOT}));
    reply(&h, "git.status", Err(AgentError::Refused { code: "NO_EXECUTOR".into(), message: "no executor is attached".into() }));
    let failed = rpc.call("vcs.refreshStatus", json!({"cwd": ROOT}));
    // Then the first is isRepo false with nothing changed, and the last a GitManagerError with the executor's own words
    assert_eq!((success(&status)["isRepo"].clone(), success(&status)["hasWorkingTreeChanges"].clone()), (json!(false), json!(false)));
    assert_eq!(success(&refs)["isRepo"], json!(false));
    assert_eq!((failure(&failed)["_tag"].as_str(), failure(&failed)["detail"].as_str(), failure(&failed)["operation"].as_str()), (Some("GitManagerError"), Some("no executor is attached"), Some("refreshStatus")));
}

#[test]
fn refs_are_listed_a_page_at_a_time_without_tags_and_a_remote_branch_names_its_remote() {
    // Given the recorded branch of the repository, a remote branch made from it, and a tag
    let (h, mut rpc) = started();
    let mut refs = recorded("git.refs", "lists the branches");
    let main = refs["refs"][0].clone();
    let with = |name: &str, full: &str, kind: &str, current: bool| {
        let mut r = main.clone();
        r["name"] = json!(name);
        r["full"] = json!(full);
        r["kind"] = json!(kind);
        r["current"] = json!(current);
        r
    };
    refs["refs"] = json!([main.clone(), with("feature/x", "refs/heads/feature/x", "branch", false), with("origin/main", "refs/remotes/origin/main", "remote", false), with("v1", "refs/tags/v1", "tag", false)]);
    reply(&h, "git.refs", Ok(refs));
    let mut upstream = recorded("git.upstreamStatus", "no upstream");
    upstream["remotes"] = json!(["origin"]);
    reply(&h, "git.upstreamStatus", Ok(upstream));
    // When the phone lists the first two, then the rest, and then searches
    let first = rpc.call("vcs.listRefs", json!({"cwd": ROOT, "limit": 2}));
    let rest = rpc.call("vcs.listRefs", json!({"cwd": ROOT, "limit": 2, "cursor": 2}));
    let found = rpc.call("vcs.listRefs", json!({"cwd": ROOT, "query": "FEATURE"}));
    // Then the pages hold the branches and the remote one (no tag), the first says where the next starts, and the remote names its remote
    let names = |v: &Value| v["refs"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!((names(success(&first)), success(&first)["nextCursor"].clone(), success(&first)["totalCount"].clone()), (vec!["main".to_string(), "feature/x".to_string()], json!(2), json!(3)));
    assert_eq!((names(success(&rest)), success(&rest)["nextCursor"].clone(), success(&rest)["refs"][0]["remoteName"].clone(), success(&rest)["refs"][0]["isRemote"].clone()), (vec!["origin/main".to_string()], Value::Null, json!("origin"), json!(true)));
    assert_eq!((success(&first)["refs"][0]["current"].clone(), success(&first)["refs"][0]["isDefault"].clone(), success(&first)["hasPrimaryRemote"].clone()), (json!(true), json!(true), json!(true)));
    assert_eq!(names(success(&found)), ["feature/x"]);
    assert_eq!(success(&found)["hasPrimaryRemote"], json!(true));
}

#[test]
fn switching_to_a_remote_branch_that_has_no_local_one_tracks_it_and_a_refusal_keeps_the_reason_the_app_tells_apart() {
    // Given a repository with a remote branch origin/feat and a local main, and a checkout that answers as the contract recorded
    let (h, mut rpc) = started();
    let listing = |names: &[&str]| json!({"refs": names.iter().map(|n| json!({"name": n, "full": n, "kind": "x", "sha": "s", "current": false})).collect::<Vec<_>>(), "detached": false, "truncated": false});
    let kinds = std::sync::Arc::new(listing(&["main"]));
    let remote_kind = std::sync::Arc::new(listing(&["origin/feat"]));
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::GitRefs { kind: Some("remote"), .. } => Some(Ok((*remote_kind).clone())),
        Op::GitRefs { .. } => Some(Ok((*kinds).clone())),
        _ => None,
    }));
    reply(&h, "git.checkout", Ok(recorded("git.checkout", "switches to another branch")));
    // When the phone switches to the remote branch and to the local one
    let tracked = rpc.call("vcs.switchRef", json!({"cwd": ROOT, "refName": "origin/feat"}));
    let local = rpc.call("vcs.switchRef", json!({"cwd": ROOT, "refName": "main"}));
    // Then the first asked the executor to track, the second did not, and the answer names the branch the executor moved to
    let checkouts: Vec<Op> = ops(&h).into_iter().filter(|op| op.method() == "git.checkout").collect();
    assert_eq!(checkouts, [Op::GitCheckout { root: ROOT.into(), target: "origin/feat".into(), track: true }, Op::GitCheckout { root: ROOT.into(), target: "main".into(), track: false }]);
    assert_eq!((success(&tracked)["refName"].clone(), success(&local)["refName"].clone()), (json!("topic"), json!("topic")));
    // And a refusal is a GitCommandError with the executor's words
    reply(&h, "git.checkout", Err(refusal("git.checkout", "option is refused")));
    let refused = rpc.call("vcs.switchRef", json!({"cwd": ROOT, "refName": "--orphan=x"}));
    assert_eq!((failure(&refused)["_tag"].as_str(), failure(&refused)["detail"].as_str(), failure(&refused)["operation"].as_str()), (Some("GitCommandError"), Some("`ref` must be a revision"), Some("switchRef")));
}

#[test]
fn a_ref_is_created_and_switched_to_on_request_and_a_name_git_refuses_is_the_declared_error() {
    // Given an agent API that creates a branch as recorded
    let (h, mut rpc) = started();
    reply(&h, "git.createRef", Ok(recorded("git.createRef", "creates a branch")));
    // When the phone creates one with and without switching, and then one git cannot use
    let made = rpc.call("vcs.createRef", json!({"cwd": ROOT, "refName": "feature/x", "switchRef": true}));
    rpc.call("vcs.createRef", json!({"cwd": ROOT, "refName": "feature/y"}));
    reply(&h, "git.createRef", Err(refusal("git.createRef", "name git cannot use")));
    let refused = rpc.call("vcs.createRef", json!({"cwd": ROOT, "refName": "a..b"}));
    // Then the answer is the name, the executor was asked to switch only the first time, and the refusal is a GitCommandError
    assert_eq!(success(&made), &json!({"refName": "feature/x"}));
    assert_eq!(ops(&h)[..2], [Op::GitCreateRef { root: ROOT.into(), name: "feature/x".into(), from: None, checkout: true }, Op::GitCreateRef { root: ROOT.into(), name: "feature/y".into(), from: None, checkout: false }]);
    assert_eq!((failure(&refused)["_tag"].as_str(), failure(&refused)["detail"].as_str()), (Some("GitCommandError"), Some("`name` must be a branch or tag name")));
}

#[test]
fn a_pull_is_pulled_when_the_head_moved_skipped_when_it_did_not_and_a_pull_that_stops_on_conflicts_or_has_no_remote_is_the_declared_error() {
    // Given a branch main with an upstream, and a pull that moved the head
    let (h, mut rpc) = started();
    let mut upstream = recorded("git.upstreamStatus", "no upstream");
    upstream["has_upstream"] = json!(true);
    upstream["upstream"] = json!("origin/main");
    reply(&h, "git.upstreamStatus", Ok(upstream));
    let integrated = |updated: bool, ok: bool| json!({"ok": ok, "head": "h", "updated": updated, "conflicted": !ok, "conflicts": if ok { json!([]) } else { json!(["c.txt"]) }, "output": ""});
    reply(&h, "git.pull", Ok(integrated(true, true)));
    // When the phone pulls, pulls again with nothing new, pulls into a conflict, and pulls where there is no remote
    let pulled = rpc.call("vcs.pull", json!({"cwd": ROOT}));
    reply(&h, "git.pull", Ok(integrated(false, true)));
    let skipped = rpc.call("vcs.pull", json!({"cwd": ROOT}));
    reply(&h, "git.pull", Ok(integrated(true, false)));
    let conflicted = rpc.call("vcs.pull", json!({"cwd": ROOT}));
    reply(&h, "git.pull", Err(refusal("git.pull", "no remote")));
    let no_remote = rpc.call("vcs.pull", json!({"cwd": ROOT}));
    // Then the results are the branch and its upstream with the status, and the two failures are GitCommandErrors saying why
    assert_eq!(success(&pulled), &json!({"status": "pulled", "refName": "main", "upstreamRef": "origin/main"}));
    assert_eq!(success(&skipped)["status"], json!("skipped_up_to_date"));
    assert!(failure(&conflicted)["detail"].as_str().unwrap().contains("c.txt"));
    assert_eq!((failure(&no_remote)["_tag"].as_str(), failure(&no_remote)["detail"].as_str()), (Some("GitCommandError"), Some("this repository has no remote configured")));
}

#[test]
fn a_review_shows_the_working_tree_the_branch_and_a_commit_each_as_a_source_with_its_files_counted_and_a_hash_of_its_diff() {
    // Given the diff the contract recorded for the README and a log with one commit
    let (h, mut rpc) = started();
    let diff = recorded("git.diff", "diffs one path");
    reply(&h, "git.diff", Ok(diff.clone()));
    let mut log = recorded("git.history", "lists the commits");
    log["commits"][0]["subject"] = json!("first commit");
    reply(&h, "git.history", Ok(log));
    // When the phone asks for the working tree, the branch against main, and a commit
    let preview = rpc.call("review.getDiffPreview", json!({"cwd": ROOT, "baseRef": "main", "commitSha": "abc123"}));
    let preview = success(&preview);
    // Then the three sources come in that order, each with the diff, its files counted and a hash that is the same for the same diff
    let kinds: Vec<(&str, &Value, &Value)> = preview["sources"].as_array().unwrap().iter().map(|s| (s["kind"].as_str().unwrap(), &s["baseRef"], &s["headRef"])).collect();
    assert_eq!(kinds, [("working-tree", &Value::Null, &Value::Null), ("branch-range", &json!("main"), &json!("HEAD")), ("commit", &json!("abc123^"), &json!("abc123"))]);
    let sources = preview["sources"].as_array().unwrap();
    assert!(sources.iter().all(|s| s["diff"] == diff["diff"] && s["files"] == json!([{"path": "README.md", "previousPath": null, "additions": 1, "deletions": 0}]) && !s["truncated"].as_bool().unwrap()));
    assert_eq!((sources[0]["diffHash"] == sources[1]["diffHash"], sources[2]["commitSubject"].clone(), preview["cwd"].clone()), (true, json!("first commit"), json!(ROOT)));
    // And the agent API was asked for HEAD of the tree, main to HEAD, and the commit against its parent
    let asked: Vec<Op> = ops(&h).into_iter().filter(|o| o.method() == "git.diff").collect();
    assert_eq!(asked, [
        Op::GitDiff { root: ROOT.into(), base: Some("HEAD".into()), head: None, path: None },
        Op::GitDiff { root: ROOT.into(), base: Some("main".into()), head: Some("HEAD".into()), path: None },
        Op::GitDiff { root: ROOT.into(), base: Some("abc123^".into()), head: Some("abc123".into()), path: None },
    ]);
}

#[test]
fn a_review_file_is_the_old_side_from_a_revision_and_the_new_side_from_the_tree_or_a_revision_and_a_side_a_change_lacks_is_empty() {
    // Given blobs of two revisions, a file in the working tree, and a revision that lacks the path
    let (h, mut rpc) = started();
    h.agent.files.lock().unwrap().insert(format!("{ROOT}/README.md"), "in the tree\n".into());
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        Op::GitBlob { rev, path, .. } if path == "gone.txt" && rev == "HEAD" => Some(Err(AgentError::Refused { code: "NOT_FOUND".into(), message: "gone.txt does not exist at HEAD".into() })),
        Op::GitBlob { rev, .. } => Some(Ok(json!({"text": format!("{rev} text\n")}))),
        _ => None,
    }));
    let input = |kind: &str, change: &str, base: Value, head: Value, old: &str, new: &str| json!({"cwd": ROOT, "sourceKind": kind, "changeType": change, "baseRef": base, "headRef": head, "oldPath": old, "newPath": new});
    // When the phone asks for a changed file of the working tree, of a branch range, of a commit, a new one and a deleted one
    let tree = rpc.call("review.getDiffFileContents", input("working-tree", "change", Value::Null, Value::Null, "README.md", "README.md"));
    let range = rpc.call("review.getDiffFileContents", input("branch-range", "change", json!("main"), json!("feat"), "a.txt", "a.txt"));
    let commit = rpc.call("review.getDiffFileContents", input("commit", "rename-changed", Value::Null, json!("abc"), "old.txt", "new.txt"));
    let new = rpc.call("review.getDiffFileContents", input("working-tree", "new", Value::Null, Value::Null, "README.md", "README.md"));
    let deleted = rpc.call("review.getDiffFileContents", input("working-tree", "deleted", Value::Null, Value::Null, "gone.txt", "gone.txt"));
    // Then each side is where its source says: HEAD and the tree, the two refs, a commit and its parent, nothing for the missing side
    assert_eq!(success(&tree), &json!({"oldContents": "HEAD text\n", "newContents": "in the tree\n"}));
    assert_eq!(success(&range), &json!({"oldContents": "main text\n", "newContents": "feat text\n"}));
    assert_eq!(success(&commit), &json!({"oldContents": "abc^ text\n", "newContents": "abc text\n"}));
    assert_eq!(success(&new), &json!({"oldContents": "", "newContents": "in the tree\n"}));
    assert_eq!(success(&deleted), &json!({"oldContents": "", "newContents": ""}));
    let blobs: Vec<(String, String)> = ops(&h).into_iter().filter_map(|o| if let Op::GitBlob { rev, path, .. } = o { Some((rev, path)) } else { None }).collect();
    assert_eq!(blobs[..4], [("HEAD".into(), "README.md".into()), ("main".into(), "a.txt".into()), ("feat".into(), "a.txt".into()), ("abc^".into(), "old.txt".into())]);
}

#[test]
fn the_status_stream_is_a_snapshot_and_then_a_frame_for_each_half_that_changed_and_none_for_a_status_that_repeats() {
    // Given a repository whose status stream says: the same status, a file more, the same again, and a commit ahead
    let (h, mut rpc) = started();
    reply(&h, "git.status", Ok(status_with(json!([changed("a.txt")]), 0)));
    reply(&h, "git.upstreamStatus", Ok(recorded("git.upstreamStatus", "no upstream")));
    h.agent.git_statuses.lock().unwrap().insert(ROOT.into(), vec![status_with(json!([changed("a.txt")]), 0), status_with(json!([changed("a.txt"), changed("b.txt")]), 0), status_with(json!([changed("a.txt"), changed("b.txt")]), 0), status_with(json!([changed("a.txt"), changed("b.txt")]), 2)]);
    // When the phone subscribes
    let id = rpc.send("subscribeVcsStatus", json!({"cwd": ROOT}));
    let mut frames: Vec<Value> = Vec::new();
    while frames.len() < 3 {
        frames.extend(rpc.chunk(&id));
    }
    // Then the first frame is the snapshot of both halves, the second the local half with the new file, the third the remote half ahead by two
    assert_eq!((frames[0]["_tag"].as_str(), frames[0]["local"]["workingTree"]["files"].as_array().map(Vec::len), frames[0]["remote"]["aheadCount"].clone()), (Some("snapshot"), Some(1), json!(0)));
    assert_eq!((frames[1]["_tag"].as_str(), frames[1]["local"]["workingTree"]["files"].as_array().map(Vec::len)), (Some("localUpdated"), Some(2)));
    assert_eq!((frames[2]["_tag"].as_str(), frames[2]["remote"]["aheadCount"].clone()), (Some("remoteUpdated"), json!(2)));
    assert_eq!(frames.len(), 3);
    assert!(h.agent.calls.lock().unwrap().contains(&"git.subscribe"));
}

fn progress(rpc: &mut Rpc, id: &Value) -> Vec<Value> {
    let mut frames: Vec<Value> = Vec::new();
    loop {
        let frame = rpc.recv();
        match frame["_tag"].as_str() {
            Some("Chunk") => {
                rpc.ack(id);
                frames.extend(frame["values"].as_array().unwrap().iter().cloned());
            }
            Some("Exit") => {
                assert_eq!(frame["exit"]["_tag"], "Success", "{frame}");
                return frames;
            }
            other => panic!("{other:?}: {frame}"),
        }
    }
}

#[test]
fn a_commit_and_push_reports_each_phase_ends_with_its_result_and_the_stream_ends_and_the_steps_ran_in_order_on_the_repository() {
    // Given a repository with a change staged by the action, a commit the executor makes as recorded, and a branch with no upstream
    let (h, mut rpc) = started();
    reply(&h, "git.status", Ok(json!({"files": [{"path": "a.txt", "index": "M", "worktree": ".", "conflicted": false}]})));
    reply(&h, "git.commit", Ok(recorded("git.commit", "commits the staged")));
    reply(&h, "git.upstreamStatus", Ok(recorded("git.upstreamStatus", "no upstream")));
    reply(&h, "git.push", Ok(json!({"remote": "origin", "branch": "main", "set_upstream": true, "output": ""})));
    // When the phone runs commit_push with a message
    let id = rpc.send("git.runStackedAction", json!({"actionId": "act-1", "cwd": ROOT, "action": "commit_push", "commitMessage": "second", "filePaths": ["a.txt"]}));
    let frames = progress(&mut rpc, &id);
    // Then the frames are the start, the two phases and the result, each naming the action and the repository
    let kinds: Vec<(&str, Option<&str>)> = frames.iter().map(|f| (f["kind"].as_str().unwrap(), f["phase"].as_str())).collect();
    assert_eq!(kinds, [("action_started", None), ("phase_started", Some("commit")), ("phase_started", Some("push")), ("action_finished", None)]);
    assert!(frames.iter().all(|f| f["actionId"] == "act-1" && f["cwd"] == ROOT && f["action"] == "commit_push"));
    assert_eq!(frames[0]["phases"], json!(["commit", "push"]));
    let result = &frames[3]["result"];
    assert_eq!((result["commit"]["status"].clone(), result["commit"]["subject"].clone(), result["push"]["status"].clone(), result["push"]["setUpstream"].clone(), result["pr"]["status"].clone()), (json!("created"), json!("second"), json!("pushed"), json!(true), json!("skipped_not_requested")));
    assert_eq!(result["push"]["upstreamBranch"], json!("origin/main"));
    // And the executor was asked to stage the named path, commit, and push setting the upstream, in that order
    let acts: Vec<Op> = ops(&h).into_iter().filter(|o| matches!(o.method(), "git.stage" | "git.commit" | "git.push")).collect();
    assert_eq!(acts, [Op::GitStage { root: ROOT.into(), paths: vec!["a.txt".into()] }, Op::GitCommit { root: ROOT.into(), message: "second".into() }, Op::GitPush { root: ROOT.into(), set_upstream: true }]);
}

#[test]
fn a_stacked_action_that_cannot_run_ends_with_a_failed_frame_naming_the_phase_a_commit_with_no_message_a_push_the_remote_refuses_and_a_pull_request_this_server_does_not_serve() {
    // Given an agent API whose push is refused as the contract records a missing remote
    let (h, mut rpc) = started();
    reply(&h, "git.upstreamStatus", Ok(recorded("git.upstreamStatus", "no upstream")));
    reply(&h, "git.push", Err(refusal("git.push", "no remote")));
    // When the phone runs a commit with no message, a push, and a commit_push_pr
    let run = |rpc: &mut Rpc, id: &str, action: &str, extra: Value| {
        let mut payload = json!({"actionId": id, "cwd": ROOT, "action": action});
        payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let id = rpc.send("git.runStackedAction", payload);
        progress(rpc, &id)
    };
    let no_message = run(&mut rpc, "a1", "commit", json!({}));
    let rejected = run(&mut rpc, "a2", "push", json!({}));
    reply(&h, "git.status", Ok(json!({"files": []})));
    let pr = run(&mut rpc, "a3", "commit_push_pr", json!({"commitMessage": "m"}));
    // Then each ends with action_failed and the phase it failed in, with the reason, and nothing ran past the failure
    let last = |frames: &[Value]| (frames.last().unwrap()["kind"].as_str().unwrap().to_string(), frames.last().unwrap()["phase"].clone(), frames.last().unwrap()["message"].as_str().unwrap().to_string());
    assert_eq!(last(&no_message), ("action_failed".into(), json!("commit"), "a commit message is required: this server does not write one".into()));
    assert_eq!(last(&rejected), ("action_failed".into(), json!("push"), "this repository has no remote configured".into()));
    assert_eq!((last(&pr).0, last(&pr).1), ("action_failed".into(), json!("push")));
    assert!(ops(&h).iter().all(|o| o.method() != "git.commit"));
}
