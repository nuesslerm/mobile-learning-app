//! `wf mimic record-projects` against a fake `gh` that keeps a list of boards in a state directory: setup creates, the run records, teardown deletes
//! (also when the run fails), the stale guard deletes only old `mla-recording-test-*` boards, a missing scope is named and the token never leaves gh.
use crate::support::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const TOKEN: &str = "ghp_fake_record_token_value_0001";
const OLD: &str = "2020-01-01T00:00:00Z";
const FUTURE: &str = "2999-01-01T00:00:00Z";

struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new(scopes: &str) -> Rig {
        let rig = Rig { dir: tempfile::tempdir().unwrap() };
        std::fs::create_dir_all(rig.state()).unwrap();
        std::fs::create_dir_all(rig.bin()).unwrap();
        std::fs::write(rig.state().join("scopes"), scopes).unwrap();
        std::fs::write(rig.state().join("boards"), "").unwrap();
        std::fs::write(rig.state().join("calls"), "").unwrap();
        std::fs::write(rig.state().join("leaks"), "").unwrap();
        let gh = rig.bin().join("gh");
        std::fs::write(&gh, FAKE_GH.replace("@STATE@", &rig.state().display().to_string()).replace("@TOKEN@", TOKEN)).unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        rig
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn set(&self) -> PathBuf {
        self.dir.path().join("out/exec-hosted-projects")
    }

    fn seed(&self, id: &str, title: &str, created: &str) {
        let mut boards = std::fs::read_to_string(self.state().join("boards")).unwrap();
        boards.push_str(&format!("{id}|{title}|{created}\n"));
        std::fs::write(self.state().join("boards"), boards).unwrap();
    }

    fn boards(&self) -> String {
        std::fs::read_to_string(self.state().join("boards")).unwrap()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.state().join("calls")).unwrap()
    }

    fn leaks(&self) -> String {
        std::fs::read_to_string(self.state().join("leaks")).unwrap()
    }

    fn flag(&self, name: &str) {
        std::fs::write(self.state().join(name), "").unwrap();
    }

    fn record(&self, extra: &[&str]) -> Run {
        let set = self.set();
        let state = self.dir.path().join("record.state.json");
        let mut args = vec!["record-projects", set.to_str().unwrap(), "--run-id", "4242", "--state", state.to_str().unwrap()];
        args.extend_from_slice(extra);
        let path = PathBuf::from(format!("{}:{}", self.bin().display(), std::env::var("PATH").unwrap_or_default()));
        mimic(&args, &[("GH_PROJECTS_RECORD_TOKEN", Path::new(TOKEN)), ("PATH", &path), ("BWS_CI_DEPLOY_ACCESS_TOKEN", Path::new("vault_token_value")), ("BWS_SERVER_URL", Path::new("https://vault.invalid"))])
    }
}

