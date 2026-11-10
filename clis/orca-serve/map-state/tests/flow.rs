//! The settings, quick-command and UI rows through the `Host` dispatcher with rca's state doubled: each change is read back by the row that reads it,
//! and by a second connection of the same rca (what a restart of the server finds).
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_state::StateArea;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    conn: Connection,
    n: u32,
}

fn connect(agent: &Arc<ScriptedAgent>) -> Connection {
    let host = Host::new("rt".into(), vec![Box::new(StateArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
    host.connection("dev", "tok", vec![])
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        Rig { conn: connect(&agent), agent, n: 0 }
    }

    /// A phone on a new server process over the same rca.
    fn after_restart(&self) -> Rig {
        Rig { conn: connect(&self.agent), agent: self.agent.clone(), n: 0 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
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

    fn writes(&self) -> usize {
        self.agent.requests().iter().filter(|(m, _)| m == "state.set").count()
    }
}

fn defaults(key: &str) -> serde_json::Map<String, Value> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/defaults.json");
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    all[key].as_object().unwrap().clone()
}

fn over_defaults(key: &str, set: Value) -> Value {
    let mut all = defaults(key);
    all.extend(set.as_object().unwrap().clone());
    Value::Object(all)
}

fn labels(result: &Value) -> Vec<String> {
    result["terminalQuickCommands"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn given_no_setting_kept_when_the_phone_reads_and_updates_settings_then_the_patch_is_merged_and_a_restarted_server_reads_it_back() {
    // Given nothing kept
    let mut rig = Rig::new();
    // When the phone reads, sets one setting, and sets another in a second patch
    let empty = rig.ok("settings.get", json!({}));
    rig.ok("settings.update", json!({"compactWorktreeCards": true}));
    let merged = rig.ok("settings.update", json!({"defaultTaskViewPreset": "prs", "defaultRepoSelection": ["r1"]}));
    // Then the first read answers the host's defaults, and the second patch kept the first one's setting over them
    assert_eq!(empty, json!({"settings": defaults("settings")}));
    assert_eq!(merged["settings"], over_defaults("settings", json!({"compactWorktreeCards": true, "defaultTaskViewPreset": "prs", "defaultRepoSelection": ["r1"]})));
    // And a server started afterwards reads both
    assert_eq!(rig.after_restart().ok("settings.get", json!({}))["settings"], merged["settings"]);
}

#[test]
fn given_settings_the_schema_refuses_when_updated_then_each_is_refused_and_nothing_is_kept() {
    let mut rig = Rig::new();
    for (patch, says) in [
        (json!({"noSuchSetting": 1}), "Unknown setting: noSuchSetting"),
        (json!({"compactWorktreeCards": "yes"}), "Invalid value for setting compactWorktreeCards"),
        (json!({"defaultTaskViewPreset": "everything"}), "Invalid value for setting defaultTaskViewPreset"),
        (json!({"defaultRepoSelection": [1]}), "Invalid value for setting defaultRepoSelection"),
        (json!({"compactWorktreeCards": true, "machineName": "x"}), "set where it runs"),
        (json!("a string"), "takes an object"),
    ] {
        // When the phone sends the patch, Then it is refused as an invalid argument saying why, and the valid part of it was not kept either
        let (code, message) = rig.err("settings.update", patch.clone());
        assert_eq!(code, "invalid_argument", "{patch}");
        assert!(message.contains(says), "{patch}: {message}");
    }
    assert_eq!(rig.ok("settings.get", json!({}))["settings"], Value::Object(defaults("settings")));
    assert_eq!(rig.writes(), 0);
}

#[test]
fn given_quick_commands_when_upserted_replaced_and_deleted_then_the_list_follows_one_command_at_a_time_in_the_form_orca_keeps_them() {
    let mut rig = Rig::new();
    // When two commands are added, one with surrounding blanks and no scope, one for an agent in a repo
    rig.ok("settings.updateTerminalQuickCommands", json!({"mutation": {"type": "upsert", "command": {"id": " build ", "label": " Build ", "command": "cargo build  \n", "appendEnter": true}}}));
    let added = rig.ok("settings.updateTerminalQuickCommands", json!({"mutation": {"type": "upsert", "command": {"id": "fix", "label": "Fix", "action": "agent-prompt", "agent": "claude", "prompt": "fix it\n", "scope": {"type": "repo", "repoId": " r1 "}}}}));
    // Then they are kept trimmed, with a global scope where none was named
    assert_eq!(added["terminalQuickCommands"][0], json!({"id": "build", "label": "Build", "scope": {"type": "global"}, "action": "terminal-command", "command": "cargo build", "appendEnter": true}));
    assert_eq!(added["terminalQuickCommands"][1], json!({"id": "fix", "label": "Fix", "scope": {"type": "repo", "repoId": "r1"}, "action": "agent-prompt", "agent": "claude", "prompt": "fix it"}));
    // When the first is edited by its id and the second deleted
    rig.ok("settings.updateTerminalQuickCommands", json!({"mutation": {"type": "upsert", "command": {"id": "build", "label": "Build all", "command": "cargo build --all", "appendEnter": false}}}));
    let left = rig.ok("settings.updateTerminalQuickCommands", json!({"mutation": {"type": "delete", "id": "fix"}}));
    // Then one command is left, with the edit, and a restarted server reads the same list
    assert_eq!((labels(&left), left["terminalQuickCommands"][0]["label"].clone(), left["terminalQuickCommands"][0]["appendEnter"].clone()), (vec!["build".to_string()], json!("Build all"), json!(false)));
    assert_eq!(rig.after_restart().ok("settings.getTerminalQuickCommands", json!({})), left);
}

#[test]
fn given_a_quick_command_that_is_malformed_when_it_is_upserted_then_it_is_refused_and_the_list_is_unchanged() {
    let mut rig = Rig::new();
    rig.ok("settings.updateTerminalQuickCommands", json!({"mutation": {"type": "upsert", "command": {"id": "a", "label": "A", "command": "ls", "appendEnter": true}}}));
    for (mutation, says) in [
        (json!({"type": "upsert", "command": {"id": "", "label": "x", "command": "ls", "appendEnter": true}}), "id is 1 to 80"),
        (json!({"type": "upsert", "command": {"id": "b", "label": "x", "command": "ls"}}), "appendEnter must be a boolean"),
        (json!({"type": "upsert", "command": {"id": "b", "label": "x", "action": "agent-prompt", "prompt": "p"}}), "agent must be a string"),
        (json!({"type": "upsert", "command": {"id": "b", "label": "x", "command": "ls", "appendEnter": true, "scope": {"type": "repo"}}}), "scope is global or a repo"),
        (json!({"type": "upsert", "command": {"id": "b", "label": "x", "action": "other", "command": "ls", "appendEnter": true}}), "action is terminal-command or agent-prompt"),
        (json!({"type": "delete", "id": ""}), "Missing quick command id"),
        (json!({"type": "rename"}), "an upsert or a delete"),
    ] {
        // When the phone sends it, Then it is refused as an invalid argument saying why
        let (code, message) = rig.err("settings.updateTerminalQuickCommands", json!({"mutation": mutation.clone()}));
        assert_eq!(code, "invalid_argument", "{mutation}");
        assert!(message.contains(says), "{mutation}: {message}");
    }
    assert_eq!(labels(&rig.ok("settings.getTerminalQuickCommands", json!({}))), ["a"]);
}

#[test]
fn given_ui_values_when_set_in_parts_then_each_part_is_merged_and_read_back_by_ui_get_and_a_restarted_server() {
    let mut rig = Rig::new();
    // When the phone reads, sets the sidebar width, then the sort order
    let empty = rig.ok("ui.get", json!({}));
    rig.ok("ui.set", json!({"sidebarWidth": 320}));
    let set = rig.ok("ui.set", json!({"sortBy": "recent"}));
    // Then the first read answers the host's defaults, and the second set kept the first one over them
    assert_eq!(empty, json!({"ui": defaults("ui")}));
    assert_eq!(set["ui"], over_defaults("ui", json!({"sidebarWidth": 320, "sortBy": "recent"})));
    assert_eq!(rig.after_restart().ok("ui.get", json!({})), set);
    // And a value that is not an object is refused without a write
    let writes = rig.writes();
    assert_eq!(rig.err("ui.set", json!([1])).0, "invalid_argument");
    assert_eq!(rig.writes(), writes);
}

#[test]
fn given_a_kept_value_of_the_wrong_shape_when_it_is_read_then_the_call_is_refused_not_answered_as_empty() {
    // Given rca holding a list where the UI object belongs, and an object where the quick commands list belongs
    let mut rig = Rig::new();
    rig.agent.set_state("orca", "ui", json!([1]));
    rig.agent.set_state("orca", "terminalQuickCommands", json!({"a": 1}));
    // When the phone reads either, or updates over them
    // Then each is refused saying what is wrong
    for (method, params, says) in [("ui.get", json!({}), "not an object"), ("ui.set", json!({"a": 1}), "not an object"), ("settings.getTerminalQuickCommands", json!({}), "not a list")] {
        let (code, message) = rig.err(method, params);
        assert_eq!(code, "runtime_error", "{method}");
        assert!(message.contains(says), "{method}: {message}");
    }
    assert_eq!(rig.writes(), 0);
}

#[test]
fn given_the_orca_pin_when_the_kept_copy_of_its_defaults_is_checked_then_it_is_the_copy_taken_at_that_pin() {
    use sha2::{Digest, Sha256};
    // Given the pin the conformance tooling reads, and the provenance recorded beside the copy of the pinned host's defaults
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |path: String| serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap();
    let pin = read(format!("{dir}/../../../tooling/orca-conformance/orca-pin.json"));
    let provenance = read(format!("{dir}/src/defaults.provenance.json"));
    let digest = format!("{:x}", Sha256::digest(std::fs::read(format!("{dir}/src/defaults.json")).unwrap()));
    // When the copy is compared with them
    // Then it was taken at the pinned commit and has not been edited since: after a pin move, copy the defaults from the new tree and record its commit and digest
    assert_eq!(provenance["orcaCommit"], pin["commit"], "the Orca pin moved: src/defaults.json was copied at another commit");
    assert_eq!(provenance["orcaVersion"], pin["version"]);
    assert_eq!(provenance["defaultsSha256"], json!(digest), "src/defaults.json differs from the copy recorded in src/defaults.provenance.json");
}
