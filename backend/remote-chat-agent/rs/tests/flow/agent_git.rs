//! `git` end to end over the agent socket, against the real `rca-exec` binary serving a temporary git repository: the repository is the
//! executor's, and every answer is checked against the repository itself (`git log`, `git status`). The flows show what a phone does (stage,
//! commit, branch, a merge that conflicts and its resolution), the pushes and pulls to the repository's own remote, what the executor refuses
//! (option injection, a remote that is not configured), a worktree workspace, the status stream, and every recorded exchange of `agent-api-git`
//! replayed as recorded.

use crate::harness::{service, Env, Options, Service, SECRET};
use crate::mapper::{Agent, WAIT};
use exec_testkit::{RcaExec, TempRepo};
use serde_json::{json, Value};

struct Rig {
    _svc: Service,
    _exec: RcaExec,
    port: u16,
    token: String,
    repo: TempRepo,
    _dir: tempfile::TempDir,
}

async fn rig(env: &Env) -> Rig {
    let repo = TempRepo::new();
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap().join("worktrees");
    std::fs::create_dir(&base).unwrap();
    let svc = service(env, Options::default());
    let port = svc.listen().await;
    let url = format!("http://127.0.0.1:{port}");
    let exec = tokio::task::block_in_place(|| RcaExec::serve_with(&url, "w1", &repo, SECRET, &["--worktree-base", base.to_str().unwrap()]));
    let token = svc.mint("git flow").await;
    Rig { _svc: svc, _exec: exec, port, token, repo, _dir: dir }
}

impl Rig {
    async fn agent(&self) -> Agent {
        Agent::open(self.port, &self.token).await
    }
}

async fn ok(agent: &mut Agent, method: &str, params: Value) -> Value {
    let answer = agent.ask(method, params).await;
    assert!(answer.get("result").is_some(), "{method} failed: {answer}");
    answer["result"].clone()
}

async fn refused(agent: &mut Agent, method: &str, params: Value) -> String {
    let answer = agent.ask(method, params).await;
    assert!(answer.get("error").is_some(), "{method} was not refused: {answer}");
    answer["error"]["code"].as_str().unwrap().to_string()
}

fn w(extra: Value) -> Value {
    let mut params = json!({"workspace": "w1"});
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    params
}

