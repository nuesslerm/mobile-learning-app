//! The M6 rows through the real router over the agent double: the files of a project written, searched, browsed and measured, the diffs of a
//! thread, the worktrees of a project and the progress of a worktree's setup. The double answers what the agent API's contract recorded for each
//! method, so what a request becomes (the `Op` the agent API is asked) and what the phone is told are both read from data the contract owns.
use super::support::*;
use std::time::Duration;
use t3_core::ports::{AgentError, Op, SetupBatch, SetupEvent};

/// The result the contract recorded for `method` of a sub-group's folder, for the exchange whose name contains `name`.
fn recorded(folder: &str, method: &str, name: &str) -> Value {
    let fixtures = agent_api_workspace::testing::group_fixtures(folder);
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name) && e.succeeds());
    found.unwrap_or_else(|| panic!("no recorded {method} exchange named like {name}")).response["result"].clone()
}

fn error_of(folder: &str, method: &str) -> String {
    let fixtures = agent_api_workspace::testing::group_fixtures(folder);
    let found = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && !e.succeeds()).unwrap_or_else(|| panic!("no recorded refusal of {method}"));
    found.response["error"]["message"].as_str().unwrap().to_string()
}

fn all_scopes() -> PairingRequest {
    PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() }
}

/// A server with a project `p1` (root `/workspace/demo`), a thread `t1` on it, and a thread `t2` in the worktree `wt_path`.
fn started(wt_path: &str) -> (Harness, Rpc) {
    let h = Harness::start();
    let mut rpc = connect_with(&h, all_scopes());
    seed(&mut rpc);
    let in_worktree = json!({"type": "thread.create", "commandId": "c-t2", "threadId": "t2", "projectId": "p1", "title": "In a worktree", "worktreePath": wt_path, "branch": "feature/setup"});
    success(&rpc.call("orchestration.dispatchCommand", in_worktree));
    forget_seeding(&h);
    (h, rpc)
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

#[test]
fn a_file_written_through_the_project_is_asked_of_the_agent_api_under_its_root_and_a_refusal_is_the_declared_write_error() {
    // Given a project whose root is /workspace/demo and an agent API that writes as recorded
    let (h, mut rpc) = started("/base/wt");
    let wrote = recorded("files-edit", "file.write", "replaces a file");
    reply(&h, "file.write", Ok(wrote));
    // When the phone writes a file, and then one the agent API refuses
    let done = rpc.call("projects.writeFile", json!({"cwd": "/workspace/demo", "relativePath": "src/new.txt", "contents": "hi\n"}));
    reply(&h, "file.write", Err(AgentError::Failed(error_of("files-edit", "file.write"))));
    let refused = rpc.call("projects.writeFile", json!({"cwd": "/workspace/demo", "relativePath": "../x", "contents": "x"}));
    // Then the agent API was asked for the file under the project's root with the contents, and the phone is told the path
    assert_eq!(ops(&h)[0], Op::FileWrite { root: "/workspace/demo".into(), path: "src/new.txt".into(), content: "hi\n".into() });
    assert_eq!(success(&done), &json!({"relativePath": "src/new.txt"}));
    // And the refusal reaches the phone as the RPC's own error carrying the agent API's message
    let error = failure(&refused);
    assert_eq!((error["_tag"].as_str(), error["message"].as_str().map(str::to_string), error["relativePath"].as_str()), (Some("ProjectWriteFileError"), Some(error_of("files-edit", "file.write")), Some("../x")));
    // And a write with no contents is a defect and asks nothing
    let asked = ops(&h).len();
    assert_eq!(rpc.call("projects.writeFile", json!({"cwd": "/workspace/demo", "relativePath": "a"}))["cause"][0]["_tag"], "Die");
    assert_eq!(ops(&h).len(), asked);
}

#[test]
fn browsing_lists_the_directories_of_the_partial_paths_directory_that_start_with_its_last_segment_and_says_why_it_could_not() {
    // Given an agent API that lists the directory the contract recorded: two directories and a file
    let (h, mut rpc) = started("/base/wt");
    let listed = recorded("files-edit", "file.browse", "lists a directory");
    let parent = listed["path"].as_str().unwrap().to_string();
    let dirs: Vec<String> = listed["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "dir").map(|e| e["name"].as_str().unwrap().to_string()).collect();
    let files: Vec<String> = listed["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "file").map(|e| e["name"].as_str().unwrap().to_string()).collect();
    reply(&h, "file.browse", Ok(listed));
    // When the phone browses the parent with a slash, with the start of the first directory's name, from a project with a relative path, and without one
    let slash = rpc.call("filesystem.browse", json!({"partialPath": format!("{parent}/")}));
    let start = &dirs[0][..1];
    let prefixed = rpc.call("filesystem.browse", json!({"partialPath": format!("{parent}/{}", start.to_uppercase())}));
    let relative = rpc.call("filesystem.browse", json!({"partialPath": "sr", "cwd": "/workspace/demo"}));
    let asked = ops(&h).len();
    let no_project = rpc.call("filesystem.browse", json!({"partialPath": "sr"}));
    let windows = rpc.call("filesystem.browse", json!({"partialPath": "C:\\Users"}));
    // Then the directories are listed whole for the slash, files are never listed, and a prefix keeps the directories it starts (whatever its case)
    let names = |exit: &Value| success(exit)["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&slash), dirs);
    assert!(files.iter().all(|f| !names(&slash).contains(f)));
    assert_eq!(names(&prefixed), dirs.iter().filter(|d| d.to_lowercase().starts_with(&start.to_lowercase())).cloned().collect::<Vec<_>>());
    assert_eq!(names(&relative), dirs.iter().filter(|d| d.starts_with("sr")).cloned().collect::<Vec<_>>());
    assert_eq!((success(&slash)["parentPath"].as_str(), success(&slash)["entries"][0]["fullPath"].as_str()), (Some(parent.as_str()), Some(format!("{parent}/{}", dirs[0]).as_str())));
    // And the agent API was asked for the directory the partial path is in: the parent as given, the project's own directory for a relative one
    let browses: Vec<Op> = ops(&h).into_iter().take(asked).collect();
    assert_eq!(browses[0], Op::FileBrowse { root: String::new(), path: parent.clone(), show_hidden: false });
    assert_eq!(browses[2], Op::FileBrowse { root: "/workspace/demo".into(), path: ".".into(), show_hidden: false });
    // And a relative path with no project and a Windows path are refused with the RPC's own reasons before the agent API is asked
    assert_eq!((failure(&no_project)["failure"].as_str(), failure(&windows)["failure"].as_str()), (Some("current_project_required"), Some("windows_path_unsupported")));
    assert_eq!(ops(&h).len(), asked);
    // And an agent API that refuses the directory is read_directory_failed with its message
    reply(&h, "file.browse", Err(AgentError::Failed(error_of("files-edit", "file.browse"))));
    let outside = rpc.call("filesystem.browse", json!({"partialPath": "/etc/"}));
    assert_eq!((failure(&outside)["failure"].as_str(), failure(&outside)["message"].as_str().map(str::to_string)), (Some("read_directory_failed"), Some(error_of("files-edit", "file.browse"))));
}

#[test]
fn the_metadata_of_each_path_is_its_kind_and_size_in_request_order_and_null_for_a_path_the_agent_api_cannot_stat() {
    // Given an agent API that stats a file as recorded, a directory, and refuses the rest
    let (h, mut rpc) = started("/base/wt");
    let file = recorded("files-edit", "file.stat", "gives the kind");
    let size = file["size"].clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::FileStat { path, .. } if path.ends_with(".png") => Some(Ok(json!({"kind": "file", "size": 9, "modified_ms": 0}))),
        Op::FileStat { path, .. } if path.ends_with(".txt") => Some(Ok(json!({"kind": "file", "size": file["size"], "modified_ms": 0}))),
        Op::FileStat { path, .. } if path.ends_with("/dir") => Some(Ok(json!({"kind": "dir", "size": 0, "modified_ms": 0}))),
        Op::FileStat { .. } => Some(Err(AgentError::Failed("missing".into()))),
        _ => None,
    }));
    // When the phone asks about a text file, a missing path, a directory and an image
    let exit = rpc.call("filesystem.getMetadata", json!({"paths": ["/p/a.txt", "/p/nope", "/p/dir", "/p/i.png"]}));
    // Then each answer is in order: a file has its byte length, a directory only its kind, an image its type, and the missing one is null
    assert_eq!(success(&exit), &json!({"entries": [{"kind": "file", "byteLength": size, "mimeType": "text/plain"}, null, {"kind": "directory"}, {"kind": "file", "byteLength": 9, "mimeType": "image/png"}]}));
    // And an empty list and a list over the batch limit are defects that ask nothing
    let asked = ops(&h).len();
    let too_many: Vec<String> = (0..65).map(|i| format!("/p/{i}")).collect();
    assert_eq!(rpc.call("filesystem.getMetadata", json!({"paths": []}))["cause"][0]["_tag"], "Die");
    assert_eq!(rpc.call("filesystem.getMetadata", json!({"paths": too_many}))["cause"][0]["_tag"], "Die");
    assert_eq!(ops(&h).len(), asked);
}

#[test]
fn searching_the_project_gives_the_agent_apis_entries_filtered_by_kind_and_images_cut_at_the_limit_and_an_empty_query_lists_the_top_level() {
    // Given an agent API whose search answers the entries the contract recorded
    let (h, mut rpc) = started("/base/wt");
    let found = recorded("files-edit", "file.search", "finds the entries");
    let all: Vec<(String, String)> = found["entries"].as_array().unwrap().iter().map(|e| (e["path"].as_str().unwrap().to_string(), e["kind"].as_str().unwrap().to_string())).collect();
    reply(&h, "file.search", Ok(found));
    h.agent.files.lock().unwrap().extend([("/workspace/demo/README.md".to_string(), "x".to_string()), ("/workspace/demo/src/a.rs".to_string(), "x".to_string())]);
    // When the phone searches, for files only, with a limit below the matches, for images, and with no query
    let search = |rpc: &mut Rpc, extra: Value| {
        let mut payload = json!({"cwd": "/workspace/demo", "query": "main", "limit": 10});
        payload.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        rpc.call("projects.searchEntries", payload)
    };
    let plain = search(&mut rpc, json!({}));
    let cut = search(&mut rpc, json!({"limit": 2}));
    let dirs = search(&mut rpc, json!({"kind": "directory"}));
    let images = search(&mut rpc, json!({"imageOnly": true}));
    let top = rpc.call("projects.searchEntries", json!({"cwd": "/workspace/demo", "query": "", "limit": 10}));
    // Then every entry arrives with its path and kind, a limit cuts and says truncated, no entry is a directory here, and no entry is an image
    let paths = |exit: &Value| success(exit)["entries"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(paths(&plain), all.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>());
    assert_eq!((paths(&cut).len(), success(&cut)["truncated"].clone()), (2, json!(true)));
    assert_eq!(success(&plain)["truncated"], json!(false));
    assert!(paths(&dirs).is_empty() && paths(&images).is_empty());
    // And the agent API was asked for the query in the project's own directory, and an empty query lists its top level instead
    assert_eq!(ops(&h)[0], Op::FileSearch { root: "/workspace/demo".into(), query: "main".into(), limit: 10 });
    assert_eq!(paths(&top), ["README.md", "src"]);
    assert_eq!(h.agent.calls.lock().unwrap().iter().filter(|c| **c == "file.list").count(), 1);
}

#[test]
fn the_diff_of_a_thread_is_the_agent_apis_diff_of_its_directory_and_only_the_threads_whole_work_can_be_given() {
    // Given a thread on the project root and one in a worktree, each with one finished turn, and an agent API that diffs as recorded
    let (h, mut rpc) = started("/base/wt");
    for thread in ["t1", "t2"] {
        success(&rpc.call("orchestration.dispatchCommand", json!({"type": "message.dispatch", "commandId": format!("c-m-{thread}"), "threadId": thread, "message": {"messageId": format!("m-{thread}"), "text": "go"}})));
        settled(&mut rpc, thread);
    }
    let turn_diff = recorded("files-edit", "turn.diff", "shows the changes");
    let thread_diff = recorded("files-edit", "thread.diff", "shows the changes");
    reply(&h, "turn.diff", Ok(turn_diff.clone()));
    reply(&h, "thread.diff", Ok(thread_diff.clone()));
    // When the phone asks for the turns 0 to 1 of the thread on the root, the whole diff of the worktree thread, and a range from a later turn
    let turns = rpc.call("orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 0, "toTurnCount": 1}));
    let whole = rpc.call("orchestration.getFullThreadDiff", json!({"threadId": "t2", "toTurnCount": 1}));
    let asked = ops(&h).len();
    let later = rpc.call("orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 1, "toTurnCount": 1}));
    let short = rpc.call("orchestration.getFullThreadDiff", json!({"threadId": "t1", "toTurnCount": 0}));
    let blind = rpc.call("orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 0, "toTurnCount": 1, "ignoreWhitespace": true}));
    let stranger = rpc.call("orchestration.getFullThreadDiff", json!({"threadId": "nope", "toTurnCount": 1}));
    let backwards = rpc.call("orchestration.getTurnDiff", json!({"threadId": "t1", "fromTurnCount": 2, "toTurnCount": 1}));
    // Then each diff is the agent API's text with the thread and the counts, taken in the thread's own directory
    assert_eq!(success(&turns), &json!({"threadId": "t1", "fromTurnCount": 0, "toTurnCount": 1, "diff": turn_diff["diff"]}));
    assert_eq!(success(&whole), &json!({"threadId": "t2", "fromTurnCount": 0, "toTurnCount": 1, "diff": thread_diff["diff"]}));
    assert_eq!(ops(&h)[..asked].iter().filter(|o| matches!(o, Op::TurnDiff { .. } | Op::ThreadDiff { .. })).cloned().collect::<Vec<_>>(), [Op::TurnDiff { root: "/workspace/demo".into(), base: "HEAD".into() }, Op::ThreadDiff { root: "/base/wt".into() }]);
    // And a range that does not start at the thread's start or end at its last finished turn, a request to ignore whitespace and an unknown
    // thread are the RPC's declared errors saying so, and none asked the agent API; a range that runs backwards is a defect
    for refused in [&later, &short] {
        let message = failure(refused)["message"].as_str().unwrap();
        assert!(message.contains("no revision is recorded"), "{message}");
    }
    assert!(failure(&blind)["message"].as_str().unwrap().contains("whitespace"));
    assert!(failure(&stranger)["message"].as_str().unwrap().contains("not found"));
    assert_eq!(failure(&later)["_tag"], "OrchestrationGetTurnDiffError");
    assert_eq!(failure(&stranger)["_tag"], "OrchestrationGetFullThreadDiffError");
    assert_eq!(backwards["cause"][0]["_tag"], "Die");
    assert_eq!(ops(&h).len(), asked);
}

#[test]
fn a_worktree_made_for_a_branch_is_a_new_branch_from_its_base_or_the_existing_one_checked_out_and_is_named_for_the_project() {
    // Given an agent API that adds worktrees as recorded
    let (h, mut rpc) = started("/base/wt");
    let made = recorded("worktrees", "worktree.create", "adds a worktree");
    reply(&h, "worktree.create", Ok(made.clone()));
    // When the phone asks for a worktree on a new branch from main, on an existing branch, and one the agent API refuses
    let fresh = rpc.call("vcs.createWorktree", json!({"cwd": "/workspace/demo", "refName": "main", "newRefName": "feature/login page", "path": null}));
    let existing = rpc.call("vcs.createWorktree", json!({"cwd": "/workspace/demo", "refName": "release", "path": null}));
    reply(&h, "worktree.create", Err(AgentError::Failed(error_of("worktrees", "worktree.create"))));
    let refused = rpc.call("vcs.createWorktree", json!({"cwd": "/workspace/demo", "refName": "--orphan", "path": null}));
    // Then the phone is told the path and the branch the agent API made, which is the new branch when one was asked for
    assert_eq!(success(&fresh)["worktree"], json!({"path": made["path"], "refName": made["branch"]}));
    assert_eq!(success(&existing)["worktree"]["path"], made["path"]);
    // And the agent API was asked for a new branch from the base, or for the branch alone, in a plain directory name made for the project
    let creates: Vec<Op> = ops(&h).into_iter().filter(|o| matches!(o, Op::WorktreeCreate { .. })).collect();
    let (Op::WorktreeCreate { root, worktree, branch, base }, Op::WorktreeCreate { branch: second, base: none, worktree: other, .. }) = (&creates[0], &creates[1]) else { panic!("{creates:?}") };
    assert_eq!((root.as_str(), branch.as_str(), base.as_deref()), ("/workspace/demo", "feature/login page", Some("main")));
    assert_eq!((second.as_str(), none.as_deref()), ("release", None));
    assert!(worktree.starts_with("feature-login-page-") && worktree.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)), "{worktree}");
    assert!(other.starts_with("release-") && other != worktree);
    // And the refusal is the git error the RPC declares, carrying the agent API's message
    let error = failure(&refused);
    assert_eq!((error["_tag"].as_str(), error["detail"].as_str().map(str::to_string)), (Some("GitCommandError"), Some(error_of("worktrees", "worktree.create"))));
}

fn setup_events(events: &[exec_git::SetupEvent]) -> Vec<SetupEvent> {
    events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let mut data = serde_json::to_value(e).unwrap();
            let kind = data.as_object_mut().unwrap().remove("kind").unwrap().as_str().unwrap().to_string();
            SetupEvent { seq: i as u64 + 1, kind, data }
        })
        .collect()
}

#[test]
fn a_worktrees_setup_is_followed_as_snapshots_that_end_done_and_a_thread_with_no_setup_gets_a_null() {
    // Given a thread in a worktree whose setup has started its first command, and a thread on the root
    let (h, mut rpc) = started("/base/wt");
    let started_events = [exec_git::SetupEvent::Started { steps: 2 }, exec_git::SetupEvent::StepStarted { step: 0, command: "pnpm i".into() }, exec_git::SetupEvent::StepOutput { step: 0, stream: "stdout".into(), data: "added 3\n".into() }];
    h.agent.setups.lock().unwrap().insert("/base/wt".into(), SetupBatch { ran: true, events: setup_events(&started_events) });
    // When the phone subscribes for the worktree thread and for the root thread
    let id = rpc.send("subscribeWorktreeSetup", json!({"threadId": "t2"}));
    let first = rpc.chunk(&id);
    let none_id = rpc.send("subscribeWorktreeSetup", json!({"threadId": "t1"}));
    let none = rpc.chunk(&none_id);
    // Then the first snapshot is running at the first command with its output, on the thread's branch, in the worktree
    let snapshot = &first[0];
    assert_eq!((snapshot["phase"].as_str(), snapshot["threadId"].as_str(), snapshot["branch"].as_str(), snapshot["worktreePath"].as_str()), (Some("running"), Some("t2"), Some("feature/setup"), Some("/base/wt")));
    assert_eq!((snapshot["stages"][1]["id"].as_str(), snapshot["stages"][1]["detail"].as_str(), snapshot["stages"][1]["tail"].clone()), (Some("setup-script"), Some("pnpm i"), json!(["added 3"])));
    // And the thread with no worktree is told null, and nothing follows it
    assert_eq!(none, [Value::Null]);
    // And when the setup goes on to finish, the next snapshot says done with every command counted
    let all = [started_events.to_vec(), vec![exec_git::SetupEvent::StepFinished { step: 0, exit_code: 0 }, exec_git::SetupEvent::StepStarted { step: 1, command: "pnpm build".into() }, exec_git::SetupEvent::StepFinished { step: 1, exit_code: 0 }, exec_git::SetupEvent::Finished { ok: true }]].concat();
    h.agent.setups.lock().unwrap().insert("/base/wt".into(), SetupBatch { ran: true, events: setup_events(&all) });
    let done = rpc.try_recv(Duration::from_secs(5)).expect("a snapshot once the setup finishes");
    assert_eq!(done["_tag"], "Chunk");
    let last = done["values"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["phase"].as_str(), last["stages"][1]["status"].as_str(), last["stages"][1]["percent"].clone(), last["endedAt"].is_string(), last["sequence"].clone()), (Some("done"), Some("done"), json!(100), true, json!(all.len())));
}

