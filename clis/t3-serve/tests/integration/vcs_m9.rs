//! The git rows of M9 through the chain: each call goes from the phone through `t3-serve serve` to the real rca-service and on to the real rca-exec,
//! whose git runs in the temporary repository (with a bare remote of its own for the pushes and pulls). A call counts for its row only when it was
//! answered and the executor ran the git action, and each test reads the repository back with git itself: the state git holds is the evidence,
//! not the answer.
use crate::chain::{hit, last_window};
use crate::rig::*;
use crate::support::*;
use std::time::{Duration, Instant};

fn ran(executor: &Value, action: &str) -> bool {
    hit(executor, "tool", action).is_some_and(|h| h["ok"] == true)
}

fn asked(agent: &[(String, bool)], method: &str) -> bool {
    agent.iter().any(|(m, ok)| m == method && *ok)
}

/// A stacked action as one call of the trace: its progress frames are read from the watch socket until the stream exits, and it is served when
/// the last frame is `action_finished`.
/// The chunks other streams of the socket sent meanwhile are acknowledged and kept in `others`.
fn stacked(phone: &mut Phone, case: &mut exec_testkit::Case, chain: &Chain, payload: Value, others: &mut Vec<Value>) -> Vec<Value> {
    case.call(&chain.tap, &rows(&["rpc:git.runStackedAction"]), || {
        let id = phone.watch.send("git.runStackedAction", payload);
        let mut frames: Vec<Value> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            assert!(Instant::now() < deadline, "the action never ended; it said {frames:?}");
            let Some(frame) = phone.watch.try_recv(Duration::from_millis(300)) else { continue };
            match frame["_tag"].as_str() {
                Some("Chunk") if frame["requestId"] == id => {
                    phone.watch.ack(&id);
                    frames.extend(frame["values"].as_array().cloned().unwrap_or_default());
                }
                Some("Exit") if frame["requestId"] == id => break,
                Some("Chunk") => {
                    phone.watch.ack(&frame["requestId"]);
                    others.push(frame);
                }
                _ => {}
            }
        }
        let finished = frames.last().is_some_and(|f| f["kind"] == "action_finished");
        (finished, frames)
    })
}

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

fn kinds(frames: &[Value]) -> Vec<&str> {
    frames.iter().map(|f| f["kind"].as_str().unwrap()).collect()
}

