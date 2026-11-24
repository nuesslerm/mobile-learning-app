//! The group over an executor registry and a double of the executor's channel: what each method asks of the executor, what the service
//! keeps and for how long, and how the executor's answers and refusals come back.
use super::*;
use crate::testing::{ScriptedSpeech, SetClock};
use agent_api_catalog as api;
use agent_api_proto::Method;
use rca_agent_core::{AgentApi, Build, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::Registry;
use serde_json::{json, Value};
use std::sync::Arc;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

fn ok(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

struct Rig {
    api: AgentApi,
    clock: Arc<SetClock>,
    channel: Arc<ScriptedChannel>,
    executors: Arc<Registry>,
}

fn rig(answers: impl Fn(&str, &Value) -> Result<ActionOutput, RemoteError> + Send + Sync + 'static) -> Rig {
    let channel = ScriptedChannel::new(answers);
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "w1", channel.clone() as Arc<dyn ExecutorChannel>).unwrap();
    let clock = Arc::new(SetClock::at(1_000));
    let groups: Vec<Arc<dyn Group>> = vec![Arc::new(CatalogGroup::new(Catalog::new(executors.clone(), clock.clone(), Arc::new(ScriptedSpeech::saying("hello world")))))];
    Rig { api: AgentApi::new(groups).unwrap(), clock, channel, executors }
}

impl Rig {
    async fn ask(&self, method: Method, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
    }
}

fn clis(found: &[(&str, Option<&str>)]) -> Value {
    json!({"clis": found.iter().map(|(program, version)| match version {
        Some(v) => json!({"program": program, "installed": true, "path": format!("/bin/{program}"), "version": v}),
        None => json!({"program": program, "installed": false}),
    }).collect::<Vec<_>>()})
}

#[test]
fn given_the_served_rows_when_compared_with_the_contract_then_they_are_its_methods_in_order_naming_the_actions_the_contract_declares() {
    // Given the rows and the contract's table of actions
    let rows: Vec<(Method, Vec<&str>)> = SERVED.iter().map(|s| (s.method, s.actions.to_vec())).collect();
    // Then each method of the contract has its row, in order, with the actions the contract declares for it
    let declared: Vec<(Method, Vec<&str>)> = api::ACTIONS.iter().map(|(m, a)| (*m, a.to_vec())).collect();
    assert_eq!(rows, declared);
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), api::METHODS);
}

#[test]
fn given_every_driver_of_the_contract_when_its_models_are_listed_then_one_is_the_default_and_the_modes_default_to_ones_it_lists() {
    for driver in api::DRIVERS {
        // When the table answers for the driver
        let listed = table::for_driver(driver);
        // Then exactly one model is the default, and each default mode is among the listed ones
        assert_eq!(listed.models.iter().filter(|m| m.is_default).count(), 1, "{driver}");
        assert!(listed.runtime_modes.iter().any(|c| c.value == listed.default_runtime_mode), "{driver}");
        assert!(listed.interaction_modes.iter().any(|c| c.value == listed.default_interaction_mode), "{driver}");
        // And the program it runs is one the executor detects
        assert!(exec_proc::AGENT_CLIS.contains(&table::program(driver)), "{driver}");
    }
}

#[test]
fn given_the_codex_catalog_when_a_client_lists_models_then_gpt_6_1_sol_is_available_for_selection() {
    let listed = table::for_driver("codex");
    assert!(listed.models.iter().any(|m| m.id == "gpt-6.1-sol" && m.label == "GPT-6.1 Sol"));
}