#[test]
fn given_a_token_with_both_scopes_when_the_job_runs_then_the_board_is_made_the_actions_recorded_and_the_board_deleted() {
    // Given a signed-in gh whose token has project and read:org
    let rig = Rig::new("project, read:org");
    // When record-projects runs
    let r = rig.record(&[]);
    // Then it succeeds and its single answer names the actions it recorded
    assert_eq!(r.code, 0, "{}", r.json);
    assert_eq!(r.json["schema"], "mimic-record-projects/1");
    assert_eq!(r.json["recorded"], 8);
    let tools: Vec<&str> = r.json["actions"].as_array().unwrap().iter().map(|a| a["tool"].as_str().unwrap()).collect();
    assert_eq!(tools[0], "hosted_project_list");
    assert!(tools.contains(&"hosted_project_field_clear"), "{tools:?}");
    // And the board was created with the run id in its name, then deleted
    assert!(rig.calls().contains("title=mla-recording-test-4242"), "{}", rig.calls());
    assert_eq!(rig.boards(), "");
    assert_eq!(r.json["board"]["deleted"], true);
    // And the set is recorded: every gh call of an action is in it with the real gh's output
    let header: Value = serde_json::from_str(&std::fs::read_to_string(rig.set().join("cassette.json")).unwrap()).unwrap();
    assert_eq!(header["provenance"]["kind"], "recorded");
    assert_eq!(header["provenance"]["target"]["name"], "gh");
    assert_eq!(header["interactions"], 8);
    let lines = std::fs::read_to_string(rig.set().join("interactions.jsonl")).unwrap();
    let first: Value = serde_json::from_str(lines.lines().next().unwrap()).unwrap();
    assert_eq!(first["id"], "hosted_project_list/00");
    assert_eq!(first["steps"][0]["payload"]["args"][0], "api");
    assert!(lines.contains("PVT_fake1"), "the reply of the fake gh is in the set");
    let missing: Vec<&str> = lines.lines().filter(|l| l.contains("999999")).collect();
    assert!(missing.iter().any(|l| l.contains("NOT_FOUND")), "the refusal is recorded as the executor answered it");
    // And the token appears nowhere: not in the answer, not in the set, and gh got it as GH_TOKEN in every call
    for text in [r.json.to_string(), r.stderr.clone(), lines, header.to_string()] {
        assert!(!text.contains(TOKEN));
    }
    assert!(!rig.calls().contains("token=bad"), "{}", rig.calls());
    // And wf mimic verify accepts the set as intact
    let v = mimic(&["verify", rig.set().to_str().unwrap()], &[]);
    assert_eq!(v.code, 0, "{}", v.json);
}

#[test]
fn given_an_action_that_fails_when_the_job_runs_then_the_board_is_still_deleted_and_no_set_is_written() {
    // Given a gh that refuses field updates
    let rig = Rig::new("project, read:org");
    std::fs::write(rig.state().join("fail_set"), "").unwrap();
    // When record-projects runs
    let r = rig.record(&[]);
    // Then it fails naming the action
    assert_eq!(r.code, 1, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("hosted_project_field_set"), "{}", r.json);
    // And teardown deleted the board anyway
    assert_eq!(rig.boards(), "");
    assert!(rig.calls().contains("delete PVT_fake1"));
    // And nothing was written as a recording
    assert!(!rig.set().join("cassette.json").exists());
}

#[test]
fn given_old_and_young_boards_when_the_job_runs_then_only_old_recording_boards_are_deleted() {
    // Given an old recording board, a young one, and an old board of the owner's that is no recording
    let rig = Rig::new("project, read:org");
    rig.seed("PVT_old", "mla-recording-test-1", OLD);
    rig.seed("PVT_young", "mla-recording-test-2", FUTURE);
    rig.seed("PVT_mine", "Roadmap", OLD);
    // When record-projects runs
    let r = rig.record(&[]);
    // Then only the old recording board went
    assert_eq!(r.code, 0, "{}", r.json);
    assert_eq!(r.json["stale"]["deleted"], json!(["mla-recording-test-1"]));
    let left = rig.boards();
    assert!(left.contains("PVT_young") && left.contains("PVT_mine") && !left.contains("PVT_old"), "{left}");
}

#[test]
fn given_a_token_without_read_org_when_the_job_runs_then_it_fails_naming_the_scope_and_creates_nothing() {
    // Given a token that has only the project scope
    let rig = Rig::new("project");
    // When record-projects runs
    let r = rig.record(&[]);
    // Then it fails naming the missing scope, never the token
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("read:org"), "{}", r.json);
    assert_eq!(r.json["evidence"]["missing_scopes"], json!(["read:org"]));
    assert!(!r.json.to_string().contains(TOKEN));
    // And no board was created
    assert!(!rig.calls().contains("createProjectV2"), "{}", rig.calls());
}

