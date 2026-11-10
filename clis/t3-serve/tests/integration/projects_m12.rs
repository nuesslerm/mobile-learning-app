//! `projects.*` and the project and thread commands through the chain: a project on a folder below the executor's root is a workspace of the real
//! rca-service with its settings in rca's state, a delete forgets both, and a folder outside the root (or one that is not there) is refused; a
//! thread the phone creates exists in rca before its first turn; and the scratch project, a new project and a project made from a thread's folder
//! are folders the real rca-exec makes (`file.mkdir`, `git.init`), which these tests look at on the executor's disk. A call counts for its row
//! only when it was answered and the agent API method its row names ran in its window.
use crate::chain::{last_window, said};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::{Script, Turn};
use std::path::{Path, PathBuf};
use std::process::Command;

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

fn answered(case: &exec_testkit::Case, method: &str) {
    let (agent, _) = last_window(case);
    assert!(agent.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {agent:?}");
}

/// The repository's directory as the executor names it (links resolved).
fn root(chain: &Chain) -> PathBuf {
    std::fs::canonicalize(chain.repo.path()).unwrap()
}

/// The workspaces rca knows with where each lies, read beside the phone's calls.
fn repos(rca: &mut Direct) -> Vec<Value> {
    rca.call("workspace", "repo.list", json!({})).expect("rca lists its workspaces")["repos"].as_array().unwrap().clone()
}

fn workspace_of(repos: &[Value], name: &str) -> Option<Value> {
    repos.iter().find(|r| r["workspace"] == name).cloned()
}

/// Git run in `dir` as the executor runs it: no configuration of the machine, a fixed author.
fn git_in(chain: &Chain, dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).env_clear().envs(chain.repo.git_env()).env("PATH", std::env::var_os("PATH").unwrap_or_default()).output().expect("git starts");
    assert!(out.status.success(), "git {args:?} in {}: {}", dir.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

fn project(id: &str, folder: &Path) -> Value {
    json!({"type": "project.create", "commandId": format!("c-{id}"), "projectId": id, "title": format!("Project {id}"), "workspaceRoot": folder.display().to_string()})
}

/// The phone's shell as it stands: its projects.
fn projects_of(phone: &mut Phone) -> Vec<Value> {
    let shell = phone.watch.send("orchestration.subscribeShell", json!({}));
    let first = phone.watch.chunk(&shell);
    first[0]["snapshot"]["projects"].as_array().unwrap().clone()
}

#[test]
fn a_project_below_the_root_is_a_workspace_of_rca_with_its_settings_in_rcas_state_a_delete_forgets_both_and_a_folder_outside_the_root_or_not_there_is_refused() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a paired phone and a folder of the repository with a file in it
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        chain.repo.write("apps/web/index.html", "<html></html>\n");
        let web = root(&chain).join("apps/web");
        let mut rca = chain.direct();
        // When the phone makes a project on that folder
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.create"], "projects.mutate", project("pw", &web)));
        answered(case, "workspace.create");
        // Then rca lists the folder as a workspace of its own below the root, and a file call in that workspace reaches the folder's files
        let listed = repos(&mut rca);
        let workspace = workspace_of(&listed, "project-pw").unwrap_or_else(|| panic!("rca has no workspace project-pw: {listed:?}"));
        assert_eq!((workspace["kind"].clone(), workspace["path"].clone(), workspace["attached"].clone()), (json!("directory"), json!("apps/web"), json!(true)));
        let files = rca.call("files", "file.list", json!({"workspace": "project-pw", "path": "."})).expect("the workspace lists its folder");
        assert_eq!(files["entries"][0]["name"], "index.html");
        // And what the project says is in rca's state, where another device reads it
        let stored = rca.call("state", "state.get", json!({"scope": "t3", "key": "project.settings:pw"})).unwrap();
        assert_eq!((stored["value"]["title"].clone(), stored["value"]["workspaceRoot"].clone()), (json!("Project pw"), json!(web.display().to_string())));
        // When the phone renames it
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.update"], "projects.mutate", json!({"type": "project.update", "commandId": "c-pu", "projectId": "pw", "title": "Web app"})));
        answered(case, "state.set");
        // Then rca's state holds the new title
        let stored = rca.call("state", "state.get", json!({"scope": "t3", "key": "project.settings:pw"})).unwrap();
        assert_eq!(stored["value"]["title"], "Web app");
        // When a project is asked for on a folder outside the root that exists, one the executor may browse but not serve, and one inside the root that is not there
        let outside = tempfile::tempdir().unwrap();
        for (n, folder) in [outside.path().to_path_buf(), chain.shared.join("alpha"), root(&chain).join("apps/missing")].into_iter().enumerate() {
            let exit = phone.calls.call("projects.mutate", project(&format!("px{n}"), &folder));
            // Then each is refused, and rca has no workspace for it
            assert_eq!(failure(&exit)["_tag"], "ProjectMutationError", "{}", folder.display());
            assert!(workspace_of(&repos(&mut rca), &format!("project-px{n}")).is_none());
        }
        assert_eq!(projects_of(&mut phone).iter().map(|p| p["id"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["pw"]);
        // When the project is deleted
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.delete"], "projects.mutate", json!({"type": "project.delete", "commandId": "c-pd", "projectId": "pw"})));
        answered(case, "workspace.remove");
        // Then rca no longer lists its workspace and its state is empty, and the folder itself is where it was
        assert!(workspace_of(&repos(&mut rca), "project-pw").is_none());
        assert_eq!(rca.call("state", "state.get", json!({"scope": "t3", "key": "project.settings:pw"})).unwrap()["value"], Value::Null);
        assert!(web.join("index.html").is_file());
    });
}

#[test]
fn a_thread_the_phone_creates_exists_in_rca_with_its_driver_and_model_before_its_first_turn() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a paired phone and a project on the repository
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        success(&phone.call(case, &chain, &["rpc:projects.mutate", "mutation:project.create"], "projects.mutate", project("p1", &root(&chain))));
        let mut rca = chain.direct();
        assert_eq!(rca.call("agent.threads", "thread.get", json!({"thread": "t9"})).unwrap_err().code, "THREAD_NOT_FOUND");
        // When the phone creates a thread that is to run Claude Code
        let create = json!({"type": "thread.create", "commandId": "c-t9", "threadId": "t9", "projectId": "p1", "title": "Notes", "modelSelection": {"instanceId": "claudeAgent", "model": "opus"}});
        success(&phone.call(case, &chain, &["rpc:orchestration.dispatchCommand", "command:thread.create"], "orchestration.dispatchCommand", create));
        // Then rca was asked for the thread in that call, and has it, idle, with the driver and model, though no turn has run
        answered(case, "thread.create");
        let thread = rca.call("agent.threads", "thread.get", json!({"thread": "t9"})).expect("rca has the thread")["thread"].clone();
        assert_eq!((thread["driver"].clone(), thread["model"].clone(), thread["workspace"].clone(), thread["status"].clone()), (json!("claudeAgent"), json!("opus"), json!("w1"), json!("idle")));
        assert!(chain.cli.invocations("claude").is_empty());
    });
}

