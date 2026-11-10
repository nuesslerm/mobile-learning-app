//! The project rows of M12 through the real router over the agent double: what a project the phone makes asks of the agent API (the folder
//! checked, made a workspace, kept in `state`, forgotten), what `projects.ensureScratch`, `projects.createNew` and `projects.createFromThread`
//! ask the executor to make, and what the phone is told. The chain (`tests/integration/projects_m12.rs`) runs the same calls through the real
//! service and executor and looks at the folders themselves.
use super::support::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use t3_core::ports::{agent_codes, AgentError, AgentEventKind, Op};

/// The executor's root, which the double answers a browse for.
const ROOT: &str = "/workspace";

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let rpc = connect_with(&h, all);
    (h, rpc)
}

fn ops(h: &Harness) -> Vec<Op> {
    h.agent.ops.lock().unwrap().clone()
}

fn methods(h: &Harness) -> Vec<&'static str> {
    ops(h).iter().map(Op::method).collect()
}

fn refused(code: &str, message: &str) -> Result<Value, AgentError> {
    Err(AgentError::Refused { code: code.into(), message: message.into() })
}

fn project(id: &str, root: &str) -> Value {
    json!({"type": "project.create", "commandId": format!("c-{id}"), "projectId": id, "title": format!("Project {id}"), "workspaceRoot": root})
}

fn projects_of(rpc: &mut Rpc) -> Vec<Value> {
    let shell = rpc.send("orchestration.subscribeShell", json!({}));
    rpc.chunk(&shell)[0]["snapshot"]["projects"].as_array().unwrap().clone()
}

/// The executor's claim of a name: the first mkdir of a path (no parents) makes it, a second is `ALREADY_EXISTS`.
fn claims(h: &Harness, taken: Arc<Mutex<HashSet<String>>>) {
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::FileMkdir { path, parents: false } => Some(if taken.lock().unwrap().insert(path.clone()) { Ok(json!({"path": path, "created": true})) } else { refused(agent_codes::ALREADY_EXISTS, &format!("{path} already exists")) }),
        _ => None,
    }));
}

#[test]
fn a_project_the_phone_makes_is_a_folder_below_the_executors_root_made_a_workspace_of_its_own_and_kept_in_the_agent_apis_state() {
    // Given a server over an agent API whose executor is rooted at /workspace
    let (h, mut rpc) = started();
    // When the phone makes a project on a folder below the root
    let made = rpc.call("projects.mutate", project("p1", "/workspace/demo"));
    // Then the project is the row the phone gets back, and the agent API was asked to show the root and the folder, then to make the folder a workspace named for the project
    assert_eq!(success(&made)["id"], "p1");
    assert_eq!(methods(&h), ["file.browse", "file.browse", "workspace.create", "state.set"]);
    let all = ops(&h);
    assert_eq!(all[2], Op::WorkspaceCreate { workspace: "project-p1".into(), path: "demo".into() });
    // And what the project says is kept in the agent API's state for the other devices
    let Op::StateSet { scope, key, value } = &all[3] else { panic!("state.set expected, got {:?}", all[3]) };
    assert_eq!((scope.as_str(), key.as_str(), value["title"].clone(), value["workspaceRoot"].clone()), ("t3", "project.settings:p1", json!("Project p1"), json!("/workspace/demo")));
    // When a project is made on the root itself
    h.agent.ops.lock().unwrap().clear();
    success(&rpc.call("projects.mutate", project("p2", ROOT)));
    // Then the root is a workspace already, and the agent API is not asked to make one
    assert!(!methods(&h).contains(&"workspace.create"), "{:?}", methods(&h));
    assert_eq!(projects_of(&mut rpc).len(), 2);
    // And deleting it does not ask the agent API to forget a workspace it never had, but empties the project's state
    h.agent.ops.lock().unwrap().clear();
    success(&rpc.call("projects.mutate", json!({"type": "project.delete", "commandId": "c-d2", "projectId": "p2"})));
    assert!(!methods(&h).contains(&"workspace.remove") && methods(&h).contains(&"state.set"), "{:?}", methods(&h));
}

