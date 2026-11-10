//! The `agent.catalog` group of the agent API (`docs/plans/rca-parity/plan.md`, M7): what an agent driver offers and what an executor has.
//! `models.list` and `commands.list` answer from the service's table of the drivers (the one place that knows a driver's models and modes),
//! `agents.detect` and `skills.list` come from the EXECUTOR (the actions `cli_detect` of `exec-proc` and `skill_scan` of `exec-fs`), never from
//! the service's own disk: the machine that runs the CLI is the machine that has it. Neutral names: snake_case fields, times in milliseconds
//! since the epoch.
use agent_api_proto::{GroupName, Method, ParamError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("agent.catalog");

pub const MODELS_LIST: Method = Method::new(GROUP, "models.list");
pub const AGENTS_DETECT: Method = Method::new(GROUP, "agents.detect");
pub const SKILLS_LIST: Method = Method::new(GROUP, "skills.list");
pub const COMMANDS_LIST: Method = Method::new(GROUP, "commands.list");

/// Every method of the group.
pub const METHODS: &[Method] = &[MODELS_LIST, AGENTS_DETECT, SKILLS_LIST, COMMANDS_LIST];

/// The executor actions each method may ask, in the order it asks them. `models.list` answers from the table alone, and `commands.list` asks
/// only when it is given a workspace (the skills the driver reads are commands too).
pub const ACTIONS: &[(Method, &[&str])] = &[(MODELS_LIST, &[]), (AGENTS_DETECT, &["cli_detect"]), (SKILLS_LIST, &["skill_scan"]), (COMMANDS_LIST, &["skill_scan"])];

/// The drivers a request may name (`agent.threads` calls them `instance_id`): the ids of the service's driver table, which a test of the
/// service holds equal.
pub const DRIVERS: [&str; 3] = ["opencode", "claudeAgent", "codex"];

/// The skills directories (`exec_fs::SKILL_DIRS`) each driver reads, in the order its skills are listed.
pub fn skill_dirs(driver: &str) -> &'static [&'static str] {
    match driver {
        "claudeAgent" | "opencode" => &[".claude/skills", ".agents/skills"],
        "codex" => &[".agents/skills", ".codex/skills"],
        _ => &[],
    }
}

pub mod code {
    /// `agents.detect` names an executor that is not attached.
    pub const UNKNOWN_EXECUTOR: &str = "UNKNOWN_EXECUTOR";

    pub const ALL: [&str; 1] = [UNKNOWN_EXECUTOR];
}

pub mod message {
    pub fn unknown_executor(executor: &str) -> String {
        format!("no executor named {executor} is attached")
    }
}

fn read<T: DeserializeOwned>(params: &Value) -> Result<T, ParamError> {
    let params = if params.is_null() { json!({}) } else { params.clone() };
    serde_json::from_value(params).map_err(|e| ParamError(e.to_string()))
}

fn check_driver(driver: &str) -> Result<(), ParamError> {
    if DRIVERS.contains(&driver) {
        return Ok(());
    }
    Err(ParamError(format!("driver is one of {}", DRIVERS.join(", "))))
}

fn check_name(name: &str, value: &str) -> Result<(), ParamError> {
    if value.trim().is_empty() {
        return Err(ParamError(format!("{name} is required")));
    }
    Ok(())
}

/// One choice of a list the person picks from (an effort, a mode).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `models.list`: the models, runtime modes and interaction modes of `driver`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelsParams {
    pub driver: String,
}

impl ModelsParams {
    pub fn parse(params: &Value) -> Result<ModelsParams, ParamError> {
        let p: ModelsParams = read(params)?;
        check_driver(&p.driver)?;
        Ok(p)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The model a turn runs on when none is named; one model of a listing is the default.
    pub is_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// The efforts the driver can pass for this model: none when it passes none.
    pub efforts: Vec<Choice>,
}

/// The `runtime_mode` and `interaction_mode` values `turn.start` takes for the driver, each with the default a turn that names none runs with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelsResult {
    pub driver: String,
    pub models: Vec<Model>,
    pub runtime_modes: Vec<Choice>,
    pub default_runtime_mode: String,
    pub interaction_modes: Vec<Choice>,
    pub default_interaction_mode: String,
}