#[tokio::test]
async fn given_an_executor_with_two_of_the_clis_when_agents_are_detected_then_each_driver_is_reported_in_table_order_and_the_answer_is_kept_until_it_is_old_or_refreshed() {
    // Given an executor that has claude and codex
    let rig = rig(|tool, _| match tool {
        "cli_detect" => ok(clis(&[("opencode", None), ("claude", Some("2.1.7 (Claude Code)")), ("codex", Some("codex-cli 0.9.1")), ("gh", Some("gh version 2.83.0")), ("git", Some("git version 2.43.0"))])),
        other => panic!("{other}"),
    });
    // When detected, then asked again a moment later
    let first = rig.ask(api::AGENTS_DETECT, json!({})).await;
    rig.clock.advance(1_000);
    let second = rig.ask(api::AGENTS_DETECT, json!({})).await;
    // Then the drivers come in table order with their versions, the second answer is the first (the executor was asked once), and it says when it was got
    let agents = first["result"]["agents"].as_array().unwrap();
    assert_eq!(agents.iter().map(|a| a["driver"].as_str().unwrap()).collect::<Vec<_>>(), ["claudeAgent", "codex"]);
    assert_eq!((agents[0]["installed"].clone(), agents[0]["version"].clone()), (json!(true), json!("2.1.7 (Claude Code)")));
    assert_eq!((second.clone(), rig.channel.calls().len()), (first.clone(), 1));
    assert_eq!(first["result"]["checked_at_ms"], json!(1_000));
    // And the programs asked are the table's, in the table's order, then the tools
    assert_eq!(rig.channel.calls()[0].1, json!({"programs": ["opencode", "claude", "codex", "git", "gh", "glab"]}));
    // And the tools come in their own order whatever order the executor answered in, one it did not answer for not installed
    assert_eq!(first["result"]["tools"], json!([
        {"program": "git", "installed": true, "path": "/bin/git", "version": "git version 2.43.0"},
        {"program": "gh", "installed": true, "path": "/bin/gh", "version": "gh version 2.83.0"},
        {"program": "glab", "installed": false},
    ]));
    // When a refresh is asked, or the kept answer is older than its life
    rig.ask(api::AGENTS_DETECT, json!({"refresh": true})).await;
    rig.clock.advance(service::DETECTION_TTL_MS);
    rig.ask(api::AGENTS_DETECT, json!({"executor": "e1"})).await;
    // Then the executor was asked each time
    assert_eq!(rig.channel.calls().len(), 3);
}

#[tokio::test]
async fn given_an_executor_with_no_agent_cli_when_agents_are_detected_then_no_driver_is_listed_and_the_tools_still_are_and_a_cli_that_did_not_answer_stays_listed() {
    // Given an executor that has only git
    let bare = rig(|_, _| ok(clis(&[("git", Some("git version 2.43.0"))])));
    // When agents are detected
    let none = bare.ask(api::AGENTS_DETECT, json!({})).await;
    // Then no driver is listed and git is reported
    assert_eq!(none["result"]["agents"], json!([]));
    assert_eq!(none["result"]["tools"][0], json!({"program": "git", "installed": true, "path": "/bin/git", "version": "git version 2.43.0"}));
    // Given an executor whose codex is there but did not answer its version probe
    let silent = rig(|_, _| ok(json!({"clis": [{"program": "codex", "installed": true, "path": "/bin/codex", "error": "timed out"}]})));
    // When agents are detected
    let hung = silent.ask(api::AGENTS_DETECT, json!({})).await;
    // Then codex is listed with the error and no version
    assert_eq!(hung["result"]["agents"], json!([{"driver": "codex", "program": "codex", "installed": true, "path": "/bin/codex", "error": "timed out"}]));
}

#[tokio::test]
async fn given_executors_that_are_not_attached_or_several_when_agents_are_detected_then_the_refusal_names_what_is_wrong_and_the_executor_is_not_asked() {
    // Given one executor
    let rig = rig(|_, _| ok(clis(&[])));
    // When an unknown one is named, Then it is UNKNOWN_EXECUTOR
    let unknown = rig.ask(api::AGENTS_DETECT, json!({"executor": "e9"})).await;
    assert_eq!(unknown["error"], json!({"code": "UNKNOWN_EXECUTOR", "message": "no executor named e9 is attached"}));
    // When a second executor attaches and none is named, Then the refusal lists them
    rig.executors.open_executor("e2", "w2", rig.channel.clone() as Arc<dyn ExecutorChannel>).unwrap();
    let several = rig.ask(api::AGENTS_DETECT, json!({})).await;
    assert_eq!(several["error"], json!({"code": "INVALID_PARAMS", "message": "several executors are attached (e1, e2): name one"}));
    assert!(rig.channel.calls().is_empty());
}