#[test]
fn a_project_folder_that_is_outside_the_root_missing_or_no_directory_is_refused_with_nothing_made_or_stored() {
    // Given an executor that refuses what is outside its root, does not have /workspace/missing and knows /workspace/file.txt is no directory
    let (h, mut rpc) = started();
    *h.agent.responder.lock().unwrap() = Some(Box::new(|op| match op {
        Op::FileBrowse { path, .. } if path == "/workspace/missing" => Some(refused(agent_codes::NOT_FOUND, "/workspace/missing does not exist")),
        Op::FileBrowse { path, .. } if path == "/workspace/file.txt" => Some(refused("INVALID_ARGS", "/workspace/file.txt is not a directory")),
        _ => None,
    }));
    for (folder, says) in [("/etc", "outside the folder this server serves"), ("/workspace/missing", "does not exist"), ("/workspace/file.txt", "is not a directory")] {
        // When a project is made on it
        let exit = rpc.call("projects.mutate", project("px", folder));
        // Then the phone is told the mutation failed, and the log holds the reason
        assert_eq!(failure(&exit)["_tag"], "ProjectMutationError", "{folder}");
        assert!(h.log.lines().iter().any(|l| l.starts_with("rpc.command.refused") && l.contains(says)), "{folder}: {:?}", h.log.lines());
    }
    // And no workspace was made and no project exists
    assert!(!methods(&h).contains(&"workspace.create") && !methods(&h).contains(&"state.set"));
    assert!(projects_of(&mut rpc).is_empty());
}

#[test]
fn a_project_that_is_updated_keeps_its_settings_in_state_moves_its_workspace_with_its_folder_and_one_that_is_deleted_forgets_it() {
    // Given a project, and an agent API that already has the workspace the project is named for the first time it is asked to make it
    let (h, mut rpc) = started();
    let exists = Arc::new(Mutex::new(true));
    let once = exists.clone();
    *h.agent.responder.lock().unwrap() = Some(Box::new(move |op| match op {
        Op::WorkspaceCreate { .. } if std::mem::take(&mut *once.lock().unwrap()) => Some(refused(agent_codes::WORKSPACE_EXISTS, "workspace project-p1 exists")),
        _ => None,
    }));
    success(&rpc.call("projects.mutate", project("p1", "/workspace/demo")));
    // Then the workspace that was there was forgotten and made again for the folder
    assert_eq!(methods(&h).iter().filter(|m| m.starts_with("workspace.")).copied().collect::<Vec<_>>(), ["workspace.create", "workspace.remove", "workspace.create"]);
    // When only the title changes
    h.agent.ops.lock().unwrap().clear();
    success(&rpc.call("projects.mutate", json!({"type": "project.update", "commandId": "c-u1", "projectId": "p1", "title": "Renamed"})));
    // Then no folder is asked about, and state holds the new title
    assert_eq!(methods(&h), ["state.set"]);
    assert!(matches!(&ops(&h)[0], Op::StateSet { key, value, .. } if key == "project.settings:p1" && value["title"] == "Renamed"));
    // When the project moves to another folder
    h.agent.ops.lock().unwrap().clear();
    success(&rpc.call("projects.mutate", json!({"type": "project.update", "commandId": "c-u2", "projectId": "p1", "workspaceRoot": "/workspace/other"})));
    // Then the new folder is checked and made the project's workspace, and state says where it is
    assert!(ops(&h).contains(&Op::WorkspaceCreate { workspace: "project-p1".into(), path: "other".into() }));
    assert!(ops(&h).iter().any(|op| matches!(op, Op::StateSet { value, .. } if value["workspaceRoot"] == "/workspace/other")));
    // When a thread is put in the project, a delete is refused and the agent API is not asked to forget anything
    success(&rpc.call("orchestration.dispatchCommand", thread_create("c-t1", "t1", "p1")));
    h.agent.ops.lock().unwrap().clear();
    let blocked = rpc.call("projects.mutate", json!({"type": "project.delete", "commandId": "c-d0", "projectId": "p1"}));
    assert_eq!(failure(&blocked)["_tag"], "ProjectMutationError");
    assert!(!methods(&h).contains(&"workspace.remove"));
    // And once the thread is gone the project is deleted: its workspace forgotten and its state emptied
    success(&rpc.call("orchestration.dispatchCommand", json!({"type": "thread.delete", "commandId": "c-t1-del", "threadId": "t1"})));
    h.agent.ops.lock().unwrap().clear();
    success(&rpc.call("projects.mutate", json!({"type": "project.delete", "commandId": "c-d1", "projectId": "p1"})));
    assert_eq!(ops(&h), [Op::FileBrowse { root: String::new(), path: ".".into(), show_hidden: true }, Op::WorkspaceRemove { workspace: "project-p1".into() }, Op::StateSet { scope: "t3".into(), key: "project.settings:p1".into(), value: Value::Null }]);
    assert!(projects_of(&mut rpc).is_empty());
}

