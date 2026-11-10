//! The files, diffs and worktrees of M6 through the chain: each call goes from the phone through `t3-serve serve` to the real rca-service and on to the
//! real rca-exec working in a temporary repository, and its window shows the agent API method rca answered and the executor action that ran. A
//! call counts for its row only when it was answered and the action did its work, which each test reads back from the disk.
use crate::chain::{hit, last_window, writes};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::Script;
use std::time::{Duration, Instant};

fn path(p: impl AsRef<std::path::Path>) -> String {
    p.as_ref().display().to_string()
}

fn every_scope() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

/// A thread of project p1 that works in `worktree`, as a client makes one after it has made the worktree.
fn thread_in(phone: &mut Phone, case: &mut exec_testkit::Case, chain: &Chain, thread: &str, worktree: &str, branch: &str) {
    let command = json!({"type": "thread.create", "commandId": format!("c-{thread}"), "threadId": thread, "projectId": "p1", "title": thread, "worktreePath": worktree, "branch": branch, "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}});
    success(&phone.call(case, chain, &["rpc:orchestration.dispatchCommand", "command:thread.create"], "orchestration.dispatchCommand", command));
}

/// What a window's executor ran, as (action, answered) in order.
fn actions(executor: &Value) -> Vec<(String, bool)> {
    executor.as_array().unwrap().iter().filter(|h| h["tool"].is_string()).map(|h| (h["tool"].as_str().unwrap().to_string(), h["ok"] == true)).collect()
}

#[test]
fn a_file_is_written_searched_and_measured_in_the_repository_and_a_directory_outside_it_is_browsed_only_inside_the_allowlist() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository with a file the model wrote, a directory the executor may browse beside it, and a paired phone
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("notes/a.txt", "hello from the model")]));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        phone.message(case, &chain, "t1", "c-m1", "make a note");
        let root = path(chain.repo.path());
        // When the phone writes a file, searches for the model's file and measures three paths
        let wrote = phone.call(case, &chain, &["rpc:projects.writeFile"], "projects.writeFile", json!({"cwd": root, "relativePath": "docs/new/b.txt", "contents": "written by the phone"}));
        let (wrote_agent, wrote_executor) = last_window(case);
        let found = phone.call(case, &chain, &["rpc:projects.searchEntries"], "projects.searchEntries", json!({"cwd": root, "query": "a.txt", "limit": 10}));
        let (found_agent, found_executor) = last_window(case);
        let measured = phone.call(case, &chain, &["rpc:filesystem.getMetadata"], "filesystem.getMetadata", json!({"paths": [format!("{root}/notes/a.txt"), format!("{root}/notes"), format!("{root}/nothing.txt")]}));
        let (measured_agent, measured_executor) = last_window(case);
        // Then the file is on the disk with its parents made, the search finds the model's file relative to the project, and the measures are the disk's
        assert_eq!(success(&wrote), &json!({"relativePath": "docs/new/b.txt"}));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("docs/new/b.txt")).unwrap(), "written by the phone");
        assert_eq!(success(&found)["entries"], json!([{"path": "notes/a.txt", "kind": "file"}]));
        assert_eq!(success(&measured)["entries"], json!([{"kind": "file", "byteLength": "hello from the model".len(), "mimeType": "text/plain"}, {"kind": "directory"}, null]));
        // And each call went to the agent API method of its row and the executor ran the action that did the work
        assert_eq!(wrote_agent, [("worktree.list".to_string(), true), ("file.write".to_string(), true)]);
        assert!(hit(&wrote_executor, "tool", "write_file").is_some_and(|h| h["ok"] == true), "{wrote_executor}");
        assert_eq!(found_agent.last(), Some(&("file.search".to_string(), true)));
        assert!(hit(&found_executor, "tool", "search_entries").is_some_and(|h| h["ok"] == true), "{found_executor}");
        assert_eq!(measured_agent.iter().filter(|(m, _)| m == "file.stat").count(), 3);
        assert_eq!(actions(&measured_executor).iter().filter(|(a, _)| a == "stat").map(|(_, ok)| *ok).collect::<Vec<_>>(), [true, true, false]);
        // When the phone browses the shared directory by a prefix, and then a directory outside the allowlist
        let shared = path(&chain.shared);
        let listed = phone.call(case, &chain, &["rpc:filesystem.browse"], "filesystem.browse", json!({"partialPath": format!("{shared}/al")}));
        let (listed_agent, listed_executor) = last_window(case);
        let refused = phone.call(case, &chain, &["rpc:filesystem.browse"], "filesystem.browse", json!({"partialPath": "/etc/"}));
        let (refused_agent, refused_executor) = last_window(case);
        // Then the directories that start with the prefix are listed, the files are not, and the executor ran browse for it
        assert_eq!(success(&listed)["entries"], json!([{"name": "alpha", "fullPath": format!("{shared}/alpha")}]));
        assert_eq!(success(&listed)["parentPath"], json!(shared));
        assert_eq!(listed_agent, [("file.browse".to_string(), true)]);
        assert!(hit(&listed_executor, "tool", "browse").is_some_and(|h| h["ok"] == true), "{listed_executor}");
        // And the directory outside the allowlist is refused as the RPC declares, after the executor was asked and said no: the request arrived
        assert_eq!((failure(&refused)["_tag"].as_str(), failure(&refused)["failure"].as_str()), (Some("FilesystemBrowseError"), Some("read_directory_failed")));
        assert_eq!(refused_agent, [("file.browse".to_string(), false)]);
        assert!(hit(&refused_executor, "tool", "browse").is_some_and(|h| h["ok"] == false), "{refused_executor}");
        assert_eq!(case.calls().iter().filter(|c| c["ok"] == false).count(), 1, "only the refused browse is an unserved call");
    });
}

