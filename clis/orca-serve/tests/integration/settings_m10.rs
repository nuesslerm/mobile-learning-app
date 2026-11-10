//! The rows of M10 through the chain: the phone's commit message, kept state and accounts, from `orca-serve` through the real rca-service (and, for the
//! commit message, the real executor over a repository on its disk). A row counts only when the agent method it maps to was answered in its call's
//! window; what each call did is then read from rca itself (its kept state, its providers, the repository's index and log, the request a model API
//! received), never from the answer alone. The model API is a local HTTP endpoint the service is configured with, which records what it is asked.
use crate::rig::*;
use exec_testkit::script::Turn;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

const SELECTOR: &str = "id:w1::";
const MODEL_KEY: &str = "chain-model-key";
const CODEX_TOKEN: &str = "codex-device-token-4711";
const CLAUDE_TOKEN: &str = "claude-paste-token-8123";

fn sel(extra: Value) -> Value {
    let mut params = json!({"worktree": SELECTOR});
    if let (Some(map), Some(more)) = (params.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    params
}

/// A chat-completions endpoint that answers one fixed message and keeps what it was asked: each request's `Authorization` header and JSON body.
struct ModelApi {
    base: String,
    asked: Arc<Mutex<Vec<(String, Value)>>>,
}

impl ModelApi {
    fn start(message: &str) -> ModelApi {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (kept, message) = (asked.clone(), message.to_string());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                let body = loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    raw.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                        let wanted: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                        if rest.len() >= wanted || n == 0 {
                            let auth = head.lines().find_map(|l| l.strip_prefix("authorization: ").or_else(|| l.strip_prefix("Authorization: "))).unwrap_or("").to_string();
                            break (auth, serde_json::from_str::<Value>(rest).unwrap_or(Value::Null));
                        }
                    }
                    if n == 0 {
                        break (String::new(), Value::Null);
                    }
                };
                kept.lock().unwrap().push(body);
                let answer = json!({"model": "chain-model", "choices": [{"message": {"role": "assistant", "content": message}}]}).to_string();
                let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{answer}", answer.len()).as_bytes());
            }
        });
        ModelApi { base, asked }
    }

    fn requests(&self) -> Vec<(String, Value)> {
        self.asked.lock().unwrap().clone()
    }
}

#[test]
fn a_commit_message_is_written_by_the_service_s_model_from_the_staged_diff_alone_and_nothing_is_committed() {
    exec_testkit::case!(SUITE, |case| {
        // Given a model API the service is configured with, a repository with one staged edit and one unstaged file, and a paired phone
        let model = ModelApi::start("feat: describe the staged edit");
        let chain = Chain::start_with_env(None, &[("RCA_MODEL_API_BASE_URL", &model.base), ("KIMI_API_KEY", MODEL_KEY)]);
        let mut phone = Phone::connect(case, &chain);
        let repo = &chain.repo;
        repo.write("README.md", "a staged edit\n");
        repo.git(&["add", "README.md"]);
        repo.write("scratch.txt", "an unstaged file\n");
        let commits = repo.git(&["rev-list", "--count", "HEAD"]);
        // When the phone generates a commit message
        let generated = phone.call(case, &chain, &["rpc:git.generateCommitMessage"], "git.generateCommitMessage", sel(json!({})));
        // Then rca read the diff from the executor and asked the model, whose message is the answer
        answered(case, "git.generateCommitMessage", true);
        assert!(ran(case, "git_diff"), "{}", chain.evidence());
        assert_eq!(generated["result"], json!({"success": true, "message": "feat: describe the staged edit"}));
        // And the model API was asked once, with the service's key, for the staged edit and not for the unstaged file
        let asked = model.requests();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert_eq!(asked[0].0, format!("Bearer {MODEL_KEY}"));
        let prompt = asked[0].1["messages"][1]["content"].as_str().unwrap();
        assert!(prompt.contains("README.md") && prompt.contains("a staged edit"), "{prompt}");
        assert!(!prompt.contains("scratch.txt"), "{prompt}");
        // And nothing was committed or staged by it
        assert_eq!((repo.git(&["rev-list", "--count", "HEAD"]), repo.git(&["status", "--porcelain"])), (commits.clone(), "M  README.md\n?? scratch.txt".to_string()));
        // When the phone commits with that message
        let committed = phone.call(case, &chain, &["rpc:git.commit"], "git.commit", sel(json!({"message": generated["result"]["message"]})));
        // Then git's newest commit has it as its subject
        assert_eq!(committed["result"]["success"], json!(true));
        assert_eq!(repo.git(&["log", "-1", "--format=%s"]), "feat: describe the staged edit");
        // When it generates again with nothing staged
        let nothing = phone.call(case, &chain, &[], "git.generateCommitMessage", sel(json!({})));
        // Then the answer is a failed generation naming that, and the model was not asked again
        assert_eq!(nothing["result"]["success"], json!(false));
        assert!(nothing["result"]["error"].as_str().unwrap().contains("nothing is staged"), "{nothing}");
        assert_eq!(model.requests().len(), 1);
    });
}

