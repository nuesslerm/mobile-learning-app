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
    ws: Workspaces,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let ws = Workspaces::default();
        ws.workspace("main", "root", Some("/srv/main")).install(&agent);
        let repo = Repos::default();
        repo.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(GitArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, repo, ws, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn call(&mut self, method: &str, mut params: Value) -> Value {
        self.n += 1;
        params["worktree"] = json!("main");
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        serde_json::from_str(&self.conn.on_message(&frame)[0]).unwrap()
    }

    /// A request sent as the phone built it, with no selector added.
    fn raw(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
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

    /// What rca was asked about the repository and its files.
    fn sent_all(&self) -> Vec<(String, Value)> {
        self.agent.requests().into_iter().filter(|(m, _)| m.starts_with("git.") || m.starts_with("file.")).collect()
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

#[test]
fn given_a_branch_ahead_of_its_base_when_pull_request_fields_are_generated_then_the_answer_is_the_models_fields_in_the_phones_shape_and_nothing_is_opened() {
    // Given a branch two commits ahead of main
    let mut rig = Rig::new();
    rig.repo.branch_ahead(2);
    // When the phone asks for the fields with the form as it stands
    let generated = rig.ok("git.generatePullRequestFields", json!({"base": "main", "title": "", "body": "", "draft": true, "provider": "github", "useTemplate": false, "agentCmdOverrides": {}}));
    // Then the answer is `success` with the fields (the base is the one asked for), and rca was given the form without the phone's agent settings
    assert_eq!(generated, json!({"success": true, "fields": {"base": "main", "title": "Merge main into main", "body": "## Problem\n\nx\n\n## Solution\n\ny", "draft": true}}));
    assert_eq!(rig.repo.pull_request_asked(), [json!({"base": "main", "title": "", "body": "", "draft": true})]);
    assert_eq!(rig.repo.subjects(), ["first"]);
}

#[test]
fn given_no_commit_ahead_or_no_model_when_pull_request_fields_are_generated_then_the_answer_is_a_failed_generation_with_rca_s_reason() {
    // Given a branch level with its base
    let mut level = Rig::new();
    // When the phone asks, Then it is a failed generation naming the base, and the model was not asked
    let nothing = level.ok("git.generatePullRequestFields", json!({"base": "main", "title": "t", "body": "b", "draft": false}));
    assert_eq!(nothing, json!({"success": false, "error": "the branch has no commit that main lacks"}));
    assert!(level.repo.pull_request_asked().is_empty());
    // Given a branch ahead and a service without a model, Then that is a failed generation too, with the service's words
    let mut down = Rig::new();
    down.repo.branch_ahead(1);
    down.repo.model_down();
    assert_eq!(down.ok("git.generatePullRequestFields", json!({"base": "main", "title": "t", "body": "b", "draft": false})), json!({"success": false, "error": "no model is configured"}));
    // And an answer of rca that lacks a field is refused, not read as an empty title
    let mut bad = Rig::new();
    bad.agent.reply("git.generatePullRequestFields", |_| Ok(json!({"base": "main", "title": " ", "body": "", "draft": false})));
    let (code, message) = bad.err("git.generatePullRequestFields", json!({"base": "main", "title": "", "body": "", "draft": false}));
    assert_eq!(code, "runtime_error");
    assert!(message.contains("no title"), "{message}");
}

#[test]
fn given_params_the_host_schema_refuses_when_pull_request_fields_are_asked_then_each_is_refused_in_its_words_before_the_selector_is_resolved_and_rca_is_asked_for_nothing() {
    let mut rig = Rig::new();
    rig.repo.branch_ahead(1);
    let form = || json!({"worktree": "a", "base": "main", "title": "", "body": "", "draft": false});
    let without = |key: &str| {
        let mut p = form();
        p.as_object_mut().unwrap().remove(key);
        p
    };
    let with = |key: &str, v: Value| {
        let mut p = form();
        p[key] = v;
        p
    };
    for (params, says) in [
        (without("worktree"), "Missing worktree selector"),
        (without("base"), "Invalid input: expected string, received undefined"),
        (with("base", json!("")), "Missing base branch"),
        (with("title", json!(3)), "Invalid input: expected string, received number"),
        (without("body"), "Invalid input: expected string, received undefined"),
        (with("draft", json!("no")), "Invalid input: expected boolean, received string"),
        (without("draft"), "Invalid input: expected boolean, received undefined"),
    ] {
        // When it is sent, Then it is refused as invalid_argument with that sentence (a selector no workspace owns would otherwise be selector_not_found)
        let r = rig.conn.on_message(&json!({"id": "x", "method": "git.generatePullRequestFields", "params": params}).to_string());
        let r: Value = serde_json::from_str(&r[0]).unwrap();
        assert_eq!((r["error"]["code"].clone(), r["error"]["message"].clone()), (json!("invalid_argument"), json!(says)), "{params}");
    }
    assert!(rig.repo.pull_request_asked().is_empty());
    // And a valid form for a worktree no workspace owns is the selector error
    let r = rig.conn.on_message(&json!({"id": "x", "method": "git.generatePullRequestFields", "params": form()}).to_string());
    assert_eq!(serde_json::from_str::<Value>(&r[0]).unwrap()["error"]["code"], "selector_not_found");
}

const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

fn pad(c: char) -> String {
    c.to_string().repeat(40)
}

#[test]
fn given_two_commits_when_a_files_branch_diff_is_read_then_it_is_the_text_of_each_side_and_a_missing_side_is_empty() {
    // Given a repository whose merge base holds a file, whose head changed it and added another
    let mut rig = Rig::new();
    rig.repo.blob(OLD, "src/a.txt", b"one\ntwo\n").blob(NEW, "src/a.txt", b"one\n2\n").blob(NEW, "src/new.txt", b"fresh\n");
    // When the phone reads the diff of the changed file and of the added one
    let compare = json!({"mergeBase": OLD, "headOid": NEW});
    let changed = rig.ok("git.branchDiff", json!({"filePath": "src/a.txt", "compare": compare}));
    let added = rig.ok("git.branchDiff", json!({"filePath": "src/new.txt", "compare": compare}));
    // Then the first is the two texts, the second has an empty original
    assert_eq!(changed, json!({"kind": "text", "originalContent": "one\ntwo\n", "modifiedContent": "one\n2\n", "originalIsBinary": false, "modifiedIsBinary": false}));
    assert_eq!((added["originalContent"].clone(), added["modifiedContent"].clone()), (json!(""), json!("fresh\n")));
    // And a rename reads the original side under the old path
    rig.repo.blob(OLD, "old/name.txt", b"same\n").blob(NEW, "new/name.txt", b"same\n");
    let renamed = rig.ok("git.branchDiff", json!({"filePath": "new/name.txt", "oldPath": "old/name.txt", "compare": compare}));
    assert_eq!((renamed["originalContent"].clone(), renamed["modifiedContent"].clone()), (json!("same\n"), json!("same\n")));
}

#[test]
fn given_a_commit_when_a_files_commit_diff_is_read_then_the_parent_is_the_original_and_a_root_commit_has_none() {
    // Given a commit, its parent, and a root commit, each holding the file
    let mut rig = Rig::new();
    let (parent, commit, root) = (pad('a'), pad('b'), pad('c'));
    rig.repo.blob(&parent, "f.txt", b"before").blob(&commit, "f.txt", b"after").blob(&root, "f.txt", b"first");
    // When the phone reads the commit's diff, and the root commit's (no parent, and a null one)
    let diff = rig.ok("git.commitDiff", json!({"filePath": "f.txt", "commitOid": commit, "parentOid": parent}));
    let first = rig.ok("git.commitDiff", json!({"filePath": "f.txt", "commitOid": root}));
    let first_null = rig.ok("git.commitDiff", json!({"filePath": "f.txt", "commitOid": root, "parentOid": null}));
    // Then the parent is the original side, and a root commit's original is empty
    assert_eq!((diff["originalContent"].clone(), diff["modifiedContent"].clone()), (json!("before"), json!("after")));
    assert_eq!((first["originalContent"].clone(), first["modifiedContent"].clone()), (json!(""), json!("first")));
    assert_eq!(first, first_null);
}

#[test]
fn given_binary_and_oversized_files_when_diffed_then_the_result_is_binary_with_the_preview_the_pinned_host_gives_or_the_text_is_withheld() {
    // Given a png with a NUL byte, a binary with no preview, and a text file far past the render limit
    let mut rig = Rig::new();
    let (a, b) = (pad('a'), pad('b'));
    rig.repo.blob(&a, "pic.png", b"\x89PNG\0old").blob(&b, "pic.png", b"\x89PNG\0new").blob(&b, "data.bin", b"\0\x01\x02").blob(&b, "big.txt", &vec![b'x'; 6_000_001]);
    // When each is read through the commit diff
    let png = rig.ok("git.commitDiff", json!({"filePath": "pic.png", "commitOid": b, "parentOid": a}));
    let bin = rig.ok("git.commitDiff", json!({"filePath": "data.bin", "commitOid": b}));
    let big = rig.ok("git.commitDiff", json!({"filePath": "big.txt", "commitOid": b}));
    // Then the image is binary on both sides with its bytes in base64 and its mime type, the other has no content, and the big text is withheld with its counts
    assert_eq!((png["kind"].clone(), png["isImage"].clone(), png["mimeType"].clone(), png["originalIsBinary"].clone(), png["modifiedIsBinary"].clone()), (json!("binary"), json!(true), json!("image/png"), json!(true), json!(true)));
    assert_eq!((png["originalContent"].clone(), png["modifiedContent"].clone()), (json!("iVBORwBvbGQ="), json!("iVBORwBuZXc=")));
    assert_eq!((bin["kind"].clone(), bin["modifiedContent"].clone(), bin["originalIsBinary"].clone(), bin["modifiedIsBinary"].clone(), bin.get("mimeType").cloned()), (json!("binary"), json!(""), json!(false), json!(true), None));
    assert_eq!((big["kind"].clone(), big["modifiedContent"].clone(), big["largeDiffRenderLimit"]["reason"].clone(), big["largeDiffRenderLimit"]["characterCount"].clone()), (json!("text"), json!(""), json!("character-count"), json!(6_000_001)));
}

#[test]
fn given_malformed_diff_requests_when_made_then_each_is_refused_in_the_order_and_words_of_the_pinned_schema() {
    // Given a host with a workspace
    let mut rig = Rig::new();
    let cases = [
        ("git.branchDiff", json!({}), "Missing file path"),
        ("git.branchDiff", json!({"filePath": "a"}), "Invalid input: expected object, received undefined"),
        ("git.branchDiff", json!({"filePath": "a", "compare": {}}), "Invalid input: expected string, received undefined"),
        ("git.branchDiff", json!({"filePath": "a", "compare": {"headOid": "abc", "mergeBase": OLD}}), "Expected a full git object id"),
        ("git.commitDiff", json!({"filePath": "a"}), "Invalid input: expected string, received undefined"),
        ("git.commitDiff", json!({"filePath": "a", "commitOid": "xyz"}), "Expected a full git object id"),
        ("git.commitDiff", json!({"filePath": "a", "commitOid": OLD, "oldPath": 3}), "Invalid input: expected string, received number"),
    ];
    // When each is sent, Then it is refused as invalid_argument with that sentence, and rca is asked for no blob
    for (method, params, message) in cases {
        assert_eq!(rig.err(method, params.clone()), ("invalid_argument".to_string(), message.to_string()), "{method} {params}");
    }
    assert!(rig.sent().iter().all(|(m, _)| m != "git.blob"));
}

#[test]
fn given_no_generation_running_when_the_phone_cancels_one_then_the_host_says_ok_and_a_selector_no_workspace_owns_is_refused() {
    // Given a host with a workspace and no generation in flight
    let mut rig = Rig::new();
    // When the phone cancels a commit-message generation and a pull-request-fields generation
    let message = rig.ok("git.cancelGenerateCommitMessage", json!({}));
    let fields = rig.ok("git.cancelGeneratePullRequestFields", json!({}));
    // Then each is ok, and neither asked rca for anything of git
    assert_eq!((message, fields), (json!({"ok": true}), json!({"ok": true})));
    assert!(rig.sent().is_empty(), "{:?}", rig.sent());
    // And a worktree no workspace owns is the selector error, as for every row
    let r = rig.conn.on_message(&json!({"id": "x", "method": "git.cancelGenerateCommitMessage", "params": {"worktree": "nope"}}).to_string());
    let r: Value = serde_json::from_str(&r[0]).unwrap();
    assert_eq!(r["error"]["code"], "selector_not_found");
}

#[test]
fn given_diff_paths_the_host_would_not_accept_when_asked_then_they_are_the_plain_runtime_error_and_a_path_with_a_trailing_slash_or_backslash_is_normalized() {
    // Given a repository with a file under a directory
    let mut rig = Rig::new();
    let commit = pad('b');
    rig.repo.blob(&commit, "src/a.txt", b"hello");
    // When bad paths are asked for (after a valid schema), Then each is the host's invalid_relative_path error and rca reads no blob
    for bad in ["../x", "/abs", "C:/x", "a//b", "./a", ".", "/", "///"] {
        assert_eq!(rig.err("git.commitDiff", json!({"filePath": bad, "commitOid": commit})), ("runtime_error".to_string(), "invalid_relative_path".to_string()), "{bad}");
    }
    assert!(rig.sent().iter().all(|(m, _)| m != "git.blob"));
    // And a backslash path and a trailing slash read the same file, and an empty oldPath is no oldPath
    let read = rig.ok("git.commitDiff", json!({"filePath": "src\\a.txt", "commitOid": commit, "oldPath": ""}));
    assert_eq!(read["modifiedContent"], json!("hello"));
}

fn text_diff(original: &str, modified: &str) -> Value {
    json!({"kind": "text", "originalContent": original, "modifiedContent": modified, "originalIsBinary": false, "modifiedIsBinary": false})
}

const MAX: u64 = 8 << 20;

#[test]
fn given_a_staged_edit_when_the_staged_diff_is_read_then_it_is_head_against_the_index_and_rca_is_asked_for_the_two_blobs_only() {
    // Given a file committed as one line, staged as another and edited again in the working tree
    let mut rig = Rig::new();
    rig.repo.blob("HEAD", "a.txt", b"one\n").staged("a.txt", b"two\n");
    rig.ws.put("main", "a.txt", b"three\n");
    // When the phone reads its staged diff
    let diff = rig.ok("git.diff", json!({"filePath": "a.txt", "staged": true}));
    // Then it is HEAD against the index, and rca was asked for the two blobs and nothing of the working tree
    assert_eq!(diff, text_diff("one\n", "two\n"));
    assert_eq!(rig.sent_all(), [("git.blob".to_string(), json!({"rev": "HEAD", "path": "a.txt", "max_bytes": MAX, "workspace": "main"})), ("git.blob".to_string(), json!({"path": "a.txt", "max_bytes": MAX, "workspace": "main"}))]);
}

#[test]
fn given_unstaged_edits_when_read_then_the_index_is_the_original_an_untracked_file_falls_back_to_head_and_compare_against_head_skips_the_index() {
    // Given a file committed, staged with a change and edited again, and a file only in the working tree
    let mut rig = Rig::new();
    rig.repo.blob("HEAD", "a.txt", b"one\n").staged("a.txt", b"two\n").blob("HEAD", "gone-from-index.txt", b"head\n");
    rig.ws.put("main", "a.txt", b"three\n").put("main", "new.txt", b"x\n").put("main", "gone-from-index.txt", b"tree\n");
    // When the phone reads the unstaged diffs
    let edited = rig.ok("git.diff", json!({"filePath": "a.txt", "staged": false}));
    let untracked = rig.ok("git.diff", json!({"filePath": "new.txt", "staged": false}));
    let fallback = rig.ok("git.diff", json!({"filePath": "gone-from-index.txt", "staged": false}));
    let against_head = rig.ok("git.diff", json!({"filePath": "a.txt", "staged": false, "compareAgainstHead": true}));
    // Then the index is the original side, a file nowhere in git has an empty one, a file the index lacks is read at HEAD, and HEAD is read when asked
    assert_eq!((edited, untracked, fallback, against_head), (text_diff("two\n", "three\n"), text_diff("", "x\n"), text_diff("head\n", "tree\n"), text_diff("one\n", "three\n")));
}

#[test]
fn given_a_conflicted_file_when_its_unstaged_diff_is_read_then_the_original_is_empty_not_head() {
    // Given a file conflicted by a merge: the index holds only its unmerged stages
    let mut rig = Rig::new();
    rig.repo.blob("HEAD", "c.txt", b"base\n").unmerged("c.txt");
    rig.ws.put("main", "c.txt", b"<<<<<<< ours\n");
    // When the phone reads its unstaged diff
    let diff = rig.ok("git.diff", json!({"filePath": "c.txt", "staged": false}));
    // Then the original side is empty, as the pinned host reads an index entry it cannot read, and is not HEAD's
    assert_eq!(diff, text_diff("", "<<<<<<< ours\n"));
}

#[test]
fn given_a_deleted_png_when_read_staged_and_unstaged_then_the_binary_result_says_modified_deleted_with_the_old_bytes_in_base64() {
    // Given a committed image that is deleted and staged, and a second one deleted in the working tree only
    let png = b"\x89PNG\0\x01";
    let mut rig = Rig::new();
    rig.repo.blob("HEAD", "a.png", png).blob("HEAD", "b.png", png).staged("b.png", png);
    // When the phone reads the staged diff of the first and the unstaged diff of the second
    let staged = rig.ok("git.diff", json!({"filePath": "a.png", "staged": true}));
    let unstaged = rig.ok("git.diff", json!({"filePath": "b.png", "staged": false}));
    // Then each is a binary image result with the old bytes for preview, an empty new side, and the deletion said last
    use base64::Engine;
    let expected = json!({"kind": "binary", "originalContent": base64::engine::general_purpose::STANDARD.encode(png), "modifiedContent": "", "originalIsBinary": true, "modifiedIsBinary": false, "isImage": true, "mimeType": "image/png", "modifiedDeleted": true});
    assert_eq!((&staged, &unstaged), (&expected, &expected));
    assert_eq!(staged.as_object().unwrap().keys().next_back().unwrap(), "modifiedDeleted");
}

#[test]
fn given_a_staged_rename_when_read_then_the_original_is_read_at_the_old_path_and_the_unstaged_diff_ignores_it() {
    // Given a file renamed and changed in the index, unchanged in the working tree since
    let mut rig = Rig::new();
    rig.repo.blob("HEAD", "old.txt", b"one\n").staged("new.txt", b"one\nmore\n");
    rig.ws.put("main", "new.txt", b"one\nmore\n");
    // When the phone reads the staged diff naming the old path, and the unstaged one
    let staged = rig.ok("git.diff", json!({"filePath": "new.txt", "oldPath": "old.txt", "staged": true}));
    let unstaged = rig.ok("git.diff", json!({"filePath": "new.txt", "oldPath": "old.txt", "staged": false}));
    // Then the staged original is the old path at HEAD, and the unstaged one is the index at the new path
    assert_eq!((staged, unstaged), (text_diff("one\n", "one\nmore\n"), text_diff("one\nmore\n", "one\nmore\n")));
}

#[test]
fn given_a_working_file_over_ten_mib_when_read_then_it_is_binary_too_large_and_no_chunk_is_read() {
    // Given an untracked working file one byte over the host's 10 MiB
    let mut rig = Rig::new();
    rig.ws.put("main", "big.log", &vec![b'a'; (10 << 20) + 1]);
    // When the phone reads its unstaged diff
    let diff = rig.ok("git.diff", json!({"filePath": "big.log", "staged": false}));
    // Then the new side is withheld as too large, and only its size was asked
    assert_eq!(diff, json!({"kind": "binary", "originalContent": "", "modifiedContent": "", "originalIsBinary": false, "modifiedIsBinary": true, "tooLarge": true}));
    assert!(rig.sent_all().iter().all(|(m, _)| m != "file.readChunk"), "{:?}", rig.sent_all());
}

#[test]
fn given_a_diff_larger_than_the_reply_budget_when_read_then_it_is_diff_too_large_with_the_budget() {
    // Given a staged file and a working file of 2.5 million characters each, under the render limit but not the remote reply's
    let n = 2_500_000;
    let mut rig = Rig::new();
    rig.repo.staged("w.txt", &vec![b'x'; n]);
    rig.ws.put("main", "w.txt", &vec![b'y'; n]);
    // When the phone reads its unstaged diff as request r1
    let reply = rig.call("git.diff", json!({"filePath": "w.txt", "staged": false}));
    // Then it is refused with the host's code, words and the budget left after the request id
    assert_eq!(reply["error"], json!({"code": "diff_too_large", "message": "This diff is too large to open over a remote connection.", "data": {"maxBytes": 4 * 1024 * 1024 - 4096 - "\"r1\"".len()}}));
}

#[test]
fn given_malformed_git_diff_requests_when_made_then_each_is_refused_in_the_order_and_words_of_the_pinned_schema_before_the_selector_is_resolved() {
    let mut rig = Rig::new();
    let cases = [
        (Value::Null, "Missing worktree selector"),
        (json!({"filePath": "a", "staged": true}), "Missing worktree selector"),
        (json!({"worktree": 7, "filePath": "a", "staged": true}), "Missing worktree selector"),
        (json!({"worktree": "", "filePath": "a", "staged": true}), "Missing worktree selector"),
        (json!({"worktree": "nope", "staged": true}), "Missing file path"),
        (json!({"worktree": "nope", "filePath": 5, "staged": true}), "Missing file path"),
        (json!({"worktree": "nope", "filePath": "a"}), "Invalid input: expected boolean, received undefined"),
        (json!({"worktree": "nope", "filePath": "a", "staged": "yes"}), "Invalid input: expected boolean, received string"),
        (json!({"worktree": "nope", "filePath": "a", "staged": true, "compareAgainstHead": null}), "Invalid input: expected boolean, received null"),
        (json!({"worktree": "nope", "filePath": "a", "staged": true, "oldPath": 5}), "Invalid input: expected string, received number"),
    ];
    for (params, message) in cases {
        // Given a request that breaks the schema (with an unknown selector where it names one)
        // When it is sent
        let reply = rig.raw("git.diff", params.clone());
        // Then it is the schema's first complaint, before any selector or repository is looked at
        assert_eq!((reply["error"]["code"].clone(), reply["error"]["message"].clone()), (json!("invalid_argument"), json!(message)), "{params}");
    }
    assert_eq!(rig.sent_all(), []);
    // And a valid request with an unknown selector is the selector's refusal, and a path that leaves the tree is the host's plain error
    assert_eq!(rig.raw("git.diff", json!({"worktree": "nope", "filePath": "a", "staged": true}))["error"]["code"], "selector_not_found");
    assert_eq!(rig.err("git.diff", json!({"filePath": "../a", "staged": true})), ("runtime_error".to_string(), "invalid_relative_path".to_string()));
    assert_eq!(rig.err("git.diff", json!({"filePath": "a", "oldPath": "/abs", "staged": false})), ("runtime_error".to_string(), "invalid_relative_path".to_string()));
}

fn remotes_asked(rig: &Rig) -> Vec<Value> {
    rig.agent.requests().into_iter().filter(|(m, _)| m == "pr.remotes").map(|(_, p)| p).collect()
}

const SHA: &str = "AbCdEf0123456789abcdef0123456789abcdef01";

#[test]
fn given_an_origin_on_a_public_forge_when_the_commit_url_is_read_then_it_is_the_forges_commit_page_and_rca_was_asked_the_remotes_once() {
    // Given an origin on github.com and an upstream on gitlab.com
    let mut rig = Rig::new();
    rig.repo.remote_url("upstream", "https://gitlab.com/up/stream.git").remote_url("origin", "git@github.com:Octo/Hello.git");
    // When the phone asks for a commit's page, with a key the schema does not know
    let page = rig.ok("git.remoteCommitUrl", json!({"sha": SHA, "extra": 1}));
    // Then it is origin's GitHub commit page with the sha as given, and rca was asked the remotes once, naming the workspace only
    assert_eq!(page, json!(format!("https://github.com/Octo/Hello/commit/{SHA}")));
    assert_eq!(remotes_asked(&rig), [json!({"workspace": "main"})]);
    // And a GitLab origin with a subgroup, a Bitbucket one and a 64 hex id are each that forge's page
    for (url, want) in [("https://gitlab.com/grp/sub/proj.git", "https://gitlab.com/grp/sub/proj/-/commit/"), ("git@bitbucket.org:team/repo.git", "https://bitbucket.org/team/repo/commits/")] {
        let mut rig = Rig::new();
        rig.repo.remote_url("origin", url);
        let sha = "a".repeat(64);
        assert_eq!(rig.ok("git.remoteCommitUrl", json!({"sha": sha})), json!(format!("{want}{sha}")));
    }
}

#[test]
fn given_no_origin_or_an_origin_no_public_forge_hosts_when_the_commit_url_is_read_then_it_is_null() {
    let cases: [&[(&str, &str)]; 6] = [&[], &[("upstream", "git@github.com:o/r.git")], &[("origin", "https://www.github.com/a/b")], &[("origin", "https://ghe.example.com/a/b")], &[("origin", "/srv/local.git")], &[("origin", "https://github.com/onlyowner")]];
    for remotes in cases {
        // Given a repository whose origin is missing, only an upstream, or on no public forge
        let mut rig = Rig::new();
        for (name, url) in remotes {
            rig.repo.remote_url(name, url);
        }
        // When the commit page is read, Then there is none
        assert_eq!(rig.ok("git.remoteCommitUrl", json!({"sha": SHA})), Value::Null, "{remotes:?}");
    }
    // And a folder git refuses is null too, as the host swallows git's error
    let mut rig = Rig::new();
    rig.repo.remotes_fail("git exited 128: fatal: not a git repository (or any of the parent directories): .git");
    assert_eq!(rig.ok("git.remoteCommitUrl", json!({"sha": SHA})), Value::Null);
}

#[test]
fn given_a_bad_worktree_or_sha_when_the_commit_url_is_read_then_it_is_the_hosts_schema_refusal_before_the_selector_and_rca_was_asked_nothing() {
    let mut rig = Rig::new();
    let cases = [
        (Value::Null, "Missing worktree selector"),
        (json!({}), "Missing worktree selector"),
        (json!({"sha": "a"}), "Missing worktree selector"),
        (json!({"worktree": 7, "sha": "a"}), "Missing worktree selector"),
        (json!({"worktree": "nowhere", "sha": "a"}), "Expected a full git object id"),
        (json!({"worktree": "nowhere"}), "Expected a full git object id"),
        (json!({"worktree": "nowhere", "sha": &SHA[..39]}), "Expected a full git object id"),
        (json!({"worktree": "nowhere", "sha": 7}), "Expected a full git object id"),
        (json!({"worktree": "nowhere", "sha": format!(" {}", &SHA[1..])}), "Expected a full git object id"),
    ];
    for (params, message) in cases {
        // Given a request whose selector or object id breaks the schema
        // When it is sent, Then it is the schema's first complaint
        let reply = rig.raw("git.remoteCommitUrl", params.clone());
        assert_eq!((reply["error"]["code"].clone(), reply["error"]["message"].clone()), (json!("invalid_argument"), json!(message)), "{params}");
    }
    // And a valid request naming no workspace is the selector's refusal, and none of them asked rca for remotes
    assert_eq!(rig.raw("git.remoteCommitUrl", json!({"worktree": "nowhere", "sha": SHA}))["error"], json!({"code": "selector_not_found", "message": "selector_not_found"}));
    assert_eq!(remotes_asked(&rig), Vec::<Value>::new());
}

#[test]
fn an_unregistered_checkout_reads_its_detached_head_and_cannot_change_the_parent_workspace() {
    // Given the repository is on main and Git also knows a detached checkout.
    let mut rig = Rig::new();
    let sha = "d".repeat(40);
    let detached = sha.clone();
    rig.agent.reply("worktree.list", move |_| Ok(json!({"worktrees":[{"path":"/linked","head":detached,"is_main":false}]})));
    rig.agent.reply("git.history", |_| Ok(json!({"commits":[],"truncated":false})));
    // When the client requests branch-only history and tries to stage a file in that checkout.
    let selector = "id:main::/linked";
    let history = rig.raw("git.history", json!({"worktree":selector,"branchOnly":true,"baseRef":"main","skip":50}));
    let stage = rig.raw("git.stage", json!({"worktree":selector,"filePath":"tracked.txt"}));
    let unknown = rig.raw("git.history", json!({"worktree":"id:main::/unknown"}));
    // Then only the history request reaches Git, with the checkout's own revision and no parent branch flags.
    assert_eq!(history["ok"], true, "{history}");
    assert!(history["result"].get("currentRef").is_none());
    assert_eq!(history["result"]["hasOutgoingChanges"], false);
    assert_eq!(stage["ok"], false);
    assert_eq!(unknown["error"]["code"], "selector_not_found");
    assert_eq!(rig.sent_all(), vec![("git.history".into(), json!({"workspace":"main","limit":50,"skip":50,"rev":format!("main..{sha}")}))]);
}

#[test]
fn a_detached_checkout_can_read_immutable_file_diffs_without_reading_the_parent_working_tree() {
    // Given an unregistered checkout and a file renamed between two commits.
    let mut rig = Rig::new();
    let (parent, commit) = (pad('a'), pad('b'));
    let head = commit.clone();
    rig.agent.reply("worktree.list", move |_| {
        Ok(json!({"worktrees":[{"path":"/linked","head":head,"is_main":false}]}))
    });
    rig.repo
        .blob(&parent, "old.txt", b"first")
        .blob(&commit, "new.txt", b"second");
    let selector = "id:main::/linked";
    // When the phone opens that file in the commit view and the branch comparison.
    let single = rig.raw("git.commitDiff", json!({"worktree":selector,"filePath":"new.txt","oldPath":"old.txt","commitOid":commit,"parentOid":parent}));
    let branch = rig.raw("git.branchDiff", json!({"worktree":selector,"filePath":"new.txt","oldPath":"old.txt","compare":{"headOid":commit,"mergeBase":parent}}));
    // Then both views read the immutable blobs, and a working tree diff still requires its own workspace.
    assert_eq!(single["ok"], true, "{single}");
    assert_eq!(branch["result"], single["result"]);
    assert_eq!(single["result"]["originalContent"], "first");
    assert_eq!(single["result"]["modifiedContent"], "second");
    assert_eq!(
        rig.raw(
            "git.diff",
            json!({"worktree":selector,"filePath":"new.txt","staged":false})
        )["ok"],
        false
    );
    assert!(rig
        .sent_all()
        .iter()
        .all(|(method, _)| method == "git.blob"));
}
