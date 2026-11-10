//! The executor's widened confinement and its worktree actions, on real repositories in temp directories: the root and the worktree base are
//! the two places a path may name, every other place is refused whatever the spelling (traversal, symlink, option), and git is never handed
//! an argument it could read as an option.
use super::{accept, until_result, Seen};
use base64::Engine;
use exec_proto::{Code, ExecError};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> String {
    let done = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir).output().unwrap();
    assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    String::from_utf8_lossy(&done.stdout).trim().to_string()
}

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> Result<Value, ExecError> {
    execute(ex, "t1", tool, &args, &mut |_, _| {}, &mut |_| {})
}

fn refused(result: Result<Value, ExecError>) -> Code {
    result.expect_err("the request should have been refused").code
}

/// A committed repository as the root, an empty worktree base, and a directory outside both that holds a secret.
struct Rig {
    _dirs: Vec<TempDir>,
    root: PathBuf,
    base: PathBuf,
    outside: PathBuf,
    ex: LocalExecutor,
}

fn rig() -> Rig {
    let dirs: Vec<TempDir> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let [root, base, outside]: [PathBuf; 3] = std::array::from_fn(|i| std::fs::canonicalize(dirs[i].path()).unwrap());
    std::fs::write(root.join("a.txt"), "root file\n").unwrap();
    std::fs::write(outside.join("secret.txt"), "secret\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first"]);
    let ex = LocalExecutor::new(&root).unwrap().with_worktree_base(&base).unwrap();
    Rig { _dirs: dirs, root, base, outside, ex }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn add(r: &Rig, name: &str, branch: &str) -> Value {
    call(&r.ex, "git_worktree_add", json!({"name": name, "branch": branch, "base": "HEAD"})).unwrap()
}

fn worktrees_on_disk(r: &Rig) -> usize {
    git(&r.root, &["worktree", "list", "--porcelain"]).lines().filter(|l| l.starts_with("worktree ")).count()
}

#[test]
fn a_worktree_is_added_worked_in_set_up_and_removed_and_the_root_is_never_touched() {
    // Given a repository and an executor with a worktree base
    let r = rig();
    // When a worktree is added on a new branch
    let added = add(&r, "wt", "feature/x");
    let wt = r.base.join("wt");
    // Then it is a checkout under the base on that branch, at the root's head
    assert_eq!(added["path"], path(&wt));
    assert_eq!((added["head"].as_str().unwrap(), git(&wt, &["branch", "--show-current"])), (git(&r.root, &["rev-parse", "HEAD"]).as_str(), "feature/x".to_string()));
    assert!(wt.join("a.txt").is_file());
    // And it is listed after the repository's own working tree
    let listed = call(&r.ex, "git_worktree_list", json!({})).unwrap();
    assert_eq!(listed["worktrees"][0]["is_main"], true);
    assert_eq!((listed["worktrees"][1]["path"].clone(), listed["worktrees"][1]["branch"].clone(), listed["worktrees"][1]["is_main"].clone()), (json!(path(&wt)), json!("feature/x"), json!(false)));
    // When files are written, read and listed in it by absolute path
    call(&r.ex, "write_file", json!({"path": path(&wt.join("n.txt")), "content": "in worktree"})).unwrap();
    let read = call(&r.ex, "read_file", json!({"path": path(&wt.join("n.txt"))})).unwrap();
    let names: Vec<Value> = call(&r.ex, "list_dir", json!({"path": path(&wt)})).unwrap()["entries"].as_array().unwrap().iter().map(|e| e["name"].clone()).collect();
    // Then they are in the worktree and not in the root
    assert_eq!(read["content"], "in worktree");
    assert!(names.contains(&json!("n.txt")) && !r.root.join("n.txt").exists());
    // When the setup commands run, one writing a file and reading the directory it ran in
    let setup = call(&r.ex, "worktree_setup", json!({"name": "wt", "commands": ["echo built > out.txt", "basename \"$PWD\" > cwd.txt"]})).unwrap();
    // Then both ran in the worktree
    assert_eq!(setup["ok"], true);
    assert_eq!((std::fs::read_to_string(wt.join("out.txt")).unwrap(), std::fs::read_to_string(wt.join("cwd.txt")).unwrap()), ("built\n".to_string(), "wt\n".to_string()));
    assert!(!r.root.join("out.txt").exists());
    // When it is removed while it has changes
    let dirty = call(&r.ex, "git_worktree_remove", json!({"name": "wt"})).unwrap_err();
    // Then git's refusal comes back and the worktree is still there
    assert!(dirty.code == Code::Io && dirty.message.contains("force"), "{dirty:?}");
    assert!(wt.join("n.txt").is_file());
    // When it is removed with force
    assert_eq!(call(&r.ex, "git_worktree_remove", json!({"name": "wt", "force": true})).unwrap(), json!({"removed": true}));
    // Then the directory is gone, and only the repository's own working tree is listed
    assert!(!wt.exists());
    assert_eq!(worktrees_on_disk(&r), 1);
}

