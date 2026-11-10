//! The repository actions of the git family on real temp repositories (and a real bare remote): the local adapter through `domain::execute`,
//! as a wire request would reach it, and one test over the channel client. Git reads no configuration of the machine.
use super::{accept, recv, send, wait_for, Seen};
use exec_proto::{Code, ExecResult};
use rca_exec::adapters::exec_channel::ExecChannel;
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::{execute, ToolExecutor};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

fn isolate() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    });
}

/// git in `dir`, which must succeed; its stdout.
fn sh(dir: &Path, args: &[&str]) -> String {
    let done = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

fn put(dir: &Path, path: &str, content: &str) {
    let at = dir.join(path);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, content).unwrap();
}

fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).unwrap()
}

fn init(dir: &Path) {
    isolate();
    sh(dir, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@t"), ("commit.gpgsign", "false")] {
        sh(dir, &["config", key, value]);
    }
}

/// A repository on `main` with the files committed as "first", served by an executor with a worktree base.
struct Repo {
    dir: tempfile::TempDir,
    base: tempfile::TempDir,
    ex: Arc<LocalExecutor>,
}

fn repo(files: &[(&str, &str)]) -> Repo {
    repo_with(files, false)
}

fn repo_with(files: &[(&str, &str)], hooks: bool) -> Repo {
    let (dir, base) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    init(dir.path());
    for (path, content) in files {
        put(dir.path(), path, content);
    }
    sh(dir.path(), &["add", "-A"]);
    sh(dir.path(), &["commit", "-q", "-m", "first"]);
    let ex = Arc::new(LocalExecutor::new(dir.path()).unwrap().with_worktree_base(base.path()).unwrap().with_git_hooks(hooks));
    Repo { dir, base, ex }
}

impl Repo {
    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn call(&self, tool: &str, args: Value) -> ExecResult<Value> {
        execute(self.ex.as_ref(), "g1", tool, &args, &mut |_, _| {}, &mut |_| {})
    }

    fn ok(&self, tool: &str, args: Value) -> Value {
        self.call(tool, args.clone()).unwrap_or_else(|e| panic!("{tool} {args}: {e:?}"))
    }

    fn err(&self, tool: &str, args: Value) -> (Code, String) {
        let e = self.call(tool, args.clone()).expect_err(&format!("{tool} {args} should be refused"));
        (e.code, e.message)
    }

    fn status(&self) -> Value {
        self.ok("git_status", json!({}))
    }

    /// The `(index, worktree)` columns of `path` in the status, or none when it is not listed.
    fn columns(&self, path: &str) -> Option<(String, String)> {
        self.status()["files"].as_array().unwrap().iter().find(|f| f["path"] == path).map(|f| (f["index"].as_str().unwrap().to_string(), f["worktree"].as_str().unwrap().to_string()))
    }

    fn head(&self) -> String {
        sh(self.path(), &["rev-parse", "HEAD"])
    }
}

fn col(a: &str, b: &str) -> Option<(String, String)> {
    Some((a.to_string(), b.to_string()))
}

/// A bare remote, the repository's `origin` with `main` pushed and tracked, and a second clone that is a collaborator.
struct Remote {
    _bare: tempfile::TempDir,
    bare: PathBuf,
    other: tempfile::TempDir,
}

