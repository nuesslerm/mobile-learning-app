//! The terminal-artifact rows through the chain: a path a terminal printed is tapped, granted to the phone, read, previewed and saved on the real
//! executor. What the executor serves beyond its root is the owner's switch (`RCA_EXEC_ARTIFACT_ALLOW`, none by default). A row counts only when
//! the agent method that does its work answered in the call's window.
use crate::agent_sessions::{answered, executor_ran};
use crate::rig::*;
use serde_json::{json, Value};

const WS: &str = "id:w1::";

fn grant(resolved: &Value) -> Value {
    json!({"worktree": WS, "grantId": resolved["result"]["openTarget"]["grantId"], "absolutePath": resolved["result"]["absolutePath"]})
}

#[test]
fn a_tapped_file_outside_the_repository_is_granted_read_previewed_and_saved_only_where_the_owner_allowed_and_a_rewrite_makes_it_stale() {
    exec_testkit::case!(SUITE, |case| {
        // Given an executor told to allow one directory beside its repository, a log and a secret file outside it, and a paired phone
        let allowed = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let log = std::fs::canonicalize(allowed.path()).unwrap().join("build.log");
        std::fs::write(&log, "first line\n").unwrap();
        std::fs::write(elsewhere.path().join("secret.txt"), "not for the phone").unwrap();
        let chain = Chain::start_with_env(None, &[("RCA_EXEC_ARTIFACT_ALLOW", allowed.path().to_str().unwrap())]);
        let mut phone = Phone::connect(case, &chain);
        // When the phone taps the log
        let resolved = phone.call(case, &chain, &["rpc:files.resolveTerminalPath"], "files.resolveTerminalPath", json!({"worktree": WS, "pathText": log.to_str().unwrap()}));
        // Then the executor described the file and the phone holds a grant for its canonical path
        answered(case, "artifact.stat");
        assert!(executor_ran(case, "artifact_stat"));
        assert_eq!(resolved["result"]["openTarget"]["kind"], "absolute-file", "{resolved}");
        assert_eq!(resolved["result"]["openTarget"]["byteLength"], 11);
        assert_eq!(resolved["result"]["absolutePath"], log.to_str().unwrap());
        // When it reads, previews and saves under the grant
        let read = phone.call(case, &chain, &["rpc:files.readTerminalArtifact"], "files.readTerminalArtifact", grant(&resolved));
        answered(case, "artifact.read");
        let preview = phone.call(case, &chain, &["rpc:files.readTerminalArtifactPreview"], "files.readTerminalArtifactPreview", grant(&resolved));
        let mut save = grant(&resolved);
        save["content"] = json!("second\n");
        let saved = phone.call(case, &chain, &["rpc:files.writeTerminalArtifact"], "files.writeTerminalArtifact", save);
        answered(case, "artifact.write");
        // Then the content, the preview and the file on disk are the host's answers
        assert_eq!(read["result"]["content"], "first line\n", "{read}");
        assert_eq!(preview["result"], json!({"content": "first line\n", "isBinary": false}));
        assert_eq!(saved["result"], json!({"ok": true}), "{saved}");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "second\n");
        // When someone else rewrites the file and the phone reads and saves again
        std::fs::write(&log, "someone else, longer than before\n").unwrap();
        let stale = phone.refused(case, &chain, &["rpc:files.readTerminalArtifact"], "files.readTerminalArtifact", grant(&resolved));
        let mut again = grant(&resolved);
        again["content"] = json!("overwrite");
        let refused = phone.refused(case, &chain, &["rpc:files.writeTerminalArtifact"], "files.writeTerminalArtifact", again);
        // Then both are refused as stale and the other text stands
        assert_eq!(stale["error"]["message"], "terminal_file_grant_stale", "{stale}");
        assert_eq!(refused["error"]["message"], "terminal_file_grant_stale", "{refused}");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "someone else, longer than before\n");
        // When the phone taps a file in neither the repository nor an allowed directory
        let outside = phone.refused(case, &chain, &["rpc:files.resolveTerminalPath"], "files.resolveTerminalPath", json!({"worktree": WS, "pathText": elsewhere.path().join("secret.txt").to_str().unwrap()}));
        // Then it is refused as not granted, and no grant was made
        assert_eq!(outside["error"]["message"], "Files outside this workspace are not granted on this host.", "{outside}");
    });
}

#[test]
fn with_no_directory_allowed_a_file_in_the_repository_is_still_tapped_and_one_outside_is_refused() {
    exec_testkit::case!(SUITE, |case| {
        // Given the default executor (nothing allowed beyond its root) with a file in its repository and a file outside it
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("x.txt"), "x").unwrap();
        let chain = Chain::start(None);
        std::fs::write(chain.repo.path().join("in.txt"), "inside").unwrap();
        let mut phone = Phone::connect(case, &chain);
        // When the phone taps the file in the repository by its relative path and the one outside by its absolute path
        let inside = phone.call(case, &chain, &["rpc:files.resolveTerminalPath"], "files.resolveTerminalPath", json!({"worktree": WS, "pathText": "in.txt"}));
        let outside = phone.refused(case, &chain, &["rpc:files.resolveTerminalPath"], "files.resolveTerminalPath", json!({"worktree": WS, "pathText": elsewhere.path().join("x.txt").to_str().unwrap()}));
        // Then the first opens as a workspace file and the second is refused
        assert_eq!(inside["result"]["openTarget"], json!({"kind": "worktree-file", "provider": "local", "relativePath": "in.txt", "absolutePath": "in.txt"}), "{inside}");
        assert_eq!(outside["error"]["message"], "Files outside this workspace are not granted on this host.", "{outside}");
    });
}
