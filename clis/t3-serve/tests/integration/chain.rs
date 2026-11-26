//! The phone's wire through the chain, one behaviour per test. Each test names the coverage rows its calls exercise (`rows`, checked against
//! the methods list) and keeps the trace of what each call did; a row is served only by a call that was answered and went where its row says.
use crate::rig::*;
use crate::support::mimic_t3;
use crate::support::*;
use exec_testkit::script::{Script, Turn};
use exec_testkit::tap::Seen;
use std::time::{Duration, Instant};
use t3_contracts::history::Cursor;

pub(crate) fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

/// A turn of Claude Code that says one thing, writes `path` through the executor's MCP tool, and says another.
pub(crate) fn writes(path: &str, content: &str) -> Turn {
    Turn::replaying(&[said("writing the note")]).then_calling("k1", "write_file", json!({"path": path, "content": content, "create_dirs": true})).then_line(&said("done"))
}

/// A turn that starts, says one thing and then stays until it is killed.
pub(crate) fn stays() -> Turn {
    Turn::replaying(&[said("working")]).hanging()
}

pub(crate) fn hit<'a>(hits: &'a Value, key: &str, name: &str) -> Option<&'a Value> {
    hits.as_array().and_then(|all| all.iter().find(|h| h[key] == name))
}

/// What the last call of the trace sent to the agent API, in order, and the tools the executor ran.
pub(crate) fn last_window(case: &exec_testkit::Case) -> (Vec<(String, bool)>, Value) {
    let call = case.calls().last().expect("a call was made");
    let agent = call["agent"].as_array().unwrap().iter().map(|h| (h["method"].as_str().unwrap().to_string(), h["ok"] == true)).collect();
    (agent, call["executor"].clone())
}