#[test]
fn the_status_refs_and_diffs_of_the_repository_are_read_and_a_branch_is_made_and_switched_to_through_the_executors_git() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired phone and a repository with an edited file, a new file, and a commit more on main to look at as a commit
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let root = chain.repo.path().display().to_string();
        chain.repo.write("notes.txt", "one\n");
        chain.repo.commit_all("add notes");
        let sha = chain.repo.head();
        chain.repo.write("README.md", "temp repo\nedited\n");
        chain.repo.write("new.txt", "new\n");
        // When the phone refreshes the status
        let status = phone.call(case, &chain, &["rpc:vcs.refreshStatus"], "vcs.refreshStatus", json!({"cwd": root}));
        let (agent, executor) = last_window(case);
        // Then the executor's git answered status, upstream and diff, and the status is what git shows: the edit counted, the untracked file listed
        assert!(asked(&agent, "git.status") && asked(&agent, "git.upstreamStatus") && asked(&agent, "git.diff"), "{agent:?}");
        assert!(ran(&executor, "git_status") && ran(&executor, "git_upstream") && ran(&executor, "git_diff"), "{executor}");
        let status = success(&status);
        assert_eq!((status["refName"].clone(), status["hasWorkingTreeChanges"].clone(), status["hasUpstream"].clone()), (json!("main"), json!(true), json!(false)));
        assert_eq!(status["workingTree"]["files"], json!([{"path": "README.md", "insertions": 1, "deletions": 0}, {"path": "new.txt", "insertions": 0, "deletions": 0}]));
        assert_eq!(chain.repo.git(&["status", "--porcelain"]), " M README.md\n?? new.txt");
        // When the phone makes a branch and switches to it
        let made = phone.call(case, &chain, &["rpc:vcs.createRef"], "vcs.createRef", json!({"cwd": root, "refName": "feature/x", "switchRef": true}));
        let (agent, executor) = last_window(case);
        // Then git has the branch and is on it
        assert_eq!(success(&made), &json!({"refName": "feature/x"}));
        assert!(asked(&agent, "git.createRef") && ran(&executor, "git_create_ref"), "{agent:?} {executor}");
        assert_eq!(chain.repo.git(&["branch", "--show-current"]), "feature/x");
        // When the phone lists the refs it sees both branches and which one is current, and a search narrows them
        let refs = phone.call(case, &chain, &["rpc:vcs.listRefs"], "vcs.listRefs", json!({"cwd": root}));
        let (agent, executor) = last_window(case);
        assert!(asked(&agent, "git.refs") && ran(&executor, "git_refs"), "{agent:?} {executor}");
        let listed: Vec<(&str, bool)> = success(&refs)["refs"].as_array().unwrap().iter().map(|r| (r["name"].as_str().unwrap(), r["current"] == true)).collect();
        assert_eq!(listed, [("feature/x", true), ("main", false)]);
        assert_eq!(success(&refs)["hasPrimaryRemote"], json!(false));
        chain.repo.git(&["remote", "add", "origin", "/nonexistent/origin.git"]);
        let found = phone.call(case, &chain, &["rpc:vcs.listRefs"], "vcs.listRefs", json!({"cwd": root, "query": "mai"}));
        assert_eq!((success(&found)["totalCount"].clone(), success(&found)["hasPrimaryRemote"].clone()), (json!(1), json!(true)));
        let local = phone.call(case, &chain, &["rpc:vcs.listRefs"], "vcs.listRefs", json!({"cwd": root, "refKind": "local"}));
        assert_eq!(success(&local)["hasPrimaryRemote"], json!(true));
        // When the phone switches back to main
        let switched = phone.call(case, &chain, &["rpc:vcs.switchRef"], "vcs.switchRef", json!({"cwd": root, "refName": "main"}));
        let (agent, executor) = last_window(case);
        // Then git is on main again, and the answer names it
        assert_eq!(success(&switched), &json!({"refName": "main"}));
        assert!(asked(&agent, "git.checkout") && ran(&executor, "git_checkout"), "{agent:?} {executor}");
        assert_eq!(chain.repo.git(&["branch", "--show-current"]), "main");
        // When the phone asks for the review of the working tree and of the commit
        let preview = phone.call(case, &chain, &["rpc:review.getDiffPreview"], "review.getDiffPreview", json!({"cwd": root, "commitSha": sha}));
        let (agent, executor) = last_window(case);
        assert!(asked(&agent, "git.diff") && asked(&agent, "git.history") && ran(&executor, "git_diff") && ran(&executor, "git_log"), "{agent:?} {executor}");
        let sources = success(&preview)["sources"].as_array().unwrap();
        // Then the working tree's diff is the edit, and the commit's is the file it added, titled by its subject
        assert_eq!((sources[0]["kind"].as_str(), sources[0]["diff"].as_str().unwrap().contains("+edited")), (Some("working-tree"), true));
        assert_eq!((sources[1]["kind"].as_str(), sources[1]["title"].as_str(), sources[1]["files"][0]["path"].as_str(), sources[1]["files"][0]["additions"].clone()), (Some("commit"), Some("add notes"), Some("notes.txt"), json!(1)));
        // When it asks for the two sides of the edited file and of the committed one
        let side = |phone: &mut Phone, case: &mut exec_testkit::Case, input: Value| {
            let exit = phone.call(case, &chain, &["rpc:review.getDiffFileContents"], "review.getDiffFileContents", input);
            success(&exit).clone()
        };
        let edited = side(&mut phone, case, json!({"cwd": root, "sourceKind": "working-tree", "changeType": "change", "baseRef": null, "headRef": null, "oldPath": "README.md", "newPath": "README.md"}));
        let (agent, executor) = last_window(case);
        assert!(asked(&agent, "git.blob") && ran(&executor, "git_blob") && ran(&executor, "read_file"), "{agent:?} {executor}");
        let added = side(&mut phone, case, json!({"cwd": root, "sourceKind": "commit", "changeType": "new", "baseRef": null, "headRef": sha, "oldPath": "notes.txt", "newPath": "notes.txt"}));
        // Then each side is the content git holds at that revision, and the tree's file is read as it is now
        assert_eq!(edited, json!({"oldContents": "temp repo\n", "newContents": "temp repo\nedited\n"}));
        assert_eq!(added, json!({"oldContents": "", "newContents": "one\n"}));
        // And a branch name git refuses, and a ref that is an option, are refused as the app declares after the executor was asked
        let bad = phone.calls.call("vcs.createRef", json!({"cwd": root, "refName": "a..b"}));
        assert_eq!((failure(&bad)["_tag"].as_str(), failure(&bad)["detail"].as_str()), (Some("GitCommandError"), Some("`name` must be a branch or tag name")));
        let option = phone.calls.call("vcs.switchRef", json!({"cwd": root, "refName": "--orphan=x"}));
        assert_eq!(failure(&option)["_tag"], "GitCommandError");
        assert_eq!(chain.repo.git(&["branch", "--show-current"]), "main");
    });
}