#[tokio::test]
async fn given_skills_in_the_workspace_and_the_home_when_commands_are_listed_then_only_the_ones_the_driver_reads_come_back_the_workspace_first_and_a_name_once() {
    // Given an executor that finds a skill in each of the three directories and one of the same name in the home
    let rig = rig(|tool, args| match tool {
        "skill_scan" => {
            assert_eq!(args, &json!({"root": ".", "home": true}));
            ok(json!({"skills": [
                {"name": "claude-one", "description": "d1", "scope": "workspace", "dir": ".claude/skills", "path": ".claude/skills/claude-one/SKILL.md"},
                {"name": "shared", "scope": "workspace", "dir": ".agents/skills", "path": ".agents/skills/shared/SKILL.md"},
                {"name": "codex-one", "scope": "workspace", "dir": ".codex/skills", "path": ".codex/skills/codex-one/SKILL.md"},
                {"name": "shared", "scope": "home", "dir": ".claude/skills", "path": "~/.claude/skills/shared/SKILL.md"}
            ], "truncated": false}))
        }
        other => panic!("{other}"),
    });
    // When the commands of each driver are listed for the workspace
    let names = |answer: &Value| answer["result"]["commands"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_string(), c["path"].as_str().unwrap().to_string(), c["builtin"].clone())).collect::<Vec<_>>();
    let claude = names(&rig.ask(api::COMMANDS_LIST, json!({"driver": "claudeAgent", "workspace": "w1"})).await);
    let codex = names(&rig.ask(api::COMMANDS_LIST, json!({"driver": "codex", "workspace": "w1"})).await);
    // Then claude reads .claude/skills first and .agents/skills next (the workspace's `shared` hides the home's), codex reads .agents and .codex
    assert_eq!(claude.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["claude-one", "shared"]);
    assert_eq!(claude[1].1, ".agents/skills/shared/SKILL.md");
    assert_eq!(codex.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["shared", "codex-one"]);
    assert!(claude.iter().chain(&codex).all(|c| c.2 == json!(false)));
    // And with no workspace the executor is not asked
    let before = rig.channel.calls().len();
    let none = rig.ask(api::COMMANDS_LIST, json!({"driver": "codex"})).await;
    assert_eq!((none["result"]["commands"].clone(), rig.channel.calls().len()), (json!([]), before));
}

#[tokio::test]
async fn given_a_workspace_nobody_serves_or_an_executor_that_refuses_when_skills_are_listed_then_the_refusal_is_the_contracts_and_the_executors_keeps_its_code() {
    // Given an executor that refuses skill_scan for a root outside the workspace
    let rig = rig(|_, _| Err(RemoteError::Action { code: "PATH_OUTSIDE_ROOT".into(), message: "root is outside the workspace".into() }));
    // When a workspace nobody serves is asked, Then it is NO_EXECUTOR and nothing was sent
    let nobody = rig.ask(api::SKILLS_LIST, json!({"workspace": "w9"})).await;
    assert_eq!(nobody["error"], json!({"code": "NO_EXECUTOR", "message": "no executor is attached for workspace w9"}));
    assert!(rig.channel.calls().is_empty());
    // When the served one is asked, Then the executor's code and message come back as they are
    let refused = rig.ask(api::SKILLS_LIST, json!({"workspace": "w1"})).await;
    assert_eq!(refused["error"], json!({"code": "PATH_OUTSIDE_ROOT", "message": "root is outside the workspace"}));
}

#[tokio::test]
async fn given_named_skill_directories_when_skills_are_listed_then_the_executor_scans_those_and_the_present_ones_come_back() {
    // Given an executor that answers which named directories exist
    let rig = rig(|tool, args| match tool {
        "skill_scan" => {
            assert_eq!(args, &json!({"root": ".", "home": true, "dirs": [".trae/skills"], "home_dirs": [".grok/skills"]}));
            ok(json!({"skills": [], "truncated": false, "present": [{"scope": "home", "dir": ".grok/skills"}]}))
        }
        other => panic!("{other}"),
    });
    // When the skills are listed with directories of the workspace and of the home
    let listed = rig.ask(api::SKILLS_LIST, json!({"workspace": "w1", "dirs": [".trae/skills"], "home_dirs": [".grok/skills"]})).await;
    // Then the present directory comes back as the executor reported it
    assert_eq!(listed["result"]["present"], json!([{"scope": "home", "dir": ".grok/skills"}]));
    // And a directory that climbs out is refused before the executor is asked
    let before = rig.channel.calls().len();
    let bad = rig.ask(api::SKILLS_LIST, json!({"workspace": "w1", "dirs": ["../x"]})).await;
    assert_eq!((bad["error"]["code"].clone(), rig.channel.calls().len()), (json!("INVALID_PARAMS"), before));
}