#[test]
fn an_existing_branch_is_checked_out_when_no_base_is_given_and_a_branch_already_checked_out_leaves_no_worktree() {
    // Given a repository with a second branch
    let r = rig();
    git(&r.root, &["branch", "existing"]);
    // When a worktree is added on it without a base
    let added = call(&r.ex, "git_worktree_add", json!({"name": "e", "branch": "existing"})).unwrap();
    // Then that branch is checked out, at its own head
    assert_eq!((added["branch"].clone(), added["head"].clone()), (json!("existing"), json!(git(&r.root, &["rev-parse", "existing"]))));
    // When the same branch is asked for under another name
    let again = call(&r.ex, "git_worktree_add", json!({"name": "e2", "branch": "existing"})).unwrap_err();
    // Then git refuses and nothing is left under the base for it
    assert_eq!(again.code, Code::Io);
    assert!(!r.base.join("e2").exists());
    // And a name that is taken is ALREADY_EXISTS without git being asked
    assert_eq!(refused(call(&r.ex, "git_worktree_add", json!({"name": "e", "branch": "other", "base": "HEAD"}))), Code::AlreadyExists);
}

#[test]
fn names_branches_and_bases_that_git_would_read_as_options_or_paths_are_refused_before_git_runs() {
    // Given a repository and what is on disk next to it
    let r = rig();
    let before = (worktrees_on_disk(&r), std::fs::read_dir(&r.base).unwrap().count());
    let attempts = [
        json!({"name": "../evil", "branch": "b", "base": "HEAD"}),
        json!({"name": "-f", "branch": "b", "base": "HEAD"}),
        json!({"name": "a/b", "branch": "b", "base": "HEAD"}),
        json!({"name": path(&r.outside), "branch": "b", "base": "HEAD"}),
        json!({"name": "ok", "branch": "--orphan", "base": "HEAD"}),
        json!({"name": "ok", "branch": "-b", "base": "HEAD"}),
        json!({"name": "ok", "branch": "a..b", "base": "HEAD"}),
        json!({"name": "ok", "branch": "b", "base": "--detach"}),
        json!({"name": "ok", "branch": "b", "base": "--output=/tmp/x"}),
    ];
    for args in attempts {
        // When each is asked, Then it is INVALID_ARGS
        assert_eq!(refused(call(&r.ex, "git_worktree_add", args.clone())), Code::InvalidArgs, "{args}");
    }
    for name in ["../root", "--force", "-x", "a/b"] {
        assert_eq!(refused(call(&r.ex, "git_worktree_remove", json!({"name": name}))), Code::InvalidArgs, "{name}");
        assert_eq!(refused(call(&r.ex, "worktree_setup", json!({"name": name, "commands": ["touch pwned"]}))), Code::InvalidArgs, "{name}");
    }
    // And nothing was created, removed or run anywhere
    assert_eq!((worktrees_on_disk(&r), std::fs::read_dir(&r.base).unwrap().count()), before);
    assert!(!r.root.join("pwned").exists() && !r.base.join("pwned").exists() && !r.base.parent().unwrap().join("evil").exists());
}

#[test]
fn only_a_worktree_git_lists_is_removed_so_a_directory_that_merely_sits_in_the_base_survives() {
    // Given a directory in the base that git knows nothing about, and the main working tree
    let r = rig();
    std::fs::create_dir(r.base.join("plain")).unwrap();
    std::fs::write(r.base.join("plain/keep.txt"), "mine").unwrap();
    // When it is removed by name, with force
    let gone = call(&r.ex, "git_worktree_remove", json!({"name": "plain", "force": true})).unwrap_err();
    // Then it is NOT_FOUND and the files are still there
    assert_eq!(gone.code, Code::NotFound);
    assert_eq!(std::fs::read_to_string(r.base.join("plain/keep.txt")).unwrap(), "mine");
    // And a worktree that was never added is NOT_FOUND too, and the repository is intact
    assert_eq!(refused(call(&r.ex, "git_worktree_remove", json!({"name": "never"}))), Code::NotFound);
    assert!(r.root.join("a.txt").is_file());
}

#[test]
fn the_repositorys_own_hooks_do_not_run_when_a_worktree_is_added() {
    // Given a repository whose post-checkout hook writes a marker outside it
    let r = rig();
    let marker = r.outside.join("hook-ran");
    let hook = r.root.join(".git/hooks/post-checkout");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", path(&marker))).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    // And the hook does run when git adds a worktree on its own, so the marker proves something
    git(&r.root, &["worktree", "add", "-q", "-b", "control", path(&r.outside.join("control"))]);
    assert!(marker.exists(), "the hook did not run for a plain git worktree add");
    std::fs::remove_file(&marker).unwrap();
    // When the executor adds a worktree
    add(&r, "wt", "feature/hooks");
    // Then the hook did not run
    assert!(!marker.exists());
}

#[test]
fn an_executor_without_a_worktree_base_refuses_the_worktree_actions_and_any_path_beside_its_root() {
    // Given an executor with a root only
    let r = rig();
    let ex = LocalExecutor::new(&r.root).unwrap();
    // When a worktree is asked for, and a path under what would have been the base
    let add = call(&ex, "git_worktree_add", json!({"name": "wt", "branch": "b", "base": "HEAD"})).unwrap_err();
    let read = call(&ex, "read_file", json!({"path": path(&r.base.join("x"))}));
    // Then the first says how to configure it and nothing was made, and the path is outside the root
    assert!(add.code == Code::Io && add.message.contains("--worktree-base"), "{add:?}");
    assert_eq!((worktrees_on_disk(&r), refused(read)), (1, Code::PathOutsideRoot));
}