#[test]
fn a_commit_and_push_are_stacked_with_progress_a_pull_brings_what_another_clone_pushed_and_the_status_stream_follows_the_repository() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired phone, a repository with a bare remote it has configured, and a change in its tree
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let root = chain.repo.path().display().to_string();
        let remote = tempfile::tempdir().unwrap();
        let bare = remote.path().join("origin.git");
        chain.repo.git(&["init", "--quiet", "--bare", "--initial-branch=main", bare.to_str().unwrap()]);
        chain.repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        chain.repo.write("feature.txt", "work\n");
        // When the phone follows the status of the repository
        let (watch, first) = phone.stream(case, &chain, &["rpc:subscribeVcsStatus"], "subscribeVcsStatus", json!({"cwd": root}));
        let (agent, executor) = last_window(case);
        // Then the first frame is the snapshot, with the untracked file, run through the executor's status
        assert!(asked(&agent, "git.status") && ran(&executor, "git_status"), "{agent:?} {executor}");
        assert_eq!((first[0]["_tag"].as_str(), first[0]["local"]["workingTree"]["files"][0]["path"].as_str(), first[0]["remote"]["hasUpstream"].clone()), (Some("snapshot"), Some("feature.txt"), json!(false)));
        // When the phone commits and pushes the change in one action
        let mut meanwhile: Vec<Value> = Vec::new();
        let frames = stacked(&mut phone, case, &chain, json!({"actionId": "act-1", "cwd": root, "action": "commit_push", "commitMessage": "add the feature"}), &mut meanwhile);
        let (agent, executor) = last_window(case);
        // Then the progress named both phases and ended with the result, the executor ran stage, commit and push, and git holds the commit on both ends
        assert_eq!(kinds(&frames), ["action_started", "phase_started", "phase_started", "action_finished"]);
        assert!(["git.stage", "git.commit", "git.push"].iter().all(|m| asked(&agent, m)), "{agent:?}");
        assert!(["git_stage", "git_commit", "git_push"].iter().all(|a| ran(&executor, a)), "{executor}");
        let result = &frames[3]["result"];
        assert_eq!((result["commit"]["status"].clone(), result["commit"]["subject"].clone(), result["push"]["status"].clone(), result["push"]["setUpstream"].clone()), (json!("created"), json!("add the feature"), json!("pushed"), json!(true)));
        assert_eq!(chain.repo.git(&["log", "--format=%s", "-1"]), "add the feature");
        assert_eq!(chain.repo.git(&["--git-dir", bare.to_str().unwrap(), "rev-parse", "main"]), chain.repo.head());
        // And the status stream told the phone the tree became clean and that the branch has an upstream
        let deadline = Instant::now() + Duration::from_secs(25);
        let mut told: Vec<Value> = meanwhile.iter().filter(|f| f["requestId"] == watch).flat_map(|f| f["values"].as_array().cloned().unwrap_or_default()).collect();
        while !(told.iter().any(|f| f["_tag"] == "localUpdated" && f["local"]["hasWorkingTreeChanges"] == false) && told.iter().any(|f| f["_tag"] == "remoteUpdated" && f["remote"]["hasUpstream"] == true)) {
            assert!(Instant::now() < deadline, "the stream said {told:?}");
            if let Some(frame) = phone.watch.try_recv(Duration::from_millis(300)) {
                if frame["_tag"] == "Chunk" {
                    phone.watch.ack(&frame["requestId"]);
                    if frame["requestId"] == watch {
                        told.extend(frame["values"].as_array().cloned().unwrap_or_default());
                    }
                }
            }
        }
        // When another clone pushes a commit and the phone pulls
        let other = remote.path().join("other");
        chain.repo.git(&["clone", "--quiet", bare.to_str().unwrap(), other.to_str().unwrap()]);
        let in_other = |args: &[&str]| {
            let out = std::process::Command::new("git").args(args).current_dir(&other).env_clear().envs(chain.repo.git_env()).env("PATH", std::env::var_os("PATH").unwrap_or_default()).output().unwrap();
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        std::fs::write(other.join("remote.txt"), "from afar\n").unwrap();
        in_other(&["add", "--all"]);
        in_other(&["commit", "--quiet", "-m", "from afar"]);
        in_other(&["push", "--quiet", "origin", "main"]);
        chain.repo.git(&["fetch", "--quiet", "origin"]);
        let pulled = phone.call(case, &chain, &["rpc:vcs.pull"], "vcs.pull", json!({"cwd": root}));
        let (agent, executor) = last_window(case);
        // Then the working tree has the pushed file, and the answer says it pulled the branch from its upstream
        assert_eq!(success(&pulled), &json!({"status": "pulled", "refName": "main", "upstreamRef": "origin/main"}));
        assert!(asked(&agent, "git.pull") && ran(&executor, "git_pull"), "{agent:?} {executor}");
        assert!(chain.repo.path().join("remote.txt").exists());
        // And pulling again finds nothing new
        let again = phone.call(case, &chain, &["rpc:vcs.pull"], "vcs.pull", json!({"cwd": root}));
        assert_eq!(success(&again)["status"], json!("skipped_up_to_date"));
        // When the phone stops following, the executor's watch ends (its git_watch answers once the stream is gone)
        case.call(&chain.tap, &rows(&["rpc:subscribeVcsStatus"]), || {
            phone.watch.send_raw(&json!({"_tag": "Interrupt", "requestId": watch}).to_string());
            std::thread::sleep(Duration::from_millis(3500));
            (true, ())
        });
        assert!(ran(&last_window(case).1, "git_watch"), "the watch never ended: {}", last_window(case).1);
    });
}