#[test]
fn the_diff_of_a_thread_is_what_git_shows_in_its_directory_for_the_whole_thread_and_for_its_turns() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose one turn changed the repository's README
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("README.md", "temp repo\nchanged by the model\n")]));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        phone.message(case, &chain, "t1", "c-m1", "change the readme");
        // When the phone asks for the diff of turns 0 to 1, and for the thread's whole diff
        let turn = phone.call(case, &chain, &["rpc:orchestration.getTurnDiff"], "orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 0, "toTurnCount": 1}));
        let (turn_agent, turn_executor) = last_window(case);
        let whole = phone.call(case, &chain, &["rpc:orchestration.getFullThreadDiff"], "orchestration.getFullThreadDiff", json!({"threadId": "t1", "toTurnCount": 1}));
        let (whole_agent, whole_executor) = last_window(case);
        // Then both are git's diff of the change the model made, with the thread and the counts
        for diff in [&turn, &whole] {
            let text = success(diff)["diff"].as_str().unwrap();
            assert!(text.contains("diff --git a/README.md b/README.md") && text.contains("+changed by the model"), "{text}");
        }
        assert_eq!((success(&turn)["fromTurnCount"].clone(), success(&turn)["toTurnCount"].clone()), (json!(0), json!(1)));
        // And each asked rca for its own method and the executor ran git_diff
        assert_eq!(turn_agent.last(), Some(&("turn.diff".to_string(), true)));
        assert_eq!(whole_agent.last(), Some(&("thread.diff".to_string(), true)));
        assert!(hit(&turn_executor, "tool", "git_diff").is_some_and(|h| h["ok"] == true) && hit(&whole_executor, "tool", "git_diff").is_some_and(|h| h["ok"] == true));
        // When the phone asks for a range that starts after the thread's start, the request never reaches rca
        let later = phone.call(case, &chain, &["rpc:orchestration.getTurnDiff"], "orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 1, "toTurnCount": 1}));
        assert!(failure(&later)["message"].as_str().unwrap().contains("no revision is recorded"));
        assert!(last_window(case).0.is_empty());
    });
}

