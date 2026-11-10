//! The phone's git rows through the chain: a real repository on the executor's disk, reached through the real rca-service and rca-exec. A row counts
//! only when the agent method it maps to was answered in its call's window and the executor ran the git action; what each call changed is then read
//! from the repository itself (its files, its `git status`, its log, a bare remote's refs), never from the answer alone.
use crate::rig::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

const SELECTOR: &str = "id:w1::";

fn sel(extra: Value) -> Value {
    let mut params = json!({"worktree": SELECTOR});
    if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    params
}

/// The phone's source control list as `area:status:path`.
fn entries(reply: &Value) -> Vec<String> {
    reply["result"]["entries"].as_array().unwrap().iter().map(|e| format!("{}:{}:{}", e["area"].as_str().unwrap(), e["status"].as_str().unwrap(), e["path"].as_str().unwrap())).collect()
}

/// git in a directory other than the executor's repository, with the repository's isolating environment.
fn git_in(dir: &Path, env: &[(String, String)], args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).env_clear().envs(env.iter().cloned()).env("PATH", std::env::var_os("PATH").unwrap_or_default()).output().expect("git starts");
    assert!(out.status.success(), "git {args:?} in {}: {}", dir.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

/// One call of the trace, served, with the agent method answered and the executor's action run.
fn served(case: &mut exec_testkit::Case, phone: &mut Phone, chain: &Chain, method: &str, params: Value, agent_method: &str, action: &str) -> Value {
    let reply = phone.call(case, chain, &[&format!("rpc:{method}")], method, sel(params));
    answered(case, agent_method, true);
    assert!(ran(case, action), "{method} did not run {action}: {}", chain.evidence());
    reply
}

#[test]
fn a_working_tree_is_read_staged_committed_discarded_and_compared_and_each_change_is_in_the_repository_itself() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository with a modified file and an untracked one, and a paired phone
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let repo = &chain.repo;
        repo.write("README.md", "changed\n");
        repo.write("a.txt", "one\n");
        // When the phone reads the status
        let status = served(case, &mut phone, &chain, "git.status", json!({}), "git.status", "git_status");
        // Then it lists the two files in the areas git has them in, on the branch at the repository's head
        assert_eq!(entries(&status), ["unstaged:modified:README.md", "untracked:untracked:a.txt"]);
        assert_eq!((status["result"]["branch"].clone(), status["result"]["head"].clone(), status["result"]["conflictOperation"].clone()), (json!("main"), json!(repo.head()), json!("unknown")));
        // When it stages the untracked file
        served(case, &mut phone, &chain, "git.stage", json!({"filePath": "a.txt"}), "git.stage", "git_stage");
        // Then git has it added
        assert!(repo.git(&["status", "--porcelain"]).contains("A  a.txt"));
        // When it stages the other by a list, then unstages the first in a list
        served(case, &mut phone, &chain, "git.bulkStage", json!({"filePaths": ["README.md"]}), "git.stage", "git_stage");
        served(case, &mut phone, &chain, "git.bulkUnstage", json!({"filePaths": ["a.txt"]}), "git.unstage", "git_unstage");
        // Then only the modified file is in the index
        assert_eq!(repo.git(&["status", "--porcelain"]), "M  README.md\n?? a.txt");
        // When it unstages that one alone and stages everything by scope
        served(case, &mut phone, &chain, "git.unstage", json!({"filePath": "README.md"}), "git.unstage", "git_unstage");
        assert_eq!(repo.git(&["status", "--porcelain"]), " M README.md\n?? a.txt");
        let scoped = served(case, &mut phone, &chain, "git.bulkStage", json!({"filePaths": [], "scope": "all"}), "git.stage", "git_stage");
        // Then both are staged and the answer says what scope it staged
        assert_eq!((repo.git(&["status", "--porcelain"]), scoped["result"]["stagedScope"].clone()), ("M  README.md\nA  a.txt".to_string(), json!("all")));
        // When it commits
        let committed = served(case, &mut phone, &chain, "git.commit", json!({"message": "Chain commit"}), "git.commit", "git_commit");
        // Then git's newest commit has the subject and the tree is clean
        assert_eq!(committed["result"]["success"], json!(true));
        assert_eq!((repo.git(&["log", "-1", "--format=%s"]), repo.git(&["status", "--porcelain"])), ("Chain commit".to_string(), String::new()));
        // When the phone reads the history
        let history = served(case, &mut phone, &chain, "git.history", json!({"limit": 5}), "git.history", "git_log");
        // Then the newest commit is that one over the first, and carries git's id
        let items: Vec<&str> = history["result"]["items"].as_array().unwrap().iter().map(|i| i["subject"].as_str().unwrap()).collect();
        assert_eq!(items, ["Chain commit", "first commit"]);
        assert_eq!(history["result"]["items"][0]["id"], json!(repo.head()));
        // When it commits again with nothing staged
        let empty = phone.call(case, &chain, &[], "git.commit", sel(json!({"message": "Nothing"})));
        // Then the host's answer is a failure, and git has no new commit
        assert_eq!(empty["result"]["success"], json!(false));
        assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "2");
        // When tracked and untracked files are changed and the phone discards them one at a time
        repo.write("README.md", "scribble\n");
        repo.write("junk.txt", "junk\n");
        served(case, &mut phone, &chain, "git.discard", json!({"filePath": "README.md"}), "git.discard", "git_discard");
        assert_eq!(std::fs::read_to_string(repo.path().join("README.md")).unwrap(), "changed\n");
        repo.write("README.md", "scribble again\n");
        served(case, &mut phone, &chain, "git.discard", json!({"filePath": "junk.txt"}), "git.discard", "git_discard");
        served(case, &mut phone, &chain, "git.discard", json!({"filePath": "README.md"}), "git.discard", "git_discard");
        // Then the tracked file is back as committed and the untracked one is gone from the disk
        assert_eq!((std::fs::read_to_string(repo.path().join("README.md")).unwrap(), repo.path().join("junk.txt").exists()), ("changed\n".to_string(), false));
        // When a branch exists and the phone lists the branches and checks it out
        repo.git(&["branch", "feature"]);
        let branches = served(case, &mut phone, &chain, "git.localBranches", json!({}), "git.refs", "git_refs");
        assert_eq!((branches["result"]["current"].clone(), branches["result"]["branches"].clone()), (json!("main"), json!(["feature", "main"])));
        let moved = served(case, &mut phone, &chain, "git.checkout", json!({"branch": "feature"}), "git.checkout", "git_checkout");
        // Then the repository is on it
        assert_eq!((moved["result"]["branch"].clone(), repo.git(&["rev-parse", "--abbrev-ref", "HEAD"])), (json!("feature"), "feature".to_string()));
        // And a branch git does not have is refused after the request reached rca
        let unknown = phone.refused(case, &chain, &["rpc:git.checkout"], "git.checkout", sel(json!({"branch": "nope"})));
        answered(case, "git.checkout", false);
        assert_eq!(unknown["error"]["code"], json!("runtime_error"));
        // When the branch gains a commit of two files and is compared with main
        repo.write("b.txt", "two\n");
        repo.write("c.txt", "three\n");
        served(case, &mut phone, &chain, "git.bulkStage", json!({"filePaths": [], "scope": "all"}), "git.stage", "git_stage");
        served(case, &mut phone, &chain, "git.commit", json!({"message": "Feature work"}), "git.commit", "git_commit");
        let compare = served(case, &mut phone, &chain, "git.branchCompare", json!({"baseRef": "main"}), "git.compare", "git_compare");
        // Then the summary is ready with both ends' commits and the two added files with their line counts
        let summary = &compare["result"]["summary"];
        assert_eq!((summary["status"].clone(), summary["changedFiles"].clone(), summary["commitsAhead"].clone(), summary["compareRef"].clone()), (json!("ready"), json!(2), json!(1), json!("feature")));
        assert_eq!((summary["headOid"].clone(), summary["baseOid"].clone(), summary["mergeBase"].clone()), (json!(repo.head()), json!(repo.git(&["rev-parse", "main"])), json!(repo.git(&["rev-parse", "main"]))));
        assert_eq!(compare["result"]["entries"], json!([{"path": "b.txt", "status": "added", "added": 1, "removed": 0}, {"path": "c.txt", "status": "added", "added": 1, "removed": 0}]));
        // When the commit is compared with its parent
        let commit = served(case, &mut phone, &chain, "git.commitCompare", json!({"commitId": repo.head()}), "git.compare", "git_compare");
        // Then its summary names the parent and the same two files, and an id git has not got is an invalid commit, not a failure
        assert_eq!((commit["result"]["summary"]["parentOid"].clone(), commit["result"]["entries"].as_array().unwrap().len()), (json!(repo.git(&["rev-parse", "HEAD~1"])), 2));
        let missing = phone.call(case, &chain, &[], "git.commitCompare", sel(json!({"commitId": "0".repeat(40)})));
        assert_eq!(missing["result"]["summary"]["status"], json!("invalid-commit"));
        let bad = phone.call(case, &chain, &[], "git.branchCompare", sel(json!({"baseRef": "no-such-branch"})));
        assert_eq!(bad["result"]["summary"]["status"], json!("invalid-base"));
    });
}