#[test]
fn a_message_runs_a_turn_whose_file_is_written_by_rca_exec_and_the_phone_sees_the_run_on_its_streams() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a CLI whose turn writes notes/a.txt through its MCP tool, and a paired phone with its streams open on the thread
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("notes/a.txt", "hello from the model")]).printing_version("0.0.0-flow"));
        let mut phone = Phone::connect(case, &chain);
        assert_eq!(success(&phone.call(case, &chain, &["rpc:server.probe"], "server.probe", json!({}))), &json!({}));
        let (_, config) = phone.stream(case, &chain, &["rpc:subscribeServerConfig"], "subscribeServerConfig", json!({}));
        let expected_instances = json!({
            "opencode": {"driver": "opencode", "enabled": true},
            "claudeAgent": {"driver": "claudeAgent", "enabled": true},
            "codex": {"driver": "codex", "enabled": true},
        });
        assert_eq!(config[0]["config"]["settings"]["providerInstances"], expected_instances);
        let config = phone.call(case, &chain, &["rpc:server.getConfig"], "server.getConfig", json!({}));
        assert_eq!(success(&config)["settings"]["providerInstances"], expected_instances);
        phone.stream(case, &chain, &["rpc:subscribeServerLifecycle"], "subscribeServerLifecycle", json!({}));
        phone.seed(case, &chain);
        let (_, shell) = phone.stream(case, &chain, &["rpc:orchestration.subscribeShell"], "orchestration.subscribeShell", json!({}));
        assert_eq!((shell[0]["snapshot"]["projects"][0]["id"].as_str(), shell[0]["snapshot"]["threads"][0]["id"].as_str()), (Some("p1"), Some("t1")));
        let before_watch = chain.tap.mark();
        let (thread_stream, _) = phone.stream(case, &chain, &["rpc:orchestration.subscribeThread"], "orchestration.subscribeThread", json!({"threadId": "t1"}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !chain.tap.since(before_watch).iter().any(|hit| matches!(hit, Seen::Agent { method, ok: true, .. } if method == "thread.subscribe")) {
            assert!(Instant::now() < deadline, "thread event watcher did not subscribe: {}", chain.evidence());
            std::thread::sleep(Duration::from_millis(5));
        }
        // When the phone sends a message and the run it starts ends
        let projection = phone.message(case, &chain, "t1", "c-m1", "make a note");
        // Then the file is in the repository, written by the CLI running at the workspace host, and not in the directory the service runs from
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/a.txt")).unwrap(), "hello from the model");
        assert_eq!(chain.repo.git(&["status", "--porcelain"]), "?? notes/");
        assert!(!chain.service.home().join("notes").exists());
        // And the message's window shows where it went: the agent API calls in order, each answered, and the command the executor ran for the CLI
        let (agent, executor) = last_window(case);
        assert_eq!(agent, [("thread.create".to_string(), true), ("turn.start".to_string(), true), ("thread.subscribe".to_string(), true)]);
        assert_eq!(hit(&executor, "tool", "session_send"), Some(&json!({"tool": "session_send", "ok": true})), "the CLI did not run through rca-exec: {executor}");
        // And the CLI was started once, at the workspace host's root, and the thread holds both sides of the turn with a completed run
        assert_eq!(chain.cli.invocations("claude").len(), 1);
        let texts: Vec<(&str, &str)> = projection["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap(), m["text"].as_str().unwrap())).collect();
        assert_eq!(texts.first(), Some(&("user", "make a note")));
        assert!(texts.last().is_some_and(|(role, text)| *role == "assistant" && text.contains("done")), "{texts:?}");
        assert!(projection["turnItems"].as_array().unwrap().iter().any(|i| i["name"] == "write_file" && i["status"] == "completed"), "{projection}");
        // And the thread stream the phone had open carried the run from its creation to its end, and the projection read on its own says the same
        let events: Vec<String> = phone.drain_watch().iter().filter(|f| f["requestId"] == thread_stream && f["_tag"] == "Chunk").flat_map(|f| f["values"].as_array().unwrap().clone()).filter(|v| v["kind"] == "event").map(|v| v["event"]["type"].as_str().unwrap().to_string()).collect();
        assert!(events.contains(&"run.created".to_string()) && events.last().is_some_and(|e| e == "run.updated"), "{events:?}");
        // When the phone closes its sockets and opens the thread on a new connection, both messages are in the snapshot.
        drop(phone);
        let mut reopened = Phone::connect(case, &chain);
        let (_, frames) = reopened.stream(case, &chain, &["rpc:orchestration.subscribeThread"], "orchestration.subscribeThread", json!({"threadId": "t1"}));
        let snapshot = &frames[0]["projection"];
        assert_eq!(snapshot["messages"], projection["messages"]);
        assert_eq!(snapshot["runs"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["runs"][0]["status"], "completed");
        assert_eq!(chain.direct().events("t1", "turn.started").len(), 1);
    });
}

#[test]
fn what_a_turn_wrote_is_read_back_as_transcript_history_and_files_through_the_executor() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose turn wrote notes/a.txt
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("notes/a.txt", "hello from the model")]).printing_version("0.0.0-flow"));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        phone.message(case, &chain, "t1", "c-m1", "make a note");
        // When the phone reads the thread's transcript and a page of its history
        let transcript = phone.get(case, &chain, &["http:orchestration.threadTranscript"], "/api/orchestration/threads/t1/transcript");
        let items = transcript.body["items"].as_array().unwrap();
        let newest = items.last().expect("the turn left items");
        let cursor = Cursor { snapshot_sequence: 0, source_thread_id: "t1".into(), source_item_id: newest["sourceItemId"].as_str().unwrap().into(), position: newest["position"].as_u64().unwrap(), inclusive: true };
        let page = phone.get(case, &chain, &["http:orchestration.threadHistoryPage"], &format!("/api/orchestration/threads/t1/history?cursor={}", cursor.encode()));
        // Then the transcript holds the user's message and the tool row, and the page walks the same rows older-first
        let kinds: Vec<&str> = items.iter().map(|r| r["item"]["type"].as_str().unwrap()).collect();
        assert!(kinds.contains(&"user_message") && kinds.contains(&"command_execution"), "{kinds:?}");
        assert_eq!(page.body["items"].as_array().unwrap().iter().map(|r| r["item"]["id"].clone()).collect::<Vec<_>>(), items.iter().map(|r| r["item"]["id"].clone()).collect::<Vec<_>>());
        // When the phone lists the notes directory and reads the file the model wrote
        let cwd = chain.repo.path().canonicalize().unwrap();
        let listed = phone.call(case, &chain, &["rpc:projects.listEntries"], "projects.listEntries", json!({"cwd": cwd, "directoryPath": "notes"}));
        let (listing_agent, listing_executor) = last_window(case);
        let read = phone.call(case, &chain, &["rpc:projects.readFile"], "projects.readFile", json!({"cwd": cwd, "relativePath": "notes/a.txt"}));
        let (read_agent, read_executor) = last_window(case);
        // Then the entries and the contents are the repository's, and each call went agent API, then executor
        assert_eq!(success(&listed)["entries"][0]["path"], "notes/a.txt");
        assert_eq!(success(&read)["contents"], "hello from the model");
        assert_eq!((listing_agent, read_agent), (vec![("worktree.list".to_string(), true), ("file.list".to_string(), true)], vec![("worktree.list".to_string(), true), ("file.readChunk".to_string(), true)]));
        assert!(hit(&listing_executor, "tool", "list_dir").is_some_and(|h| h["ok"] == true) && hit(&read_executor, "tool", "read_chunk").is_some_and(|h| h["ok"] == true), "{listing_executor} {read_executor}");
    });
}

