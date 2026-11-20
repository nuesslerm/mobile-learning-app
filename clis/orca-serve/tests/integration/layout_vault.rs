//! The rows over the executor's own files and home, through the chain: the editor, diff and markdown tabs of the kept layout, the SSH target rows,
//! the CLI session files of the executor's home, a chat's reveal and the saved default model. A row counts only when the agent method that does its
//! work answered in the call's window.
use crate::agent_sessions::{answered, create_params, executor_ran};
use crate::rig::*;
use exec_testkit::script::Script;
use serde_json::{json, Value};

const WS_SELECTOR: &str = "id:w1::";

#[test]
fn the_phone_opens_a_markdown_tab_reads_and_saves_it_and_opens_a_diff_and_the_ssh_rows_name_the_executor() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a markdown file in the executor's repository and a paired phone
        let chain = Chain::start(None);
        std::fs::write(chain.repo.path().join("notes.md"), "# one\n").unwrap();
        let mut phone = Phone::connect(case, &chain);
        // When the phone opens the file and a diff of it
        let opened = phone.call(case, &chain, &["rpc:files.open"], "files.open", json!({"worktree": WS_SELECTOR, "relativePath": "notes.md"}));
        // Then the executor was asked for the file and the tab is kept
        answered(case, "file.stat");
        assert_eq!(opened["result"]["relativePath"], "notes.md", "{opened}");
        let diff = phone.call(case, &chain, &["rpc:files.openDiff"], "files.openDiff", json!({"worktree": WS_SELECTOR, "relativePath": "notes.md"}));
        answered(case, "state.set");
        assert!(diff["result"].is_object(), "{diff}");
        // And the markdown tab reads the file, saves a new text against the version read, and the file holds it
        let tab = "markdown:notes.md";
        let read = phone.call(case, &chain, &["rpc:markdown.readTab"], "markdown.readTab", json!({"worktree": WS_SELECTOR, "tabId": tab}));
        assert_eq!(read["result"]["content"], "# one\n", "{read}");
        let saved = phone.call(case, &chain, &["rpc:markdown.saveTab"], "markdown.saveTab", json!({"worktree": WS_SELECTOR, "tabId": tab, "baseVersion": read["result"]["version"], "content": "# two\n"}));
        answered(case, "file.write");
        assert_eq!(saved["result"]["content"], "# two\n", "{saved}");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes.md")).unwrap(), "# two\n");
        // When the phone asks for the SSH targets, the state of the executor and a connect to it
        let listed = phone.call(case, &chain, &["rpc:ssh.listTargetSummaries"], "ssh.listTargetSummaries", json!({}));
        answered(case, "repo.list");
        let target = listed["result"]["targets"][0]["id"].as_str().expect("the executor is a target").to_string();
        let state = phone.call(case, &chain, &["rpc:ssh.getState"], "ssh.getState", json!({"targetId": target}));
        let connected = phone.call(case, &chain, &["rpc:ssh.connect"], "ssh.connect", json!({"targetId": target}));
        // Then it is the one attached executor, connected
        assert_eq!(state["result"]["state"]["status"], "connected", "{state}");
        assert!(connected["result"].is_object(), "{connected}");
    });
}

#[test]
fn the_phone_lists_the_session_files_of_the_executors_home_reveals_a_chat_and_saves_a_default_model() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a claude session file in the executor's home, a paired phone and a claude chat
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![]).printing_version("2.1.30 (Claude Code)"));
        let file = chain.home.path().join(".claude/projects/-work-app/sess-1.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, concat!(
            r#"{"type":"user","sessionId":"sess-1","cwd":"/work/app","gitBranch":"main","timestamp":"2026-11-01T10:00:00Z","message":{"role":"user","content":"fix the build"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-11-01T10:00:05Z","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":"done"}]}}"#, "\n",
        )).unwrap();
        let mut phone = Phone::connect(case, &chain);
        // When the phone lists the vault's sessions
        let listed = phone.call(case, &chain, &["rpc:aiVault.listSessions"], "aiVault.listSessions", json!({}));
        // Then the service listed them from the executor's home, and the claude session is there with its directory
        answered(case, "sessions.list");
        assert!(executor_ran(case, "session_scan"), "the executor scanned its home");
        let sessions = listed["result"]["sessions"].as_array().expect("sessions");
        assert!(sessions.iter().any(|s| s["sessionId"] == "sess-1"), "{listed}");
        // And a resume of it is prepared for the host's own home
        let prepared = phone.call(case, &chain, &["rpc:aiVault.prepareSessionResume"], "aiVault.prepareSessionResume", json!({"agent": "codex", "filePath": "~/.codex/sessions/c1.jsonl", "codexHome": null}));
        assert_eq!(prepared["result"], json!({"useRealCodexHome": false}), "{prepared}");
        // When the phone reveals a chat that exists
        let made = phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params("claude_v1", "v1", None));
        assert_eq!(made["result"]["ok"], true, "{made}");
        let revealed = phone.call(case, &chain, &["rpc:agentSession.reveal"], "agentSession.reveal", json!({"sessionId": "claude_v1"}));
        // Then its tab is kept
        answered(case, "state.set");
        assert_eq!(revealed["result"]["ok"], true, "{revealed}");
        // When the phone picks a model as the default for new chats
        let picked = phone.call(case, &chain, &["rpc:settings.mutateNativeChatSessionOptions"], "settings.mutateNativeChatSessionOptions", json!({"type": "apply-picks", "agent": "claude", "picks": [{"modelId": "opus", "optionId": "effort", "value": "high"}]}));
        // Then the setting is stored through the service
        answered(case, "state.set");
        assert!(picked["result"].is_object(), "{picked}");
        let _: &Value = &picked;
    });
}