#[test]
fn a_thread_the_phone_creates_exists_in_the_agent_api_at_once_with_its_instance_and_model_and_a_retry_asks_nothing() {
    // Given a project
    let (h, mut rpc) = started();
    success(&rpc.call("projects.mutate", project("p1", "/workspace/demo")));
    h.agent.ops.lock().unwrap().clear();
    // When the phone creates a thread that is to run Claude Code on a model
    let create = json!({"type": "thread.create", "commandId": "c-t1", "threadId": "t1", "projectId": "p1", "title": "Notes", "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}});
    success(&rpc.call("orchestration.dispatchCommand", create.clone()));
    // Then the agent API was asked for the thread, before any turn, with the instance and the model
    let made = h.agent.created.lock().unwrap().clone();
    assert_eq!(made.iter().map(|t| (t.thread_id.as_str(), t.instance_id.as_str(), t.model.as_deref())).collect::<Vec<_>>(), [("t1", "claudeAgent", Some("opus"))]);
    assert!(h.agent.turns.lock().unwrap().is_empty());
    // And the same command again is a receipt: nothing more is asked
    success(&rpc.call("orchestration.dispatchCommand", create));
    assert_eq!(h.agent.created.lock().unwrap().len(), 1);
    // And an agent API that cannot be reached leaves the thread made all the same, and says so in the log
    *h.agent.responder.lock().unwrap() = None;
    assert!(h.log.lines().iter().all(|l| !l.starts_with("thread.create.agent.failed")));
}

#[test]
fn a_new_project_is_a_folder_the_executor_makes_below_its_root_with_a_repository_and_a_first_commit_and_a_taken_name_gets_the_next_one() {
    // Given an executor that makes the first folder of a name and refuses it the second time
    let (h, mut rpc) = started();
    claims(&h, Arc::default());
    // When the phone makes a new project named for a pinball app, twice
    let first = rpc.call("projects.createNew", json!({"name": "Pinball Stats"}));
    let second = rpc.call("projects.createNew", json!({"name": "Pinball Stats"}));
    // Then the first is in a folder of the name's slug and the second in the next free one, each with its own project
    let (one, two) = (success(&first).clone(), success(&second).clone());
    assert_eq!((one["workspaceRoot"].clone(), two["workspaceRoot"].clone()), (json!("/workspace/.rca/projects/pinball-stats"), json!("/workspace/.rca/projects/pinball-stats-2")));
    assert_ne!(one["projectId"], two["projectId"]);
    assert!(one.get("commitError").is_none());
    let rows = projects_of(&mut rpc);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|p| p["title"] == "Pinball Stats"));
    // And the executor was asked, for the first, to make `.rca/projects`, keep the repository around it from seeing it, claim the folder, and make it a repository with a README and an icon
    let all = ops(&h);
    assert_eq!(all[0], Op::FileMkdir { path: ".rca/projects".into(), parents: true });
    assert_eq!(all[1], Op::FileWrite { root: String::new(), path: ".rca/.gitignore".into(), content: "*\n".into() });
    assert_eq!(all[2], Op::FileMkdir { path: ".rca/projects/pinball-stats".into(), parents: false });
    let Op::GitInit { path, initial_branch, files, message } = &all[3] else { panic!("git.init expected, got {:?}", all[3]) };
    assert_eq!((path.as_str(), initial_branch.clone(), message.as_deref()), (".rca/projects/pinball-stats", None, Some("Initial commit")));
    assert_eq!(files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), ["README.md", "assets/icon.svg"]);
    assert!(files[0].1.contains("# Pinball Stats") && files[1].1.contains(">PS</text>"));
    // And the folder was made a workspace of the agent API like any project's
    assert!(all.iter().any(|op| matches!(op, Op::WorkspaceCreate { path, .. } if path == ".rca/projects/pinball-stats")));
}