#[test]
fn a_base_that_contains_the_root_or_sits_inside_it_is_refused_when_the_executor_is_built() {
    // Given a root and a base inside it, then a base that contains it
    let r = rig();
    let inside = r.root.join("worktrees");
    let contains = r.root.parent().unwrap();
    // When the executor is built with either, Then it is refused naming both
    for base in [inside.as_path(), contains] {
        let built = LocalExecutor::new(&r.root).unwrap().with_worktree_base(base);
        assert!(built.err().is_some_and(|e| e.contains("must not contain each other")), "{base:?}");
    }
}

#[test]
fn the_two_places_are_confined_apart_so_traversal_and_symlinks_cannot_cross_between_them_or_leave_them() {
    // Given a worktree with a symlink to the outside secret, one to a root file, and a root symlink to a worktree file
    let r = rig();
    add(&r, "wt", "feature/links");
    let wt = r.base.join("wt");
    std::fs::write(wt.join("mine.txt"), "worktree").unwrap();
    symlink(r.outside.join("secret.txt"), wt.join("to-secret")).unwrap();
    symlink(r.root.join("a.txt"), wt.join("to-root")).unwrap();
    symlink(wt.join("mine.txt"), r.root.join("to-worktree")).unwrap();
    symlink(r.outside.join("nowhere"), wt.join("dangling")).unwrap();
    // When each file action names the links, and the spellings of a path that leaves, Then every one is PATH_OUTSIDE_ROOT
    let leaving: Vec<String> = [path(&wt.join("to-secret")).to_string(), path(&wt.join("to-root")).to_string(), path(&wt.join("dangling")).to_string(), path(&r.root.join("to-worktree")).to_string(), "to-worktree".into(), path(&r.outside.join("secret.txt")).to_string(), "../secret.txt".into(), "/etc/passwd".into(), format!("{}/wt/../../x", path(&r.base)), format!("{}/../x", path(&wt))].into();
    for target in &leaving {
        for (tool, args) in [
            ("read_file", json!({"path": target})),
            ("read_chunk", json!({"path": target})),
            ("stat", json!({"path": target})),
            ("write_file", json!({"path": target, "content": "pwned"})),
            ("create_file", json!({"path": target, "content": "pwned"})),
            ("edit_file", json!({"path": target, "old": "secret", "new": "pwned"})),
            ("list_dir", json!({"path": target})),
            ("search", json!({"pattern": "secret", "kind": "content", "path": target})),
        ] {
            let result = call(&r.ex, tool, args);
            assert_eq!(result.as_ref().err().map(|e| e.code), Some(Code::PathOutsideRoot), "{tool} {target}: {result:?}");
        }
    }
    // And nothing outside was written
    assert_eq!(std::fs::read_to_string(r.outside.join("secret.txt")).unwrap(), "secret\n");
    assert!(!r.outside.join("nowhere").exists() && !r.root.join("x").exists());
    // And a command's directory is held to the same places: the worktree is fine, the outside is not
    let ok = call(&r.ex, "run_command", json!({"command": "pwd", "cwd": path(&wt)})).unwrap();
    assert_eq!(ok["exit_code"], 0);
    assert_eq!(refused(call(&r.ex, "run_command", json!({"command": "pwd", "cwd": path(&r.outside)}))), Code::PathOutsideRoot);
}

#[test]
fn read_chunk_returns_the_bytes_of_a_binary_file_exactly_and_never_more_than_the_cap() {
    // Given a file of every byte value, and one larger than the cap
    let r = rig();
    let all: Vec<u8> = (0..=255u8).collect();
    std::fs::write(r.root.join("all.bin"), &all).unwrap();
    let big = vec![7u8; exec_fs::CHUNK_MAX as usize + 1000];
    std::fs::write(r.root.join("big.bin"), &big).unwrap();
    let decode = |v: &Value| base64::engine::general_purpose::STANDARD.decode(v["data"].as_str().unwrap()).unwrap();
    // When ranges of the first are read, one in the middle and one ending past the end
    let mid = call(&r.ex, "read_chunk", json!({"path": "all.bin", "offset": 250, "length": 4})).unwrap();
    let tail = call(&r.ex, "read_chunk", json!({"path": "all.bin", "offset": 250, "length": 100})).unwrap();
    // Then the bytes are exact and eof is true only when the range reached the end
    assert_eq!((decode(&mid), mid["eof"].clone()), (all[250..254].to_vec(), json!(false)));
    assert_eq!((decode(&tail), tail["eof"].clone(), tail["bytes"].clone(), tail["total_bytes"].clone()), (all[250..].to_vec(), json!(true), json!(6), json!(256)));
    // When the big file is read with a length far above the cap, then from where the cap ended
    let first = call(&r.ex, "read_chunk", json!({"path": "big.bin", "length": exec_fs::CHUNK_MAX * 8})).unwrap();
    let rest = call(&r.ex, "read_chunk", json!({"path": "big.bin", "offset": exec_fs::CHUNK_MAX})).unwrap();
    // Then the first stops at the cap and is not the end, and the rest finishes it
    assert_eq!((first["bytes"].clone(), first["eof"].clone()), (json!(exec_fs::CHUNK_MAX), json!(false)));
    assert_eq!((decode(&rest).len(), rest["eof"].clone()), (1000, json!(true)));
}