#[tokio::test]
async fn given_sessions_in_an_executors_home_when_listed_then_the_executors_answer_comes_back_under_its_name_and_the_asked_agents_and_limit_are_passed_on() {
    // Given an executor whose session_scan finds one claude session
    let rig = rig(|tool, args| match tool {
        "session_scan" => {
            assert_eq!(args, &json!({"agents": ["claude"], "limit": 5}));
            ok(json!({"sessions": [{"agent": "claude", "session_id": "s1", "path": "~/.claude/projects/p/s1.jsonl", "title": "Fix it", "cwd": "/w", "modified_ms": 9, "message_count": 2, "first_prompt": "fix it"}], "truncated": true}))
        }
        other => panic!("{other}"),
    });
    // When the sessions of claude are listed with a limit
    let listed = rig.ask(api::SESSIONS_LIST, json!({"agents": ["claude"], "limit": 5})).await;
    // Then the session comes back with the executor that was asked, and the cut is reported
    assert_eq!(listed["result"]["executor"], json!("e1"));
    assert_eq!(listed["result"]["sessions"][0]["path"], json!("~/.claude/projects/p/s1.jsonl"));
    assert_eq!(listed["result"]["truncated"], json!(true));
    // And a CLI that keeps no sessions is refused before the executor is asked, and an executor's refusal keeps its code
    let before = rig.channel.calls().len();
    let bad = rig.ask(api::SESSIONS_LIST, json!({"agents": ["opencode"]})).await;
    assert_eq!((bad["error"]["code"].clone(), rig.channel.calls().len()), (json!("INVALID_PARAMS"), before));
}

#[tokio::test]
async fn given_a_speech_engine_when_a_recording_is_transcribed_then_its_words_come_back_and_a_missing_engine_or_a_failure_is_refused_as_unavailable() {
    // Given a service whose engine hears a recording and says "hello world"
    let rig = rig(|_, _| ok(json!({})));
    // When the status is asked and a base64 recording is transcribed, with and without a format
    let status = rig.ask(api::SPEECH_STATUS, json!({})).await;
    let said = rig.ask(api::SPEECH_TRANSCRIBE, json!({"audio": "UklGRg==", "format": "webm"})).await;
    let not_base64 = rig.ask(api::SPEECH_TRANSCRIBE, json!({"audio": "***"})).await;
    // Then the engine is named, the words come back with how long they took, and text that is no base64 is a refusal of the params
    assert_eq!(status["result"], json!({"available": true, "engine": "whisper-cpp"}));
    assert_eq!(said["result"], json!({"text": "hello world", "took_ms": 0}));
    assert_eq!((not_base64["error"]["code"].as_str(), not_base64["error"]["message"].as_str()), (Some("INVALID_PARAMS"), Some("`audio` is not base64")));
    // And a service with no engine says so, and an engine that fails refuses with its own words
    let none = Catalog::new(rig.executors.clone(), rig.clock.clone(), Arc::new(ScriptedSpeech { engine: None, said: Ok(String::new()), heard: Default::default() }));
    assert_eq!(none.speech_status(), api::SpeechStatusResult { available: false, engine: "none".into() });
    let failing = Catalog::new(rig.executors.clone(), rig.clock.clone(), Arc::new(ScriptedSpeech { engine: Some("sidecar".into()), said: Err("the sidecar is down".into()), heard: Default::default() }));
    let refused = failing.transcribe(api::SpeechTranscribeParams { audio: "UklGRg==".into(), format: None }).await.unwrap_err();
    assert_eq!((refused.code.as_str(), refused.message.as_str()), ("SPEECH_UNAVAILABLE", "the sidecar is down"));
    let no_engine = none.transcribe(api::SpeechTranscribeParams { audio: "UklGRg==".into(), format: None }).await.unwrap_err();
    assert_eq!(no_engine.code, "SPEECH_UNAVAILABLE");
}
