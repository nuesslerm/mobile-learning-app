//! The git rows through the `Host` dispatcher with an in-memory repository as rca: each reply is read the way the phone reads it, a change is read
//! back from the status or the history. Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_git::testing::Repos;
use orca_map_git::GitArea;
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    repo: Repos,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        Workspaces::default().workspace("main", "root", Some("/srv/main")).install(&agent);
        let repo = Repos::default();
        repo.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(GitArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, repo, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn call(&mut self, method: &str, mut params: Value) -> Value {
        self.n += 1;
        params["worktree"] = json!("main");
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        serde_json::from_str(&self.conn.on_message(&frame)[0]).unwrap()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> (String, String) {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }

    /// The phone's source control list: each entry as `area:status:path`.
    fn entries(&mut self) -> Vec<String> {
        let status = self.ok("git.status", json!({}));
        status["entries"].as_array().unwrap().iter().map(|e| format!("{}:{}:{}", e["area"].as_str().unwrap(), e["status"].as_str().unwrap(), e["path"].as_str().unwrap())).collect()
    }

    fn sent(&self) -> Vec<(String, Value)> {
        self.agent.requests().into_iter().filter(|(m, _)| m.starts_with("git.")).collect()
    }
}

#[test]
fn given_a_working_tree_with_every_kind_of_change_when_the_status_is_read_then_each_file_is_listed_in_the_area_the_phone_stages_it_from() {
    // Given a modified file, a file modified both in the index and in the tree, an untracked file, a renamed one and a merge conflict
    let mut rig = Rig::new();
    rig.repo.file("a.txt", ".", "M").file("both.txt", "M", "M").file("new.txt", "?", "?").file("gone.txt", ".", "D").conflict("c.txt", "U", "U", "merge").track("origin/main", 2, 1);
    // When the phone reads the status
    let status = rig.ok("git.status", json!({}));
    // Then every change is in the area it can be staged or unstaged from, and the conflict is one unresolved row
    assert_eq!(rig.entries(), ["unstaged:modified:a.txt", "staged:modified:both.txt", "unstaged:modified:both.txt", "untracked:untracked:new.txt", "unstaged:deleted:gone.txt", "unstaged:modified:c.txt"]);
    let conflict = status["entries"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((conflict["conflictKind"].clone(), conflict["conflictStatus"].clone()), (json!("both_modified"), json!("unresolved")));
    // And the operation, the branch and how far it is from its upstream are read with it
    assert_eq!((status["conflictOperation"].clone(), status["branch"].clone(), status["upstreamStatus"].clone()), (json!("merge"), json!("main"), json!({"hasUpstream": true, "upstreamName": "origin/main", "ahead": 2, "behind": 1})));
}

#[test]
fn given_changed_files_when_staged_unstaged_and_discarded_then_the_status_shows_each_file_where_git_would_put_it() {
    // Given a modified, an untracked and a deleted file
    let mut rig = Rig::new();
    rig.repo.file("a.txt", ".", "M").file("new.txt", "?", "?").file("gone.txt", ".", "D");
    // When one file is staged
    rig.ok("git.stage", json!({"filePath": "a.txt"}));
    // Then it moved to the staged area and left the unstaged one
    assert_eq!(rig.entries(), ["staged:modified:a.txt", "untracked:untracked:new.txt", "unstaged:deleted:gone.txt"]);
    // When it is unstaged again
    rig.ok("git.unstage", json!({"filePath": "a.txt"}));
    // Then it is back in the unstaged area
    assert_eq!(rig.entries()[0], "unstaged:modified:a.txt");
    // When everything is staged with the scope `all`
    let staged = rig.ok("git.bulkStage", json!({"filePaths": [], "scope": "all"}));
    // Then the untracked file is added too, and the answer echoes the scope it staged
    assert_eq!(rig.entries(), ["staged:modified:a.txt", "staged:added:new.txt", "staged:deleted:gone.txt"]);
    assert_eq!(staged["stagedScope"], json!("all"));
    // When the three are unstaged in one call and the scope `tracked` stages
    rig.ok("git.bulkUnstage", json!({"filePaths": ["a.txt", "new.txt", "gone.txt"]}));
    rig.ok("git.bulkStage", json!({"filePaths": [], "scope": "tracked"}));
    // Then the untracked file stays out
    assert_eq!(rig.entries(), ["staged:modified:a.txt", "untracked:untracked:new.txt", "staged:deleted:gone.txt"]);
    // When the phone discards the untracked file
    rig.ok("git.discard", json!({"filePath": "new.txt"}));
    // Then the untracked file is gone from the list and the staged change stays
    assert_eq!(rig.entries(), ["staged:modified:a.txt", "staged:deleted:gone.txt"]);
}

#[test]
fn given_staged_changes_when_committed_then_the_history_gains_the_commit_and_the_status_is_clean_and_a_commit_with_nothing_staged_is_not_a_success() {
    // Given a staged file and a modified one
    let mut rig = Rig::new();
    rig.repo.file("a.txt", "M", ".").file("b.txt", ".", "M");
    // When the phone commits with a multi-line message
    let done = rig.ok("git.commit", json!({"message": "Add a\n\nbecause"}));
    // Then it succeeded, the staged file left the list, the other change stays, and the history's newest commit carries the subject
    assert_eq!(done["success"], json!(true));
    assert_eq!(rig.entries(), ["unstaged:modified:b.txt"]);
    let history = rig.ok("git.history", json!({"limit": 2}));
    let subjects: Vec<&str> = history["items"].as_array().unwrap().iter().map(|i| i["subject"].as_str().unwrap()).collect();
    assert_eq!((subjects, history["hasMore"].clone(), history["items"][0]["author"].clone()), (vec!["Add a", "first"], json!(false), json!("A U Thor")));
    assert_eq!(history["items"][0]["parentIds"].as_array().unwrap().len(), 1);
    // When it commits again with nothing staged
    let again = rig.ok("git.commit", json!({"message": "nothing"}));
    // Then the host's answer is a failure with the executor's sentence, and no commit was made
    assert_eq!((again["success"].clone(), again["error"].clone()), (json!(false), json!("nothing to commit")));
    assert_eq!(rig.repo.subjects(), ["first", "Add a"]);
    // And a blank message is refused before rca is asked
    let before = rig.sent().len();
    assert_eq!(rig.err("git.commit", json!({"message": "  "})).0, "invalid_argument");
    assert_eq!(rig.sent().len(), before);
}

#[test]
fn given_branches_when_listed_and_checked_out_then_the_current_branch_follows_and_an_unknown_one_is_refused_with_git_s_words() {
    // Given two branches
    let mut rig = Rig::new();
    rig.repo.branch("feature");
    // When the phone lists them
    let before = rig.ok("git.localBranches", json!({}));
    // Then main is the current one
    assert_eq!((before["current"].clone(), before["branches"].clone()), (json!("main"), json!(["main", "feature"])));
    // When it checks out the other
    let moved = rig.ok("git.checkout", json!({"branch": "feature"}));
    // Then the list and the status say it is current
    assert_eq!(moved, json!({"ok": true, "branch": "feature"}));
    assert_eq!(rig.ok("git.localBranches", json!({}))["current"], json!("feature"));
    assert_eq!(rig.ok("git.status", json!({}))["branch"], json!("feature"));
    // And a branch git does not know is a runtime error that keeps git's sentence, and the branch stays
    let (code, message) = rig.err("git.checkout", json!({"branch": "nope"}));
    assert_eq!((code.as_str(), message.contains("did not match")), ("runtime_error", true));
    assert_eq!(rig.repo.current(), "feature");
}

#[test]
fn given_a_remote_when_pushed_fetched_pulled_and_fast_forwarded_then_the_upstream_is_published_and_the_branch_catches_up_and_an_unknown_remote_is_refused() {
    // Given a configured remote that will have two new commits, and a local commit to publish
    let mut rig = Rig::new();
    rig.repo.remote("origin").incoming(2).file("a.txt", "M", ".");
    rig.ok("git.commit", json!({"message": "local"}));
    assert_eq!(rig.ok("git.upstreamStatus", json!({}))["hasUpstream"], json!(false));
    // When the phone pushes
    rig.ok("git.push", json!({"publish": true}));
    // Then the branch is published under the remote and its upstream is set
    assert_eq!(rig.repo.pushed(), [("origin".to_string(), "main".to_string(), true)]);
    assert_eq!(rig.ok("git.upstreamStatus", json!({})), json!({"hasUpstream": true, "upstreamName": "origin/main", "ahead": 0, "behind": 0}));
    // When it fetches through an explicit push target
    rig.ok("git.fetch", json!({"pushTarget": {"remoteName": "origin", "branchName": "main"}}));
    // Then the remote was fetched and the branch is two behind
    assert_eq!(rig.repo.fetched(), ["origin"]);
    assert_eq!(rig.ok("git.upstreamStatus", json!({}))["behind"], json!(2));
    // When it pulls
    rig.ok("git.pull", json!({}));
    // Then the incoming commits are on the branch
    assert_eq!((rig.repo.behind(), rig.repo.subjects().len()), (0, 4));
    // When two more arrive and it fast-forwards
    rig.ok("git.fetch", json!({}));
    rig.ok("git.fastForward", json!({}));
    assert_eq!(rig.ok("git.upstreamStatus", json!({}))["behind"], json!(0));
    // And a remote the repository does not have is refused with rca's words, and a push target of another branch is refused before anything is pushed
    assert!(rig.err("git.fetch", json!({"pushTarget": {"remoteName": "fork", "branchName": "main"}})).1.contains("not configured"));
    let pushes = rig.repo.pushed().len();
    assert_eq!(rig.err("git.push", json!({"pushTarget": {"remoteName": "origin", "branchName": "other"}})).0, "invalid_argument");
    assert_eq!(rig.repo.pushed().len(), pushes);
    // And the upstream status of a push target is not answered as if it were the branch's own
    assert_eq!(rig.err("git.upstreamStatus", json!({"pushTarget": {"remoteName": "origin", "branchName": "main"}})).0, "invalid_argument");
}

#[test]
fn given_a_merge_stopped_on_conflicts_when_resolved_completed_or_aborted_then_the_status_follows_and_an_unfinished_merge_names_its_files() {
    // Given a merge stopped on two files
    let mut rig = Rig::new();
    rig.repo.conflict("c.txt", "U", "U", "merge").conflict("d.txt", "A", "A", "merge");
    assert_eq!(rig.ok("git.status", json!({}))["conflictOperation"], json!("merge"));
    // When the phone completes the merge before resolving
    let early = rig.ok("git.completeMerge", json!({}));
    // Then it is not a success and names the files
    assert_eq!(early["success"], json!(false));
    assert!(early["error"].as_str().unwrap().contains("c.txt") && early["error"].as_str().unwrap().contains("d.txt"));
    // When both are resolved and the merge is completed
    rig.ok("git.resolveConflict", json!({"filePath": "c.txt", "resolution": "mark_resolved"}));
    rig.ok("git.resolveConflict", json!({"filePath": "d.txt", "resolution": "keep_file"}));
    assert_eq!(rig.entries(), ["staged:modified:c.txt", "staged:modified:d.txt"]);
    let done = rig.ok("git.completeMerge", json!({}));
    // Then the merge is over and its commit is in the history
    assert_eq!(done["success"], json!(true));
    assert_eq!((rig.ok("git.status", json!({}))["conflictOperation"].clone(), rig.repo.subjects().last().cloned()), (json!("unknown"), Some("merge finished".to_string())));
    // When a second merge stops and the phone aborts it
    rig.repo.conflict("e.txt", "U", "U", "merge");
    rig.ok("git.abortMerge", json!({}));
    // Then the conflict and the operation are gone
    assert_eq!((rig.entries().contains(&"unstaged:modified:e.txt".to_string()), rig.repo.operation()), (false, None));
    // And a file that is not conflicted is refused by the executor, and a resolution this host does not know is invalid
    assert_eq!(rig.err("git.resolveConflict", json!({"filePath": "zzz", "resolution": "mark_resolved"})).0, "invalid_argument");
    assert_eq!(rig.err("git.resolveConflict", json!({"filePath": "zzz", "resolution": "whatever"})).0, "invalid_argument");
}

#[test]
fn given_files_one_side_deleted_when_the_deletion_is_kept_then_the_file_leaves_the_status_by_the_side_that_deleted_it_and_a_file_nobody_deleted_is_refused() {
    // Given a file deleted by them, one deleted by us, one deleted by both and one modified on both sides
    let mut rig = Rig::new();
    rig.repo.conflict("by-them.txt", "U", "D", "merge").conflict("by-us.txt", "D", "U", "merge").conflict("by-both.txt", "D", "D", "merge").conflict("edited.txt", "U", "U", "merge");
    // When the phone keeps each deletion
    for path in ["by-them.txt", "by-us.txt", "by-both.txt"] {
        rig.ok("git.resolveConflict", json!({"filePath": path, "resolution": "keep_deletion"}));
    }
    // Then the three are no longer conflicts, and each was resolved by the side that deleted it
    assert_eq!(rig.entries(), ["unstaged:modified:edited.txt"]);
    let sides: Vec<String> = rig.sent().into_iter().filter(|(m, _)| m == "git.conflictResolve").map(|(_, p)| p["resolution"].as_str().unwrap().to_string()).collect();
    assert_eq!(sides, ["theirs", "ours", "ours"]);
    // And the file modified on both sides has no deletion to keep, and stays in conflict
    assert_eq!(rig.err("git.resolveConflict", json!({"filePath": "edited.txt", "resolution": "keep_deletion"})).0, "invalid_argument");
    assert_eq!(rig.entries(), ["unstaged:modified:edited.txt"]);
}

#[test]
fn given_a_rebase_that_stops_on_conflicts_when_asked_and_aborted_then_the_error_names_the_file_and_the_status_shows_the_rebase_until_it_is_aborted() {
    // Given a base whose rebase conflicts in one file
    let mut rig = Rig::new();
    rig.repo.rebase_stops_on("r.txt");
    // When the phone rebases from it
    let (code, message) = rig.err("git.rebaseFromBase", json!({"baseRef": "origin/main"}));
    // Then the refusal names the file, and the status shows the rebase with its conflict
    assert_eq!(code, "runtime_error");
    assert!(message.contains("r.txt"), "{message}");
    assert_eq!(rig.ok("git.status", json!({}))["conflictOperation"], json!("rebase"));
    assert_eq!(rig.entries(), ["unstaged:modified:r.txt"]);
    // When it is aborted
    rig.ok("git.abortRebase", json!({}));
    // Then the status is clean again, and a base that looks like an option is refused before rca is asked
    assert_eq!((rig.entries(), rig.repo.operation()), (Vec::<String>::new(), None));
    let before = rig.sent().len();
    assert_eq!(rig.err("git.rebaseFromBase", json!({"baseRef": "--exec=x"})).0, "invalid_argument");
    assert_eq!(rig.sent().len(), before);
}

#[test]
fn given_a_branch_and_its_commits_when_compared_then_the_summary_and_files_are_the_review_the_phone_shows_and_a_bad_base_is_a_summary_status() {
    // Given a branch that changed two files
    let mut rig = Rig::new();
    rig.repo.changed("a.txt", "modified").changed("b.txt", "added");
    rig.ok("git.commit", json!({"message": "x"}));
    // When the phone compares it with main
    let compare = rig.ok("git.branchCompare", json!({"baseRef": "main"}));
    // Then the summary is ready with the two files and the oids of both ends, and each file carries its counts
    let summary = &compare["summary"];
    assert_eq!((summary["status"].clone(), summary["changedFiles"].clone(), summary["compareRef"].clone()), (json!("ready"), json!(2), json!("main")));
    assert_eq!(summary["headOid"], summary["baseOid"]);
    assert_eq!(compare["entries"][1], json!({"path": "b.txt", "status": "added", "added": 2, "removed": 1}));
    // When the base is one git cannot resolve
    let bad = rig.ok("git.branchCompare", json!({"baseRef": "nope"}));
    // Then the summary says so with git's words and lists no files
    assert_eq!((bad["summary"]["status"].clone(), bad["entries"].clone()), (json!("invalid-base"), json!([])));
    assert!(bad["summary"]["errorMessage"].as_str().unwrap().contains("unknown revision"));
    // When the phone compares the newest commit with its parent, and a commit that is not in the repository
    rig.repo.file("c.txt", "M", ".");
    rig.ok("git.commit", json!({"message": "second"}));
    let newest = rig.ok("git.history", json!({}))["items"][0]["id"].as_str().unwrap().to_string();
    let parent = rig.ok("git.history", json!({}))["items"][0]["parentIds"][0].as_str().unwrap().to_string();
    let commit = rig.ok("git.commitCompare", json!({"commitId": newest}));
    let missing = rig.ok("git.commitCompare", json!({"commitId": format!("{:040x}", 99)}));
    // Then the first is ready against its parent, the second an invalid commit, and an id that is not a full object id is refused
    assert_eq!((commit["summary"]["status"].clone(), commit["summary"]["parentOid"].clone(), commit["summary"]["changedFiles"].clone()), (json!("ready"), json!(parent), json!(2)));
    assert_eq!(missing["summary"]["status"], json!("invalid-commit"));
    assert_eq!(rig.err("git.commitCompare", json!({"commitId": "abc"})).0, "invalid_argument");
}

#[test]
fn given_a_selector_no_workspace_owns_when_a_row_is_called_then_it_is_the_hosts_selector_error_and_rca_is_asked_nothing_of_git() {
    // Given a host whose only workspace is main
    let mut rig = Rig::new();
    // When a row names another worktree, and one names none
    let ghost = rig.agent.clone();
    let frame = json!({"id": "g1", "method": "git.status", "params": {"worktree": "path:/nowhere"}}).to_string();
    let missing = json!({"id": "g2", "method": "git.status", "params": {}}).to_string();
    let replies: Vec<Value> = [frame, missing].iter().map(|f| serde_json::from_str(&rig.conn.on_message(f)[0]).unwrap()).collect();
    // Then the first is selector_not_found and the second invalid, and no git request was sent
    assert_eq!((replies[0]["error"]["code"].clone(), replies[1]["error"]["code"].clone()), (json!("selector_not_found"), json!("invalid_argument")));
    assert!(ghost.requests().iter().all(|(m, _)| !m.starts_with("git.")));
}

#[test]
fn given_rca_answers_without_what_its_contract_requires_when_the_status_or_the_history_is_read_then_the_call_is_refused_and_no_blank_row_is_made() {
    for (method, answer, missing) in [
        ("git.status", json!({"branch": "main", "files": [{"index": ".", "worktree": "M", "conflicted": false}]}), "a file with no path"),
        ("git.status", json!({"branch": "main", "files": [{"path": "a.txt", "worktree": "M", "conflicted": false}]}), "a file with no index"),
        ("git.status", json!({"branch": "main"}), "no files list"),
        ("git.history", json!({"commits": [{"sha": "a".repeat(40), "body": ""}]}), "a commit with no subject"),
        ("git.history", json!({"commits": [{"sha": "a".repeat(40), "subject": "s"}]}), "a commit with no body"),
        ("git.history", json!({"commits": [{"subject": "s", "body": ""}]}), "a commit with no sha"),
        ("git.history", json!({"truncated": false}), "no commits list"),
    ] {
        // Given rca answers the request with a record that lacks a required field (the status it is asked alongside being sound)
        let mut rig = Rig::new();
        rig.agent.reply("git.status", |_| Ok(json!({"branch": "main", "ahead": 0, "behind": 0, "files": []})));
        rig.agent.reply(method, move |_| Ok(answer.clone()));
        // When the phone reads it
        let (code, message) = rig.err(method, json!({}));
        // Then the call is refused naming what was missing, instead of a blank row
        assert_eq!(code, "runtime_error", "{method}: {message}");
        assert!(message.contains(missing), "{method}: {message}");
    }
}

#[test]
fn given_a_staged_change_when_a_message_is_generated_then_the_model_is_given_the_staged_files_and_the_message_is_the_phones_to_commit_with() {
    // Given one staged and one unstaged file
    let mut rig = Rig::new();
    rig.repo.file("a.txt", "M", ".").file("b.txt", ".", "M");
    // When the phone generates a commit message
    let generated = rig.ok("git.generateCommitMessage", json!({}));
    // Then the answer is the model's message for the staged file only, and nothing was committed
    assert_eq!(generated, json!({"success": true, "message": "feat: change a.txt"}));
    assert_eq!(rig.repo.model_asked(), [["a.txt"]]);
    assert_eq!(rig.repo.subjects(), ["first"]);
    // And the phone commits with that message, which lands in the history
    rig.ok("git.commit", json!({"message": generated["message"]}));
    assert_eq!(rig.repo.subjects().last().unwrap(), "feat: change a.txt");
}

#[test]
fn given_nothing_staged_or_no_model_when_a_message_is_generated_then_the_answer_is_a_failed_generation_with_rca_s_reason_and_other_refusals_stay_refusals() {
    // Given an unstaged change only
    let mut rig = Rig::new();
    rig.repo.file("b.txt", ".", "M");
    // When the phone generates a message
    let nothing = rig.ok("git.generateCommitMessage", json!({}));
    // Then it is a failed generation naming that nothing is staged, and the model was not asked
    assert_eq!((nothing["success"].clone(), nothing["error"].as_str().unwrap().contains("no staged change")), (json!(false), true));
    assert!(rig.repo.model_asked().is_empty());
    // When the service has no model
    let mut down = Rig::new();
    down.repo.file("a.txt", "M", ".").model_down();
    let unavailable = down.ok("git.generateCommitMessage", json!({}));
    // Then that is a failed generation too, with the service's words
    assert_eq!(unavailable, json!({"success": false, "error": "no model is configured"}));
}

#[test]
fn given_rca_answers_a_generation_without_a_message_when_it_is_asked_then_the_call_is_refused() {
    let mut rig = Rig::new();
    rig.agent.reply("git.generateCommitMessage", |_| Ok(json!({"model": "m"})));
    let (code, message) = rig.err("git.generateCommitMessage", json!({}));
    assert_eq!(code, "runtime_error");
    assert!(message.contains("no message"), "{message}");
}