#[test]
fn create_file_never_opens_a_path_that_exists_and_never_follows_a_link_out_of_its_place() {
    // Given a root file, a link to it, a link to the outside secret and a dangling link
    let r = rig();
    symlink(r.root.join("a.txt"), r.root.join("to-a")).unwrap();
    symlink(r.outside.join("secret.txt"), r.root.join("to-secret")).unwrap();
    symlink(r.outside.join("new.txt"), r.root.join("dangling")).unwrap();
    // When a file is created over each, Then an existing file is ALREADY_EXISTS and unchanged, and the others are outside the root
    assert_eq!(refused(call(&r.ex, "create_file", json!({"path": "a.txt", "content": "x"}))), Code::AlreadyExists);
    assert_eq!(refused(call(&r.ex, "create_file", json!({"path": "to-a", "content": "x"}))), Code::AlreadyExists);
    assert_eq!(refused(call(&r.ex, "create_file", json!({"path": "to-secret", "content": "x"}))), Code::PathOutsideRoot);
    assert_eq!(refused(call(&r.ex, "create_file", json!({"path": "dangling", "content": "x"}))), Code::PathOutsideRoot);
    assert_eq!((std::fs::read_to_string(r.root.join("a.txt")).unwrap(), std::fs::read_to_string(r.outside.join("secret.txt")).unwrap()), ("root file\n".to_string(), "secret\n".to_string()));
    assert!(!r.outside.join("new.txt").exists());
    // And a file is created inside a worktree by its absolute path
    add(&r, "wt", "feature/create");
    call(&r.ex, "create_file", json!({"path": path(&r.base.join("wt/made/new.txt")), "content": "hi", "create_dirs": true})).unwrap();
    assert_eq!(std::fs::read_to_string(r.base.join("wt/made/new.txt")).unwrap(), "hi");
}

#[test]
fn put_blob_writes_bytes_exactly_into_new_files_and_refuses_traversal_links_overwrites_bad_data_and_the_size_cap() {
    // Given an executor over a root with a file, a link to a directory outside, a link to an outside file and a dangling link
    let r = rig();
    std::fs::create_dir_all(r.outside.join("dir")).unwrap();
    symlink(r.outside.join("dir"), r.root.join("out-dir")).unwrap();
    symlink(r.outside.join("secret.txt"), r.root.join("to-secret")).unwrap();
    symlink(r.outside.join("fresh.bin"), r.root.join("dangling")).unwrap();
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let bytes: Vec<u8> = (0..=255u8).collect();
    // When binary bytes are put under directories that do not exist yet
    let put = call(&r.ex, "put_blob", json!({"path": ".rca/attachments/a1/shot.png", "data": b64(&bytes)})).unwrap();
    // Then they are on disk byte for byte, the answer names the path and the count, and the parents were made
    assert_eq!((put["path"].clone(), put["bytes"].clone()), (json!(".rca/attachments/a1/shot.png"), json!(256)));
    assert_eq!(std::fs::read(r.root.join(".rca/attachments/a1/shot.png")).unwrap(), bytes);
    // When a path that exists is put over, Then it is ALREADY_EXISTS and keeps its bytes
    assert_eq!(refused(call(&r.ex, "put_blob", json!({"path": "a.txt", "data": b64(b"x")}))), Code::AlreadyExists);
    assert_eq!(std::fs::read_to_string(r.root.join("a.txt")).unwrap(), "root file\n");
    // When the path climbs out, passes a link to an outside directory, or is a link to or past the outside, Then every one is outside the root and nothing is written
    for bad in ["../escape.bin", "out-dir/new.bin", "to-secret", "dangling", "a/../../escape.bin"] {
        assert_eq!(refused(call(&r.ex, "put_blob", json!({"path": bad, "data": b64(b"x")}))), Code::PathOutsideRoot, "{bad}");
    }
    assert_eq!(std::fs::read_to_string(r.outside.join("secret.txt")).unwrap(), "secret\n");
    assert!(!r.outside.join("dir/new.bin").exists() && !r.outside.join("fresh.bin").exists());
    // And data that is not base64, or that decodes past the cap, is INVALID_ARGS and leaves no file
    assert_eq!(refused(call(&r.ex, "put_blob", json!({"path": "bad.bin", "data": "***"}))), Code::InvalidArgs);
    let too_big = b64(&vec![0u8; exec_fs::BLOB_MAX as usize + 1]);
    assert_eq!(refused(call(&r.ex, "put_blob", json!({"path": "big.bin", "data": too_big}))), Code::InvalidArgs);
    assert!(!r.root.join("bad.bin").exists() && !r.root.join("big.bin").exists());
}

