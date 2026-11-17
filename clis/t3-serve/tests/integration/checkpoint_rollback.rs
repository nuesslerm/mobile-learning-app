//! `checkpoint.rollback` through the chain: rca-exec records the workspace's files as each turn starts (a ref under `refs/rca/checkpoints/`),
//! and a rollback that restores files puts them back from the checkpoint of the first turn it drops, through `git_discard`. The test reads the
//! files back from the executor's directory. A rollback to a turn whose successor has no checkpoint is refused by rca by name and changes nothing.
use crate::chain::{hit, last_window, said};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::{Script, Turn};

fn cmd(kind: &str, extra: Value) -> Value {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut c = json!({"type": kind, "commandId": format!("c-{kind}-rb-{n}"), "threadId": "t1"});
    c.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
    c
}

fn write(id: &str, path: &str, content: &str) -> Turn {
    Turn::replaying(&[said("writing")]).then_calling(id, "write_file", json!({"path": path, "content": content, "create_dirs": true}))
}

/// Turn one writes notes/a.txt; turn two rewrites it, rewrites the committed README.md and adds notes/b.txt.
fn two_turns() -> Script {
    let second = write("k1", "notes/a.txt", "two\n").then_calling("k2", "write_file", json!({"path": "README.md", "content": "changed\n"})).then_calling("k3", "write_file", json!({"path": "notes/b.txt", "content": "new\n", "create_dirs": true})).then_line(&said("done"));
    Script::of(vec![write("k1", "notes/a.txt", "one\n").then_line(&said("done")), second])
}

fn read(chain: &Chain, path: &str) -> Option<String> {
    std::fs::read_to_string(chain.repo.path().join(path)).ok()
}

/// Both turns run, then the test makes the edits of the turns' aftermath a person or a tool would: a tracked file deleted, a written one removed.
fn after_two_turns(case: &mut exec_testkit::Case, chain: &Chain, phone: &mut Phone) -> (String, String) {
    chain.cli.script("claude", &two_turns());
    phone.seed(case, chain);
    let first = phone.message(case, chain, "t1", "c-m1", "make a note");
    let second = phone.message(case, chain, "t1", "c-m2", "change things");
    assert_eq!(second["runs"].as_array().unwrap().len(), 2, "{second}");
    (first["runs"][0]["id"].as_str().unwrap().to_string(), second["runs"][1]["id"].as_str().unwrap().to_string())
}

#[test]
fn a_rollback_puts_the_files_back_to_how_the_first_dropped_turn_found_them_and_a_rollback_that_drops_nothing_touches_none() {
    exec_testkit::case!(SUITE, |case| {
        // Given two turns: the second rewrote notes/a.txt and the committed README.md and added notes/b.txt, and the files were then deleted
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        let (first, _second) = after_two_turns(case, &chain, &mut phone);
        let committed = chain.repo.git(&["show", "HEAD:README.md"]);
        assert_eq!((read(&chain, "notes/a.txt").as_deref(), read(&chain, "README.md").as_deref(), read(&chain, "notes/b.txt").as_deref()), (Some("two\n"), Some("changed\n"), Some("new\n")));
        std::fs::remove_file(chain.repo.path().join("README.md")).unwrap();
        std::fs::remove_file(chain.repo.path().join("notes/a.txt")).unwrap();
        // And a checkpoint exists for each turn under rca's ref namespace, none of which moved HEAD or the branch
        let refs = chain.repo.git(&["for-each-ref", "--format=%(refname)", "refs/rca/checkpoints/"]);
        assert_eq!(refs.lines().count(), 2, "{refs}");
        assert_eq!(chain.repo.git(&["rev-list", "--count", "HEAD"]).trim(), "1");
        // When the phone rolls the thread back to the first run (T3's default restores the files)
        let rolled = phone.command(case, &chain, cmd("checkpoint.rollback", json!({"checkpointId": first, "scopeId": "s1"})));
        // Then it was accepted, rca restored from the checkpoint through git_discard, and the files read back as the second turn found them
        success(&rolled);
        let (agent, executor) = last_window(case);
        assert!(agent.iter().any(|(m, ok)| m == "thread.rollback" && *ok), "{agent:?}");
        assert!(hit(&executor, "tool", "git_discard").is_some_and(|h| h["ok"] == true), "{executor}");
        assert_eq!((read(&chain, "notes/a.txt").as_deref(), read(&chain, "README.md").map(|t| t.trim_end().to_string()), read(&chain, "notes/b.txt")), (Some("one\n"), Some(committed), None));
        assert_eq!(chain.repo.git(&["rev-list", "--count", "HEAD"]).trim(), "1");
        // When a file is added and the phone rolls back to the same run again, nothing is dropped
        std::fs::write(chain.repo.path().join("later.txt"), "kept\n").unwrap();
        success(&phone.command(case, &chain, cmd("checkpoint.rollback", json!({"checkpointId": first, "scopeId": "s1"}))));
        // Then no file was restored and the added one is still there
        let (agent, executor) = last_window(case);
        assert!(agent.iter().any(|(m, ok)| m == "thread.rollback" && *ok), "{agent:?}");
        assert!(hit(&executor, "tool", "git_discard").is_none(), "{executor}");
        assert_eq!(read(&chain, "later.txt").as_deref(), Some("kept\n"));
    });
}