#[test]
fn a_worktree_made_for_a_new_branch_is_a_checkout_under_the_base_whose_files_and_diff_are_the_worktrees_and_not_the_repositorys() {
    exec_testkit::case!(SUITE, |case| {
        // Given a phone paired with every scope (making a worktree needs source-control write) and the repository the executor serves
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &every_scope());
        phone.seed(case, &chain);
        let root = path(chain.repo.path());
        // When the phone makes a worktree on a new branch from HEAD
        let made = phone.call(case, &chain, &["rpc:vcs.createWorktree"], "vcs.createWorktree", json!({"cwd": root, "refName": "HEAD", "newRefName": "feature/chain", "path": null}));
        let (made_agent, made_executor) = last_window(case);
        let worktree = success(&made)["worktree"]["path"].as_str().unwrap().to_string();
        // Then the worktree is a checkout of the repository under the executor's base, on the new branch, and the repository has it listed
        assert!(std::path::Path::new(&worktree).starts_with(&chain.base), "{worktree}");
        assert_eq!(success(&made)["worktree"]["refName"], "feature/chain");
        assert_eq!(std::fs::read_to_string(std::path::Path::new(&worktree).join("README.md")).unwrap(), "temp repo\n");
        assert_eq!(chain.repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 2);
        assert_eq!(chain.repo.git(&["-C", &worktree, "branch", "--show-current"]).trim(), "feature/chain");
        assert_eq!(made_agent, [("worktree.list".to_string(), true), ("worktree.create".to_string(), true)]);
        assert!(hit(&made_executor, "tool", "git_worktree_add").is_some_and(|h| h["ok"] == true), "{made_executor}");
        // When a thread works in the worktree and the phone writes a file and asks for the thread's diff there
        thread_in(&mut phone, case, &chain, "t2", &worktree, "feature/chain");
        phone.call(case, &chain, &["rpc:projects.writeFile"], "projects.writeFile", json!({"cwd": worktree, "relativePath": "README.md", "contents": "temp repo\nin the worktree\n"}));
        let (write_agent, _) = last_window(case);
        let diff = phone.call(case, &chain, &["rpc:orchestration.getFullThreadDiff"], "orchestration.getFullThreadDiff", json!({"threadId": "t2", "toTurnCount": 0}));
        // Then the file is in the worktree and not in the repository, the agent API placed the write in the worktree's workspace, and the diff shows it
        assert_eq!(std::fs::read_to_string(std::path::Path::new(&worktree).join("README.md")).unwrap(), "temp repo\nin the worktree\n");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("README.md")).unwrap(), "temp repo\n");
        assert_eq!(write_agent, [("worktree.list".to_string(), true), ("file.write".to_string(), true)]);
        let text = success(&diff)["diff"].as_str().unwrap();
        assert!(text.contains("+in the worktree") && !text.contains("+changed"), "{text}");
        // When a branch git would read as an option is asked for, rca refuses it and nothing is made
        let refused = phone.call(case, &chain, &["rpc:vcs.createWorktree"], "vcs.createWorktree", json!({"cwd": root, "refName": "HEAD", "newRefName": "--orphan", "path": null}));
        assert_eq!(failure(&refused)["_tag"], "GitCommandError");
        assert_eq!(last_window(case).0.last(), Some(&("worktree.create".to_string(), false)));
        assert_eq!(chain.repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 2);
    });
}

/// Every chunk of the stream until a snapshot has one of the phases in `until`, or its last snapshot at the deadline.
fn snapshots_until(phone: &mut Phone, id: &Value, until: &[&str]) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen: Vec<Value> = Vec::new();
    while Instant::now() < deadline {
        if let Some(frame) = phone.watch.try_recv(Duration::from_millis(500)) {
            if frame["_tag"] == "Chunk" && &frame["requestId"] == id {
                phone.watch.ack(id);
                seen.extend(frame["values"].as_array().unwrap().clone());
                if seen.last().is_some_and(|s| s["phase"].as_str().is_some_and(|p| until.contains(&p))) {
                    break;
                }
            }
        }
    }
    seen
}