#[test]
fn make_dir_claims_a_name_makes_the_parents_it_is_asked_for_and_refuses_every_way_out_of_the_root() {
    // Given an executor over a root with a file, and a link to a directory outside, a link to an outside file and a dangling link
    let r = rig();
    std::fs::create_dir_all(r.outside.join("dir")).unwrap();
    symlink(r.outside.join("dir"), r.root.join("out-dir")).unwrap();
    symlink(r.outside.join("secret.txt"), r.root.join("to-secret")).unwrap();
    symlink(r.outside.join("fresh"), r.root.join("dangling")).unwrap();
    // When a chain of directories is made with the parents asked for, and then again
    let first = call(&r.ex, "make_dir", json!({"path": ".rca/projects", "parents": true})).unwrap();
    let again = call(&r.ex, "make_dir", json!({"path": ".rca/projects", "parents": true})).unwrap();
    // Then the directories are on disk, the first made them and the second found them
    assert!(r.root.join(".rca/projects").is_dir());
    assert_eq!((first, again), (json!({"path": ".rca/projects", "created": true}), json!({"path": ".rca/projects", "created": false})));
    // When a name is claimed below a parent that exists, and the same name is claimed again
    let claimed = call(&r.ex, "make_dir", json!({"path": ".rca/projects/a"})).unwrap();
    std::fs::write(r.root.join(".rca/projects/a/keep.txt"), "mine").unwrap();
    let taken = refused(call(&r.ex, "make_dir", json!({"path": ".rca/projects/a"})));
    // Then the first made it, the second is ALREADY_EXISTS and what is in it is left alone
    assert_eq!(claimed["created"], true);
    assert_eq!((taken, std::fs::read_to_string(r.root.join(".rca/projects/a/keep.txt")).unwrap()), (Code::AlreadyExists, "mine".to_string()));
    // And a missing parent is NOT_FOUND without the parents, and a file in the way is ALREADY_EXISTS either way
    assert_eq!(refused(call(&r.ex, "make_dir", json!({"path": "no/such/dir"}))), Code::NotFound);
    assert!(!r.root.join("no").exists());
    assert_eq!(refused(call(&r.ex, "make_dir", json!({"path": "a.txt", "parents": true}))), Code::AlreadyExists);
    assert_eq!(std::fs::read_to_string(r.root.join("a.txt")).unwrap(), "root file\n");
    // When the path climbs out, passes a link to an outside directory, or is a link to or past the outside, or is outside by its absolute spelling
    let made = r.outside.join("made");
    let absolute = path(&made);
    for bad in ["../escape", "out-dir/new", "to-secret", "dangling", "a/../../escape", absolute] {
        // Then every one is outside the root and nothing is made there
        assert_eq!(refused(call(&r.ex, "make_dir", json!({"path": bad, "parents": true}))), Code::PathOutsideRoot, "{bad}");
    }
    assert!(!r.outside.join("dir/new").exists() && !r.outside.join("fresh").exists() && !r.outside.join("made").exists());
    // And a directory is made in a worktree by its absolute path
    add(&r, "wt", "feature/dirs");
    call(&r.ex, "make_dir", json!({"path": path(&r.base.join("wt/inner/deep")), "parents": true})).unwrap();
    assert!(r.base.join("wt/inner/deep").is_dir());
}

/// Makes `dir` a repository that has its own identity, so a commit made in it works whatever the machine's git has.
fn repository_with_identity(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for args in [&["init", "-q", "--initial-branch=main"][..], &["config", "user.name", "Chain Author"], &["config", "user.email", "chain@example.test"], &["config", "commit.gpgsign", "false"]] {
        let done = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
    }
}

#[test]
fn git_init_makes_a_repository_below_the_root_with_its_first_files_in_one_commit_and_never_touches_the_repository_around_it() {
    // Given an executor over a repository, and an empty directory below it that has an identity of its own
    let r = rig();
    let project = r.root.join(".rca/projects/demo");
    repository_with_identity(&project);
    let outer_head = git(&r.root, &["rev-parse", "HEAD"]);
    let files = json!([{"path": "README.md", "content": "# Demo\n"}, {"path": "assets/icon.svg", "content": "<svg/>\n"}]);
    // When it is made a repository with two first files
    let done = call(&r.ex, "git_init", json!({"path": ".rca/projects/demo", "initial_branch": "main", "files": files, "message": "Initial commit"})).unwrap();
    // Then the commit holds exactly those files with that message, on that branch, and the answer names the commit
    assert_eq!((done["path"].clone(), done["branch"].clone(), done["commit"].clone()), (json!(".rca/projects/demo"), json!("main"), json!(git(&project, &["rev-parse", "HEAD"]))));
    assert_eq!(git(&project, &["ls-tree", "-r", "--name-only", "HEAD"]), "README.md\nassets/icon.svg");
    assert_eq!((git(&project, &["log", "--format=%s"]), git(&project, &["branch", "--show-current"]), done.get("commit_error")), ("Initial commit".to_string(), "main".to_string(), None));
    assert_eq!(std::fs::read_to_string(project.join("assets/icon.svg")).unwrap(), "<svg/>\n");
    // And the repository around it is where it was: same head, the new directory only untracked
    assert_eq!(git(&r.root, &["rev-parse", "HEAD"]), outer_head);
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "?? .rca/");
}