#[test]
fn given_two_projects_when_the_phone_searches_files_and_skills_then_only_the_selected_projects_matches_are_returned() {
    exec_testkit::case!(SUITE, |case| {
        // Given two projects below one executor, each with its own README and Codex skill
        let chain = Chain::start();
        for project in ["one", "two"] {
            chain.repo.write(&format!("{project}/README-{project}.md"), project);
            chain.repo.write(&format!("{project}/.agents/skills/{project}/SKILL.md"), &format!("---\nname: {project}\ndescription: Project {project}\n---\n"));
        }
        let mut phone = Phone::connect(case, &chain);
        let cwd = chain.repo.path().join("two").canonicalize().unwrap();
        // When the phone searches and refreshes the catalog for the second project
        let files = phone.call(case, &chain, &["rpc:projects.searchEntries"], "projects.searchEntries", json!({"cwd": cwd, "query": "README", "limit": 10, "kind": "file"}));
        let catalog = phone.call(case, &chain, &["rpc:server.refreshProviders"], "server.refreshProviders", json!({"instanceId": "codex", "cwd": cwd, "workspaceSnapshotsOnDemand": true}));
        // Then paths are relative to the selected project and its skill came from the same directory
        assert_eq!(success(&files)["entries"], json!([{"path": "README-two.md", "kind": "file"}]));
        let provider = hit(&success(&catalog)["providers"], "instanceId", "codex").unwrap();
        let snapshot = &provider["workspaceSnapshots"][0];
        assert_eq!(snapshot["cwd"], json!(cwd));
        assert!(hit(&snapshot["skills"], "name", "two").is_some(), "{snapshot}");
        assert!(hit(&snapshot["skills"], "name", "one").is_none(), "{snapshot}");
    });
}

#[test]
fn a_launch_creates_the_thread_runs_its_first_message_through_rca_and_answers_with_the_projection() {
    exec_testkit::case!(SUITE, |case| {
        // Given a project on the repository and a CLI whose turn writes launched.txt
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![writes("launched.txt", "from a launch")]).printing_version("0.0.0-flow"));
        let mut phone = Phone::connect(case, &chain);
        let project = json!({"type": "project.create", "commandId": "c-p1", "projectId": "p1", "title": "Chain", "workspaceRoot": chain.repo.path().display().to_string()});
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.create"], "projects.mutate", project));
        // When the phone launches a new task with a first message
        let launch = json!({
            "commandId": "c-new", "creationSource": "mobile", "projectId": "p1", "threadId": "t-new", "title": "Fix the build", "generateTitle": true,
            "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}, "runtimeMode": "full-access", "interactionMode": "default",
            "workspaceStrategy": {"type": "root"}, "initialMessage": {"messageId": "m-new", "text": "make the file", "attachments": []},
        });
        let answer = case.call(&chain.tap, &rows(&["rpc:orchestration.launchThread"]), || {
            let exit = phone.calls.call("orchestration.launchThread", launch);
            let projection = phone.settled("t-new");
            let completed = projection["runs"].as_array().and_then(|r| r.last()).is_some_and(|r| r["status"] == "completed");
            (exit["_tag"] == "Success" && completed, (exit, projection))
        });
        // Then the answer names the new thread and it is not a resumption, and the executor wrote the first message's file in the repository
        assert_eq!((success(&answer.0)["threadId"].as_str(), success(&answer.0)["resumed"].as_bool()), (Some("t-new"), Some(false)));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("launched.txt")).unwrap(), "from a launch");
        // And the launch's window holds the agent API calls of the turn and the executor's run of the CLI
        let (agent, executor) = last_window(case);
        assert_eq!(agent.iter().map(|(m, ok)| (m.as_str(), *ok)).collect::<Vec<_>>(), [("thread.create", true), ("turn.start", true), ("thread.subscribe", true)]);
        assert!(hit(&executor, "tool", "session_send").is_some_and(|h| h["ok"] == true), "{executor}");
        // And the thread holds the message and a completed run
        assert_eq!((answer.1["thread"]["id"].as_str(), answer.1["runs"][0]["status"].as_str()), (Some("t-new"), Some("completed")));
    });
}