#[test]
fn commits_are_published_fetched_pulled_and_fast_forwarded_against_a_bare_remote_and_the_repository_holds_what_the_phone_moved() {
    exec_testkit::case!(SUITE, |case| {
        // Given a repository whose remote is a bare repository, and a paired phone
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let repo = &chain.repo;
        let env = repo.git_env();
        let bare = tempfile::Builder::new().prefix("orca-chain-remote-").tempdir().unwrap();
        git_in(bare.path(), &env, &["init", "--quiet", "--bare", "--initial-branch=main"]);
        repo.git(&["remote", "add", "origin", bare.path().to_str().unwrap()]);
        let upstream = |case: &mut exec_testkit::Case, phone: &mut Phone| served(case, phone, &chain, "git.upstreamStatus", json!({}), "git.upstreamStatus", "git_upstream")["result"].clone();
        assert_eq!(upstream(case, &mut phone)["hasUpstream"], json!(false));
        // When the phone publishes the branch
        served(case, &mut phone, &chain, "git.push", json!({"publish": true}), "git.push", "git_push");
        // Then the remote holds main at the repository's head and the branch tracks it
        assert_eq!(git_in(bare.path(), &env, &["rev-parse", "main"]), repo.head());
        assert_eq!(upstream(case, &mut phone), json!({"hasUpstream": true, "upstreamName": "origin/main", "ahead": 0, "behind": 0}));
        // When another clone pushes a commit and the phone fetches through a push target
        let other = tempfile::Builder::new().prefix("orca-chain-other-").tempdir().unwrap();
        let work = other.path().join("work");
        git_in(other.path(), &env, &["clone", "--quiet", bare.path().to_str().unwrap(), "work"]);
        let push_from_other = |name: &str| {
            std::fs::write(work.join(name), "from the remote\n").unwrap();
            git_in(&work, &env, &["add", "--all"]);
            git_in(&work, &env, &["commit", "--quiet", "--message", &format!("add {name}")]);
            git_in(&work, &env, &["push", "--quiet", "origin", "main"]);
            git_in(&work, &env, &["rev-parse", "HEAD"])
        };
        let first = push_from_other("remote1.txt");
        assert_eq!(upstream(case, &mut phone)["behind"], json!(0));
        served(case, &mut phone, &chain, "git.fetch", json!({"pushTarget": {"remoteName": "origin", "branchName": "main"}}), "git.fetch", "git_fetch");
        // Then the branch is one behind, and the file is not yet in the tree
        assert_eq!((upstream(case, &mut phone)["behind"].clone(), repo.path().join("remote1.txt").exists()), (json!(1), false));
        // When it fast-forwards
        served(case, &mut phone, &chain, "git.fastForward", json!({}), "git.fastForward", "git_fast_forward");
        // Then the repository is at the commit the other clone pushed, with its file
        assert_eq!((repo.head(), repo.path().join("remote1.txt").exists()), (first, true));
        // When another commit is pushed and the phone pulls through a push target
        let second = push_from_other("remote2.txt");
        served(case, &mut phone, &chain, "git.pull", json!({"pushTarget": {"remoteName": "origin", "branchName": "main"}}), "git.pull", "git_pull");
        // Then the repository is at the newest commit
        assert_eq!((repo.head(), repo.path().join("remote2.txt").exists()), (second, true));
        // When the phone commits locally and pushes
        repo.write("local.txt", "mine\n");
        served(case, &mut phone, &chain, "git.bulkStage", json!({"filePaths": [], "scope": "all"}), "git.stage", "git_stage");
        served(case, &mut phone, &chain, "git.commit", json!({"message": "local"}), "git.commit", "git_commit");
        assert_eq!(upstream(case, &mut phone)["ahead"], json!(1));
        served(case, &mut phone, &chain, "git.push", json!({}), "git.push", "git_push");
        // Then the remote has it
        assert_eq!(git_in(bare.path(), &env, &["rev-parse", "main"]), repo.head());
        // And a remote the repository did not configure is refused after the request reached rca, and a push target of another branch is refused before
        let fork = phone.refused(case, &chain, &["rpc:git.fetch"], "git.fetch", sel(json!({"pushTarget": {"remoteName": "fork", "branchName": "main"}})));
        answered(case, "git.fetch", false);
        assert!(fork["error"]["message"].as_str().unwrap().contains("fork"), "{fork}");
        let other_branch = phone.refused(case, &chain, &["rpc:git.push"], "git.push", sel(json!({"pushTarget": {"remoteName": "origin", "branchName": "elsewhere"}})));
        assert_eq!(other_branch["error"]["code"], json!("invalid_argument"));
        assert!(!last_window(case).iter().any(|(m, _)| m == "git.push"), "a push of another branch must not reach rca");
        assert_eq!(git_in(bare.path(), &env, &["branch", "--list"]), "* main");
    });
}