#[test]
fn the_scratch_project_is_made_once_on_a_folder_the_executor_makes_below_its_root_and_the_repository_around_it_never_sees_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a paired phone, on a repository that has no .rca folder
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let scratch = root(&chain).join(".rca/scratch");
        assert!(!root(&chain).join(".rca").exists());
        // When the phone asks for the scratch project
        let first = phone.call(case, &chain, &["rpc:projects.ensureScratch"], "projects.ensureScratch", json!({}));
        let id = success(&first)["projectId"].as_str().unwrap().to_string();
        // Then the executor made the folder (the window says so) and it is a directory on its disk, inside a folder that keeps the repository's git from seeing it
        answered(case, "file.mkdir");
        assert!(scratch.is_dir());
        assert_eq!(std::fs::read_to_string(root(&chain).join(".rca/.gitignore")).unwrap(), "*\n");
        assert_eq!(chain.repo.git(&["status", "--porcelain"]), "");
        // And the project is on that folder, named for a thread with no project, and rca has the folder as a workspace of its own
        let rows = projects_of(&mut phone);
        assert_eq!((rows[0]["id"].clone(), rows[0]["title"].clone(), rows[0]["workspaceRoot"].clone()), (json!(id), json!("No project"), json!(scratch.display().to_string())));
        let mut rca = chain.direct();
        assert_eq!(workspace_of(&repos(&mut rca), &format!("project-{id}")).map(|w| w["path"].clone()), Some(json!(".rca/scratch")));
        // When the phone asks again
        let second = phone.call(case, &chain, &["rpc:projects.ensureScratch"], "projects.ensureScratch", json!({}));
        // Then it is the same project, the executor was asked to make nothing, and no second project exists
        assert_eq!(success(&second)["projectId"], id.as_str());
        let (agent, _) = last_window(case);
        assert!(!agent.iter().any(|(m, _)| m == "file.mkdir" || m == "workspace.create"), "{agent:?}");
        assert_eq!(projects_of(&mut phone).len(), 1);
    });
}