#[test]
fn given_a_board_left_by_a_killed_run_when_teardown_runs_then_the_board_is_deleted() {
    // Given a state file naming a board that still exists
    let rig = Rig::new("project, read:org");
    rig.seed("PVT_left", "mla-recording-test-4242", FUTURE);
    let state = rig.dir.path().join("record.state.json");
    std::fs::write(&state, r#"{"id":"PVT_left","title":"mla-recording-test-4242","number":3}"#).unwrap();
    // When only teardown runs
    let r = rig.record(&["--teardown"]);
    // Then the board is gone and the state file with it
    assert_eq!(r.code, 0, "{}", r.json);
    assert_eq!(rig.boards(), "");
    assert!(!state.exists());
}

#[test]
fn given_no_token_when_the_job_runs_then_it_names_the_key() {
    // Given an environment without the token
    let rig = Rig::new("project, read:org");
    let set = rig.set();
    let path = PathBuf::from(format!("{}:{}", rig.bin().display(), std::env::var("PATH").unwrap_or_default()));
    // When record-projects runs
    let r = mimic(&["record-projects", set.to_str().unwrap()], &[("PATH", &path), ("GH_PROJECTS_RECORD_TOKEN", Path::new(""))]);
    // Then the error names GH_PROJECTS_RECORD_TOKEN
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("GH_PROJECTS_RECORD_TOKEN"), "{}", r.json);
}

#[test]
fn given_vault_variables_in_the_environment_when_the_job_runs_then_no_gh_call_sees_them() {
    // Given a run whose environment holds the vault token
    let rig = Rig::new("project, read:org");
    // When record-projects runs
    let r = rig.record(&[]);
    // Then it succeeds and gh never saw a BWS_ variable
    assert_eq!(r.code, 0, "{}", r.json);
    assert_eq!(rig.leaks(), "", "gh saw the vault environment");
}

#[test]
fn given_a_token_with_admin_org_when_the_job_runs_then_read_org_counts_as_granted() {
    // Given a token whose scopes name admin:org, which includes read:org
    let rig = Rig::new("project, admin:org");
    // When record-projects runs
    let r = rig.record(&[]);
    // Then the scope check passes
    assert_eq!(r.code, 0, "{}", r.json);
}

#[test]
fn given_a_board_that_cannot_be_deleted_when_teardown_runs_then_it_fails_and_keeps_the_state_file() {
    // Given a state file naming a board and a gh that refuses the deletion
    let rig = Rig::new("project, read:org");
    rig.seed("PVT_left", "mla-recording-test-4242", FUTURE);
    rig.flag("fail_delete");
    let state = rig.dir.path().join("record.state.json");
    std::fs::write(&state, r#"{"id":"PVT_left","title":"mla-recording-test-4242","number":3}"#).unwrap();
    // When only teardown runs
    let r = rig.record(&["--teardown"]);
    // Then it fails, names the board, and the state file stays for a retry
    assert_eq!(r.code, 1, "{}", r.json);
    assert_eq!(r.json["board"]["deleted"], false);
    assert!(r.json["problems"][0].as_str().unwrap().contains("PVT_left"), "{}", r.json);
    assert!(state.exists());
}

#[test]
fn given_setup_that_dies_after_the_board_exists_when_the_job_runs_then_the_board_is_still_deleted() {
    // Given a gh that refuses to create fields
    let rig = Rig::new("project, read:org");
    rig.flag("fail_field");
    // When record-projects runs
    let r = rig.record(&[]);
    // Then it fails in setup, and the board found through the state file is deleted
    assert_eq!(r.code, 1, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("setup"), "{}", r.json);
    assert_eq!(rig.boards(), "");
    assert_eq!(r.json["board"]["deleted"], true);
    assert!(!rig.set().join("cassette.json").exists());
}

#[test]
fn given_a_guard_that_errors_when_the_job_runs_then_the_set_is_still_written_and_the_run_reports_the_problem() {
    // Given a gh whose project listing for the guard is refused
    let rig = Rig::new("project, read:org");
    rig.flag("fail_list");
    // When record-projects runs
    let r = rig.record(&[]);
    // Then the problem is reported with exit 1, the board is deleted, and the set exists for the upload
    assert_eq!(r.code, 1, "{}", r.json);
    assert!(r.json["problems"][0].as_str().unwrap().contains("stale-board guard"), "{}", r.json);
    assert_eq!(rig.boards(), "");
    assert!(rig.set().join("cassette.json").exists());
}

const FAKE_GH: &str = r#"#!/bin/sh
S='@STATE@'
case "$1" in --version) echo "gh version 2.89.0 (fake)"; exit 0;; esac
in=$(cat)
all="$*
$in"
tok=bad; [ "$GH_TOKEN" = "@TOKEN@" ] && tok=ok
echo "$* token=$tok" >> "$S/calls"
env | grep -E '^(BWS_|GITHUB_TOKEN)' >> "$S/leaks"
case "$*" in
  "api -i user") printf 'HTTP/2.0 200 OK\nX-Oauth-Scopes: %s\n\n{"login":"octocat"}\n' "$(cat "$S/scopes")"; exit 0;;
