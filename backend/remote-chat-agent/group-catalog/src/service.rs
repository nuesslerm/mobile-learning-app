//! The use cases of the group over the executors and the table of the drivers.
use crate::ports::{Clock, Refusal, Speech};
use crate::table;
use agent_api_catalog::{code, message, skill_dirs, Command, CommandKind, CommandsParams, CommandsResult, DetectParams, DetectResult, DetectedAgent, DetectedTool, ModelsParams, ModelsResult, Scope, SessionsParams, SessionsResult, VaultSession, SkillsParams, SkillsResult, SpeechStatusResult, SpeechTranscribeParams, SpeechTranscribeResult, DRIVERS, TOOLS};
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
    speech: Arc<dyn Speech>,
    detected: Mutex<HashMap<String, DetectResult>>,
}

fn relay(e: RemoteError) -> Refusal {
    Refusal::new(e.code(), e.message())
}

fn read<T: serde::de::DeserializeOwned>(result: Value) -> Result<T, Refusal> {
    serde_json::from_value(result).map_err(|e| Refusal::new(Code::InvalidArgs.as_str(), format!("the executor's answer is not the contract's: {e}")))
}

impl Catalog {
    pub fn new(executors: Arc<dyn Executors>, clock: Arc<dyn Clock>, speech: Arc<dyn Speech>) -> Arc<Catalog> {
        Arc::new(Catalog { executors, clock, speech, detected: Mutex::default() })
    }

    pub fn speech_status(&self) -> SpeechStatusResult {
        match self.speech.engine() {
            Some(engine) => SpeechStatusResult { available: true, engine },
            None => SpeechStatusResult { available: false, engine: "none".into() },
        }
    }

    pub async fn transcribe(&self, p: SpeechTranscribeParams) -> Result<SpeechTranscribeResult, Refusal> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(p.audio.trim()).map_err(|_| Refusal::new(proto_code::INVALID_PARAMS, "`audio` is not base64"))?;
        if self.speech.engine().is_none() {
            return Err(Refusal::new(code::SPEECH_UNAVAILABLE, "the service has no speech engine"));
        }
        let started = self.clock.now_ms();
        let text = self.speech.transcribe(bytes, p.format()).await.map_err(|e| Refusal::new(code::SPEECH_UNAVAILABLE, e))?;
        Ok(SpeechTranscribeResult { text, took_ms: self.clock.now_ms().saturating_sub(started) })
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
        let programs: Vec<&str> = DRIVERS.iter().map(|d| table::program(d)).chain(TOOLS).collect();
        let found: exec_proc::CliDetectResult = read(route.channel.call(exec_proc::CLI_DETECT, json!({"programs": programs})).await.map_err(relay)?.result)?;
        let agents = DRIVERS
            .iter()
            .filter_map(|driver| {
                let program = table::program(driver);
                let cli = found.clis.iter().find(|c| c.program == program && c.installed)?;
                Some(DetectedAgent { driver: driver.to_string(), program: program.into(), installed: true, path: cli.path.clone(), version: cli.version.clone(), error: cli.error.clone(), signed_in: cli.signed_in })
            })
            .collect();
        let tools = TOOLS.iter().map(|p| found.clis.iter().find(|c| c.program == *p).map_or_else(|| DetectedTool { program: p.to_string(), installed: false, path: None, version: None, error: None }, |c| DetectedTool { program: c.program.clone(), installed: c.installed, path: c.path.clone(), version: c.version.clone(), error: c.error.clone() })).collect();
        let result = DetectResult { executor: route.executor.clone(), agents, tools, checked_at_ms: now };
        self.detected.lock().unwrap().insert(route.executor, result.clone());
        Ok(result)
    }

    /// The skills of the workspace and of its executor's home, as the executor's `skill_scan` found them.
    async fn scan(&self, workspace: &str, dirs: Option<&[String]>, home_dirs: Option<&[String]>) -> Result<SkillsResult, Refusal> {
        let channel = self.executors.get(workspace).ok_or_else(|| Refusal::new(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}")))?;
        let root = self.executors.place(workspace).path(".").map_err(|e| Refusal::new(Code::InvalidArgs.as_str(), e))?;
        let mut args = json!({"root": root, "home": true});
        if let Some(d) = dirs {
            args["dirs"] = json!(d);
        }
        if let Some(d) = home_dirs {
            args["home_dirs"] = json!(d);
        }
        read(channel.call(exec_fs::SKILL_SCAN, args).await.map_err(relay)?.result)
    }

    /// The sessions the agent CLIs keep in an executor's own home, as its `session_scan` found them.
    pub async fn sessions(&self, p: SessionsParams) -> Result<SessionsResult, Refusal> {
        let route = self.executors.route_new(p.executor.as_deref()).map_err(|e| match e {
            RouteError::UnknownExecutor(name) => Refusal::new(code::UNKNOWN_EXECUTOR, message::unknown_executor(&name)),
            RouteError::AmbiguousExecutor(all) => Refusal::new(proto_code::INVALID_PARAMS, format!("several executors are attached ({}): name one", all.join(", "))),
            RouteError::NoExecutor | RouteError::UnknownWorkspace(_) => Refusal::new(Code::NoExecutor.as_str(), "no executor is attached"),
        })?;
        let mut args = json!({});
        if let Some(agents) = &p.agents {
            args["agents"] = json!(agents);
        }
        if let Some(limit) = p.limit {
            args["limit"] = json!(limit);
        }
        let found: exec_fs::SessionsFound = read(route.channel.call(exec_fs::SESSION_SCAN, args).await.map_err(relay)?.result)?;
        let sessions = found.sessions.into_iter().map(|s| VaultSession { agent: s.agent, session_id: s.session_id, path: s.path, title: s.title, cwd: s.cwd, branch: s.branch, model: s.model, created_at: s.created_at, updated_at: s.updated_at, modified_ms: s.modified_ms, message_count: s.message_count, first_prompt: s.first_prompt, last_prompt: s.last_prompt }).collect();
        Ok(SessionsResult { executor: route.executor, sessions, truncated: found.truncated })
    }

    pub async fn skills(&self, p: SkillsParams) -> Result<SkillsResult, Refusal> {
        self.scan(&p.workspace, p.dirs.as_deref(), p.home_dirs.as_deref()).await
    }

    /// The driver's own commands (a headless CLI has none) and the skills it reads, a skill of the workspace hiding one of the same name in the home.
    pub async fn commands(&self, p: CommandsParams) -> Result<CommandsResult, Refusal> {
        let mut commands: Vec<Command> = Vec::new();
        if let Some(workspace) = &p.workspace {
            let dirs = skill_dirs(&p.driver);
            let mut skills = self.scan(workspace, None, None).await?.skills;
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