#[test]
fn a_new_project_whose_commit_could_not_be_made_says_why_and_a_name_that_is_empty_too_long_or_all_taken_is_refused() {
    // Given an executor whose repository could not be committed to
    let (h, mut rpc) = started();
    let taken: Arc<Mutex<HashSet<String>>> = Arc::default();
    claims(&h, taken.clone());
    h.agent.replies.lock().unwrap().insert("git.init", Ok(json!({"path": "p", "branch": "main", "commit_error": "Git has no name or email on this machine. Set user.name and user.email, then commit."})));
    // When a new project is made
    let made = rpc.call("projects.createNew", json!({"name": "Notes"}));
    // Then the project exists and the phone is told why there is no commit
    assert_eq!(success(&made)["commitError"], "Git has no name or email on this machine. Set user.name and user.email, then commit.");
    assert_eq!(projects_of(&mut rpc).len(), 1);
    // When the name is empty, or longer than 200 characters, Then the request is a decoder defect and the executor is asked nothing more
    let before = ops(&h).len();
    for name in ["   ".to_string(), "x".repeat(201)] {
        let exit = rpc.call("projects.createNew", json!({"name": name}));
        assert_eq!(exit["cause"][0]["_tag"], "Die", "{exit}");
    }
    assert_eq!(ops(&h).len(), before);
    // When every name the executor could give is taken, Then the phone is told the name is taken and no project is made
    for n in 1..=100 {
        taken.lock().unwrap().insert(if n == 1 { ".rca/projects/busy".to_string() } else { format!(".rca/projects/busy-{n}") });
    }
    let full = rpc.call("projects.createNew", json!({"name": "Busy"}));
    assert_eq!(failure(&full)["_tag"], "OrchestrationDispatchCommandError");
    assert!(failure(&full)["message"].as_str().unwrap().contains("taken"), "{full}");
    assert_eq!(projects_of(&mut rpc).len(), 1);
}

#[test]
fn the_scratch_project_is_made_once_on_a_folder_the_executor_makes_and_every_later_call_answers_the_same_project() {
    // Given a server
    let (h, mut rpc) = started();
    // When the phone asks for the scratch project twice
    let first = rpc.call("projects.ensureScratch", json!({}));
    let made = ops(&h).len();
    let second = rpc.call("projects.ensureScratch", json!({}));
    // Then both name one project, the executor was asked for the folder only the first time, and the project is on it with the name and the icon of a thread with no project
    let id = success(&first)["projectId"].clone();
    assert_eq!(success(&second)["projectId"], id);
    assert!(ops(&h).len() > made && ops(&h)[made..].iter().all(|op| matches!(op, Op::FileBrowse { .. })), "{:?}", &ops(&h)[made..]);
    assert!(ops(&h).contains(&Op::FileMkdir { path: ".rca/scratch".into(), parents: true }));
    let rows = projects_of(&mut rpc);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["id"].clone(), rows[0]["title"].clone(), rows[0]["workspaceRoot"].clone(), rows[0]["projectIcon"]["name"].clone()), (id, json!("No project"), json!("/workspace/.rca/scratch"), json!("message-square-dashed")));
}

/// The launch of a task in `project` on the project's root, whose first message is `text`.
fn launch(command_id: &str, thread: &str, project: &str, text: &str) -> Value {
    json!({
        "commandId": command_id, "projectId": project, "threadId": thread, "title": "Fix the build", "modelSelection": {"instanceId": "opencode", "model": "rca-default"},
        "runtimeMode": "full-access", "interactionMode": "default", "workspaceStrategy": {"type": "root"}, "initialMessage": {"messageId": format!("m-{command_id}"), "text": text, "attachments": []},
    })
}