#[test]
fn a_new_project_is_a_repository_the_executor_makes_below_its_root_with_a_readme_an_icon_and_one_commit_and_a_name_that_climbs_out_stays_below_the_root() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a paired phone
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let projects = root(&chain).join(".rca/projects");
        // When the phone makes a new project, and another of the same name
        let first = phone.call(case, &chain, &["rpc:projects.createNew"], "projects.createNew", json!({"name": "Pinball Stats"}));
        let made = success(&first).clone();
        let (agent, executor) = last_window(case);
        let second = phone.call(case, &chain, &["rpc:projects.createNew"], "projects.createNew", json!({"name": "Pinball Stats"}));
        // Then the executor made the folder and the repository in the first call's window, below the root
        for method in ["file.mkdir", "git.init", "workspace.create"] {
            assert!(agent.iter().any(|(m, ok)| m == method && *ok), "{method}: {agent:?}");
        }
        assert!(executor.as_array().unwrap().iter().any(|h| h["tool"] == "git_init" && h["ok"] == true), "{executor}");
        let folder = projects.join("pinball-stats");
        assert_eq!(made["workspaceRoot"], json!(folder.display().to_string()));
        // And the folder is a repository on its own whose one commit holds the README and the icon, made by the executor's git
        assert!(folder.join(".git").is_dir());
        assert_eq!(git_in(&chain, &folder, &["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(git_in(&chain, &folder, &["log", "-1", "--format=%s"]), "Initial commit");
        assert_eq!(git_in(&chain, &folder, &["ls-tree", "-r", "--name-only", "HEAD"]), "README.md\nassets/icon.svg");
        assert!(std::fs::read_to_string(folder.join("README.md")).unwrap().contains("# Pinball Stats"));
        assert!(std::fs::read_to_string(folder.join("assets/icon.svg")).unwrap().contains(">PS</text>"));
        assert!(made.get("commitError").is_none(), "{made}");
        // And the repository around it is clean, the second project got the next free folder, and both are workspaces of rca
        assert_eq!(chain.repo.git(&["status", "--porcelain"]), "");
        assert_eq!(success(&second)["workspaceRoot"], json!(projects.join("pinball-stats-2").display().to_string()));
        assert!(projects.join("pinball-stats-2/.git").is_dir());
        let mut rca = chain.direct();
        let listed = repos(&mut rca);
        let id = made["projectId"].as_str().unwrap();
        assert_eq!(workspace_of(&listed, &format!("project-{id}")).map(|w| w["path"].clone()), Some(json!(".rca/projects/pinball-stats")));
        assert_eq!(projects_of(&mut phone).iter().map(|p| p["title"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["Pinball Stats", "Pinball Stats"]);
        // When the name is a path that tries to leave the folder of projects
        let sly = phone.call(case, &chain, &["rpc:projects.createNew"], "projects.createNew", json!({"name": "../../escape"}));
        // Then the folder is named for its letters and is below the projects folder, and nothing was made beside the repository
        assert_eq!(success(&sly)["workspaceRoot"], json!(projects.join("escape").display().to_string()));
        assert!(projects.join("escape/.git").is_dir());
        assert!(!root(&chain).parent().unwrap().join("escape").exists() && !root(&chain).join("escape").exists());
    });
}

#[test]
fn a_thread_launched_in_the_scratch_project_works_in_a_folder_the_executor_made_for_it_and_a_project_made_from_it_is_that_folder_in_place() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, the scratch project, an ordinary project with a thread, and a CLI whose turn says one thing
        let chain = Chain::start();
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("done")])]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        phone.seed(case, &chain);
        let scratch_id = success(&phone.call(case, &chain, &["rpc:projects.ensureScratch"], "projects.ensureScratch", json!({})))["projectId"].as_str().unwrap().to_string();
        // When the phone launches a task in the scratch project
        let launch = json!({
            "commandId": "c-s1", "creationSource": "mobile", "projectId": scratch_id, "threadId": "t-s1", "title": "Fix the build", "modelSelection": {"instanceId": "claudeAgent", "model": "opus"},
            "runtimeMode": "full-access", "interactionMode": "default", "workspaceStrategy": {"type": "root"}, "initialMessage": {"messageId": "m-s1", "text": "Make the build green again", "attachments": []},
        });
        let launched = case.call(&chain.tap, &rows(&["rpc:orchestration.launchThread"]), || {
            let exit = phone.calls.call("orchestration.launchThread", launch);
            let projection = phone.settled("t-s1");
            let completed = projection["runs"].as_array().and_then(|r| r.last()).is_some_and(|r| r["status"] == "completed");
            (exit["_tag"] == "Success" && completed, exit)
        });
        // Then the executor made a folder for the thread below the scratch folder, named for the day, its first words and the end of its id, and the thread works there
        answered(case, "file.mkdir");
        let folder = PathBuf::from(success(&launched)["projection"]["thread"]["worktreePath"].as_str().unwrap());
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(folder.parent().unwrap(), root(&chain).join(".rca/scratch"));
        assert!(folder.is_dir() && name.ends_with("-make-the-build-green-again-ts1") && name.len() == "2026-11-01-make-the-build-green-again-ts1".len(), "{name}");
        assert!(name.chars().take(10).enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() }), "{name}");
        // When a project is made from that thread
        let made = phone.call(case, &chain, &["rpc:projects.createFromThread"], "projects.createFromThread", json!({"threadId": "t-s1"}));
        let id = success(&made)["projectId"].as_str().unwrap().to_string();
        // Then the project is on the thread's own folder, which is where it was (made in place), and rca has it as a workspace of its own
        assert_ne!(id, scratch_id);
        assert!(folder.is_dir());
        let row = projects_of(&mut phone).into_iter().find(|p| p["id"] == id.as_str()).expect("the new project is in the shell");
        assert_eq!((row["title"].clone(), row["workspaceRoot"].clone()), (json!("Fix the build"), json!(folder.display().to_string())));
        let mut rca = chain.direct();
        assert_eq!(workspace_of(&repos(&mut rca), &format!("project-{id}")).map(|w| w["path"].clone()), Some(json!(format!(".rca/scratch/{name}"))));
        answered(case, "workspace.create");
        // And the thread is in it, and asking again names the same project and makes no other
        let thread = success(&phone.calls.call("orchestration.getThreadProjection", json!({"threadId": "t-s1"}))).clone();
        assert_eq!(thread["thread"]["projectId"], id.as_str());
        let again = phone.call(case, &chain, &["rpc:projects.createFromThread"], "projects.createFromThread", json!({"threadId": "t-s1"}));
        assert_eq!(success(&again)["projectId"], id.as_str());
        assert_eq!(projects_of(&mut phone).len(), 3);
        // And a thread of an ordinary project, or one that is not there, cannot become a project
        for thread in ["t1", "nope"] {
            let refused = phone.calls.call("projects.createFromThread", json!({"threadId": thread}));
            assert_eq!(failure(&refused)["_tag"], "OrchestrationDispatchCommandError", "{thread}");
        }
        assert_eq!(projects_of(&mut phone).len(), 3);
    });
}
