//! What a chat can run with and what the executor has, through the chain: the models of an agent, the agent CLIs the executor finds on its own
//! `PATH` (fake ones with fixed versions), the skills in its repository, and a chat's options. A row counts only when the catalog method it maps
//! to answered in its call's window and the executor ran the action that reads the machine; a refusal is shown to have reached the service.
use crate::agent_m7::{answered, asked_nothing_of, create_params, executor_ran};
use crate::rig::*;
use exec_testkit::script::Script;
use serde_json::{json, Value};

const VERSIONS: [(&str, &str); 3] = [("claude", "2.1.30 (Claude Code)"), ("codex", "codex-cli 0.9.1"), ("opencode", "1.19.0")];
const WS_SELECTOR: &str = "id:w1::";

fn install_versions(chain: &Chain) {
    for (program, version) in VERSIONS {
        chain.cli.script(program, &Script::of(vec![]).printing_version(version));
    }
}

fn ids(models: &Value) -> Vec<String> {
    models["models"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_string()).collect()
}

fn sorted(list: &Value) -> Vec<String> {
    let mut agents: Vec<String> = list.as_array().unwrap().iter().map(|a| a.as_str().unwrap().to_string()).collect();
    agents.sort();
    agents
}

/// The agents the executor reported to the service, with the line each printed for `--version`.
fn reported(rca: &mut Direct) -> Vec<(String, bool, Option<String>)> {
    let found = rca.call("agent.catalog", "agents.detect", json!({"refresh": true})).expect("the service answers the detection");
    found["agents"].as_array().unwrap().iter().map(|a| (a["driver"].as_str().unwrap().to_string(), a["installed"] == true, a["version"].as_str().map(str::to_string))).collect()
}

#[test]
fn the_phone_lists_the_models_of_an_agent_reads_a_chats_options_and_sets_one_and_its_commands_come_from_the_executors_skills() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a skill in the executor's repository, a paired phone and a claude chat
        let chain = Chain::start(None);
        install_versions(&chain);
        let skill = chain.repo.path().join(".claude/skills/review/SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "---\nname: review\ndescription: Review the diff\n---\nbody\n").unwrap();
        let mut phone = Phone::connect(case, &chain);
        let made = phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params("claude_o1", "c1", None));
        assert_eq!(made["result"]["ok"], true, "{made}");
        // When the phone asks for the models of claude
        let catalog = phone.call(case, &chain, &["rpc:agentSession.modelCatalog"], "agentSession.modelCatalog", json!({"agent": "claude"}));
        // Then the service's catalog answered, and the phone has the models
        answered(case, "models.list");
        let models = ids(&catalog["result"]);
        assert!(models.len() >= 2, "{catalog}");
        // And the chat's options list the same models with the default as the current one
        let options = phone.call(case, &chain, &["rpc:agentSession.options"], "agentSession.options", json!({"sessionId": "claude_o1"}));
        answered(case, "models.list");
        assert_eq!(ids(&options["result"]), models);
        let default = options["result"]["models"].as_array().unwrap().iter().find(|m| m["isDefault"] == true).expect("one default")["id"].clone();
        assert_eq!(options["result"]["current"]["model"], default, "{options}");
        // When a model that is not the default is picked
        let other = models.iter().find(|m| **m != default.as_str().unwrap()).unwrap().clone();
        let set = phone.call(case, &chain, &["rpc:agentSession.setOption"], "agentSession.setOption", json!({"envelope": json!({"sessionId": "claude_o1", "clientOperationId": "o1", "expectedRuntimeFence": null, "payloadFingerprint": "fp-o1"}), "key": "model", "value": other}));
        // Then it is accepted, and the chat's options show it
        answered(case, "models.list");
        assert_eq!(set["result"]["ok"], true, "{set}");
        let again = phone.call(case, &chain, &["rpc:agentSession.options"], "agentSession.options", json!({"sessionId": "claude_o1"}));
        assert_eq!(again["result"]["current"]["model"], json!(other), "{again}");
        // And a model the catalog does not list is refused after the service was asked for the catalog, leaving the pick as it was
        let refused = phone.refused(case, &chain, &["rpc:agentSession.setOption"], "agentSession.setOption", json!({"envelope": json!({"sessionId": "claude_o1", "clientOperationId": "o2", "expectedRuntimeFence": null, "payloadFingerprint": "fp-o2"}), "key": "model", "value": "no-such-model"}));
        answered(case, "models.list");
        assert_eq!(refused["result"]["ok"], false, "{refused}");
        let kept = phone.call(case, &chain, &["rpc:agentSession.options"], "agentSession.options", json!({"sessionId": "claude_o1"}));
        assert_eq!(kept["result"]["current"]["model"], json!(other));
        // When the phone asks for the chat's commands
        let commands = phone.call(case, &chain, &["rpc:agentSession.commands"], "agentSession.commands", json!({"sessionId": "claude_o1"}));
        // Then the service listed them from the executor's skills: the repository's skill is one of them and the executor scanned for it
        answered(case, "commands.list");
        assert!(executor_ran(case, "skill_scan"), "{:?}", case.calls().last());
        let listed = commands["result"]["commands"].as_array().unwrap();
        assert!(listed.iter().any(|c| c["name"] == "review"), "{commands}");
        // And a chat the host does not have asks the service nothing of the catalog
        let ghost = phone.refused(case, &chain, &["rpc:agentSession.options"], "agentSession.options", json!({"sessionId": "claude_none"}));
        asked_nothing_of(case, "models.list");
        assert_eq!(ghost["ok"], false, "{ghost}");
    });
}

