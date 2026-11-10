//! `workspace`, `workspace.worktrees` and `files.edit` end to end over the agent socket, against the real `rca-exec` binary serving a temporary
//! git repository with a worktree base. A file or worktree that exists was made by `rca-exec`: the service holds no workspace and has no way
//! to touch a path. The flows also prove what the groups never let through: a path that climbs out of a workspace, a link that leaves the
//! root, a directory outside the browse allowlist, and a worktree's work landing in the root.

use crate::harness::{delete, get, post, service, Env, Options, Service, SECRET};
use crate::mapper::Agent;
use exec_testkit::{RcaExec, TempRepo};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

struct Rig {
    _svc: Service,
    _exec: RcaExec,
    repo: TempRepo,
    /// The worktree base, outside the repository.
    base: PathBuf,
    /// A directory the executor may browse, outside the repository and the base.
    shared: PathBuf,
    port: u16,
    token: String,
    _dirs: Vec<tempfile::TempDir>,
}

async fn rig(env: &Env) -> Rig {
    let repo = TempRepo::new();
    let dirs: Vec<tempfile::TempDir> = (0..2).map(|_| tempfile::tempdir().unwrap()).collect();
    let [base, shared]: [PathBuf; 2] = std::array::from_fn(|i| std::fs::canonicalize(dirs[i].path()).unwrap());
    let base = base.join("worktrees");
    std::fs::create_dir(&base).unwrap();
    std::fs::write(shared.join("shared.txt"), "visible").unwrap();
    let svc = service(env, Options::default());
    let port = svc.listen().await;
    let flags = ["--worktree-base", base.to_str().unwrap(), "--browse-allow", shared.to_str().unwrap()];
    let exec = tokio::task::block_in_place(|| RcaExec::serve_with(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET, &flags));
    let token = svc.mint("workspace flow").await;
    Rig { _svc: svc, _exec: exec, repo, base, shared, port, token, _dirs: dirs }
}

impl Rig {
    async fn agent(&self) -> Agent {
        Agent::open(self.port, &self.token).await
    }
}

fn code(answer: &Value) -> &str {
    answer["error"]["code"].as_str().unwrap_or_else(|| panic!("no error in {answer}"))
}