#[test]
fn a_rollback_to_a_turn_with_no_checkpoint_after_it_is_refused_by_name_and_changes_neither_files_nor_thread() {
    exec_testkit::case!(SUITE, |case| {
        // Given two turns and the checkpoints gone (a turn that ran before checkpoints were taken has none)
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        let (first, _second) = after_two_turns(case, &chain, &mut phone);
        for name in chain.repo.git(&["for-each-ref", "--format=%(refname)", "refs/rca/checkpoints/"]).lines() {
            chain.repo.git(&["update-ref", "-d", name]);
        }
        // When the phone rolls back to the first run with the files restored
        let refused = phone.command(case, &chain, cmd("checkpoint.rollback", json!({"checkpointId": first, "scopeId": "s1"})));
        // Then rca was asked and refused by name, and nothing changed: the second turn's files stay and both runs remain
        let (agent, _) = last_window(case);
        assert!(agent.iter().any(|(m, ok)| m == "thread.rollback" && !*ok), "{agent:?}");
        assert!(failure(&refused)["message"].as_str().is_some_and(|m| m.contains("no file checkpoint")), "{refused}");
        assert_eq!((read(&chain, "notes/a.txt").as_deref(), read(&chain, "notes/b.txt").as_deref()), (Some("two\n"), Some("new\n")));
        let mut rca = chain.direct();
        let runs = rca.events("t1", "turn.started").len();
        assert_eq!(runs, 2);
        assert!(rca.events("t1", "thread.rolled_back").is_empty());
        // When the phone asks for the same rollback without the files
        success(&phone.command(case, &chain, cmd("checkpoint.rollback", json!({"checkpointId": first, "scopeId": "s1", "restoreFiles": false}))));
        // Then it is accepted, the thread drops the turn and the files stay as they were
        assert_eq!(rca.events("t1", "thread.rolled_back").len(), 1);
        assert_eq!(read(&chain, "notes/b.txt").as_deref(), Some("new\n"));
    });
}

#[test]
fn a_checkpoint_never_touches_the_index_the_branch_or_the_files_it_records() {
    exec_testkit::case!(SUITE, |case| {
        // Given a turn that wrote one file, run in a repository with a staged change of the person's own
        let chain = Chain::start();
        std::fs::write(chain.repo.path().join("staged.txt"), "mine\n").unwrap();
        chain.repo.git(&["add", "staged.txt"]);
        chain.cli.script("claude", &Script::of(vec![write("k1", "notes/a.txt", "one\n").then_line(&said("done"))]));
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        phone.message(case, &chain, "t1", "c-m1", "make a note");
        // Then the turn's start was checkpointed, and the index still holds only the person's staged file
        assert_eq!(chain.repo.git(&["for-each-ref", "--format=%(refname)", "refs/rca/checkpoints/"]).lines().count(), 1);
        assert_eq!(chain.repo.git(&["diff", "--cached", "--name-only"]).trim(), "staged.txt");
        assert_eq!(chain.repo.git(&["status", "--porcelain"]).lines().map(str::to_string).collect::<Vec<_>>(), ["A  staged.txt", "?? notes/"]);
        // And the checkpoint holds the staged file as it was at the start, on a commit whose parent is HEAD
        let name = chain.repo.git(&["for-each-ref", "--format=%(refname)", "refs/rca/checkpoints/"]).trim().to_string();
        assert_eq!(chain.repo.git(&["show", &format!("{name}:staged.txt")]), "mine");
        assert_eq!(chain.repo.git(&["rev-parse", &format!("{name}^")]).trim(), chain.repo.git(&["rev-parse", "HEAD"]).trim());
    });
}