esac
arg() { for a in "$@"; do case "$a" in "$KEY"=*) printf '%s' "${a#*=}";; esac; done; }
case "$all" in
  *deleteProjectV2*) if [ -e "$S/fail_delete" ]; then echo "gh: delete refused" >&2; exit 1; fi; id=$(KEY=project arg "$@"); grep -v "^$id|" "$S/boards" > "$S/b2"; mv "$S/b2" "$S/boards"; echo "delete $id" >> "$S/calls"; echo "{\"data\":{\"deleteProjectV2\":{\"projectV2\":{\"id\":\"$id\"}}}}";;
  *createProjectV2Field*) if [ -e "$S/fail_field" ]; then echo "gh: field refused" >&2; exit 1; fi; case "$all" in *TEXT*) echo '{"data":{"createProjectV2Field":{"projectV2Field":{"id":"PVTF_text","name":"notes"}}}}';; *) echo '{"data":{"createProjectV2Field":{"projectV2Field":{"id":"PVTF_num","name":"estimate"}}}}';; esac;;
  *'createProjectV2('*) title=$(KEY=title arg "$@"); echo "PVT_fake1|$title|$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$S/boards"; echo '{"data":{"createProjectV2":{"projectV2":{"id":"PVT_fake1","number":7,"url":"https://github.com/users/octocat/projects/7"}}}}';;
  *addProjectV2DraftIssue*) echo '{"data":{"addProjectV2DraftIssue":{"projectItem":{"id":"PVTI_1"}}}}';;
  *'viewer{id login}'*) echo '{"data":{"viewer":{"id":"U_1","login":"octocat"}}}';;
  *'projectsV2(first:100)'*) if [ -e "$S/fail_list" ]; then echo "gh: list refused" >&2; exit 1; fi; awk -F'|' 'BEGIN{printf "{\"data\":{\"viewer\":{\"projectsV2\":{\"nodes\":["} NF>2{printf "%s{\"id\":\"%s\",\"number\":1,\"title\":\"%s\",\"createdAt\":\"%s\"}", (n++?",":""),$1,$2,$3} END{print "]}}}}"}' "$S/boards";;
  *'projectsV2(first:40)'*) echo '{"data":{"viewer":{"login":"octocat","projectsV2":{"nodes":[{"id":"PVT_fake1","number":7,"title":"mla-recording-test-4242","url":"u","owner":{"__typename":"User","login":"octocat"}}]},"organizations":{"nodes":[]}}}}';;
  *'"num":999999'*) echo "gh: Could not resolve to a ProjectV2 with the number 999999." >&2; exit 1;;
  *'projectV2(number:$num){id title}'*) echo '{"data":{"user":{"projectV2":{"id":"PVT_fake1","title":"mla-recording-test-4242"}}}}';;
  *'user(login:$owner){login}'*) echo '{"data":{"user":{"login":"octocat"}}}';;
  *'views(first:100)'*) echo '{"data":{"user":{"projectV2":{"views":{"nodes":[{"id":"PVTV_1","number":1,"name":"View 1","layout":"TABLE_LAYOUT"}]}}}}}';;
  *updateProjectV2ItemFieldValue*) if [ -e "$S/fail_set" ]; then echo "gh: refused" >&2; exit 1; fi; echo '{"data":{"updateProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_1"}}}}';;
  *clearProjectV2ItemFieldValue*) echo '{"data":{"clearProjectV2ItemFieldValue":{"projectV2Item":{"id":"PVTI_1"}}}}';;
  *) echo "fake gh: unhandled $all" >&2; exit 1;;
esac
"#;