#[test]
fn a_stacked_action_with_nothing_to_commit_or_no_remote_ends_with_the_reason_and_leaves_the_repository_as_it_was() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired phone and a clean repository with no remote
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let root = chain.repo.path().display().to_string();
        let head = chain.repo.head();
        let run = |phone: &mut Phone, payload: Value| {
            let id = phone.watch.send("git.runStackedAction", payload);
            let mut frames: Vec<Value> = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(25);
            while Instant::now() < deadline {
                let Some(frame) = phone.watch.try_recv(Duration::from_millis(300)) else { continue };
                match frame["_tag"].as_str() {
                    Some("Chunk") if frame["requestId"] == id => {
                        phone.watch.ack(&id);
                        frames.extend(frame["values"].as_array().cloned().unwrap_or_default());
                    }
                    Some("Exit") if frame["requestId"] == id => return frames,
                    _ => {}
                }
            }
            panic!("the action never ended; it said {frames:?}");
        };
        // When the phone commits with nothing changed, and pushes where there is no remote
        let empty = run(&mut phone, json!({"actionId": "a1", "cwd": root, "action": "commit", "commitMessage": "nothing"}));
        let pushed = run(&mut phone, json!({"actionId": "a2", "cwd": root, "action": "push"}));
        // Then the commit was skipped as having no changes, the push failed with the executor's reason, and the repository is where it was
        assert_eq!(empty.last().unwrap()["result"]["commit"]["status"], json!("skipped_no_changes"));
        let failed = pushed.last().unwrap();
        assert_eq!((failed["kind"].as_str(), failed["phase"].as_str(), failed["message"].as_str()), (Some("action_failed"), Some("push"), Some("this repository has no remote configured")));
        assert_eq!(chain.repo.head(), head);
    });
}
