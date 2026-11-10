//! The use cases of the group over the executors and the table of the drivers.
use crate::ports::{Clock, Refusal};
use crate::table;
use agent_api_catalog::{code, message, skill_dirs, Command, CommandKind, CommandsParams, CommandsResult, DetectParams, DetectResult, DetectedAgent, ModelsParams, ModelsResult, Scope, SkillsParams, SkillsResult, DRIVERS};
use agent_api_proto::code as proto_code;
use exec_proto::Code;
use rca_exec_channel::remote::RemoteError;
use rca_exec_channel::{Executors, RouteError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// How long a detection is kept: the CLIs of an executor change when a person installs one, not between two screens.
pub const DETECTION_TTL_MS: u64 = 30_000;

pub struct Catalog {
    executors: Arc<dyn Executors>,
    clock: Arc<dyn Clock>,
    detected: Mutex<HashMap<String, DetectResult>>,
}

fn relay(e: RemoteError) -> Refusal {
    Refusal::new(e.code(), e.message())
}

fn read<T: serde::de::DeserializeOwned>(result: Value) -> Result<T, Refusal> {
    serde_json::from_value(result).map_err(|e| Refusal::new(Code::InvalidArgs.as_str(), format!("the executor's answer is not the contract's: {e}")))
}

impl Catalog {
    pub fn new(executors: Arc<dyn Executors>, clock: Arc<dyn Clock>) -> Arc<Catalog> {
        Arc::new(Catalog { executors, clock, detected: Mutex::default() })
    }

    pub fn models(&self, p: ModelsParams) -> Result<ModelsResult, Refusal> {
        Ok(table::for_driver(&p.driver))
    }

    pub async fn detect(&self, p: DetectParams) -> Result<DetectResult, Refusal> {
        let route = self.executors.route_new(p.executor.as_deref()).map_err(|e| match e {
            RouteError::UnknownExecutor(name) => Refusal::new(code::UNKNOWN_EXECUTOR, message::unknown_executor(&name)),
            RouteError::AmbiguousExecutor(all) => Refusal::new(proto_code::INVALID_PARAMS, format!("several executors are attached ({}): name one", all.join(", "))),
            RouteError::NoExecutor | RouteError::UnknownWorkspace(_) => Refusal::new(Code::NoExecutor.as_str(), "no executor is attached"),
        })?;
        let now = self.clock.now_ms();
        if !p.refresh() {
            if let Some(kept) = self.detected.lock().unwrap().get(&route.executor).filter(|d| now.saturating_sub(d.checked_at_ms) < DETECTION_TTL_MS) {
                return Ok(kept.clone());
            }
        }
        let programs: Vec<&str> = DRIVERS.iter().map(|d| table::program(d)).collect();
        let found: exec_proc::CliDetectResult = read(route.channel.call(exec_proc::CLI_DETECT, json!({"programs": programs})).await.map_err(relay)?.result)?;
        let agents = DRIVERS
            .iter()
            .map(|driver| {
                let program = table::program(driver);
                let cli = found.clis.iter().find(|c| c.program == program);
                DetectedAgent { driver: driver.to_string(), program: program.into(), installed: cli.is_some_and(|c| c.installed), path: cli.and_then(|c| c.path.clone()), version: cli.and_then(|c| c.version.clone()), error: cli.and_then(|c| c.error.clone()) }
            })
            .collect();
        let result = DetectResult { executor: route.executor.clone(), agents, checked_at_ms: now };
        self.detected.lock().unwrap().insert(route.executor, result.clone());
        Ok(result)
    }

    /// The skills of the workspace and of its executor's home, as the executor's `skill_scan` found them.
    async fn scan(&self, workspace: &str) -> Result<SkillsResult, Refusal> {
        let channel = self.executors.get(workspace).ok_or_else(|| Refusal::new(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}")))?;
        let root = self.executors.place(workspace).path(".").map_err(|e| Refusal::new(Code::InvalidArgs.as_str(), e))?;
        read(channel.call(exec_fs::SKILL_SCAN, json!({"root": root, "home": true})).await.map_err(relay)?.result)
    }

    pub async fn skills(&self, p: SkillsParams) -> Result<SkillsResult, Refusal> {
        self.scan(&p.workspace).await
    }

    /// The driver's own commands (a headless CLI has none) and the skills it reads, a skill of the workspace hiding one of the same name in the home.
    pub async fn commands(&self, p: CommandsParams) -> Result<CommandsResult, Refusal> {
        let mut commands: Vec<Command> = Vec::new();
        if let Some(workspace) = &p.workspace {
            let dirs = skill_dirs(&p.driver);
            let mut skills = self.scan(workspace).await?.skills;
            skills.retain(|s| dirs.contains(&s.dir.as_str()));
            skills.sort_by_key(|s| (s.scope != Scope::Workspace, dirs.iter().position(|d| *d == s.dir)));
            for skill in skills {
                if commands.iter().all(|c| c.name != skill.name) {
                    commands.push(Command { name: skill.name, kind: CommandKind::Skill, description: skill.description, argument_hint: None, builtin: false, path: Some(skill.path) });
                }
            }
        }
        Ok(CommandsResult { driver: p.driver, commands })
    }
}