#[test]
fn a_service_with_no_model_key_answers_a_failed_generation_with_its_reason() {
    exec_testkit::case!(SUITE, |case| {
        // Given a service whose model API has no key, and a staged edit
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        chain.repo.write("README.md", "staged\n");
        chain.repo.git(&["add", "README.md"]);
        // When the phone generates a message
        let reply = phone.call(case, &chain, &[], "git.generateCommitMessage", sel(json!({})));
        // Then the answer is a failed generation with the service's reason, not a made-up message
        assert_eq!(reply["result"]["success"], json!(false));
        assert!(reply["result"]["error"].as_str().unwrap().contains("KIMI_API_KEY"), "{reply}");
        answered(case, "git.generateCommitMessage", false);
    });
}

fn over_defaults(key: &str, set: Value) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/map-state/src/defaults.json");
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut merged = all[key].as_object().unwrap().clone();
    merged.extend(set.as_object().unwrap().clone());
    Value::Object(merged)
}

#[test]
fn settings_quick_commands_and_ui_state_are_kept_by_rca_and_read_back() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired phone and an rca that keeps nothing of Orca's
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        let kept = |rca: &mut Direct, key: &str| rca.call("state", "state.get", json!({"scope": "orca", "key": key})).unwrap()["value"].clone();
        // When the phone reads its settings
        let empty = phone.call(case, &chain, &["rpc:settings.get"], "settings.get", json!({}));
        // Then the read reached rca's state, the answer is the host's defaults and nothing was kept for them
        answered(case, "state.get", true);
        assert_eq!(empty["result"]["settings"], over_defaults("settings", json!({})));
        assert_eq!(kept(&mut rca, "settings"), json!(null));
        // When it updates two settings in two calls
        phone.call(case, &chain, &["rpc:settings.update"], "settings.update", json!({"compactWorktreeCards": true}));
        answered(case, "state.set", true);
        let updated = phone.call(case, &chain, &[], "settings.update", json!({"defaultTaskViewPreset": "prs"}));
        // Then rca keeps both, and the answer and the next read say so
        let both = json!({"compactWorktreeCards": true, "defaultTaskViewPreset": "prs"});
        assert_eq!(kept(&mut rca, "settings"), both);
        assert_eq!(updated["result"]["settings"], over_defaults("settings", both.clone()));
        let read = phone.call(case, &chain, &[], "settings.get", json!({}));
        assert_eq!(read["result"]["settings"], over_defaults("settings", both.clone()));
        // When it sets a setting the schema does not know
        let refused = phone.call(case, &chain, &[], "settings.update", json!({"noSuchSetting": true}));
        // Then it is refused before rca is written to, and what rca keeps is unchanged
        assert_eq!(refused["error"]["code"], json!("invalid_argument"));
        assert_eq!(kept(&mut rca, "settings"), both);
        // When it adds a quick command and reads the list
        phone.call(case, &chain, &["rpc:settings.updateTerminalQuickCommands"], "settings.updateTerminalQuickCommands", json!({"mutation": {"type": "upsert", "command": {"id": "build", "label": "Build", "command": "cargo build", "appendEnter": true}}}));
        answered(case, "state.set", true);
        let listed = phone.call(case, &chain, &["rpc:settings.getTerminalQuickCommands"], "settings.getTerminalQuickCommands", json!({}));
        answered(case, "state.get", true);
        // Then rca keeps the command as the phone reads it
        assert_eq!(listed["result"]["terminalQuickCommands"], kept(&mut rca, "terminalQuickCommands"));
        assert_eq!(listed["result"]["terminalQuickCommands"][0]["command"], json!("cargo build"));
        // When it deletes the command
        let left = phone.call(case, &chain, &[], "settings.updateTerminalQuickCommands", json!({"mutation": {"type": "delete", "id": "build"}}));
        // Then none is left, in the answer and in rca
        assert_eq!((left["result"]["terminalQuickCommands"].clone(), kept(&mut rca, "terminalQuickCommands")), (json!([]), json!([])));
        // When it sets UI values in two calls and reads them
        phone.call(case, &chain, &["rpc:ui.set"], "ui.set", json!({"sidebarWidth": 320}));
        answered(case, "state.set", true);
        phone.call(case, &chain, &[], "ui.set", json!({"sortBy": "recent"}));
        let ui = phone.call(case, &chain, &["rpc:ui.get"], "ui.get", json!({}));
        answered(case, "state.get", true);
        // Then rca keeps both values and the phone reads both
        let both = json!({"sidebarWidth": 320, "sortBy": "recent"});
        assert_eq!(kept(&mut rca, "ui"), both);
        assert_eq!(ui["result"]["ui"], over_defaults("ui", both));
        // And nothing of this is in the scope another app's state would use
        assert_eq!(rca.call("state", "state.get", json!({"scope": "t3", "key": "settings"})).unwrap()["value"], Value::Null);
    });
}