#[test]
fn an_interrupt_and_a_stop_each_end_the_turn_the_cli_is_running_by_telling_rca_to_interrupt_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given a thread whose CLI starts a turn and stays
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![stays()]).printing_version("0.0.0-flow"));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        for (n, (command, row, method)) in [("run.interrupt", "command:run.interrupt", "turn.interrupt"), ("thread.stop", "command:thread.stop", "thread.forceStop")].into_iter().enumerate() {
            // When a message starts a turn, the CLI is running, and the phone sends the command
            let message = json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "web", "commandId": format!("c-m{n}"), "threadId": "t1", "messageId": format!("m{n}"), "text": "work"});
            success(&phone.call(case, &chain, &["rpc:orchestration.dispatchCommand", "command:message.dispatch"], "orchestration.dispatchCommand", message));
            let started = std::time::Instant::now();
            while chain.cli.claude_prompts("claude").len() < n + 1 {
                assert!(started.elapsed() < std::time::Duration::from_secs(30), "the CLI never started: {}", chain.evidence());
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let stop = json!({"type": command, "commandId": format!("c-s{n}"), "threadId": "t1", "runId": "r1"});
            let projection = case.call(&chain.tap, &rows(&["rpc:orchestration.dispatchCommand", row]), || {
                let exit = phone.calls.call("orchestration.dispatchCommand", stop);
                let projection = phone.settled("t1");
                let interrupted = projection["runs"].as_array().and_then(|r| r.last()).is_some_and(|r| r["status"] == "interrupted");
                (exit["_tag"] == "Success" && interrupted, projection)
            });
            // Then rca was told to interrupt the turn (a stop forces it, which holds the queue) and answered it, and the run ended interrupted
            let (agent, _) = last_window(case);
            assert!(agent.contains(&(method.to_string(), true)), "{command}: {agent:?}");
            assert_eq!(projection["runs"].as_array().unwrap().last().unwrap()["status"], "interrupted", "{command}");
        }
    });
}

#[test]
fn a_project_is_renamed_and_deleted_and_the_shell_the_phone_reads_afterwards_no_longer_lists_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given two projects, one without a thread
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        let empty = json!({"type": "project.create", "commandId": "c-p2", "projectId": "p2", "title": "Spare", "workspaceRoot": chain.repo.path().display().to_string()});
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.create"], "projects.mutate", empty));
        // When the phone renames the spare project and deletes it
        let rename = json!({"type": "project.update", "commandId": "c-u2", "projectId": "p2", "title": "Renamed"});
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.update"], "projects.mutate", rename));
        let delete = json!({"type": "project.delete", "commandId": "c-d2", "projectId": "p2"});
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.delete"], "projects.mutate", delete));
        // Then a shell snapshot lists the project that still has its thread and not the deleted one
        let (_, shell) = phone.stream(case, &chain, &["rpc:orchestration.subscribeShell"], "orchestration.subscribeShell", json!({}));
        let projects: Vec<&str> = shell[0]["snapshot"]["projects"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(projects, ["p1"]);
        // And the rename was kept in rca's state, and the delete asked rca for that and no workspace (the spare project was the repository itself, a workspace already)
        let window = |row: &str| {
            let calls = case.calls();
            let call = calls.iter().find(|c| c["rows"].as_array().unwrap().iter().any(|r| r == row)).unwrap();
            call["agent"].as_array().unwrap().iter().map(|h| (h["method"].as_str().unwrap().to_string(), h["ok"] == true)).collect::<Vec<_>>()
        };
        assert_eq!(window("mutation:project.update"), [("state.set".to_string(), true)]);
        assert!(window("mutation:project.delete").contains(&("state.set".to_string(), true)) && !window("mutation:project.delete").iter().any(|(m, _)| m == "workspace.remove"));
    });
}