#[test]
fn a_thread_launched_in_the_scratch_project_gets_a_folder_of_its_own_and_creating_a_project_from_it_moves_every_thread_working_there() {
    // Given the scratch project, an ordinary project and a thread launched in the scratch project
    let (h, mut rpc) = started();
    success(&rpc.call("projects.mutate", project("p1", "/workspace/demo")));
    let scratch = success(&rpc.call("projects.ensureScratch", json!({})))["projectId"].as_str().unwrap().to_string();
    let launched = rpc.call("orchestration.launchThread", launch("c-s1", "t-s1", &scratch, "Make the build green again"));
    let folder = success(&launched)["projection"]["thread"]["worktreePath"].as_str().unwrap().to_string();
    // Then the executor was asked to make the thread's folder below the scratch folder, named for the day, the first words and the end of its id, and the thread works there
    assert_eq!(folder, "/workspace/.rca/scratch/2026-11-01-make-the-build-green-again-ts1");
    assert!(ops(&h).contains(&Op::FileMkdir { path: ".rca/scratch/2026-11-01-make-the-build-green-again-ts1".into(), parents: false }));
    settled(&mut rpc, "t-s1");
    // And a launch in an ordinary project asks the executor for no folder
    h.agent.ops.lock().unwrap().clear();
    success(&rpc.call("orchestration.launchThread", launch("c-o1", "t-o1", "p1", "hello")));
    assert!(!methods(&h).contains(&"file.mkdir"), "{:?}", methods(&h));
    settled(&mut rpc, "t-o1");
    // And a second thread of the scratch project that works in the same folder
    let mut sibling = thread_create("c-s2", "t-s2", &scratch);
    sibling["worktreePath"] = json!(folder);
    success(&rpc.call("orchestration.dispatchCommand", sibling));
    // When a project is made from the first thread
    h.agent.ops.lock().unwrap().clear();
    let made = rpc.call("projects.createFromThread", json!({"threadId": "t-s1"}));
    // Then the project is on the thread's folder, named for the thread, a workspace of the agent API, and both threads are in it
    let id = success(&made)["projectId"].as_str().unwrap().to_string();
    assert_ne!(id, scratch);
    let row = projects_of(&mut rpc).into_iter().find(|p| p["id"] == id.as_str()).expect("the new project is in the shell");
    assert_eq!((row["title"].clone(), row["workspaceRoot"].clone()), (json!("Fix the build"), json!(folder)));
    assert!(ops(&h).contains(&Op::WorkspaceCreate { workspace: format!("project-{id}"), path: ".rca/scratch/2026-11-01-make-the-build-green-again-ts1".into() }));
    for thread in ["t-s1", "t-s2"] {
        assert_eq!(success(&rpc.call("orchestration.getThreadProjection", json!({"threadId": thread})))["thread"]["projectId"], id.as_str(), "{thread}");
    }
    // And asking again names the same project and makes no other
    let again = rpc.call("projects.createFromThread", json!({"threadId": "t-s1"}));
    assert_eq!(success(&again)["projectId"], id.as_str());
    assert_eq!(projects_of(&mut rpc).len(), 3);
}

#[test]
fn a_project_cannot_be_made_from_a_thread_that_is_unknown_has_a_project_or_is_still_working() {
    // Given an ordinary project and its thread, the scratch project and a thread of it that is still running
    let (h, mut rpc) = started();
    success(&rpc.call("projects.mutate", project("p1", "/workspace/demo")));
    success(&rpc.call("orchestration.dispatchCommand", thread_create("c-t1", "t1", "p1")));
    let scratch = success(&rpc.call("projects.ensureScratch", json!({})))["projectId"].as_str().unwrap().to_string();
    h.agent.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    *h.agent.script.lock().unwrap() = vec![AgentEventKind::Text("working".into())];
    success(&rpc.call("orchestration.launchThread", launch("c-s1", "t-s1", &scratch, "go")));
    // When a project is asked for from an unknown thread, from the ordinary project's thread, and from the one that is still working
    for (thread, says) in [("nope", "Thread not found."), ("t1", "Only a No project thread with its own folder can become a project."), ("t-s1", "is still working")] {
        let exit = rpc.call("projects.createFromThread", json!({"threadId": thread}));
        // Then the phone is told why, with the declared error
        assert_eq!(failure(&exit)["_tag"], "OrchestrationDispatchCommandError", "{thread}");
        assert!(failure(&exit)["message"].as_str().unwrap().contains(says), "{thread}: {exit}");
    }
    // And no project was made
    assert_eq!(projects_of(&mut rpc).len(), 2);
}
