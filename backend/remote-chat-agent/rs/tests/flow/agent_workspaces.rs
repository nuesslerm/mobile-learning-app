//! The workspaces an executor owns survive a restart of the service: a worktree made over the agent socket is routed again, with its place,
//! once the executor reattaches to a service started over the same state directory, without being created again.

use crate::harness::{service, Env, Options, SECRET};
use crate::mapper::Agent;
use exec_testkit::{RcaExec, TempRepo};
use serde_json::{json, Value};

async fn ok(agent: &mut Agent, method: &str, params: Value) -> Value {
    let answer = agent.ask(method, params).await;
    assert!(answer.get("result").is_some(), "{method} failed: {answer}");
    answer["result"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_worktree_workspace_made_through_the_service_when_the_service_restarts_over_the_same_state_dir_and_the_executor_reattaches_then_the_worktree_is_served_without_being_created_again() {
    // Given a real executor with a worktree base serving a repository, and a worktree workspace made through a service with a state directory
    let env = Env::new(&[]);
    let state = env.home.join("rca-state");
    std::fs::create_dir_all(&state).unwrap();
    let options = || Options { state_dir: Some(state.clone()), ..Options::default() };
    let repo = TempRepo::new();
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap().join("worktrees");
    std::fs::create_dir(&base).unwrap();
    let flags = ["--worktree-base", base.to_str().unwrap()];
    let first = service(&env, options());
    let port = first.listen().await;
    let exec = tokio::task::block_in_place(|| RcaExec::serve_with(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET, &flags));
    let token = first.mint("workspaces flow").await;
    let mut agent = Agent::open(port, &token).await;
    ok(&mut agent, "worktree.create", json!({"workspace": "w1", "worktree": "wt", "branch": "feature/x", "base": "HEAD"})).await;
    ok(&mut agent, "file.write", json!({"workspace": "wt", "path": "a.txt", "content": "one"})).await;
    // When the service and the executor stop and start again over the same state directory, and the worktree is not created again
    drop(agent);
    drop(exec);
    drop(first);
    let second = service(&env, options());
    let port = second.listen().await;
    let _exec = tokio::task::block_in_place(|| RcaExec::serve_with(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET, &flags));
    let mut agent = Agent::open(port, &token).await;
    // Then the worktree workspace answers in its own directory, and its place is the worktree's
    ok(&mut agent, "file.write", json!({"workspace": "wt", "path": "b.txt", "content": "two"})).await;
    let read = ok(&mut agent, "file.read", json!({"workspace": "wt", "path": "a.txt"})).await;
    assert_eq!(read["content"], json!("one"), "{read}");
    assert_eq!(std::fs::read_to_string(base.join("wt/b.txt")).unwrap(), "two");
    assert!(!repo.path().join("b.txt").exists());
    assert_eq!(repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 2);
}