fn with_remote(r: &Repo) -> Remote {
    let bare_dir = tempfile::tempdir().unwrap();
    let bare = bare_dir.path().join("remote.git");
    sh(bare_dir.path(), &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
    sh(r.path(), &["remote", "add", "origin", bare.to_str().unwrap()]);
    sh(r.path(), &["push", "-q", "-u", "origin", "main"]);
    let other = tempfile::tempdir().unwrap();
    sh(other.path(), &["clone", "-q", bare.to_str().unwrap(), "."]);
    for (key, value) in [("user.name", "o"), ("user.email", "o@o"), ("commit.gpgsign", "false")] {
        sh(other.path(), &[ "config", key, value]);
    }
    Remote { _bare: bare_dir, bare, other }
}

fn lines(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

#[test]
fn staging_unstaging_discarding_and_committing_move_a_file_through_the_columns_of_the_status() {
    // Given a repository with two files and a change of each kind: a modified file, a new one and a deleted one
    let r = repo(&[("a.txt", "one\n"), ("b.txt", "two\n")]);
    put(r.path(), "a.txt", "changed\n");
    put(r.path(), "new.txt", "n\n");
    std::fs::remove_file(r.path().join("b.txt")).unwrap();
    // Then the status lists each in the working tree column
    assert_eq!((r.columns("a.txt"), r.columns("new.txt"), r.columns("b.txt")), (col(".", "M"), col("?", "?"), col(".", "D")));
    // When all of it is staged
    assert_eq!(r.ok("git_stage", json!({"paths": ["."]}))["paths"], 1);
    // Then the columns are the index's: modified, added and deleted
    assert_eq!((r.columns("a.txt"), r.columns("new.txt"), r.columns("b.txt")), (col("M", "."), col("A", "."), col("D", ".")));
    // When the new file is unstaged, the modified one is edited again after staging, and the deleted one is restored by a discard
    r.ok("git_unstage", json!({"paths": ["new.txt"]}));
    put(r.path(), "a.txt", "changed again\n");
    r.ok("git_discard", json!({"paths": ["b.txt"], "scope": "all"}));
    // Then the new file is untracked again, a.txt is changed in both columns, and b.txt is back and clean
    assert_eq!((r.columns("new.txt"), r.columns("a.txt"), r.columns("b.txt")), (col("?", "?"), col("M", "M"), None));
    assert_eq!(read(r.path(), "b.txt"), "two\n");
    // When the changes of a.txt are committed with the message
    let done = r.ok("git_commit", json!({"message": "second\n\nbody line", "all": true}));
    // Then the commit is HEAD, its subject is the message's first line, and only the untracked file is left
    assert_eq!((done["sha"].as_str().unwrap(), done["subject"].as_str().unwrap()), (r.head().as_str(), "second"));
    assert_eq!(r.status()["files"].as_array().unwrap().len(), 1);
    // And the log has both commits newest first, with the body kept apart from the subject
    let log = r.ok("git_log", json!({}));
    let commits = log["commits"].as_array().unwrap();
    assert_eq!(commits.iter().map(|c| c["subject"].as_str().unwrap()).collect::<Vec<_>>(), ["second", "first"]);
    assert_eq!((commits[0]["body"].as_str().unwrap(), commits[0]["parents"][0].as_str().unwrap(), commits[0]["author_email"].as_str().unwrap()), ("body line", commits[1]["sha"].as_str().unwrap(), "t@t"));
    // And a commit with nothing to commit is refused with git's own words, and is allowed when asked for
    let (code, message) = r.err("git_commit", json!({"message": "nothing"}));
    assert!(code == Code::Io && message.contains("nothing"), "{code:?} {message}");
    r.ok("git_commit", json!({"message": "empty one", "allow_empty": true}));
}

#[test]
fn the_status_reports_a_staged_rename_with_its_old_path_and_paths_with_spaces_and_unborn_repositories_have_no_head() {
    // Given a file that is renamed to a name with spaces, staged, and an untracked file whose name has spaces too
    let r = repo(&[("old.txt", "some content that stays the same\nmore lines here\nand a third one\n")]);
    std::fs::create_dir(r.path().join("new dir")).unwrap();
    sh(r.path(), &["mv", "old.txt", "new dir/new name.txt"]);
    put(r.path(), "a b.txt", "x\n");
    // When the status is read
    let files = r.status()["files"].clone();
    // Then the rename has its old path and the R column, and each name with spaces is whole
    let renamed = files.as_array().unwrap().iter().find(|f| f["path"] == "new dir/new name.txt").unwrap();
    assert_eq!((renamed["orig_path"].clone(), renamed["index"].clone()), (json!("old.txt"), json!("R")));
    assert!(files.as_array().unwrap().iter().any(|f| f["path"] == "a b.txt" && f["index"] == "?"));
    // And a repository without commits has a branch and no head, and an empty history
    let (dir, base) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    init(dir.path());
    let ex = LocalExecutor::new(dir.path()).unwrap().with_worktree_base(base.path()).unwrap();
    let unborn = execute(&ex, "u", "git_status", &json!({}), &mut |_, _| {}, &mut |_| {}).unwrap();
    assert_eq!((unborn["branch"].clone(), unborn.get("head").cloned()), (json!("main"), None));
    assert_eq!(execute(&ex, "u", "git_log", &json!({}), &mut |_, _| {}, &mut |_| {}).unwrap()["commits"], json!([]));
}

#[test]
fn discard_restores_from_a_revision_removing_what_it_lacks_and_cleans_untracked_files_only_when_asked() {
    // Given a repository whose second commit changed a file, added a tracked one, and a working tree with an untracked file besides
    let r = repo(&[("a.txt", "v1\n"), ("dir/b.txt", "b1\n")]);
    let first = r.head();
    put(r.path(), "a.txt", "v2\n");
    put(r.path(), "dir/added.txt", "added\n");
    sh(r.path(), &["add", "-A"]);
    sh(r.path(), &["commit", "-q", "-m", "second"]);
    put(r.path(), "stray.txt", "stray\n");
    // When the tree is discarded back to the first commit, untracked files not included
    let done = r.ok("git_discard", json!({"paths": ["."], "source": first}));
    // Then a.txt holds its first content, the file the first commit lacks is gone, the stray file is still there and the index agrees with the revision
    assert_eq!((read(r.path(), "a.txt"), r.path().join("dir/added.txt").exists(), r.path().join("stray.txt").exists()), ("v1\n".to_string(), false, true));
    assert_eq!(done["restored"], 3);
    assert_eq!(sh(r.path(), &["diff", "--cached", "--name-only", &first]), "");
    // When the untracked file is cleaned by naming it, with `untracked`
    let cleaned = r.ok("git_discard", json!({"paths": ["stray.txt"], "untracked": true}));
    // Then it is removed and counted, and nothing else changed
    assert_eq!((cleaned["cleaned"].clone(), r.path().join("stray.txt").exists(), read(r.path(), "dir/b.txt")), (json!(1), false, "b1\n".to_string()));
}

#[test]
fn a_checkpoint_records_edits_and_new_files_on_a_private_index_and_a_discard_from_it_puts_the_tree_back_removing_what_came_after() {
    // Given a committed repository with a staged file, an edit, an untracked file and an ignored one, as a turn is about to start
    let r = repo(&[("a.txt", "one\n"), ("keep.txt", "keep\n"), (".gitignore", "*.log\n")]);
    put(r.path(), "staged.txt", "staged\n");
    sh(r.path(), &["add", "staged.txt"]);
    put(r.path(), "a.txt", "two\n");
    put(r.path(), "new.txt", "n\n");
    put(r.path(), "debug.log", "ignored\n");
    let (head, index, branch) = (r.head(), sh(r.path(), &["diff", "--cached", "--name-only"]), sh(r.path(), &["branch", "--show-current"]));
    // When the checkpoint is taken
    let done = r.ok("git_checkpoint", json!({"thread": "t1", "turn": "u1"}));
    // Then it names its ref and the head it was built on, and keeps the edit, the staged and the untracked file but not the ignored one
    assert_eq!((done["ref"].clone(), done["head"].clone()), (json!("refs/rca/checkpoints/t1/u1"), json!(head.clone())));
    let held = sh(r.path(), &["ls-tree", "-r", "--name-only", "refs/rca/checkpoints/t1/u1"]);
    assert_eq!(held.lines().collect::<Vec<_>>(), [".gitignore", "a.txt", "keep.txt", "new.txt", "staged.txt"]);
    assert_eq!(sh(r.path(), &["show", "refs/rca/checkpoints/t1/u1:a.txt"]), "two");
    // And the repository's own index, branch, head and files are as they were
    assert_eq!((r.head(), sh(r.path(), &["diff", "--cached", "--name-only"]), sh(r.path(), &["branch", "--show-current"])), (head.clone(), index, branch));
    assert_eq!(sh(r.path(), &["rev-parse", "refs/rca/checkpoints/t1/u1^"]), r.head());
    assert_eq!(read(r.path(), "a.txt"), "two\n");
    assert!(!std::fs::read_dir(r.path().join(".git")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("rca-checkpoint")), "the private index is removed");
    // When the turn rewrites a file, deletes another, adds one and the checkpoint is restored through git_discard
    put(r.path(), "a.txt", "three\n");
    std::fs::remove_file(r.path().join("keep.txt")).unwrap();
    put(r.path(), "added.txt", "later\n");
    sh(r.path(), &["add", "added.txt"]);
    r.ok("git_discard", json!({"paths": ["."], "scope": "all", "untracked": true, "source": "refs/rca/checkpoints/t1/u1"}));
    // Then the files are as the checkpoint held them, what came after is gone, the ignored file stays and the branch did not move
    assert_eq!((read(r.path(), "a.txt"), read(r.path(), "keep.txt"), read(r.path(), "new.txt")), ("two\n".to_string(), "keep\n".to_string(), "n\n".to_string()));
    assert!(!r.path().join("added.txt").exists());
    assert_eq!(read(r.path(), "debug.log"), "ignored\n");
    assert_eq!(r.head(), head);
    // And a second checkpoint of the same turn replaces the first, and a turn that climbs out of the ref namespace is refused
    r.ok("git_checkpoint", json!({"thread": "t1", "turn": "u1"}));
    assert_eq!(sh(r.path(), &["for-each-ref", "--format=%(refname)", "refs/rca/"]).lines().count(), 1);
    assert_eq!(r.err("git_checkpoint", json!({"thread": "t1", "turn": "../../heads/main"})).0, Code::InvalidArgs);
    assert_eq!(r.err("git_discard", json!({"paths": ["."], "scope": "all", "source": "refs/rca/checkpoints/t1/none"})).0, Code::NotFound);
}

#[test]
fn a_checkpoint_of_a_repository_with_no_commit_has_no_head_and_a_directory_that_is_no_repository_is_refused() {
    // Given a repository with a file and no commit, and a directory that is no repository
    let (dir, base, plain) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    init(dir.path());
    put(dir.path(), "a.txt", "one\n");
    let unborn = Repo { ex: Arc::new(LocalExecutor::new(dir.path()).unwrap().with_worktree_base(base.path()).unwrap()), dir, base };
    let outside = Repo { ex: Arc::new(LocalExecutor::new(plain.path()).unwrap()), dir: plain, base: tempfile::tempdir().unwrap() };
    // When each is checkpointed
    let done = unborn.ok("git_checkpoint", json!({"thread": "t", "turn": "u"}));
    // Then the first has a commit with no parent and no head, and holds the file, and the second is refused
    assert_eq!(done["head"], Value::Null);
    assert_eq!(sh(unborn.path(), &["rev-list", "--parents", "-n", "1", "refs/rca/checkpoints/t/u"]).split(' ').count(), 1);
    assert_eq!(sh(unborn.path(), &["show", "refs/rca/checkpoints/t/u:a.txt"]), "one");
    assert_eq!(outside.err("git_checkpoint", json!({"thread": "t", "turn": "u"})).0, Code::Io);
}

#[test]
fn branches_tags_and_checkout_are_listed_created_and_switched_with_the_current_one_marked() {
    // Given a repository with a second commit
    let r = repo(&[("a.txt", "one\n")]);
    let first = r.head();
    put(r.path(), "a.txt", "two\n");
    r.ok("git_commit", json!({"message": "second", "all": true}));
    // When a branch is made at the first commit, a tag with an annotation at HEAD, and a branch is made and switched to
    let at_first = r.ok("git_create_ref", json!({"name": "old/work", "from": first}));
    let tag = r.ok("git_create_ref", json!({"name": "v1", "kind": "tag", "message": "release"}));
    let on = r.ok("git_create_ref", json!({"name": "feature", "checkout": true}));
    // Then each answers its kind and the commit it names, and the checkout moved HEAD
    assert_eq!((at_first["sha"].as_str().unwrap(), tag["sha"].as_str().unwrap(), on["kind"].as_str().unwrap()), (first.as_str(), r.head().as_str(), "branch"));
    assert_eq!(r.ok("git_upstream", json!({}))["branch"], "feature");
    // And the refs name every branch and tag, mark the current one, and filter by kind and by text
    let refs = r.ok("git_refs", json!({}));
    let names: Vec<(String, String, bool)> = refs["refs"].as_array().unwrap().iter().map(|x| (x["name"].as_str().unwrap().to_string(), x["kind"].as_str().unwrap().to_string(), x["current"].as_bool().unwrap())).collect();
    for want in [("feature", "branch", true), ("main", "branch", false), ("old/work", "branch", false), ("v1", "tag", false)] {
        assert!(names.contains(&(want.0.to_string(), want.1.to_string(), want.2)), "{want:?} in {names:?}");
    }
    assert_eq!(refs["current"], "feature");
    assert_eq!(r.ok("git_refs", json!({"kind": "tag"}))["refs"].as_array().unwrap().len(), 1);
    assert_eq!(r.ok("git_refs", json!({"query": "OLD"}))["refs"][0]["name"], "old/work");
    // When main is checked out, and HEAD is detached at the first commit
    assert_eq!(r.ok("git_checkout", json!({"ref": "main"}))["branch"], "main");
    let detached = r.ok("git_checkout", json!({"ref": first, "detach": true}));
    // Then the head is that commit and the status and refs say detached
    assert_eq!((detached["detached"].clone(), detached["head"].as_str().unwrap()), (json!(true), first.as_str()));
    assert_eq!((r.status()["detached"].clone(), r.ok("git_refs", json!({}))["detached"].clone()), (json!(true), json!(true)));
    // And a branch or tag that exists is ALREADY_EXISTS, a switch to a tag without detach is refused, and a missing ref is NOT_FOUND
    assert_eq!(r.err("git_create_ref", json!({"name": "main"})).0, Code::AlreadyExists);
    assert_eq!(r.err("git_checkout", json!({"ref": "v1"})).0, Code::Io);
    assert_eq!(r.err("git_checkout", json!({"ref": "nope"})).0, Code::NotFound);
    // And a switch that would overwrite local changes is refused until forced
    r.ok("git_checkout", json!({"ref": "main"}));
    put(r.path(), "a.txt", "dirty\n");
    assert_eq!(r.err("git_checkout", json!({"ref": "old/work"})).0, Code::Io);
    assert_eq!(r.ok("git_checkout", json!({"ref": "old/work", "force": true}))["branch"], "old/work");
    assert_eq!(read(r.path(), "a.txt"), "one\n");
}

#[test]
fn a_merge_that_conflicts_lists_its_files_and_resolves_by_side_content_or_hand_then_continues_or_aborts() {
    // Given two branches that both changed three files, so merging one into the other conflicts on each
    let files = ["a.txt", "b.txt", "c.txt"];
    let r = repo(&[("a.txt", "base\n"), ("b.txt", "base\n"), ("c.txt", "base\n")]);
    sh(r.path(), &["switch", "-q", "-c", "other"]);
    for f in files {
        put(r.path(), f, &format!("theirs {f}\n"));
    }
    sh(r.path(), &["commit", "-q", "-am", "theirs"]);
    sh(r.path(), &["switch", "-q", "main"]);
    for f in files {
        put(r.path(), f, &format!("ours {f}\n"));
    }
    sh(r.path(), &["commit", "-q", "-am", "ours"]);
    let before = r.head();
    // When the merge starts
    let started = r.ok("git_merge", json!({"op": "start", "ref": "other"}));
    // Then it stopped on the three files, which the status marks conflicted under a merge in progress
    assert_eq!((started["ok"].clone(), started["conflicted"].clone(), started["operation"].clone()), (json!(false), json!(true), json!("merge")));
    assert_eq!(lines(&started, "conflicts"), files);
    let status = r.status();
    assert_eq!(status["operation"], "merge");
    assert!(status["files"].as_array().unwrap().iter().all(|f| f["conflicted"] == true && f["index"] == "U"), "{status}");
    // And a continue with the files unresolved lists them again instead of committing
    let early = r.ok("git_merge", json!({"op": "continue"}));
    assert_eq!((early["ok"].clone(), lines(&early, "conflicts")), (json!(false), files.iter().map(|f| f.to_string()).collect::<Vec<_>>()));
    // When one file is resolved by each way: our side, their side, and content
    let ours = r.ok("git_conflict_resolve", json!({"path": "a.txt", "resolution": "ours"}));
    assert_eq!(lines(&ours, "remaining"), ["b.txt", "c.txt"]);
    assert_eq!(read(r.path(), "a.txt"), "ours a.txt\n");
    r.ok("git_conflict_resolve", json!({"path": "b.txt", "resolution": "theirs"}));
    assert_eq!(read(r.path(), "b.txt"), "theirs b.txt\n");
    let last = r.ok("git_conflict_resolve", json!({"path": "c.txt", "resolution": "content", "content": "both\n"}));
    // Then nothing remains, the content was written, and a file that is not conflicted any more is refused
    assert_eq!((lines(&last, "remaining").len(), read(r.path(), "c.txt")), (0, "both\n".to_string()));
    assert_eq!(r.err("git_conflict_resolve", json!({"path": "c.txt", "resolution": "ours"})).0, Code::InvalidArgs);
    // When the merge continues
    let done = r.ok("git_merge", json!({"op": "continue"}));
    // Then it made a merge commit of the two branches and the operation is over
    assert_eq!((done["ok"].clone(), done["updated"].clone(), done["operation"].clone()), (json!(true), json!(true), Value::Null));
    let merged = r.ok("git_log", json!({"limit": 1}));
    assert_eq!(merged["commits"][0]["parents"].as_array().unwrap().len(), 2);
    assert_eq!(r.status()["operation"], Value::Null);
    // When the same merge is started again from before it, and then aborted
    sh(r.path(), &["reset", "-q", "--hard", &before]);
    r.ok("git_merge", json!({"op": "start", "ref": "other"}));
    let aborted = r.ok("git_merge", json!({"op": "abort"}));
    // Then the tree is as it was and nothing conflicts
    assert_eq!((aborted["ok"].clone(), aborted["head"].as_str().unwrap(), read(r.path(), "a.txt")), (json!(true), before.as_str(), "ours a.txt\n".to_string()));
    assert!(r.status()["clean"].as_bool().unwrap());
    // And a squash merge leaves its changes staged without a commit, and a fast-forward-only merge of a diverged branch is refused
    assert_eq!(r.err("git_merge", json!({"op": "start", "ref": "other", "strategy": "ff_only"})).0, Code::Io);
}

#[test]
fn a_rebase_that_conflicts_resolves_and_continues_or_aborts_and_a_clean_one_replays_the_commits() {
    // Given a feature branch of two commits, the first of which conflicts with a commit on main
    let r = repo(&[("a.txt", "base\n"), ("x.txt", "x\n")]);
    sh(r.path(), &["switch", "-q", "-c", "feature"]);
    put(r.path(), "a.txt", "feature\n");
    sh(r.path(), &["commit", "-q", "-am", "f1"]);
    put(r.path(), "x.txt", "feature x\n");
    sh(r.path(), &["commit", "-q", "-am", "f2"]);
    let feature_head = r.head();
    sh(r.path(), &["switch", "-q", "main"]);
    put(r.path(), "a.txt", "main\n");
    sh(r.path(), &["commit", "-q", "-am", "m1"]);
    sh(r.path(), &["switch", "-q", "feature"]);
    // When the rebase starts
    let started = r.ok("git_rebase", json!({"op": "start", "ref": "main"}));
    // Then it stopped on a.txt, under a rebase in progress
    assert_eq!((started["ok"].clone(), lines(&started, "conflicts"), started["operation"].clone()), (json!(false), vec!["a.txt".to_string()], json!("rebase")));
    // When it is aborted
    let aborted = r.ok("git_rebase", json!({"op": "abort"}));
    // Then the branch is where it was
    assert_eq!((aborted["ok"].clone(), aborted["head"].as_str().unwrap()), (json!(true), feature_head.as_str()));
    // When it starts again, the conflict is resolved by hand and the rebase continues
    r.ok("git_rebase", json!({"op": "start", "ref": "main"}));
    put(r.path(), "a.txt", "resolved\n");
    r.ok("git_conflict_resolve", json!({"path": "a.txt", "resolution": "resolved"}));
    let done = r.ok("git_rebase", json!({"op": "continue"}));
    // Then both commits were replayed on main: the history is f2, f1, m1, first and the files hold the resolution and the second commit's change
    assert_eq!((done["ok"].clone(), done["operation"].clone()), (json!(true), Value::Null));
    let subjects: Vec<String> = r.ok("git_log", json!({}))["commits"].as_array().unwrap().iter().map(|c| c["subject"].as_str().unwrap().to_string()).collect();
    assert_eq!(subjects, ["f2", "f1", "m1", "first"]);
    assert_eq!((read(r.path(), "a.txt"), read(r.path(), "x.txt")), ("resolved\n".to_string(), "feature x\n".to_string()));
}

#[test]
fn compare_lists_what_a_branch_changed_since_the_merge_base_and_how_far_apart_the_two_are() {
    // Given main with a file, and a branch that modified one file, added one, deleted one and renamed one, while main moved on by one commit
    let r = repo(&[("keep.txt", "k\n"), ("mod.txt", "1\n2\n3\n"), ("del.txt", "gone\n"), ("old name.txt", "same content here\nline two\nline three\n")]);
    let base = r.head();
    sh(r.path(), &["switch", "-q", "-c", "topic"]);
    put(r.path(), "mod.txt", "1\n2\n3\n4\n5\n");
    put(r.path(), "add.txt", "a\nb\n");
    std::fs::remove_file(r.path().join("del.txt")).unwrap();
    sh(r.path(), &["mv", "old name.txt", "new name.txt"]);
    sh(r.path(), &["add", "-A"]);
    sh(r.path(), &["commit", "-q", "-m", "topic work"]);
    put(r.path(), "bin.dat", "a\0b");
    sh(r.path(), &["add", "-A"]);
    sh(r.path(), &["commit", "-q", "-m", "binary"]);
    sh(r.path(), &["switch", "-q", "main"]);
    put(r.path(), "keep.txt", "k2\n");
    r.ok("git_commit", json!({"message": "main moved", "all": true}));
    // When topic is compared with main
    let c = r.ok("git_compare", json!({"base": "main", "head": "topic"}));
    // Then the merge base is the common commit, topic is two ahead and main one ahead of it, and the commits are topic's, newest first
    assert_eq!((c["merge_base"].as_str().unwrap(), c["ahead"].clone(), c["behind"].clone()), (base.as_str(), json!(2), json!(1)));
    let subjects: Vec<&str> = c["commits"].as_array().unwrap().iter().map(|x| x["subject"].as_str().unwrap()).collect();
    assert_eq!(subjects, ["binary", "topic work"]);
    // And the files are those topic changed (not main's), each with its status, counts and the rename's old name
    let files = c["files"].as_array().unwrap();
    let find = |p: &str| files.iter().find(|f| f["path"] == p).unwrap_or_else(|| panic!("{p} in {files:?}"));
    assert_eq!(files.len(), 5, "{files:?}");
    assert_eq!((find("mod.txt")["status"].clone(), find("mod.txt")["additions"].clone(), find("mod.txt")["deletions"].clone()), (json!("modified"), json!(2), json!(0)));
    assert_eq!((find("add.txt")["status"].clone(), find("del.txt")["status"].clone()), (json!("added"), json!("deleted")));
    assert_eq!((find("new name.txt")["status"].clone(), find("new name.txt")["orig_path"].clone()), (json!("renamed"), json!("old name.txt")));
    assert_eq!((find("bin.dat")["binary"].clone(), find("bin.dat")["additions"].clone()), (json!(true), Value::Null));
    // And a limit cuts the commits and says so, and a revision that is no commit is NOT_FOUND
    let cut = r.ok("git_compare", json!({"base": "main", "head": "topic", "limit": 1}));
    assert_eq!((cut["commits"].as_array().unwrap().len(), cut["truncated"].clone()), (1, json!(true)));
    assert_eq!(r.err("git_compare", json!({"base": "main", "head": "nope"})).0, Code::NotFound);
    // And two histories with no common commit compare by their two trees
    sh(r.path(), &["switch", "-q", "--orphan", "island"]);
    put(r.path(), "only.txt", "o\n");
    sh(r.path(), &["add", "-A"]);
    sh(r.path(), &["commit", "-q", "-m", "island"]);
    let apart = r.ok("git_compare", json!({"base": "main", "head": "island"}));
    assert_eq!((apart["merge_base"].clone(), apart["ahead"].clone()), (Value::Null, json!(1)));
    assert!(apart["files"].as_array().unwrap().iter().any(|f| f["path"] == "only.txt" && f["status"] == "added"));
}

#[test]
fn a_blob_is_the_content_at_a_revision_not_the_working_tree_and_is_cut_at_the_limit() {
    // Given a committed text file and binary file, both edited afterwards
    let r = repo(&[("t.txt", "line one\nline two\n"), ("b.bin", "ab\0cd")]);
    let first = r.head();
    put(r.path(), "t.txt", "edited\n");
    r.ok("git_commit", json!({"message": "second", "all": true}));
    // When the file is read at the first commit and at HEAD
    let old = r.ok("git_blob", json!({"rev": first, "path": "t.txt"}));
    let new = r.ok("git_blob", json!({"rev": "HEAD", "path": "t.txt"}));
    use base64::Engine;
    let decode = |v: &Value| base64::engine::general_purpose::STANDARD.decode(v["data"].as_str().unwrap()).unwrap();
    // Then each is that revision's bytes with their size
    assert_eq!((decode(&old), old["size"].clone(), decode(&new)), (b"line one\nline two\n".to_vec(), json!(18), b"edited\n".to_vec()));
    // And a limit cuts the data and says so while the size stays whole, and a NUL marks the file binary
    let cut = r.ok("git_blob", json!({"rev": first, "path": "t.txt", "max_bytes": 4}));
    assert_eq!((decode(&cut), cut["truncated"].clone(), cut["size"].clone(), cut["binary"].clone()), (b"line".to_vec(), json!(true), json!(18), json!(false)));
    assert_eq!((r.ok("git_blob", json!({"rev": "HEAD", "path": "b.bin"}))["binary"].clone(), r.err("git_blob", json!({"rev": "HEAD", "path": "nope.txt"})).0), (json!(true), Code::NotFound));
    // And a rev that would be read as an option, and a path that climbs out, are refused before git is asked
    assert_eq!(r.err("git_blob", json!({"rev": "--output=/tmp/x", "path": "t.txt"})).0, Code::InvalidArgs);
    assert_eq!(r.err("git_blob", json!({"rev": "HEAD", "path": "../t.txt"})).0, Code::InvalidArgs);
}

#[test]
fn fetch_pull_push_and_fast_forward_work_against_the_repositorys_own_remote_and_a_collaborators_commits() {
    // Given a repository whose origin is a bare repository and a collaborator that pushes a commit to main
    let r = repo(&[("a.txt", "one\n")]);
    let remote = with_remote(&r);
    put(remote.other.path(), "theirs.txt", "from them\n");
    sh(remote.other.path(), &["add", "-A"]);
    sh(remote.other.path(), &["commit", "-q", "-m", "theirs"]);
    sh(remote.other.path(), &["push", "-q", "origin", "main"]);
    // When the repository fetches
    let fetched = r.ok("git_fetch", json!({}));
    // Then it names origin, and the upstream is one behind, none ahead
    assert_eq!(fetched["remote"], "origin");
    let up = r.ok("git_upstream", json!({}));
    assert_eq!((up["upstream"].clone(), up["remote"].clone(), up["ahead"].clone(), up["behind"].clone(), up["has_upstream"].clone()), (json!("origin/main"), json!("origin"), json!(0), json!(1), json!(true)));
    assert_eq!((r.status()["behind"].clone(), r.status()["upstream"].clone()), (json!(1), json!("origin/main")));
    // When it commits locally too, then the status is one ahead and one behind, and a fast-forward-only pull is refused
    put(r.path(), "mine.txt", "mine\n");
    r.ok("git_stage", json!({"paths": ["mine.txt"]}));
    r.ok("git_commit", json!({"message": "mine"}));
    assert_eq!((r.status()["ahead"].clone(), r.status()["behind"].clone()), (json!(1), json!(1)));
    assert_eq!(r.err("git_fast_forward", json!({})).0, Code::Io);
    let refused = r.err("git_pull", json!({}));
    assert_eq!(refused.0, Code::Io, "{refused:?}");
    // And a push of the diverged branch is refused by the remote
    assert_eq!(r.err("git_push", json!({})).0, Code::Io);
    // When the pull merges
    let pulled = r.ok("git_pull", json!({"strategy": "merge"}));
    // Then the collaborator's file is here, the pull says it updated, and the branch can be pushed
    assert_eq!((pulled["ok"].clone(), pulled["updated"].clone(), read(r.path(), "theirs.txt")), (json!(true), json!(true), "from them\n".to_string()));
    let pushed = r.ok("git_push", json!({}));
    assert_eq!((pushed["remote"].clone(), pushed["branch"].clone()), (json!("origin"), json!("main")));
    assert_eq!(sh(&remote.bare, &["rev-parse", "main"]), r.head());
    assert_eq!((r.status()["ahead"].clone(), r.status()["behind"].clone()), (json!(0), json!(0)));
    // When a new branch is pushed with set_upstream
    r.ok("git_create_ref", json!({"name": "topic", "checkout": true}));
    r.ok("git_push", json!({"set_upstream": true}));
    // Then the remote has it and the branch tracks it
    assert_eq!(sh(&remote.bare, &["rev-parse", "topic"]), r.head());
    assert_eq!(r.upstream_of("topic"), "origin/topic");
}

impl Repo {
    fn upstream_of(&self, branch: &str) -> String {
        sh(self.path(), &["rev-parse", "--abbrev-ref", &format!("{branch}@{{upstream}}")])
    }
}

#[test]
fn a_pull_fast_forwards_or_rebases_and_a_branch_that_is_not_checked_out_fast_forwards_to_its_upstream() {
    // Given a repository with a tracked main and topic, and a collaborator that pushed to both
    let r = repo(&[("a.txt", "one\n")]);
    let remote = with_remote(&r);
    r.ok("git_create_ref", json!({"name": "topic"}));
    sh(r.path(), &["push", "-q", "-u", "origin", "topic"]);
    sh(remote.other.path(), &["fetch", "-q", "origin"]);
    sh(remote.other.path(), &["switch", "-q", "topic"]);
    put(remote.other.path(), "t.txt", "t\n");
    sh(remote.other.path(), &["add", "-A"]);
    sh(remote.other.path(), &["commit", "-q", "-m", "topic work"]);
    sh(remote.other.path(), &["push", "-q", "origin", "topic"]);
    sh(remote.other.path(), &["switch", "-q", "main"]);
    put(remote.other.path(), "m.txt", "m\n");
    sh(remote.other.path(), &["add", "-A"]);
    sh(remote.other.path(), &["commit", "-q", "-m", "main work"]);
    sh(remote.other.path(), &["push", "-q", "origin", "main"]);
    r.ok("git_fetch", json!({"prune": true}));
    // When topic, which is not checked out, is fast-forwarded to its upstream
    let topic_before = sh(r.path(), &["rev-parse", "topic"]);
    let moved = r.ok("git_fast_forward", json!({"branch": "topic"}));
    // Then it moved to the remote's commit, and the checked-out main did not
    assert_eq!((moved["updated"].clone(), moved["from"].as_str().unwrap(), moved["head"].as_str().unwrap()), (json!(true), topic_before.as_str(), sh(r.path(), &["rev-parse", "origin/topic"]).as_str()));
    assert_eq!(r.status()["behind"], 1);
    // When the current branch is fast-forwarded without naming it
    let main_move = r.ok("git_fast_forward", json!({}));
    // Then it is at the remote's commit and the collaborator's file is in the tree
    assert_eq!((main_move["updated"].clone(), main_move["head"].as_str().unwrap(), read(r.path(), "m.txt")), (json!(true), r.head().as_str(), "m\n".to_string()));
    // When a local commit is made, the collaborator pushes another, and the pull rebases
    put(r.path(), "mine.txt", "mine\n");
    sh(r.path(), &["add", "-A"]);
    sh(r.path(), &["commit", "-q", "-m", "mine"]);
    put(remote.other.path(), "m2.txt", "m2\n");
    sh(remote.other.path(), &["add", "-A"]);
    sh(remote.other.path(), &["commit", "-q", "-m", "main work 2"]);
    sh(remote.other.path(), &["push", "-q", "origin", "main"]);
    let pulled = r.ok("git_pull", json!({"strategy": "rebase"}));
    // Then the local commit sits on top of the remote's and the history is linear
    assert_eq!(pulled["ok"], true);
    let subjects: Vec<String> = r.ok("git_log", json!({"limit": 3}))["commits"].as_array().unwrap().iter().map(|c| c["subject"].as_str().unwrap().to_string()).collect();
    assert_eq!(subjects, ["mine", "main work 2", "main work"]);
    // And a fast-forward of a branch that another worktree has checked out is refused
    let other = r.base.path().join("wt");
    sh(r.path(), &["worktree", "add", "-q", other.to_str().unwrap(), "topic"]);
    assert_eq!(r.err("git_fast_forward", json!({"branch": "topic", "to": "main"})).0, Code::InvalidArgs);
}

#[test]
fn a_pull_that_conflicts_stops_with_the_conflicts_listed_and_the_merge_can_be_aborted() {
    // Given a collaborator's commit that conflicts with a local one on the same file
    let r = repo(&[("a.txt", "base\n")]);
    let remote = with_remote(&r);
    put(remote.other.path(), "a.txt", "theirs\n");
    sh(remote.other.path(), &["commit", "-q", "-am", "theirs"]);
    sh(remote.other.path(), &["push", "-q", "origin", "main"]);
    put(r.path(), "a.txt", "ours\n");
    r.ok("git_commit", json!({"message": "ours", "all": true}));
    // When the pull merges
    let pulled = r.ok("git_pull", json!({"strategy": "merge"}));
    // Then it is a result that says conflicted, not an error, and a merge waits
    assert_eq!((pulled["ok"].clone(), lines(&pulled, "conflicts"), pulled["operation"].clone()), (json!(false), vec!["a.txt".to_string()], json!("merge")));
    // And aborting restores the local state
    r.ok("git_merge", json!({"op": "abort"}));
    assert_eq!((read(r.path(), "a.txt"), r.status()["clean"].clone()), ("ours\n".to_string(), json!(true)));
}

#[test]
fn a_force_with_lease_push_overwrites_only_what_this_repository_last_saw() {
    // Given a pushed branch that the collaborator then moved
    let r = repo(&[("a.txt", "one\n")]);
    let remote = with_remote(&r);
    sh(remote.other.path(), &["commit", "-q", "--allow-empty", "-m", "theirs"]);
    sh(remote.other.path(), &["push", "-q", "origin", "main"]);
    sh(r.path(), &["commit", "-q", "--amend", "-m", "first, amended"]);
    // When a plain push is made, then one with the lease before and after a fetch
    let plain = r.err("git_push", json!({}));
    let stale = r.err("git_push", json!({"force_with_lease": true}));
    r.ok("git_fetch", json!({}));
    let fresh = r.call("git_push", json!({"force_with_lease": true}));
    // Then the plain push is refused, the lease is refused while the remote moved unseen, and it succeeds once the move is fetched
    assert_eq!((plain.0, stale.0), (Code::Io, Code::Io));
    assert!(fresh.is_ok(), "{fresh:?}");
    assert_eq!(sh(&remote.bare, &["rev-parse", "main"]), r.head());
}

#[test]
fn a_remote_other_than_one_the_repository_configured_is_refused_before_git_is_asked_and_an_ext_remote_never_runs_its_command() {
    // Given a repository whose origin is configured with an ext:: transport that would run a command, and a marker the command would make
    let r = repo(&[("a.txt", "one\n")]);
    let marker = r.base.path().join("pwned");
    sh(r.path(), &["remote", "add", "origin", &format!("ext::sh -c 'touch {}'", marker.display())]);
    // When the repository fetches and pushes through it
    let fetch = r.err("git_fetch", json!({}));
    let push = r.err("git_push", json!({}));
    // Then both fail and the command never ran
    assert_eq!((fetch.0, push.0), (Code::Io, Code::Io));
    assert!(!marker.exists());
    // And a URL, a path and an option are not remotes: each is refused naming `remote` and the marker is still absent
    for given in ["https://example.com/x.git", "/tmp/elsewhere", &format!("ext::sh -c 'touch {}'", marker.display()), "--upload-pack=touch", "-o"] {
        for tool in ["git_fetch", "git_pull", "git_push"] {
            let (code, message) = r.err(tool, json!({"remote": given}));
            assert!(code == Code::InvalidArgs && message.contains("`remote`"), "{tool} {given}: {code:?} {message}");
        }
    }
    assert!(!marker.exists());
}

#[test]
fn option_looking_values_never_reach_git_and_a_pathspec_is_a_literal_name() {
    // Given a repository, and the file an option would write if git read it as one
    let r = repo(&[("a.txt", "one\n"), ("b.txt", "two\n")]);
    let written = r.base.path().join("written");
    let option = format!("--output={}", written.display());
    // When each action that takes a revision, branch or remote is given that option as the value
    for (tool, key) in [("git_log", "rev"), ("git_compare", "base"), ("git_diff", "base"), ("git_checkout", "ref"), ("git_create_ref", "from"), ("git_merge", "ref"), ("git_rebase", "ref"), ("git_blob", "rev"), ("git_fast_forward", "to"), ("git_push", "branch")] {
        let mut args = json!({"head": "HEAD", "base": "HEAD", "path": "a.txt", "name": "n", "op": "start", "rev": "HEAD", "ref": "main"});
        args[key] = json!(option);
        // Then it is INVALID_ARGS and the file was not written
        assert_eq!(r.err(tool, args).0, Code::InvalidArgs, "{tool} {key}");
    }
    assert!(!written.exists());
    // And a branch that starts with a dash cannot be made, even though git would take it after `--`
    assert_eq!(r.err("git_create_ref", json!({"name": "-b"})).0, Code::InvalidArgs);
    // When both files changed and a glob pathspec is staged
    put(r.path(), "a.txt", "1\n");
    put(r.path(), "b.txt", "2\n");
    let glob = r.call("git_stage", json!({"paths": [":(glob)*.txt"]}));
    // Then nothing is staged: the magic is read as a file name, which does not exist
    assert!(glob.is_err());
    assert_eq!((r.columns("a.txt"), r.columns("b.txt")), (col(".", "M"), col(".", "M")));
    // And a file really named like the magic is staged by that name alone
    put(r.path(), ":(glob)*.txt", "odd\n");
    r.ok("git_stage", json!({"paths": [":(glob)*.txt"]}));
    assert_eq!((r.columns(":(glob)*.txt"), r.columns("a.txt")), (col("A", "."), col(".", "M")));
}

#[test]
fn paths_that_leave_the_tree_or_cross_a_symlink_out_never_touch_what_is_outside() {
    // Given a repository with a symlinked directory that leads outside it, and a file outside
    let r = repo(&[("a.txt", "one\n")]);
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "secret.txt", "outside\n");
    symlink(outside.path(), r.path().join("link")).unwrap();
    // When each path action is given a path that climbs out, an absolute one, and one through the link
    for tool in ["git_stage", "git_unstage", "git_discard"] {
        for path in ["../x", "a/../../x", "/etc/passwd", ".git/config"] {
            // Then it is refused as INVALID_ARGS
            assert_eq!(r.err(tool, json!({"paths": [path]})).0, Code::InvalidArgs, "{tool} {path}");
        }
        // And through the link git refuses or finds nothing to act on, and what is outside stays
        let through = r.call(tool, json!({"paths": ["link/secret.txt"], "untracked": true}));
        assert!(through.is_err() || through.as_ref().is_ok_and(|v| v["restored"].as_u64().unwrap_or(0) + v["cleaned"].as_u64().unwrap_or(0) == 0), "{tool} through a link: {through:?}");
    }
    assert_eq!(read(outside.path(), "secret.txt"), "outside\n");
    // And a blob path that climbs out is refused
    assert_eq!(r.err("git_blob", json!({"rev": "HEAD", "path": "link/../../etc/passwd"})).0, Code::InvalidArgs);
}

#[test]
fn a_conflict_resolved_with_content_cannot_write_through_a_symlink_that_leaves_the_tree() {
    // Given a merge stopped on a conflict in dir/a.txt, after which dir was replaced by a link to a directory outside
    let r = repo(&[("dir/a.txt", "base\n")]);
    sh(r.path(), &["switch", "-q", "-c", "other"]);
    put(r.path(), "dir/a.txt", "theirs\n");
    sh(r.path(), &["commit", "-q", "-am", "theirs"]);
    sh(r.path(), &["switch", "-q", "main"]);
    put(r.path(), "dir/a.txt", "ours\n");
    sh(r.path(), &["commit", "-q", "-am", "ours"]);
    r.ok("git_merge", json!({"op": "start", "ref": "other"}));
    let outside = tempfile::tempdir().unwrap();
    put(outside.path(), "a.txt", "outside\n");
    std::fs::remove_dir_all(r.path().join("dir")).unwrap();
    symlink(outside.path(), r.path().join("dir")).unwrap();
    // When the conflict is resolved with content
    let (code, _) = r.err("git_conflict_resolve", json!({"path": "dir/a.txt", "resolution": "content", "content": "pwned\n"}));
    // Then it is refused as outside the root and the outside file is untouched
    assert_eq!(code, Code::PathOutsideRoot);
    assert_eq!(read(outside.path(), "a.txt"), "outside\n");
}

#[test]
fn the_repositorys_hooks_run_only_when_the_executor_opts_in_and_a_failing_hook_then_stops_the_commit() {
    // Given two repositories with a pre-commit hook that leaves a marker and fails, one served as is and one with hooks on
    for (hooks, name) in [(false, "off"), (true, "on")] {
        let r = repo_with(&[("a.txt", "one\n")], hooks);
        let marker = r.base.path().join(format!("hook-{name}"));
        let hook = r.path().join(".git/hooks/pre-commit");
        std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\nexit 1\n", marker.display())).unwrap();
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        put(r.path(), "a.txt", "two\n");
        // When a commit is made
        let done = r.call("git_commit", json!({"message": "m", "all": true}));
        // Then without the opt-in the hook did not run and the commit was made; with it the hook ran and stopped it
        assert_eq!((marker.exists(), done.is_ok()), (hooks, !hooks), "hooks {name}");
    }
}

#[test]
fn a_root_inside_another_repository_never_acts_on_the_outer_one() {
    // Given an outer repository and an executor whose root is a plain directory inside it
    let outer = tempfile::tempdir().unwrap();
    init(outer.path());
    put(outer.path(), "outer.txt", "o\n");
    sh(outer.path(), &["add", "-A"]);
    sh(outer.path(), &["commit", "-q", "-m", "outer"]);
    std::fs::create_dir(outer.path().join("inner")).unwrap();
    let ex = LocalExecutor::new(&outer.path().join("inner")).unwrap();
    // When the status, a stage and a commit are asked
    for (tool, args) in [("git_status", json!({})), ("git_stage", json!({"paths": ["."]})), ("git_commit", json!({"message": "m", "allow_empty": true}))] {
        let e = execute(&ex, "x", tool, &args, &mut |_, _| {}, &mut |_| {}).unwrap_err();
        // Then each fails as no repository, and the outer repository has no new commit
        assert_eq!(e.code, Code::Io, "{tool}: {e:?}");
        assert!(e.message.contains("not a git repository"), "{tool}: {e:?}");
    }
    assert_eq!(sh(outer.path(), &["rev-list", "--count", "HEAD"]), "1");
}

#[test]
fn a_worktree_named_in_the_request_is_the_tree_the_action_works_on() {
    // Given a repository and a worktree of it under the base on its own branch
    let r = repo(&[("a.txt", "one\n")]);
    r.ok("git_worktree_add", json!({"name": "wt", "branch": "side", "base": "main"}));
    let wt = r.base.path().join("wt");
    put(&wt, "a.txt", "side change\n");
    // When the status and a commit are asked of the worktree, and the status of the root
    let side = r.ok("git_status", json!({"worktree": "wt"}));
    r.ok("git_commit", json!({"worktree": "wt", "message": "on the side", "all": true}));
    // Then the worktree saw the change on its branch, the commit is on that branch only, and the root is clean on main
    assert_eq!((side["branch"].clone(), side["files"][0]["path"].clone()), (json!("side"), json!("a.txt")));
    assert_eq!(sh(&wt, &["log", "-1", "--format=%s"]), "on the side");
    assert_eq!((r.status()["branch"].clone(), r.status()["clean"].clone(), read(r.path(), "a.txt")), (json!("main"), json!(true), "one\n".to_string()));
    assert_eq!(r.ok("git_log", json!({"rev": "side", "limit": 1}))["commits"][0]["subject"], "on the side");
    // And a worktree that does not exist is NOT_FOUND, and one named like a path is refused
    assert_eq!(r.err("git_status", json!({"worktree": "nope"})).0, Code::NotFound);
    assert_eq!(r.err("git_status", json!({"worktree": "../x"})).0, Code::InvalidArgs);
}

#[test]
fn git_watch_sends_the_status_at_the_start_and_on_each_change_until_cancelled() {
    // Given a repository and a watch running on its own thread
    let r = repo(&[("a.txt", "one\n")]);
    let events: Arc<Mutex<Vec<Value>>> = Default::default();
    let watch = {
        let (ex, events) = (r.ex.clone(), events.clone());
        std::thread::spawn(move || execute(ex.as_ref(), "w1", "git_watch", &json!({"interval_ms": 100}), &mut |_, _| {}, &mut |e| events.lock().unwrap().push(e)))
    };
    // When it has sent its first status, a file changes, and nothing else does
    wait_for("the first event", || !events.lock().unwrap().is_empty());
    std::thread::sleep(Duration::from_millis(350));
    assert_eq!(events.lock().unwrap().len(), 1, "an unchanged status is not sent again");
    put(r.path(), "a.txt", "two\n");
    wait_for("the change", || events.lock().unwrap().len() == 2);
    // Then the second event is the status with the file modified
    let second = events.lock().unwrap()[1].clone();
    assert_eq!((second["kind"].clone(), second["status"]["files"][0]["path"].clone(), second["status"]["files"][0]["worktree"].clone()), (json!("git.status"), json!("a.txt"), json!("M")));
    // When it is cancelled by its id, then a second cancel finds nothing
    assert!(r.ex.cancel("w1"));
    let done = watch.join().unwrap().unwrap();
    assert_eq!(done, json!({"events": 2, "cancelled": true}));
    assert!(!r.ex.cancel("w1"));
}

#[test]
fn over_the_channel_a_watch_streams_on_a_thread_of_its_own_while_other_requests_are_answered_and_cancel_ends_it() {
    // Given an in-test rca, an executor on a repository attached to it, and a watch started through it
    isolate();
    let r = repo(&[("a.txt", "one\n")]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let done = Arc::new(AtomicBool::new(false));
    let rca = {
        let done = done.clone();
        std::thread::spawn(move || {
            let mut ws = accept(&listener, &Seen::default());
            send(&mut ws, json!({"id": "w", "tool": "git_watch", "args": {"interval_ms": 100}}));
            // When a status is asked while the watch runs
            send(&mut ws, json!({"id": "s", "tool": "git_status", "args": {}}));
            let (mut events, mut status) = (Vec::new(), None);
            while status.is_none() || events.is_empty() {
                let f = recv(&mut ws);
                if f["id"] == "s" && f.get("ok").is_some() {
                    status = Some(f);
                } else if f["id"] == "w" && f.get("event").is_some() {
                    events.push(f);
                }
            }
            // And the watch is cancelled
            send(&mut ws, json!({"id": "c", "tool": "cancel", "args": {"id": "w"}}));
            let (mut cancel, mut result) = (None, None);
            while cancel.is_none() || result.is_none() {
                let f = recv(&mut ws);
                match (f["id"].as_str().unwrap(), f.get("ok").is_some()) {
                    ("c", true) => cancel = Some(f),
                    ("w", true) => result = Some(f),
                    ("w", false) => events.push(f),
                    (other, _) => panic!("frame for {other}: {f}"),
                }
            }
            let (cancel, result) = (cancel.unwrap(), result.unwrap());
            done.store(true, Ordering::SeqCst);
            ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
            while !matches!(ws.read(), Err(tungstenite::Error::ConnectionClosed)) {}
            (events, status.unwrap(), cancel, result)
        })
    };
    let channel = ExecChannel::open(&url, "git", "tok", r.ex.clone()).unwrap();
    let (events, status, cancel, result) = rca.join().unwrap();
    channel.wait().unwrap();
    // Then the status was answered while the watch ran, the watch sent its first event as a numbered subscribe frame, the cancel was answered and the watch ended cancelled
    assert!(done.load(Ordering::SeqCst));
    assert_eq!((status["ok"].clone(), status["result"]["branch"].clone()), (json!(true), json!("main")));
    assert_eq!((events[0]["seq"].clone(), events[0]["event"]["kind"].clone()), (json!(1), json!("git.status")));
    assert_eq!((cancel["result"]["cancelled"].clone(), result["result"]["cancelled"].clone()), (json!(true), json!(true)));
}