#[test]
fn a_merge_stopped_on_conflicts_is_read_resolved_completed_and_aborted_and_the_repository_ends_as_git_would_leave_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given three branches that each conflict with main, one by deleting a file main edited
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let mut direct = chain.direct();
        let repo = &chain.repo;
        repo.write("file2.txt", "base\n");
        repo.commit_all("file2");
        for name in ["side", "side2"] {
            repo.git(&["checkout", "--quiet", "-b", name]);
            repo.write("README.md", &format!("{name}\n"));
            repo.commit_all(name);
            repo.git(&["checkout", "--quiet", "main"]);
        }
        repo.git(&["checkout", "--quiet", "-b", "del"]);
        repo.git(&["rm", "--quiet", "file2.txt"]);
        repo.git(&["commit", "--quiet", "--message", "delete file2"]);
        repo.git(&["checkout", "--quiet", "main"]);
        repo.write("README.md", "main\n");
        repo.write("file2.txt", "main edit\n");
        repo.commit_all("main edits");
        let start = |direct: &mut Direct, branch: &str| {
            let merged = direct.call("git", "git.merge", json!({"workspace": "w1", "op": "start", "ref": branch})).expect("the merge starts");
            assert_eq!(merged["ok"], json!(false), "{branch} should conflict: {merged}");
        };
        // When main merges the first and the phone reads the stopped merge
        start(&mut direct, "side");
        let status = served(case, &mut phone, &chain, "git.status", json!({}), "git.status", "git_status");
        // Then the operation is a merge and the file is one unresolved conflict, modified on both sides
        assert_eq!(status["result"]["conflictOperation"], json!("merge"));
        let conflict = &status["result"]["entries"][0];
        assert_eq!((conflict["path"].clone(), conflict["conflictKind"].clone(), conflict["conflictStatus"].clone()), (json!("README.md"), json!("both_modified"), json!("unresolved")));
        // When it completes the merge before resolving
        let early = served(case, &mut phone, &chain, "git.completeMerge", json!({}), "git.merge", "git_merge");
        // Then it is not a success and names the file, and the merge is still open
        assert_eq!(early["result"]["success"], json!(false));
        assert!(early["result"]["error"].as_str().unwrap().contains("README.md"), "{early}");
        assert!(repo.path().join(".git/MERGE_HEAD").exists());
        // When the file is edited, marked resolved and the merge completed
        repo.write("README.md", "resolved\n");
        served(case, &mut phone, &chain, "git.resolveConflict", json!({"filePath": "README.md", "resolution": "mark_resolved"}), "git.conflictResolve", "git_conflict_resolve");
        let done = served(case, &mut phone, &chain, "git.completeMerge", json!({}), "git.merge", "git_merge");
        // Then git made a merge commit of two parents and no merge is open
        assert_eq!(done["result"]["success"], json!(true), "{done}");
        assert_eq!(repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"]).split(' ').count(), 3);
        assert!(!repo.path().join(".git/MERGE_HEAD").exists());
        // When main merges the branch that deleted a file main edited, and the phone keeps the deletion
        start(&mut direct, "del");
        let kinds = served(case, &mut phone, &chain, "git.status", json!({}), "git.status", "git_status");
        assert_eq!(kinds["result"]["entries"][0]["conflictKind"], json!("deleted_by_them"));
        served(case, &mut phone, &chain, "git.resolveConflict", json!({"filePath": "file2.txt", "resolution": "keep_deletion"}), "git.conflictResolve", "git_conflict_resolve");
        let kept = served(case, &mut phone, &chain, "git.completeMerge", json!({}), "git.merge", "git_merge");
        // Then the file is gone from the disk and from the tree of the merge commit
        assert_eq!(kept["result"]["success"], json!(true), "{kept}");
        assert_eq!((repo.path().join("file2.txt").exists(), repo.git(&["ls-tree", "--name-only", "HEAD"]).contains("file2.txt")), (false, false));
        // When the third merge stops and the phone aborts it
        start(&mut direct, "side2");
        served(case, &mut phone, &chain, "git.abortMerge", json!({}), "git.merge", "git_merge");
        // Then the tree is clean, the merge is closed and the file is as the last merge left it
        assert_eq!((repo.git(&["status", "--porcelain"]), repo.path().join(".git/MERGE_HEAD").exists(), std::fs::read_to_string(repo.path().join("README.md")).unwrap()), (String::new(), false, "resolved\n".to_string()));
        // And a resolution for a file that is not in conflict is refused after the request reached rca
        let stray = phone.refused(case, &chain, &["rpc:git.resolveConflict"], "git.resolveConflict", sel(json!({"filePath": "README.md", "resolution": "mark_resolved"})));
        answered(case, "git.conflictResolve", false);
        assert_eq!(stray["error"]["code"], json!("invalid_argument"));
    });
}