#[test]
fn git_init_makes_a_bare_repository_on_the_asked_branch_and_reports_a_commit_that_failed_without_failing() {
    // Given an executor, a directory with no files to commit, and one whose commits cannot be signed
    let r = rig();
    std::fs::create_dir_all(r.root.join("empty")).unwrap();
    let unsigned = r.root.join("unsigned");
    repository_with_identity(&unsigned);
    for args in [&["config", "commit.gpgsign", "true"][..], &["config", "gpg.program", "false"]] {
        assert!(std::process::Command::new("git").args(args).current_dir(&unsigned).output().unwrap().status.success());
    }
    // When the empty one is made a repository on a named branch, and the other is given a first file
    let empty = call(&r.ex, "git_init", json!({"path": "empty", "initial_branch": "trunk"})).unwrap();
    let failed = call(&r.ex, "git_init", json!({"path": "unsigned", "initial_branch": "main", "files": [{"path": "a.txt", "content": "x"}]})).unwrap();
    // Then the first is a repository on that branch with nothing committed
    assert_eq!(empty, json!({"path": "empty", "branch": "trunk"}));
    assert_eq!(git(&r.root.join("empty"), &["symbolic-ref", "--short", "HEAD"]), "trunk");
    // And the second says why there is no commit, and the file is there all the same
    assert!(failed.get("commit").is_none() && failed["commit_error"].as_str().is_some_and(|why| !why.is_empty()), "{failed}");
    assert_eq!(std::fs::read_to_string(unsigned.join("a.txt")).unwrap(), "x");
}