fn codex_login() -> Turn {
    Turn::default().then_line("Open https://login.example.test/device to sign in.").then_line("Enter the code ABCD-1234 there.").then_sleeping(500).then_writing_credential("CODEX_HOME", "auth.json", &format!("{{\"access_token\":\"{CODEX_TOKEN}\"}}"))
}

fn claude_login() -> Turn {
    Turn::default().then_line("Open https://login.example.test/paste and paste the code.").then_reading_a_line().then_writing_credential("CLAUDE_CONFIG_DIR", ".credentials.json", "{\"access_token\":\"{paste}\"}")
}

fn selected(rca: &mut Direct) -> Value {
    rca.call("accounts", "provider.list", json!({})).unwrap()["selected"].clone()
}

#[test]
fn the_accounts_screen_lists_the_signed_in_providers_and_selecting_one_changes_the_provider_rca_uses() {
    exec_testkit::case!(SUITE, |case| {
        // Given Codex signed in by a device code and Claude by a pasted code, each by rca's own login, and a paired phone
        let chain = Chain::start(None);
        chain.cli.script("codex", &exec_testkit::script::Script::of(vec![codex_login()]));
        chain.cli.script("claude", &exec_testkit::script::Script::of(vec![claude_login()]));
        let mut rca = chain.direct();
        let codex = rca.call("accounts", "provider.auth.start", json!({"provider": "codex"})).unwrap();
        let done = rca.call("accounts", "provider.auth.complete", json!({"auth": codex["auth"]["auth"], "timeout_ms": 30000})).unwrap();
        assert_eq!(done["auth"]["status"], json!("succeeded"), "{done}");
        let claude = rca.call("accounts", "provider.auth.start", json!({"provider": "claudeAgent", "method": "paste"})).unwrap();
        rca.call("accounts", "provider.auth.respond", json!({"auth": claude["auth"]["auth"], "text": CLAUDE_TOKEN})).unwrap();
        let done = rca.call("accounts", "provider.auth.complete", json!({"auth": claude["auth"]["auth"], "timeout_ms": 30000})).unwrap();
        assert_eq!(done["auth"]["status"], json!("succeeded"), "{done}");
        let mut phone = Phone::connect(case, &chain);
        // When the phone lists the accounts
        let listed = phone.call(case, &chain, &["rpc:accounts.list"], "accounts.list", json!({"refreshUsage": true}));
        // Then rca was asked for its providers, and the screen has one account for each signed-in provider, the active one being the provider rca uses
        answered(case, "provider.list", true);
        let snapshot = &listed["result"];
        assert_eq!((snapshot["claude"]["accounts"][0]["id"].clone(), snapshot["codex"]["accounts"][0]["id"].clone()), (json!("claudeAgent"), json!("codex")));
        let chosen = selected(&mut rca);
        assert!(chosen == json!("claudeAgent") || chosen == json!("codex"), "rca chose {chosen}");
        let active = |side: &str| snapshot[side]["activeAccountId"].clone();
        assert_eq!((active("claude"), active("codex")), if chosen == json!("claudeAgent") { (chosen.clone(), Value::Null) } else { (Value::Null, chosen.clone()) });
        // When the phone selects Claude's account
        let picked = phone.call(case, &chain, &["rpc:accounts.selectClaude"], "accounts.selectClaude", json!({"accountId": "claudeAgent"}));
        // Then rca's selected provider is Claude, and the answer's snapshot shows it
        answered(case, "provider.select", true);
        assert_eq!((selected(&mut rca), picked["result"]["claude"]["activeAccountId"].clone(), picked["result"]["codex"]["activeAccountId"].clone()), (json!("claudeAgent"), json!("claudeAgent"), Value::Null));
        // When it selects Codex, and then Claude and Codex again through the row that names a target
        phone.call(case, &chain, &["rpc:accounts.selectCodex"], "accounts.selectCodex", json!({"accountId": "codex"}));
        answered(case, "provider.select", true);
        assert_eq!(selected(&mut rca), json!("codex"));
        phone.call(case, &chain, &[], "accounts.selectClaude", json!({"accountId": "claudeAgent"}));
        phone.call(case, &chain, &["rpc:accounts.selectCodexForTarget"], "accounts.selectCodexForTarget", json!({"accountId": "codex", "target": {"runtime": "host", "wslDistro": null}}));
        answered(case, "provider.select", true);
        // Then Codex is the selected provider again
        assert_eq!(selected(&mut rca), json!("codex"));
        // When it asks for a WSL target, and for an account that is no provider's
        let wsl = phone.call(case, &chain, &[], "accounts.selectCodexForTarget", json!({"accountId": "codex", "target": {"runtime": "wsl", "wslDistro": "Ubuntu"}}));
        let stranger = phone.call(case, &chain, &[], "accounts.selectClaude", json!({"accountId": "someone-else"}));
        // Then both are refused and Codex is still selected
        assert_eq!((wsl["error"]["code"].clone(), stranger["error"]["code"].clone(), selected(&mut rca)), (json!("invalid_argument"), json!("invalid_argument"), json!("codex")));
    });
}