#[test]
fn a_rebase_from_a_base_is_applied_and_one_that_stops_on_conflicts_is_aborted_and_the_branch_is_as_it_was() {
    exec_testkit::case!(SUITE, |case| {
        // Given two topic branches cut from the first commit, and a main that has moved on and edited the readme
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let repo = &chain.repo;
        for (name, file) in [("clean", "clean.txt"), ("clash", "README.md")] {
            repo.git(&["checkout", "--quiet", "-b", name]);
            repo.write(file, &format!("{name}\n"));
            repo.commit_all(name);
            repo.git(&["checkout", "--quiet", "main"]);
        }
        repo.write("README.md", "main moved\n");
        repo.commit_all("main moved");
        let main = repo.git(&["rev-parse", "main"]);
        // When the phone rebases the clean topic from main
        repo.git(&["checkout", "--quiet", "clean"]);
        served(case, &mut phone, &chain, "git.rebaseFromBase", json!({"baseRef": "main"}), "git.rebase", "git_rebase");
        // Then main is an ancestor of the topic and the topic's commit is on top
        assert_eq!((repo.git(&["merge-base", "main", "HEAD"]), repo.git(&["log", "-1", "--format=%s"])), (main.clone(), "clean".to_string()));
        // When the clashing topic is rebased
        repo.git(&["checkout", "--quiet", "clash"]);
        let tip = repo.head();
        let stopped = phone.refused(case, &chain, &["rpc:git.rebaseFromBase"], "git.rebaseFromBase", sel(json!({"baseRef": "main"})));
        answered(case, "git.rebase", true);
        // Then the refusal names the file, and the status shows a rebase in progress with the conflict
        assert_eq!(stopped["error"]["code"], json!("runtime_error"));
        assert!(stopped["error"]["message"].as_str().unwrap().contains("README.md"), "{stopped}");
        let status = served(case, &mut phone, &chain, "git.status", json!({}), "git.status", "git_status");
        assert_eq!((status["result"]["conflictOperation"].clone(), status["result"]["entries"][0]["conflictStatus"].clone()), (json!("rebase"), json!("unresolved")));
        // When the phone aborts the rebase
        served(case, &mut phone, &chain, "git.abortRebase", json!({}), "git.rebase", "git_rebase");
        // Then the branch is at its old tip and the tree is clean
        assert_eq!((repo.head(), repo.git(&["status", "--porcelain"]), repo.git(&["rev-parse", "--abbrev-ref", "HEAD"])), (tip, String::new(), "clash".to_string()));
    });
}