#[test]
fn a_worktree_where_no_setup_ran_is_a_null_snapshot_and_cancelling_stops_the_setup_of_the_threads_worktree_only() {
    // Given a thread in a worktree where no setup ran, and an agent API that cancels as recorded
    let (h, mut rpc) = started("/base/wt");
    let cancelled = recorded("worktrees", "worktree.setup.cancel", "stops a setup");
    reply(&h, "worktree.setup.cancel", Ok(json!({"workspace": cancelled["workspace"], "cancelled": true})));
    // When the phone follows the setup, cancels it, cancels for a thread outside any worktree and for one that does not exist
    let id = rpc.send("subscribeWorktreeSetup", json!({"threadId": "t2"}));
    let frames = rpc.chunk(&id);
    let stopped = rpc.call("worktreeSetup.cancel", json!({"threadId": "t2"}));
    let asked = ops(&h).len();
    let outside = rpc.call("worktreeSetup.cancel", json!({"threadId": "t1"}));
    // Then the follow is a single null, the cancel is the agent API's answer for the worktree, and a thread outside one cancels nothing
    assert_eq!(frames, [Value::Null]);
    assert_eq!(success(&stopped), &json!({"cancelled": true}));
    assert_eq!(ops(&h).last().unwrap(), &Op::WorktreeSetupCancel { root: "/base/wt".into() });
    assert_eq!(success(&outside), &json!({"cancelled": false}));
    assert_eq!(ops(&h).len(), asked);
}