/// `agents.detect`: which agent CLIs `executor` (the only one attached when absent) has. The service keeps an answer a short while;
/// `refresh` asks the executor again.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectParams {
    #[serde(default)]
    pub executor: Option<String>,
    #[serde(default)]
    pub refresh: Option<bool>,
}

impl DetectParams {
    pub fn parse(params: &Value) -> Result<DetectParams, ParamError> {
        let p: DetectParams = read(params)?;
        if let Some(executor) = &p.executor {
            check_name("executor", executor)?;
        }
        Ok(p)
    }

    pub fn refresh(&self) -> bool {
        self.refresh.unwrap_or(false)
    }
}

/// What the executor found for one driver's CLI (`program`). `version` and `path` are the executor's (`cli_detect`); a CLI that is there but
/// did not answer has `error` and no version; one that is not there has only `installed: false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedAgent {
    pub driver: String,
    pub program: String,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectResult {
    /// The executor that was asked, by the name it attached under.
    pub executor: String,
    /// One entry per driver, in the order of `DRIVERS`.
    pub agents: Vec<DetectedAgent>,
    /// When the executor answered (an answer the service kept says when it was got, not when it was asked for).
    pub checked_at_ms: u64,
}

/// `skills.list`: the skills of `workspace` and of its executor's own home.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillsParams {
    pub workspace: String,
}