fn files(status: &Value) -> Vec<(String, String, String)> {
    status["files"].as_array().unwrap().iter().map(|f| (f["path"].as_str().unwrap().into(), f["index"].as_str().unwrap().into(), f["worktree"].as_str().unwrap().into())).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_repository_when_a_phone_stages_commits_and_branches_then_the_repository_holds_exactly_that() {
    // Given the repository with an edited file and a new one, and a phone on the socket
    let env = Env::process(&[]);
    let rig = rig(&env).await;
    rig.repo.write("README.md", "temp repo\nedited\n");
    rig.repo.write("new.txt", "new\n");
    let mut agent = rig.agent().await;
    let status = ok(&mut agent, "git.status", w(json!({}))).await;
    assert_eq!((status["branch"].clone(), status["clean"].clone(), files(&status)), (json!("main"), json!(false), vec![("README.md".into(), ".".into(), "M".into()), ("new.txt".into(), "?".into(), "?".into())]));
    // When only the edit is staged
    assert_eq!(ok(&mut agent, "git.stage", w(json!({"paths": ["README.md"]}))).await, json!({"paths": 1}));
    // Then the index holds the edit and the new file is still untracked
    assert_eq!(rig.repo.git(&["status", "--porcelain"]), "M  README.md\n?? new.txt");
    // When it is committed on a new branch the phone switched to
    ok(&mut agent, "git.createRef", w(json!({"name": "feature/x", "checkout": true}))).await;
    let done = ok(&mut agent, "git.commit", w(json!({"message": "edit the readme\n\nwith a body"}))).await;
    // Then the branch has the commit the answer names, and main does not
    assert_eq!(rig.repo.git(&["rev-parse", "feature/x"]), done["sha"].as_str().unwrap());
    assert_eq!(rig.repo.git(&["log", "--format=%s", "main..feature/x"]), "edit the readme");
    let history = ok(&mut agent, "git.history", w(json!({"limit": 5}))).await;
    assert_eq!((history["commits"][0]["subject"].clone(), history["commits"][0]["body"].clone()), (json!("edit the readme"), json!("with a body")));
    // And the refs, the compare and the diff of a commit read the same history
    let refs = ok(&mut agent, "git.refs", w(json!({"kind": "branch"}))).await;
    assert_eq!((refs["current"].clone(), refs["refs"].as_array().unwrap().len()), (json!("feature/x"), 2));
    let compare = ok(&mut agent, "git.compare", w(json!({"base": "main", "head": "feature/x"}))).await;
    assert_eq!((compare["ahead"].clone(), compare["behind"].clone(), compare["files"][0]["path"].clone()), (json!(1), json!(0), json!("README.md")));
    let diff = ok(&mut agent, "git.diff", w(json!({"base": "main", "head": "feature/x"}))).await;
    assert!(diff["diff"].as_str().unwrap().contains("+edited"));
    // When the new file is staged and unstaged again, and the edit of a tracked file is discarded
    ok(&mut agent, "git.stage", w(json!({"paths": ["new.txt"]}))).await;
    assert_eq!(rig.repo.git(&["status", "--porcelain"]), "A  new.txt");
    ok(&mut agent, "git.unstage", w(json!({"paths": ["new.txt"]}))).await;
    rig.repo.write("README.md", "scribbles\n");
    let discarded = ok(&mut agent, "git.discard", w(json!({"paths": ["README.md"]}))).await;
    // Then the file is as committed and the untracked one is left alone
    assert_eq!(discarded["restored"], json!(1));
    assert_eq!((std::fs::read_to_string(rig.repo.path().join("README.md")).unwrap(), rig.repo.git(&["status", "--porcelain"])), ("temp repo\nedited\n".to_string(), "?? new.txt".to_string()));
    // And switching back to main restores the older file
    let moved = ok(&mut agent, "git.checkout", w(json!({"ref": "main"}))).await;
    assert_eq!((moved["branch"].clone(), std::fs::read_to_string(rig.repo.path().join("README.md")).unwrap()), (json!("main"), "temp repo\n".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_two_branches_that_change_one_line_when_merged_then_the_conflict_is_listed_resolved_by_each_means_and_continued_or_aborted() {
    // Given main and a branch that each committed their own line of one file
    let env = Env::process(&[]);
    let rig = rig(&env).await;
    rig.repo.write("c.txt", "base\n");
    rig.repo.commit_all("base");
    rig.repo.git(&["checkout", "--quiet", "-b", "topic"]);
    rig.repo.write("c.txt", "topic\n");
    rig.repo.commit_all("topic");
    rig.repo.git(&["checkout", "--quiet", "main"]);
    rig.repo.write("c.txt", "main\n");
    rig.repo.commit_all("main");
    let mut agent = rig.agent().await;
    // When the merge starts
    let stopped = ok(&mut agent, "git.merge", w(json!({"op": "start", "ref": "topic"}))).await;
    // Then it stopped on the file, the status says a merge waits, and the file holds the markers
    assert_eq!((stopped["ok"].clone(), stopped["conflicts"].clone(), stopped["operation"].clone()), (json!(false), json!(["c.txt"]), json!("merge")));
    let status = ok(&mut agent, "git.status", w(json!({}))).await;
    assert_eq!((status["operation"].clone(), status["files"][0]["conflicted"].clone()), (json!("merge"), json!(true)));
    assert!(std::fs::read_to_string(rig.repo.path().join("c.txt")).unwrap().contains("<<<<<<<"));
    // When it is resolved with the content of the phone and continued
    let resolved = ok(&mut agent, "git.conflictResolve", w(json!({"path": "c.txt", "resolution": "content", "content": "both\n"}))).await;
    assert_eq!(resolved["remaining"], json!([]));
    let done = ok(&mut agent, "git.merge", w(json!({"op": "continue"}))).await;
    // Then the merge commit has two parents and the file is the phone's
    assert_eq!(done["ok"], json!(true));
    assert_eq!((rig.repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"]).split(' ').count(), std::fs::read_to_string(rig.repo.path().join("c.txt")).unwrap()), (3, "both\n".to_string()));
    // When a second merge conflicts and is aborted, and a rebase conflicts and takes their side
    rig.repo.git(&["reset", "--quiet", "--hard", "HEAD~1"]);
    ok(&mut agent, "git.merge", w(json!({"op": "start", "ref": "topic"}))).await;
    ok(&mut agent, "git.merge", w(json!({"op": "abort"}))).await;
    assert_eq!((rig.repo.git(&["status", "--porcelain"]), std::fs::read_to_string(rig.repo.path().join("c.txt")).unwrap()), (String::new(), "main\n".to_string()));
    let rebase = ok(&mut agent, "git.rebase", w(json!({"op": "start", "ref": "topic"}))).await;
    assert_eq!((rebase["ok"].clone(), rebase["operation"].clone()), (json!(false), json!("rebase")));
    ok(&mut agent, "git.conflictResolve", w(json!({"path": "c.txt", "resolution": "theirs"}))).await;
    let finished = ok(&mut agent, "git.rebase", w(json!({"op": "continue"}))).await;
    // Then the rebased commit sits on topic and holds the side the phone picked (theirs in a rebase is the commit being replayed)
    assert_eq!(finished["ok"], json!(true));
    assert_eq!((rig.repo.git(&["log", "--format=%s", "-2"]), std::fs::read_to_string(rig.repo.path().join("c.txt")).unwrap()), ("main\ntopic".to_string(), "main\n".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_repository_with_a_remote_when_pushed_fetched_pulled_and_fast_forwarded_then_only_the_repositorys_own_remote_is_reached_and_option_like_values_are_refused() {
    // Given a bare remote that the repository has configured, and a second clone that pushes a commit to it
    let env = Env::process(&[]);
    let rig = rig(&env).await;
    let remote = tempfile::tempdir().unwrap();
    let bare = remote.path().join("origin.git");
    rig.repo.git(&["init", "--quiet", "--bare", "--initial-branch=main", bare.to_str().unwrap()]);
    rig.repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    let mut agent = rig.agent().await;
    let before = ok(&mut agent, "git.upstreamStatus", w(json!({}))).await;
    assert_eq!((before["has_upstream"].clone(), before["remotes"].clone()), (json!(false), json!(["origin"])));
    // When main is pushed with an upstream
    let pushed = ok(&mut agent, "git.push", w(json!({"set_upstream": true}))).await;
    // Then the remote has it and the branch tracks it
    assert_eq!((pushed["remote"].clone(), pushed["branch"].clone()), (json!("origin"), json!("main")));
    assert_eq!(rig.repo.git(&["--git-dir", bare.to_str().unwrap(), "rev-parse", "main"]), rig.repo.head());
    assert_eq!(ok(&mut agent, "git.upstreamStatus", w(json!({}))).await["upstream"], json!("origin/main"));
    // When another clone pushes a commit and the phone fetches, pulls and fast-forwards
    let other = remote.path().join("other");
    rig.repo.git(&["clone", "--quiet", bare.to_str().unwrap(), other.to_str().unwrap()]);
    let in_other = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(&other).env_clear().envs(rig.repo.git_env()).env("PATH", std::env::var_os("PATH").unwrap_or_default()).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    std::fs::write(other.join("remote.txt"), "from afar\n").unwrap();
    in_other(&["add", "--all"]);
    in_other(&["commit", "--quiet", "-m", "from afar"]);
    in_other(&["push", "--quiet", "origin", "main"]);
    ok(&mut agent, "git.fetch", w(json!({}))).await;
    let behind = ok(&mut agent, "git.upstreamStatus", w(json!({}))).await;
    assert_eq!((behind["ahead"].clone(), behind["behind"].clone()), (json!(0), json!(1)));
    let pulled = ok(&mut agent, "git.pull", w(json!({}))).await;
    // Then the working tree has the file and the pull says it moved
    assert_eq!((pulled["ok"].clone(), pulled["updated"].clone(), rig.repo.path().join("remote.txt").exists()), (json!(true), json!(true), true));
    // And a remote that is a URL, a path or an option, and a branch that is an option, are refused before git runs
    for params in [json!({"remote": bare.to_str().unwrap()}), json!({"remote": "ext::sh -c touch /tmp/pwned"}), json!({"remote": "--upload-pack=x"}), json!({"branch": "--force"}), json!({"remote": "nope"})] {
        assert!(["INVALID_ARGS", "NOT_FOUND"].contains(&refused(&mut agent, "git.push", w(params.clone())).await.as_str()), "{params}");
    }
    assert_eq!(refused(&mut agent, "git.checkout", w(json!({"ref": "--orphan=x"}))).await, "INVALID_ARGS");
    assert_eq!(refused(&mut agent, "git.stage", w(json!({"paths": ["../outside"]}))).await, "INVALID_ARGS");
    assert_eq!(refused(&mut agent, "git.stage", w(json!({"paths": [".git/config"]}))).await, "INVALID_ARGS");
    assert_eq!(refused(&mut agent, "git.status", json!({"workspace": "nobody"})).await, "NO_EXECUTOR");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_worktree_workspace_when_git_is_asked_then_it_answers_for_the_worktrees_branch_and_changes_not_the_repositorys() {
    // Given a worktree on its own branch with a file changed in it only
    let env = Env::process(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    ok(&mut agent, "worktree.create", json!({"workspace": "w1", "worktree": "wt", "branch": "feature/wt", "base": "HEAD"})).await;
    ok(&mut agent, "file.write", json!({"workspace": "wt", "path": "only-here.txt", "content": "x\n"})).await;
    // When status is asked of both, and the worktree commits
    let main = ok(&mut agent, "git.status", json!({"workspace": "w1"})).await;
    let wt = ok(&mut agent, "git.status", json!({"workspace": "wt"})).await;
    ok(&mut agent, "git.stage", json!({"workspace": "wt", "paths": ["."]})).await;
    ok(&mut agent, "git.commit", json!({"workspace": "wt", "message": "in the worktree"})).await;
    // Then each answers for its own branch and files, and only the worktree's branch has the commit
    assert_eq!((main["branch"].clone(), main["clean"].clone(), wt["branch"].clone(), files(&wt)), (json!("main"), json!(true), json!("feature/wt"), vec![("only-here.txt".into(), "?".into(), "?".into())]));
    assert_eq!(rig.repo.git(&["log", "--format=%s", "-1", "feature/wt"]), "in the worktree");
    assert_eq!(rig.repo.git(&["log", "--format=%s", "-1", "main"]), "first commit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_subscription_to_the_status_when_files_change_then_each_new_status_is_an_event_numbered_from_the_callers_last_and_the_first_is_sent_at_once() {
    // Given a clean repository and a phone subscribed after event 10
    let env = Env::process(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    let opened = ok(&mut agent, "git.subscribe", w(json!({"after_seq": 10, "interval_ms": 100}))).await;
    assert_eq!(opened, json!({"workspace": "w1", "seq": 10}));
    // Then the first event is the current status
    let first = tokio::time::timeout(WAIT, agent.event()).await.expect("the first status");
    assert_eq!((first.seq, first.kind.as_str(), first.data["status"]["clean"].clone()), (11, "git.status", json!(true)));
    // When a file appears and is staged
    rig.repo.write("late.txt", "x\n");
    let second = tokio::time::timeout(WAIT, agent.event()).await.expect("the status with the file");
    rig.repo.git(&["add", "late.txt"]);
    let third = tokio::time::timeout(WAIT, agent.event()).await.expect("the status with the file staged");
    // Then each event is numbered after the one before and shows the change
    assert_eq!((second.seq, files(&second.data["status"])), (12, vec![("late.txt".into(), "?".into(), "?".into())]));
    assert_eq!((third.seq, files(&third.data["status"])), (13, vec![("late.txt".into(), "A".into(), ".".into())]));
}

/// A fixture's exchange compared as recorded, on a service and executor of its own over the repository every exchange starts from: its
/// README edited and a file `new.txt` that is not tracked.
async fn replay(fixture: &agent_api_proto::testing::Fixture) {
    for exchange in &fixture.exchanges {
        let env = Env::process(&[]);
        let rig = rig(&env).await;
        rig.repo.write("README.md", "temp repo\nedited\n");
        rig.repo.write("new.txt", "new\n");
        let mut agent = rig.agent().await;
        for before in &exchange.before {
            let answered = agent.ask(before["method"].as_str().unwrap(), before["params"].clone()).await;
            assert!(answered.get("result").is_some(), "{}: the setup {before} failed: {answered}", exchange.name);
        }
        let mut answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
        answer["id"] = exchange.response["id"].clone();
        assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
        let declared: Vec<&str> = rca_group_git::SERVED.iter().find(|s| s.method.name() == fixture.method).map(|s| s.actions.to_vec()).unwrap();
        assert!(exchange.actions.iter().all(|a| declared.contains(&a.as_str())), "{}: {} calls {:?}, the table declares {declared:?}", fixture.method, exchange.name, exchange.actions);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_method_of_the_git_contract_has_recorded_exchanges_that_the_service_answers_as_recorded_over_the_real_executor() {
    // Given the recorded exchanges of the git crate and its methods
    let recorded = agent_api_git::testing::fixtures();
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    let mut methods: Vec<&str> = agent_api_git::METHODS.iter().map(|m| m.name()).collect();
    methods.sort_unstable();
    // Then each method has one fixture, and every exchange asks for its own method
    assert_eq!(named, methods);
    assert!(recorded.iter().all(|f| f.exchanges.iter().all(|e| e.request["method"] == f.method.as_str() && !e.name.is_empty())));
    // And each is answered as recorded by the service in front of the real executor
    for fixture in &recorded {
        replay(fixture).await;
    }
}