#[test]
fn the_imported_session_of_the_real_server_replays_through_mimic_against_t3_serve_over_the_whole_chain_with_no_difference_the_imported_allowlist_does_not_name() {
    exec_testkit::case!(SUITE, |_case| {
        // Given the chain, and the set imported from the goldens of t3@0.1.6 with its imported allowlist
        let chain = std::rc::Rc::new(Chain::start());
        let (set, allow) = (mimic_t3::load_set(mimic_t3::SET), mimic_t3::load_allowlist(mimic_t3::SET));
        // When the client side of every interaction is replayed against `t3-serve serve`, wired to the real rca-service and rca-exec
        let mut candidate = chain_driver(&chain, mimic_t3::granted_scopes(&set), Vec::new(), false);
        let verdict = mimic_core::usecase::replay(&set, &mut candidate, &allow, &mimic_core::presets::t3_diff_options(), None).expect("the replay runs");
        // Then every masked answer equals the recorded one, apart from the divergences the allowlist names
        let found = mimic_t3::differences(&verdict);
        assert!(found.is_empty(), "{} differences:\n{}\n{}", found.len(), found.join("\n"), chain.evidence());
    });
}

#[test]
#[ignore = "requires RCA_NATIVE_FIXTURE_DIR and a connected native client"]
fn native_startup_queue_fixture() {
    exec_testkit::case!(SUITE, |case| {
        let output = std::env::var("RCA_NATIVE_FIXTURE_DIR").unwrap();
        let output = std::path::Path::new(&output);
        let chain = Chain::start();
        let first = Turn::replaying(&[]).then_sleeping(30_000).then_line(&said("RCAFIXTURE_FIRST_REPLY"));
        let second = Turn::replaying(&[said("RCAFIXTURE_SECOND_REPLY")]);
        chain.cli.script("claude", &Script::of(vec![first, second]).printing_version("2.2.0"));
        let mut probe = Phone::connect(case, &chain);
        probe.seed(case, &chain);
        let config = json!({"address": format!("http://{}", chain.addr), "pairingCode": chain.pair_with(&[]), "thread": "t1"});
        std::fs::write(output.join("session.local"), config.to_string()).unwrap();
        let mut direct = chain.direct();
        let until = Instant::now() + Duration::from_secs(900);
        let mut queued_before_first_finished = false;
        while Instant::now() < until {
            let projection = success(&probe.calls.call("orchestration.getThreadProjection", json!({"threadId": "t1"}))).clone();
            let runs = projection["runs"].as_array().unwrap();
            if runs.len() == 2 && runs[0]["status"] == "running" && runs[1]["status"] == "queued" {
                queued_before_first_finished = true;
            }
            if runs.len() > 2 {
                panic!("duplicate runs: {runs:?}");
            }
            if runs.len() == 2 && runs.iter().all(|run| run["status"] == "completed") {
                assert!(queued_before_first_finished, "did not observe a queued follow-up during the first run");
                let listed = direct.call("agent.threads", "thread.list", json!({})).expect("RCA lists the fixture thread");
                let threads = listed["threads"].as_array().expect("thread array");
                assert_eq!(threads.len(), 1);
                let thread = threads[0]["id"].as_str().expect("RCA thread ID");
                let starts = direct.events(thread, "turn.started");
                let finishes = direct.events(thread, "turn.finished");
                assert_eq!((starts.len(), finishes.len()), (2, 2));
                let turns = starts.iter().map(|event| event["turn"].clone()).collect::<Vec<_>>();
                let finished_turns = finishes.iter().map(|event| event["turn"].clone()).collect::<Vec<_>>();
                assert_eq!(turns, finished_turns);
                assert_ne!(turns[0], turns[1]);
                let receipt = json!({"queuedDuringFirstRun": queued_before_first_finished, "turns": turns, "completedTurns": finished_turns});
                std::fs::write(output.join("queue-receipt.json"), receipt.to_string()).unwrap();
                while Instant::now() < until && !output.join("close-fixture").exists() {
                    std::thread::sleep(Duration::from_millis(200));
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("native fixture timed out: {}", chain.evidence());
    });
}

#[test]
#[ignore = "requires RCA_NATIVE_FIXTURE_DIR and a connected native client"]
fn native_held_queue_reorder_fixture() {
    exec_testkit::case!(SUITE, |case| {
        let output = std::path::PathBuf::from(std::env::var("RCA_NATIVE_FIXTURE_DIR").unwrap());
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![
            stays(),
            Turn::replaying(&[]).then_sleeping(1_500).then_line(&said("RCAFIXTURE_QUEUE_FIRST_REPLY")),
            Turn::replaying(&[said("RCAFIXTURE_QUEUE_SECOND_REPLY")]),
        ]).printing_version("2.2.0"));
        let mut probe = Phone::connect(case, &chain);
        probe.seed(case, &chain);
        let dispatch = |id: &str, text: &str, mode: &str| {
            json!({"type": "message.dispatch", "createdBy": "user", "creationSource": "mobile", "commandId": id, "messageId": id, "threadId": "t1", "text": text, "dispatchMode": {"type": mode}})
        };
        success(&probe.calls.call("orchestration.dispatchCommand", dispatch("start", "hold the fixture", "start_immediately")));
        let run = probe.run_of("t1");
        let mut direct = chain.direct();
        direct.wait_event("t1", "turn.started");
        let deadline = Instant::now() + Duration::from_secs(15);
        for (index, (id, prompt)) in [("a", "RCAFIXTURE_MOVE_ALPHA"), ("b", "RCAFIXTURE_MOVE_BRAVO")].into_iter().enumerate() {
            success(&probe.calls.call("orchestration.dispatchCommand", dispatch(id, prompt, "queue_after_active")));
            loop {
                let queue = direct.call("agent.queue", "queue.list", json!({"thread": "t1"})).unwrap();
                if queue["items"].as_array().unwrap().len() > index {
                    break;
                }
                assert!(Instant::now() < deadline, "fixture queue did not load item {id}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let queue = direct.call("agent.queue", "queue.list", json!({"thread": "t1"})).unwrap();
        let initial = queue["items"].as_array().unwrap().iter().map(|item| item["item"].clone()).collect::<Vec<_>>();
        success(&probe.calls.call("orchestration.forceStopThread", json!({"threadId": "t1", "runId": run})));
        assert_eq!(direct.events("t1", "turn.started").len(), 1);
        let config = json!({"address": format!("http://{}", chain.addr), "pairingCode": chain.pair_with(&[]), "thread": "t1"});
        std::fs::write(output.join("session.local"), config.to_string()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(480);
        let mut held_order = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "native reorder fixture timed out");
            if held_order.is_empty() {
                let rows = direct.call("agent.queue", "queue.list", json!({"thread": "t1"})).unwrap();
                let items = rows["items"].as_array().unwrap().iter().map(|item| item["item"].clone()).collect::<Vec<_>>();
                if items == [initial[1].clone(), initial[0].clone()] {
                    assert_eq!(direct.events("t1", "turn.started").len(), 1, "reordering must keep the queue held");
                    held_order = items;
                    std::fs::write(output.join("held-reordered.json"), json!({"original": initial, "reorderedWhileHeld": held_order}).to_string()).unwrap();
                }
            }
            let finished = direct.events("t1", "turn.finished");
            if finished.len() >= 3 {
                assert_eq!(held_order.len(), 2);
                let starts = direct.events("t1", "turn.started");
                let turn_ids = |events: Vec<Value>| events.into_iter().map(|event| event["turn"].clone()).collect::<Vec<_>>();
                let started = turn_ids(starts);
                let finished = turn_ids(finished);
                assert_eq!(started.len(), 3);
                assert_eq!(started, finished);
                assert_eq!(started[1..], held_order);
                let receipt = json!({"originalOrder": initial, "orderWhileHeld": held_order, "started": started, "finished": finished});
                std::fs::write(output.join("reorder-resume-receipt.json"), receipt.to_string()).unwrap();
                while Instant::now() < deadline && !output.join("close-fixture").exists() {
                    std::thread::sleep(Duration::from_millis(100));
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}