#[test]
fn the_setup_of_a_worktree_rca_started_is_followed_by_the_phone_to_done_and_a_long_one_is_cancelled_by_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given a worktree whose setup another client of rca started (two commands, the second writing a file), and a thread of the phone in it
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &every_scope());
        phone.seed(case, &chain);
        let mut side = chain.direct();
        let mut setup = |name: &str, commands: Value| {
            let made = side.call("workspace.worktrees", "worktree.create", json!({"workspace": "w1", "worktree": name, "branch": format!("feature/{name}"), "base": "HEAD", "setup": commands}));
            made.expect("rca makes the worktree")["path"].as_str().unwrap().to_string()
        };
        let quick = setup("quick", json!(["echo first", "echo second > made.txt"]));
        thread_in(&mut phone, case, &chain, "t2", &quick, "feature/quick");
        // When the phone follows the thread's worktree setup
        let (id, first) = case.call(&chain.tap, &rows(&["rpc:subscribeWorktreeSetup"]), || {
            let id = phone.watch.send("subscribeWorktreeSetup", json!({"threadId": "t2"}));
            let first = phone.watch.chunk(&id);
            (first.first().is_some_and(|s| s.is_object()), (id, first))
        });
        let (agent, _) = last_window(case);
        let mut snapshots = first;
        if snapshots.last().is_some_and(|s| s["phase"] == "running") {
            snapshots.extend(snapshots_until(&mut phone, &id, &["done", "failed", "cancelled"]));
        }
        // Then the setup ends done, both commands counted, in the worktree the thread is in, and the second command's file is on the disk
        let last = snapshots.last().unwrap();
        assert_eq!((last["phase"].as_str(), last["threadId"].as_str(), last["worktreePath"].as_str(), last["branch"].as_str()), (Some("done"), Some("t2"), Some(quick.as_str()), Some("feature/quick")));
        assert_eq!((last["stages"][1]["id"].as_str(), last["stages"][1]["percent"].clone(), last["error"].clone()), (Some("setup-script"), json!(100), Value::Null));
        assert_eq!(std::fs::read_to_string(std::path::Path::new(&quick).join("made.txt")).unwrap(), "second\n");
        assert_eq!(agent, [("worktree.list".to_string(), true), ("worktree.setup.subscribe".to_string(), true)]);
        // When a setup that sleeps is started for another thread and the phone cancels it
        let slow = setup("slow", json!(["sleep 60"]));
        thread_in(&mut phone, case, &chain, "t3", &slow, "feature/slow");
        let running = Instant::now();
        let stopped = loop {
            let exit = phone.call(case, &chain, &["rpc:worktreeSetup.cancel"], "worktreeSetup.cancel", json!({"threadId": "t3"}));
            if success(&exit)["cancelled"] == true || running.elapsed() > Duration::from_secs(20) {
                break exit;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let (cancel_agent, _) = last_window(case);
        // Then the cancel stopped it, the agent API was asked to, and the setup the phone follows afterwards ended cancelled
        assert_eq!(success(&stopped), &json!({"cancelled": true}));
        assert_eq!(cancel_agent, [("worktree.list".to_string(), true), ("worktree.setup.cancel".to_string(), true)]);
        let id = phone.watch.send("subscribeWorktreeSetup", json!({"threadId": "t3"}));
        let mut after = phone.watch.chunk(&id);
        if after.last().is_some_and(|s| s["phase"] == "running") {
            after.extend(snapshots_until(&mut phone, &id, &["cancelled", "failed", "done"]));
        }
        assert_eq!(after.last().unwrap()["phase"], "cancelled", "{after:?}");
        // And a thread outside any worktree follows nothing: null
        let none = phone.watch.send("subscribeWorktreeSetup", json!({"threadId": "t1"}));
        assert_eq!(phone.watch.chunk(&none), [Value::Null]);
    });
}