#[test]
fn git_init_holds_the_directory_and_each_first_file_to_the_root_runs_no_hook_and_never_replaces_a_file() {
    // Given an executor, a link to a directory outside, a repository with a pre-commit hook that leaves a mark, and a directory with a file
    let r = rig();
    std::fs::create_dir_all(r.outside.join("dir")).unwrap();
    symlink(r.outside.join("dir"), r.root.join("out-dir")).unwrap();
    let hooked = r.root.join("hooked");
    repository_with_identity(&hooked);
    std::fs::write(hooked.join(".git/hooks/pre-commit"), format!("#!/bin/sh\ntouch {}/hook-ran\n", path(&r.outside))).unwrap();
    std::fs::set_permissions(hooked.join(".git/hooks/pre-commit"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let linked = r.root.join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    symlink(r.outside.join("dir"), linked.join("assets")).unwrap();
    std::fs::create_dir_all(r.root.join("taken")).unwrap();
    std::fs::write(r.root.join("taken/README.md"), "mine\n").unwrap();
    // When a repository is made outside the root, through a link, or by an absolute path
    for bad in ["../escape", "out-dir", path(&r.outside)] {
        // Then each is outside the root and nothing is made there
        assert_eq!(refused(call(&r.ex, "git_init", json!({"path": bad}))), Code::PathOutsideRoot, "{bad}");
    }
    assert!(!r.outside.join("dir/.git").exists() && !r.outside.join(".git").exists());
    // When a first file passes a link that leaves the root
    let through = refused(call(&r.ex, "git_init", json!({"path": "linked", "files": [{"path": "assets/icon.svg", "content": "x"}]})));
    // Then it is refused before the repository is made, and nothing is written outside
    assert_eq!(through, Code::PathOutsideRoot);
    assert!(!linked.join(".git").exists() && !r.outside.join("dir/icon.svg").exists());
    // When a first file would replace one that is there, Then it is ALREADY_EXISTS and the file keeps its content
    assert_eq!(refused(call(&r.ex, "git_init", json!({"path": "taken", "files": [{"path": "README.md", "content": "new\n"}]}))), Code::AlreadyExists);
    assert_eq!(std::fs::read_to_string(r.root.join("taken/README.md")).unwrap(), "mine\n");
    // When the repository with the hook gets a first commit
    let done = call(&r.ex, "git_init", json!({"path": "hooked", "initial_branch": "main", "files": [{"path": "a.txt", "content": "x"}]})).unwrap();
    // Then the commit happened and the repository's hook did not run (hooks are off unless the executor was started with them)
    assert!(done["commit"].is_string(), "{done}");
    assert!(!r.outside.join("hook-ran").exists());
    // And a path that is a file, or a directory that is not there, is refused by its own code
    assert_eq!(refused(call(&r.ex, "git_init", json!({"path": "a.txt"}))), Code::InvalidArgs);
    assert_eq!(refused(call(&r.ex, "git_init", json!({"path": "no/such"}))), Code::NotFound);
}

#[test]
fn browse_lists_only_the_root_the_base_and_the_allowlist_and_refuses_every_way_out_of_them() {
    // Given an executor allowed to browse one more directory, which holds a folder, a hidden folder and a link to the outside
    let r = rig();
    let shown = r.outside.join("shown");
    std::fs::create_dir_all(shown.join("projects")).unwrap();
    std::fs::create_dir_all(shown.join(".config")).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let elsewhere_path = std::fs::canonicalize(elsewhere.path()).unwrap();
    std::fs::write(elsewhere_path.join("private.txt"), "x").unwrap();
    symlink(&elsewhere_path, shown.join("link-out")).unwrap();
    let ex = LocalExecutor::new(&r.root).unwrap().with_worktree_base(&r.base).unwrap().with_browse_allow(std::slice::from_ref(&shown)).unwrap();
    // When the allowed directory is browsed, with and without hidden names
    let plain = call(&ex, "browse", json!({"path": path(&shown)})).unwrap();
    let hidden = call(&ex, "browse", json!({"path": path(&shown), "show_hidden": true})).unwrap();
    let names = |v: &Value| v["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    // Then its folders are listed (the link to a directory as one), and hidden names only when asked
    assert_eq!(names(&plain), ["link-out", "projects"]);
    assert_eq!(names(&hidden), [".config", "link-out", "projects"]);
    // And its parent is not offered, since the parent is not an allowed place
    assert!(plain.get("parent").is_none());
    // When a folder is entered and its parent asked for
    let inner = call(&ex, "browse", json!({"path": path(&shown.join("projects"))})).unwrap();
    // Then the parent is the allowed directory
    assert_eq!(inner["parent"], path(&shown));
    // When the link out, a `..` out of the allowed directory, the filesystem root, a sibling of an allowed name, and the base's parent are browsed
    let outside_names: Vec<String> = vec![path(&shown.join("link-out")).into(), format!("{}/..", path(&shown)), "/".into(), format!("{}-sibling", path(&shown)), path(r.base.parent().unwrap()).into(), format!("{}/projects/../../..", path(&shown))];
    // Then each is refused and none shows what is there
    for target in &outside_names {
        let result = call(&ex, "browse", json!({"path": target}));
        assert!(matches!(result.as_ref().map_err(|e| e.code), Err(Code::PathOutsideRoot) | Err(Code::NotFound)), "{target}: {result:?}");
    }
    // And the allowlist is for browse only: the same directory cannot be read as a file path
    assert_eq!(refused(call(&ex, "list_dir", json!({"path": path(&shown)}))), Code::PathOutsideRoot);
    assert_eq!(refused(call(&ex, "read_file", json!({"path": path(&elsewhere_path.join("private.txt"))}))), Code::PathOutsideRoot);
}

#[test]
fn an_allowlist_entry_that_does_not_exist_stops_the_executor_from_being_built() {
    // Given a directory that is not there
    let r = rig();
    // When it is named as an allowlist entry, Then building fails naming it
    let built = LocalExecutor::new(&r.root).unwrap().with_browse_allow(&[r.outside.join("missing")]);
    assert!(built.err().is_some_and(|e| e.contains("missing")));
}

#[test]
fn search_entries_does_not_follow_a_link_out_of_the_root_and_skips_dependencies() {
    // Given a link to a directory outside the root that holds a matching file, and a matching file in node_modules
    let r = rig();
    std::fs::create_dir(r.outside.join("dir")).unwrap();
    std::fs::write(r.outside.join("dir/secret-token.txt"), "x").unwrap();
    symlink(r.outside.join("dir"), r.root.join("linked")).unwrap();
    std::fs::create_dir_all(r.root.join("node_modules/pkg")).unwrap();
    std::fs::write(r.root.join("node_modules/pkg/token.js"), "x").unwrap();
    std::fs::write(r.root.join("token.rs"), "x").unwrap();
    // When the entries named token are searched
    let found = call(&r.ex, "search_entries", json!({"query": "token"})).unwrap();
    // Then only the root's own file is found
    assert_eq!(found["entries"], json!([{"path": "token.rs", "kind": "file"}]));
}

#[test]
fn a_diff_and_an_entry_search_can_be_scoped_to_a_worktree_and_stay_inside_it() {
    // Given a worktree with an edited file, a file only it has, and the root unchanged
    let r = rig();
    add(&r, "wt", "feature/scoped");
    let wt = r.base.join("wt");
    std::fs::write(wt.join("a.txt"), "root file\nmore\n").unwrap();
    std::fs::write(wt.join("only-here.rs"), "x").unwrap();
    // When the diff is asked of the worktree and of the root
    let in_wt = call(&r.ex, "git_diff", json!({"worktree": "wt"})).unwrap();
    let in_root = call(&r.ex, "git_diff", json!({})).unwrap();
    // Then only the worktree's change is in the first
    assert!(in_wt["diff"].as_str().unwrap().contains("+more"), "{in_wt}");
    assert_eq!(in_root["diff"], "");
    // And a path of the diff is held to the worktree: climbing out of it, or naming the root's file by absolute path, is refused
    assert_eq!(refused(call(&r.ex, "git_diff", json!({"worktree": "wt", "path": "../../x"}))), Code::PathOutsideRoot);
    assert_eq!(refused(call(&r.ex, "git_diff", json!({"worktree": "wt", "path": path(&r.outside.join("secret.txt"))}))), Code::PathOutsideRoot);
    // And a worktree that is not there, or whose name is a path, is refused
    assert_eq!(refused(call(&r.ex, "git_diff", json!({"worktree": "nope"}))), Code::NotFound);
    assert_eq!(refused(call(&r.ex, "git_diff", json!({"worktree": "../outside"}))), Code::InvalidArgs);
    // When entries are searched in the worktree by its absolute path
    let found = call(&r.ex, "search_entries", json!({"query": "only-here", "path": path(&wt)})).unwrap();
    // Then the entry is named relative to the worktree, and the root does not have it
    assert_eq!(found["entries"], json!([{"path": "only-here.rs", "kind": "file"}]));
    assert_eq!(call(&r.ex, "search_entries", json!({"query": "only-here"})).unwrap()["entries"], json!([]));
    // And a search directory outside both places, or one that climbs out of the worktree, is refused
    assert_eq!(refused(call(&r.ex, "search_entries", json!({"query": "secret", "path": path(&r.outside)}))), Code::PathOutsideRoot);
    assert_eq!(refused(call(&r.ex, "search_entries", json!({"query": "a", "path": format!("{}/../{}", path(&wt), "x")}))), Code::PathOutsideRoot);
}

#[test]
fn a_worktree_setup_cancelled_while_a_command_runs_stops_it_and_runs_nothing_after_it() {
    // Given a worktree and a setup whose first command sleeps
    let r = Arc::new(rig());
    add(&r, "wt", "feature/cancel");
    let started = Instant::now();
    let worker = {
        let r = r.clone();
        std::thread::spawn(move || execute(&r.ex, "setup-1", "worktree_setup", &json!({"name": "wt", "commands": ["sleep 30", "touch after.txt"]}), &mut |_, _| {}, &mut |_| {}))
    };
    // When it is cancelled by its id once it runs
    let until = Instant::now() + Duration::from_secs(10);
    while !call(&r.ex, "cancel", json!({"id": "setup-1"})).unwrap()["cancelled"].as_bool().unwrap() {
        assert!(Instant::now() < until, "the setup never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = worker.join().unwrap().unwrap();
    // Then it ended at once, reports the cancel, and the second command did not run
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!((result["ok"].clone(), result["cancelled"].clone()), (json!(false), json!(true)));
    assert!(!r.base.join("wt/after.txt").exists());
}

#[test]
fn a_worktree_setup_that_outlasts_its_timeout_is_killed_and_says_so() {
    // Given a worktree and a command that sleeps far longer than the timeout
    let r = rig();
    add(&r, "wt", "feature/timeout");
    // When the setup runs with a short timeout
    let started = Instant::now();
    let result = call(&r.ex, "worktree_setup", json!({"name": "wt", "commands": ["sleep 30", "touch after.txt"], "timeout_ms": 300})).unwrap();
    // Then it was cut at the timeout, reports it, and the next command did not run
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!((result["ok"].clone(), result["timed_out"].clone()), (json!(false), json!(true)));
    assert!(!r.base.join("wt/after.txt").exists());
}

fn serve_with(args: &[&str]) -> std::process::Child {
    std::process::Command::new(super::BIN).arg("serve").args(args).env("RCA_TEST_BEARER", "tok").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().unwrap()
}

#[test]
fn the_serve_verb_names_its_executor_and_serves_the_worktree_base_and_the_allowlist_it_was_given() {
    // Given an in-test rca, a repository, a worktree base and a directory to browse
    let r = rig();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    // When `rca-exec serve` is started with an executor id, the base and the allowlist, and rca asks for a worktree, a browse of the allowed place, and a browse of the root's parent
    let child = serve_with(&["--rca-url", &url, "--workspace", "main", "--executor", "box 1", "--root", path(&r.root), "--worktree-base", path(&r.base), "--browse-allow", path(&r.outside), "--secret-env", "RCA_TEST_BEARER"]);
    let mut ws = accept(&listener, &seen);
    super::send(&mut ws, json!({"id": "a1", "tool": "git_worktree_add", "args": {"name": "wt", "branch": "feature/serve", "base": "HEAD"}}));
    let (_, added) = until_result(&mut ws, "a1");
    super::send(&mut ws, json!({"id": "a2", "tool": "browse", "args": {"path": path(&r.outside)}}));
    let (_, browsed) = until_result(&mut ws, "a2");
    super::send(&mut ws, json!({"id": "a3", "tool": "browse", "args": {"path": path(r.root.parent().unwrap())}}));
    let (_, parent) = until_result(&mut ws, "a3");
    ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
    while ws.read().is_ok() {}
    let (code, lines) = super::json_lines(child);
    // Then the channel named the executor and its root workspace, the worktree exists under the base, the allowed place was listed and the other refused
    assert_eq!(seen.lock().unwrap()[0].0, "/exec/v2/executors?workspace=main&executor=box%201");
    assert_eq!((added["ok"].clone(), r.base.join("wt").is_dir()), (json!(true), true));
    assert_eq!((browsed["ok"].clone(), browsed["result"]["entries"][0]["name"].clone()), (json!(true), json!("secret.txt")));
    assert_eq!((parent["ok"].clone(), parent["error"]["code"].clone()), (json!(false), json!("PATH_OUTSIDE_ROOT")));
    assert_eq!(code, 0);
    assert_eq!((lines[0]["executor"].clone(), lines[0]["worktreeBase"].clone()), (json!("box 1"), json!(path(&r.base))));
}

#[test]
fn the_serve_verb_fails_with_json_and_never_connects_for_a_base_inside_the_root_or_an_allowlist_entry_that_is_missing() {
    // Given a root, and a listener that must never be reached
    let r = rig();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let inside = r.root.join("wt-base");
    let missing = r.outside.join("missing");
    for extra in [["--worktree-base", path(&inside)], ["--browse-allow", path(&missing)]] {
        // When serve is started with the bad option
        let child = serve_with(&["--rca-url", &url, "--workspace", "w", "--root", path(&r.root), extra[0], extra[1], "--secret-env", "RCA_TEST_BEARER"]);
        let (code, lines) = super::json_lines(child);
        // Then it printed one failure with a next step and exited 1
        assert_eq!((code, lines.len(), lines[0]["ok"].clone()), (1, 1, json!(false)), "{extra:?}");
        assert!(lines[0]["next"].as_str().is_some_and(|n| n.contains("--worktree-base")), "{extra:?}");
    }
    // And rca was never connected to
    assert!(listener.accept().is_err());
}