impl SkillsParams {
    pub fn parse(params: &Value) -> Result<SkillsParams, ParamError> {
        let p: SkillsParams = read(params)?;
        check_name("workspace", &p.workspace)?;
        Ok(p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Workspace,
    Home,
}

/// A skill as `skill_scan` found it. `dir` is the skills directory it lies in (`.claude/skills`, `.agents/skills`, `.codex/skills`); `path` is
/// its `SKILL.md`, relative to the workspace's executor root, or `~/<dir>/<name>/SKILL.md` for the executor's home.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub scope: Scope,
    pub dir: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillsResult {
    pub skills: Vec<Skill>,
    /// The executor stopped at its most (`skill_scan`'s `SKILLS_MAX`): there are more.
    pub truncated: bool,
}

/// `commands.list`: the commands `driver` offers in a chat, and, given `workspace`, the skills of that workspace and its executor's home that the
/// driver reads (a skill is a command by its name).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandsParams {
    pub driver: String,
    #[serde(default)]
    pub workspace: Option<String>,
}

impl CommandsParams {
    pub fn parse(params: &Value) -> Result<CommandsParams, ParamError> {
        let p: CommandsParams = read(params)?;
        check_driver(&p.driver)?;
        if let Some(workspace) = &p.workspace {
            check_name("workspace", workspace)?;
        }
        Ok(p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Command,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub kind: CommandKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    /// The driver's own command, not a user's command or skill of the same name.
    pub builtin: bool,
    /// The `SKILL.md` of a skill, as `Skill::path` names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandsResult {
    pub driver: String,
    pub commands: Vec<Command>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{fixtures, FIXTURE_CLIS};
    use agent_api_proto::Request;

    fn parse_params(method: &str, params: &Value) -> Result<(), ParamError> {
        match method {
            "models.list" => ModelsParams::parse(params).map(drop),
            "agents.detect" => DetectParams::parse(params).map(drop),
            "skills.list" => SkillsParams::parse(params).map(drop),
            "commands.list" => CommandsParams::parse(params).map(drop),
            other => panic!("{other} is no method of the group"),
        }
    }

    fn parse_result(method: &str, result: &Value) -> Result<(), String> {
        let read = |r: Result<(), serde_json::Error>| r.map_err(|e| e.to_string());
        match method {
            "models.list" => read(serde_json::from_value::<ModelsResult>(result.clone()).map(drop)),
            "agents.detect" => read(serde_json::from_value::<DetectResult>(result.clone()).map(drop)),
            "skills.list" => read(serde_json::from_value::<SkillsResult>(result.clone()).map(drop)),
            "commands.list" => read(serde_json::from_value::<CommandsResult>(result.clone()).map(drop)),
            other => panic!("{other} is no method of the group"),
        }
    }

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_every_method_has_a_success_and_a_refusal_the_params_and_results_read() {
        // Given the recorded exchanges of the group
        let recorded = fixtures();
        // Then there is one file per method, and no more
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        for fixture in &recorded {
            let method = fixture.method.as_str();
            // And each has an exchange that succeeds and one that is refused
            assert!(fixture.exchanges.iter().any(|e| e.succeeds()) && fixture.exchanges.iter().any(|e| !e.succeeds()), "{method}");
            for exchange in &fixture.exchanges {
                let what = format!("{method}: {}", exchange.name);
                for request in exchange.before.iter().chain([&exchange.request]) {
                    Request::from_value(request).unwrap_or_else(|_| panic!("{what}: not a request"));
                }
                assert_eq!(exchange.request["method"].as_str(), Some(method), "{what}");
                if exchange.succeeds() {
                    assert_eq!(parse_params(method, &exchange.request["params"]), Ok(()), "{what}");
                    assert_eq!(parse_result(method, &exchange.response["result"]), Ok(()), "{what}");
                } else {
                    let code = exchange.response["error"]["code"].as_str().unwrap();
                    assert!(code::ALL.contains(&code) || ["INVALID_PARAMS", "NO_EXECUTOR", "AMBIGUOUS_EXECUTOR", "UNKNOWN_WORKSPACE"].contains(&code), "{what}: {code}");
                    if code == "INVALID_PARAMS" {
                        assert_eq!(parse_params(method, &exchange.request["params"]).unwrap_err().0, exchange.response["error"]["message"].as_str().unwrap(), "{what}");
                    }
                }
            }
        }
    }

    #[test]
    fn given_the_action_table_when_checked_then_every_method_has_a_row_whose_actions_are_executor_actions_and_the_recordings_ask_only_those() {
        // Given the executor actions that exist and the method rows of the table
        let known: Vec<&str> = exec_proc::ACTIONS.iter().chain(exec_fs::ACTIONS).map(|a| a.name).collect();
        // Then each method of the group has exactly one row, and its actions are real ones (the two this group adds among them)
        let rows: Vec<&str> = ACTIONS.iter().map(|(m, _)| m.name()).collect();
        assert_eq!(rows, METHODS.iter().map(|m| m.name()).collect::<Vec<_>>());
        for (method, actions) in ACTIONS {
            assert!(actions.iter().all(|a| known.contains(a)), "{}: {actions:?}", method.name());
        }
        assert_eq!(ACTIONS.iter().flat_map(|(_, a)| a.iter().copied()).collect::<std::collections::BTreeSet<_>>(), [exec_fs::SKILL_SCAN, exec_proc::CLI_DETECT].into());
        // And every recorded exchange asks no action outside its row, a success of a method that has some asks them all, and a method with none asks none
        for fixture in &fixtures() {
            let row = ACTIONS.iter().find(|(m, _)| m.name() == fixture.method).map(|(_, a)| *a).unwrap();
            for exchange in &fixture.exchanges {
                let what = format!("{}: {}", fixture.method, exchange.name);
                assert!(exchange.actions.iter().all(|a| row.contains(&a.as_str())), "{what}: {:?}", exchange.actions);
                if row.is_empty() {
                    assert!(exchange.actions.is_empty(), "{what}");
                }
                if exchange.succeeds() && fixture.method != "commands.list" {
                    assert_eq!(exchange.actions, row, "{what}");
                }
            }
        }
    }

    #[test]
    fn given_commands_with_and_without_a_workspace_when_recorded_then_only_the_one_with_a_workspace_asked_the_executor() {
        // Given the recorded commands.list exchanges
        let recorded = fixtures();
        let fixture = recorded.iter().find(|f| f.method == "commands.list").unwrap();
        // Then a success that names a workspace asked skill_scan once, and a success that names none asked nothing
        let successes = || fixture.exchanges.iter().filter(|e| e.succeeds());
        assert!(successes().any(|e| e.request["params"].get("workspace").is_some() && e.actions == ["skill_scan"]));
        assert!(successes().any(|e| e.request["params"].get("workspace").is_none() && e.actions.is_empty()));
        assert!(successes().all(|e| e.request["params"].get("workspace").is_some() == !e.actions.is_empty()));
    }

    #[test]
    fn given_the_recorded_detection_when_read_then_it_names_each_driver_once_in_order_and_matches_the_executor_the_fixtures_set_up() {
        // Given the recorded successes of agents.detect
        let recorded = fixtures();
        let fixture = recorded.iter().find(|f| f.method == "agents.detect").unwrap();
        for exchange in fixture.exchanges.iter().filter(|e| e.succeeds() && e.request["params"].get("executor").is_none()) {
            let result: DetectResult = serde_json::from_value(exchange.response["result"].clone()).unwrap();
            // Then the drivers come in the table's order, and a driver is installed exactly when its program is one the executor was set up with, with that version
            assert_eq!(result.agents.iter().map(|a| a.driver.as_str()).collect::<Vec<_>>(), DRIVERS);
            for agent in &result.agents {
                let set_up = FIXTURE_CLIS.iter().find(|(program, _)| *program == agent.program);
                assert_eq!((agent.installed, agent.version.as_deref()), (set_up.is_some(), set_up.map(|(_, v)| *v)), "{}", agent.program);
            }
        }
    }

    #[test]
    fn given_the_skills_directories_of_each_driver_when_compared_with_the_executors_then_they_are_among_the_ones_it_scans_and_the_recorded_commands_come_from_them() {
        // Given the directories skill_scan reads
        // Then every driver reads some of them and no other, and an unknown driver reads none
        for driver in DRIVERS {
            assert!(!skill_dirs(driver).is_empty() && skill_dirs(driver).iter().all(|d| exec_fs::SKILL_DIRS.contains(d)), "{driver}");
        }
        assert!(skill_dirs("gemini").is_empty());
        // And each skill the recorded commands.list successes name lies in a directory of its driver
        let recorded = fixtures();
        for exchange in recorded.iter().filter(|f| f.method == "commands.list").flat_map(|f| &f.exchanges).filter(|e| e.succeeds()) {
            let result: CommandsResult = serde_json::from_value(exchange.response["result"].clone()).unwrap();
            for command in result.commands.iter().filter(|c| c.kind == CommandKind::Skill) {
                let path = command.path.as_deref().unwrap();
                assert!(skill_dirs(&result.driver).iter().any(|d| path.starts_with(d)), "{} of {}", path, result.driver);
            }
        }
    }

    #[test]
    fn given_params_a_caller_may_get_wrong_when_parsed_then_each_is_refused_naming_what_is_wrong_and_the_defaults_hold() {
        // Given requests with an unknown driver, no driver, an unknown key, a blank executor and a blank workspace
        let bad = [
            ModelsParams::parse(&json!({"driver": "gemini"})).unwrap_err().0,
            ModelsParams::parse(&json!({})).unwrap_err().0,
            CommandsParams::parse(&json!({"driver": "codex", "cwd": "/"})).unwrap_err().0,
            DetectParams::parse(&json!({"executor": " "})).unwrap_err().0,
            SkillsParams::parse(&json!({"workspace": ""})).unwrap_err().0,
        ];
        // Then each says what is wrong
        assert_eq!(bad[0], "driver is one of opencode, claudeAgent, codex");
        assert!(bad[1].contains("driver") && bad[2].contains("cwd"), "{bad:?}");
        assert_eq!((bad[3].as_str(), bad[4].as_str()), ("executor is required", "workspace is required"));
        // And a detection that names nothing is for the only executor, not refreshed; a null params is the same
        assert_eq!(DetectParams::parse(&json!({})), Ok(DetectParams { executor: None, refresh: None }));
        assert!(!DetectParams::parse(&Value::Null).unwrap().refresh() && DetectParams::parse(&json!({"refresh": true})).unwrap().refresh());
    }
}