async fn ok(agent: &mut Agent, method: &str, params: Value) -> Value {
    let answer = agent.ask(method, params).await;
    assert!(answer.get("result").is_some(), "{method} failed: {answer}");
    answer["result"].clone()
}

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..300 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{what}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_worktree_made_over_the_socket_when_files_are_written_set_up_diffed_and_removed_then_the_work_is_in_the_worktree_and_never_in_the_root() {
    // Given the service, the real executor with a worktree base, and a mapper
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    // When a worktree is made on a new branch with two setup commands, and its setup is followed
    let made = ok(&mut agent, "worktree.create", json!({"workspace": "w1", "worktree": "wt", "branch": "feature/x", "base": "HEAD", "setup": ["echo built > out.txt", "basename \"$PWD\" > cwd.txt"]})).await;
    let wt = rig.base.join("wt");
    // Then the executor made it under the base on that branch, and the answer names its workspace, repository and path
    assert_eq!((made["workspace"].clone(), made["repo"].clone(), made["executor"].clone(), made["branch"].clone(), made["setup"].clone()), (json!("wt"), json!("w1"), json!("w1"), json!("feature/x"), json!(true)));
    assert_eq!(made["path"], json!(wt.to_str().unwrap()));
    assert_eq!(std::fs::read_to_string(wt.join("README.md")).unwrap(), "temp repo\n");
    assert_eq!(rig.repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 2);
    ok(&mut agent, "worktree.setup.subscribe", json!({"workspace": "wt"})).await;
    let events = agent.events_through("setup.finished").await;
    // And the setup events came in order, numbered from 1, the commands ran in the worktree and the last event says it went well
    assert_eq!(events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(), ["setup.started", "step.started", "step.finished", "step.started", "step.finished", "setup.finished"]);
    assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    assert!(events.iter().all(|e| e.thread.is_none() && e.data["workspace"] == "wt"));
    assert_eq!(events[5].data["ok"], json!(true));
    assert_eq!((std::fs::read_to_string(wt.join("out.txt")).unwrap(), std::fs::read_to_string(wt.join("cwd.txt")).unwrap()), ("built\n".to_string(), "wt\n".to_string()));
    assert!(!rig.repo.path().join("out.txt").exists());
    // When files are written, created, stat, read in chunks, searched and browsed in the worktree by paths relative to it
    assert_eq!(ok(&mut agent, "file.write", json!({"workspace": "wt", "path": "README.md", "content": "temp repo\nchanged\n"})).await, json!({"bytes": 18}));
    ok(&mut agent, "file.create", json!({"workspace": "wt", "path": "src/new.rs", "content": "fn main() {}", "create_dirs": true})).await;
    let again = agent.ask("file.create", json!({"workspace": "wt", "path": "src/new.rs"})).await;
    let stat = ok(&mut agent, "file.stat", json!({"workspace": "wt", "path": "src/new.rs"})).await;
    let chunk = ok(&mut agent, "file.readChunk", json!({"workspace": "wt", "path": "src/new.rs", "offset": 3, "length": 4})).await;
    let found = ok(&mut agent, "file.search", json!({"workspace": "wt", "query": "new"})).await;
    let listed = ok(&mut agent, "file.browse", json!({"workspace": "wt", "path": "src"})).await;
    // Then they are in the worktree and not in the root, a second create is refused with the executor's code, and the searches are relative to the worktree
    assert_eq!(std::fs::read_to_string(wt.join("src/new.rs")).unwrap(), "fn main() {}");
    assert_eq!((std::fs::read_to_string(rig.repo.path().join("README.md")).unwrap(), rig.repo.path().join("src").exists()), ("temp repo\n".to_string(), false));
    assert_eq!(code(&again), "ALREADY_EXISTS");
    assert_eq!((stat["kind"].clone(), stat["size"].clone()), (json!("file"), json!(12)));
    assert_eq!((chunk["bytes"].clone(), chunk["total_bytes"].clone()), (json!(4), json!(12)));
    assert_eq!(found["entries"], json!([{"path": "src/new.rs", "kind": "file"}]));
    assert_eq!(listed["entries"], json!([{"name": "new.rs", "kind": "file", "path": wt.join("src/new.rs").to_str().unwrap()}]));
    // And the diff of the worktree has its change, the root's has none, and a path of the diff stays in the worktree (the executor holds it there)
    let thread = ok(&mut agent, "thread.diff", json!({"workspace": "wt", "path": "README.md"})).await;
    let turn = ok(&mut agent, "turn.diff", json!({"workspace": "wt", "base": "HEAD"})).await;
    let root = ok(&mut agent, "thread.diff", json!({"workspace": "w1"})).await;
    assert!(thread["diff"].as_str().unwrap().contains("+changed"), "{thread}");
    assert!(turn["diff"].as_str().unwrap().contains("+changed"), "{turn}");
    assert_eq!(root["diff"], json!(""));
    assert_eq!(code(&agent.ask("thread.diff", json!({"workspace": "wt", "path": "../../w1/README.md"})).await), "PATH_OUTSIDE_ROOT");
    // When the worktrees are listed, the one with changes is removed without force, and then with it
    let listing = ok(&mut agent, "worktree.list", json!({"workspace": "w1"})).await;
    let dirty = agent.ask("worktree.remove", json!({"workspace": "wt"})).await;
    // Then both worktrees are listed, the executor's refusal comes back and the worktree is still there with its workspace
    assert_eq!(listing["worktrees"].as_array().unwrap().iter().filter(|w| w["is_main"] == false).map(|w| w["branch"].clone()).collect::<Vec<_>>(), [json!("feature/x")]);
    assert_eq!(code(&dirty), "IO_ERROR");
    assert!(wt.join("src/new.rs").is_file());
    let removed = ok(&mut agent, "worktree.remove", json!({"workspace": "wt", "force": true})).await;
    assert_eq!(removed, json!({"workspace": "wt", "removed": true}));
    assert!(!wt.exists());
    assert_eq!(rig.repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1);
    assert_eq!(code(&agent.ask("file.read", json!({"workspace": "wt", "path": "README.md"})).await), "NO_EXECUTOR");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_setup_that_is_running_when_it_is_cancelled_over_the_socket_then_the_command_is_stopped_nothing_after_it_runs_and_the_stream_ends_cancelled() {
    // Given a worktree whose setup touches a file, sleeps and would touch another
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    ok(&mut agent, "worktree.create", json!({"workspace": "w1", "worktree": "wt", "branch": "b", "base": "HEAD", "setup": ["touch started.txt", "sleep 3", "touch finished.txt"]})).await;
    let wt = rig.base.join("wt");
    ok(&mut agent, "worktree.setup.subscribe", json!({"workspace": "wt"})).await;
    until("the first command to run", || wt.join("started.txt").exists()).await;
    // When it is cancelled
    let cancelled = ok(&mut agent, "worktree.setup.cancel", json!({"workspace": "wt"})).await;
    let events = agent.events_through("setup.finished").await;
    // Then the stream ends with a setup.finished that says cancelled, and the command after the sleep never runs
    assert_eq!(cancelled, json!({"workspace": "wt", "cancelled": true}));
    let last = events.last().unwrap();
    assert_eq!((last.data["ok"].clone(), last.data["cancelled"].clone()), (json!(false), json!(true)));
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert!(!wt.join("finished.txt").exists(), "the executor ran the command after the cancelled one");
    // And a second cancel finds it over
    assert_eq!(ok(&mut agent, "worktree.setup.cancel", json!({"workspace": "wt"})).await["cancelled"], json!(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_workspace_that_is_a_directory_when_files_are_asked_for_then_they_are_below_it_and_no_path_link_or_browse_leaves_what_the_executor_allows() {
    // Given a repository with a committed apps/web, a link in it to a secret outside, and a workspace for that directory
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    rig.repo.write("apps/web/index.ts", "one\n");
    rig.repo.write("apps/api/index.ts", "api\n");
    rig.repo.commit_all("add apps");
    let secret = rig.shared.join("shared.txt");
    std::os::unix::fs::symlink(&secret, rig.repo.path().join("apps/web/leak.txt")).unwrap();
    let mut agent = rig.agent().await;
    let made = ok(&mut agent, "workspace.create", json!({"workspace": "web", "path": "apps/web"})).await;
    assert_eq!(made, json!({"workspace": "web", "executor": "w1", "path": "apps/web"}));
    // When it is written, searched and diffed
    ok(&mut agent, "file.write", json!({"workspace": "web", "path": "index.ts", "content": "one\ntwo\n"})).await;
    ok(&mut agent, "file.write", json!({"workspace": "w1", "path": "apps/api/index.ts", "content": "api\nchanged\n"})).await;
    let found = ok(&mut agent, "file.search", json!({"workspace": "web", "query": "index"})).await;
    let diff = ok(&mut agent, "file.diff", json!({"workspace": "web"})).await;
    let listed = ok(&mut agent, "file.list", json!({"workspace": "web", "path": "."})).await;
    // Then the write is under apps/web, the search finds only its own files, the diff is its own change and not apps/api's, and the listing is its directory
    assert_eq!(std::fs::read_to_string(rig.repo.path().join("apps/web/index.ts")).unwrap(), "one\ntwo\n");
    assert_eq!(found["entries"], json!([{"path": "index.ts", "kind": "file"}]));
    let text = diff["diff"].as_str().unwrap();
    assert!(text.contains("apps/web/index.ts") && text.contains("+two") && !text.contains("apps/api"), "{text}");
    assert!(listed["entries"].as_array().unwrap().iter().any(|e| e["name"] == "index.ts"));
    // When a path leaves the workspace, a link inside it leaves the root, and directories are browsed in and out of the allowlist
    let climbing = agent.ask("file.read", json!({"workspace": "web", "path": "../api/index.ts"})).await;
    let link = agent.ask("file.read", json!({"workspace": "web", "path": "leak.txt"})).await;
    let link_write = agent.ask("file.write", json!({"workspace": "web", "path": "leak.txt", "content": "pwned"})).await;
    let outside = agent.ask("file.browse", json!({"workspace": "web", "path": "/etc"})).await;
    let allowed = ok(&mut agent, "file.browse", json!({"workspace": "web", "path": rig.shared.to_str().unwrap()})).await;
    // Then the first is refused by the service, the links and /etc by the executor, and the allowlisted directory is shown
    assert_eq!(code(&climbing), "INVALID_PARAMS");
    assert_eq!((code(&link), code(&link_write), code(&outside)), ("PATH_OUTSIDE_ROOT", "PATH_OUTSIDE_ROOT", "PATH_OUTSIDE_ROOT"));
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "visible");
    assert_eq!(allowed["entries"][0]["name"], json!("shared.txt"));
    // And repo.list says how each workspace lies, and removing the directory workspace forgets it without touching the directory
    let repos = ok(&mut agent, "repo.list", json!({})).await;
    assert_eq!(repos["repos"], json!([{"workspace": "w1", "executor": "w1", "kind": "root", "attached": true}, {"workspace": "web", "executor": "w1", "kind": "directory", "attached": true, "path": "apps/web"}]));
    assert_eq!(ok(&mut agent, "workspace.remove", json!({"workspace": "web"})).await["removed"], json!(true));
    assert!(rig.repo.path().join("apps/web/index.ts").is_file());
    assert_eq!(code(&agent.ask("file.read", json!({"workspace": "web", "path": "index.ts"})).await), "NO_EXECUTOR");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_the_retired_worktree_and_diff_routes_when_called_with_the_secret_then_each_is_404_and_nothing_is_run() {
    // Given the service
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    // When the routes that cloned and removed worktrees and served diff pages are called, with the secret and a body that used to be valid
    let called = [
        svc.send(post("/api/worktree").secret().json(json!({"repo": "acme/widget", "branch": "feature"}))).await,
        svc.send(delete("/api/worktree/acme-widget").secret()).await,
        svc.send(post("/api/diff").secret().json(json!({"diff": "diff --git a/a b/a\n", "chatId": 1, "cwd": "/"}))).await,
        svc.send(get("/api/diff/0123456789abcdef").secret()).await,
        svc.send(get("/api/diff/0123456789abcdef/expand?file=a&start=1&end=2").secret()).await,
        svc.send(post("/api/diff/0123456789abcdef/annotations").secret().json(json!({"annotations": []}))).await,
    ];
    // Then every one is the JSON 404 of a path that is not a route, and no program (git, gh) was run
    for answer in &called {
        assert_eq!((answer.status.as_u16(), answer.json()), (404, json!({"error": "not_found"})));
    }
    assert!(svc.commands.of("git").is_empty() && svc.commands.of("gh").is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_an_executor_attached_for_a_repository_when_the_service_previews_files_then_it_reads_its_own_volume_and_never_the_executors_workspace() {
    // Given a fork directory of the service with its own page, and an executor serving a repository with a page of its own
    let env = Env::new(&[]);
    let fork = env.dir("fork");
    std::fs::write(fork.join("own.md"), "# service volume\n").unwrap();
    env.set("FORK_DIR", &fork.to_string_lossy());
    let rig = rig(&env).await;
    rig.repo.write("executor-only.md", "# executor workspace\n");
    let mut agent = rig.agent().await;
    assert_eq!(ok(&mut agent, "file.read", json!({"workspace": "w1", "path": "executor-only.md"})).await["content"], json!("# executor workspace\n"));
    // When the service's preview is asked for each, by name and by the absolute path of the executor's page
    let svc = &rig._svc;
    let own = svc.send(get("/api/file/own.md")).await;
    let by_name = svc.send(get("/api/file/executor-only.md")).await;
    let absolute = svc.send(get(&format!("/api/file/{}/executor-only.md", rig.repo.path().display()))).await;
    // Then its own page renders and the executor's is not found or refused, whichever way it is named
    assert_eq!(own.status.as_u16(), 200);
    assert!(own.text().contains("service volume"));
    for answer in [&by_name, &absolute] {
        assert!(matches!(answer.status.as_u16(), 403 | 404), "{} {}", answer.status, answer.text());
        assert!(!answer.text().contains("executor workspace"));
    }
}