#[test]
fn the_executors_agent_clis_are_found_on_its_path_and_a_missing_one_is_reported_missing_and_its_skills_are_discovered() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain whose executor has claude, codex and opencode on its PATH, each printing a fixed version, and a skill in its repository
        let chain = Chain::start(None);
        install_versions(&chain);
        let skill = chain.repo.path().join(".claude/skills/review/SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "---\nname: review\ndescription: Review the diff\n---\nbody\n").unwrap();
        let personal = chain.home.path().join(".claude/skills/standup/SKILL.md");
        std::fs::create_dir_all(personal.parent().unwrap()).unwrap();
        std::fs::write(&personal, "---\nname: standup\ndescription: Summarise yesterday\n---\nbody\n").unwrap();
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        // When the phone asks which agents the executor has
        let detected = phone.call(case, &chain, &["rpc:preflight.detectAgents"], "preflight.detectAgents", json!({}));
        // Then the executor ran the detection and every agent is there
        answered(case, "agents.detect");
        assert!(executor_ran(case, "cli_detect"), "{:?}", case.calls().last());
        assert_eq!(sorted(&detected["result"]), VERSIONS.map(|(p, _)| p.to_string()), "{detected}");
        // And what the service holds of each CLI is the version it printed
        let seen = reported(&mut rca);
        for (program, version) in VERSIONS {
            let found = seen.iter().find(|(driver, ..)| driver.to_lowercase().starts_with(program)).unwrap_or_else(|| panic!("{program} is not reported: {seen:?}"));
            assert_eq!((found.1, found.2.as_deref()), (true, Some(version)), "{seen:?}");
        }
        // And a chat can be created for an agent whose CLI is there
        let supported = phone.call(case, &chain, &["rpc:agentSession.createSupport"], "agentSession.createSupport", json!({"worktree": WS_SELECTOR, "agent": "opencode"}));
        assert_eq!(supported["result"], json!({"supported": true}), "{supported}");
        // And the executor itself, asked by its connection id, answers the same list
        let remote = phone.call(case, &chain, &["rpc:preflight.detectRemoteAgents"], "preflight.detectRemoteAgents", json!({"connectionId": "w1"}));
        answered(case, "agents.detect");
        assert_eq!(sorted(&remote["result"]), sorted(&detected["result"]));
        // When opencode is gone from the executor's PATH and the phone refreshes
        std::fs::remove_file(chain.cli.dir().join("opencode")).unwrap();
        let refreshed = phone.call(case, &chain, &["rpc:preflight.refreshAgents"], "preflight.refreshAgents", json!({}));
        // Then the executor looked again and reports opencode missing, the others still there
        answered(case, "agents.detect");
        assert!(executor_ran(case, "cli_detect"), "{:?}", case.calls().last());
        assert_eq!(sorted(&refreshed["result"]["agents"]), ["claude", "codex"], "{refreshed}");
        let after = reported(&mut rca);
        assert!(after.iter().any(|(driver, installed, _)| driver.to_lowercase().contains("open") && !installed), "{after:?}");
        // And a chat for the missing agent is not supported while one for claude is
        let missing = phone.call(case, &chain, &["rpc:agentSession.createSupport"], "agentSession.createSupport", json!({"worktree": WS_SELECTOR, "agent": "opencode"}));
        assert_eq!(missing["result"], json!({"supported": false, "reason": "agent"}), "{missing}");
        let claude = phone.call(case, &chain, &["rpc:agentSession.createSupport"], "agentSession.createSupport", json!({"worktree": WS_SELECTOR, "agent": "claude"}));
        assert_eq!(claude["result"], json!({"supported": true}));
        // And a connection the service has no executor for is refused after the service was asked
        let ghost = phone.refused(case, &chain, &["rpc:preflight.detectRemoteAgents"], "preflight.detectRemoteAgents", json!({"connectionId": "ghost"}));
        assert_eq!(ghost["ok"], false, "{ghost}");
        let window = last_window(case);
        assert!(window.iter().any(|(m, ok)| m == "agents.detect" && !*ok), "the service was not asked for the unknown executor: {window:?}");
        assert_eq!(ghost["error"]["message"], "Remote connection dropped. Click Reconnect on the SSH target before retrying.", "{ghost}");
        // When the phone discovers the skills of the executor's workspace
        let discovered = phone.call(case, &chain, &["rpc:skills.discover"], "skills.discover", json!({"worktreeId": "w1::"}));
        // Then the executor scanned its disk and the repository's skill is listed with its file under the repository
        answered(case, "skills.list");
        assert!(executor_ran(case, "skill_scan"), "{:?}", case.calls().last());
        let skills = discovered["result"]["skills"].as_array().unwrap();
        let found = skills.iter().find(|s| s["name"] == "review").unwrap_or_else(|| panic!("{discovered}"));
        assert_eq!((found["description"].clone(), found["sourceKind"].clone()), (json!("Review the diff"), json!("repo")));
        assert!(found["skillFilePath"].as_str().unwrap().ends_with(".claude/skills/review/SKILL.md"), "{found}");
        let home_skill = skills.iter().find(|s| s["name"] == "standup").unwrap_or_else(|| panic!("{discovered}"));
        assert_eq!((home_skill["description"].clone(), home_skill["sourceKind"].clone()), (json!("Summarise yesterday"), json!("home")));
        assert!(discovered["result"]["sources"].as_array().unwrap().iter().any(|s| s["sourceKind"] == "repo" && s["path"].as_str().unwrap().ends_with(".claude/skills") && s["exists"] == true), "{discovered}");
    });
}
