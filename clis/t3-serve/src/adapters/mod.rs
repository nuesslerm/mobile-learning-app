//! System implementations of the core's ports.
use serde_json::Value;
use std::path::Path;
use t3_core::ports::{AgentApi, AgentError, AgentEvent, AgentThread, AgentTurn, CatalogModel, Clock, DirEntry, Follow, History, HostProbe, HostSample, Ids, Log, Millis, ModelCatalog, Op, SetupBatch};

pub mod rca;

/// The agent API when no `--rca-url` is given: every turn and file operation answers that nothing executes here, and no thread has a log.
pub struct NoAgent;

const NO_AGENT: &str = "no agent API is configured (start serve with --rca-url)";

impl AgentApi for NoAgent {
    fn configured(&self) -> bool {
        false
    }
    fn run(&self, _: &Op) -> Result<Value, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn ensure_thread(&self, _: &AgentThread) -> Result<u64, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn start_turn(&self, _: &AgentTurn) -> Result<String, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn interrupt(&self, _: &str) -> Result<(), AgentError> {
        Ok(())
    }
    fn follow(&self, _: &str, _: u64, _: &mut dyn FnMut(AgentEvent) -> Follow) -> Result<(), AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn history(&self, _: &str, _: u64, _: u32) -> Result<History, AgentError> {
        Err(AgentError::NoThread)
    }
    fn list_dir(&self, _: &str, _: &str) -> Result<Vec<DirEntry>, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn read_file(&self, _: &str, _: &str) -> Result<String, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn setup_events(&self, _: &str, _: u64, _: std::time::Duration) -> Result<SetupBatch, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn terminal_events(&self, _: &str, _: &str, _: Option<u64>, _: std::time::Duration) -> Result<t3_core::ports::TerminalBatch, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn git_follow(&self, _: &str, _: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn auth_follow(&self, _: &str, _: &mut dyn FnMut(Value) -> Follow) -> Result<(), AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn clone_follow(&self, _: &str, _: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn schedule_follow(&self, _: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn preview_follow(&self, _: &str, _: u64, _: &mut dyn FnMut(Option<Value>) -> Follow) -> Result<(), AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
    fn terminals(&self) -> Result<Vec<t3_core::ports::TerminalSummary>, AgentError> {
        Err(AgentError::Failed(NO_AGENT.into()))
    }
}

/// The host as Linux's `/proc` shows it. The processor share is the busy share since the last reading, so the first has none.
#[derive(Default)]
pub struct ProcHost {
    last: std::sync::Mutex<Option<(u64, u64)>>,
}

fn meminfo_kb(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next()?.parse().ok())
}

/// The idle and total jiffies of the `cpu` line of `/proc/stat`.
fn cpu_jiffies(stat: &str) -> Option<(u64, u64)> {
    let fields: Vec<u64> = stat.lines().next()?.strip_prefix("cpu")?.split_whitespace().filter_map(|f| f.parse().ok()).collect();
    let idle = fields.get(3)? + fields.get(4).copied().unwrap_or(0);
    Some((idle, fields.iter().sum()))
}

impl HostProbe for ProcHost {
    fn sample(&self) -> Result<HostSample, String> {
        let memory = std::fs::read_to_string("/proc/meminfo").map_err(|e| format!("/proc/meminfo: {e}"))?;
        let (total, available) = (meminfo_kb(&memory, "MemTotal"), meminfo_kb(&memory, "MemAvailable"));
        let (Some(total), Some(available)) = (total, available) else { return Err("/proc/meminfo has no MemTotal and MemAvailable".into()) };
        let now = std::fs::read_to_string("/proc/stat").ok().and_then(|s| cpu_jiffies(&s));
        let utilization = {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            let share = match (*last, now) {
                (Some((idle0, total0)), Some((idle1, total1))) if total1 > total0 => Some(1.0 - (idle1 - idle0.min(idle1)) as f64 / (total1 - total0) as f64),
                _ => None,
            };
            if now.is_some() {
                *last = now;
            }
            share.map(|s| s.clamp(0.0, 1.0))
        };
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get() as u64);
        Ok(HostSample { cpu_count: cpus, cpu_utilization: utilization, available_memory_bytes: available * 1024, total_memory_bytes: total * 1024 })
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> Millis {
        wf_kit::time::now_ms()
    }
}

pub struct OsIds;

impl Ids for OsIds {
    fn random_bytes(&self, n: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; n];
        getrandom::fill(&mut bytes).expect("the operating system's random source");
        bytes
    }
}

/// logfmt lines on stderr (stdout carries the verbs' JSON).
pub struct StderrLog;

impl Log for StderrLog {
    fn event(&self, name: &'static str, fields: &[(&'static str, String)]) {
        wf_kit::logfmt::mcp("info", "t3", name, fields);
    }
}

/// The models of the `rca` provider, read once: a JSON file (`[{slug, name}]`), else `slug=Name,slug=Name`, else one default.
pub struct StaticCatalog(pub Vec<CatalogModel>);

impl ModelCatalog for StaticCatalog {
    fn models(&self) -> Vec<CatalogModel> {
        self.0.clone()
    }
}

impl StaticCatalog {
    pub fn load(file: Option<&Path>, inline: Option<&str>) -> Result<StaticCatalog, String> {
        let models = match (file, inline) {
            (Some(path), _) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("catalog file {}: {e}", path.display()))?;
                let value: Value = serde_json::from_str(&text).map_err(|e| format!("catalog file {}: {e}", path.display()))?;
                let list = value.get("models").unwrap_or(&value).as_array().ok_or("the catalog file is a JSON array of {slug, name}")?.clone();
                list.iter()
                    .map(|m| match (m["slug"].as_str(), m["name"].as_str().or(m["slug"].as_str())) {
                        (Some(slug), Some(name)) => Ok(CatalogModel { slug: slug.into(), name: name.into() }),
                        _ => Err("every catalog entry needs a slug".to_string()),
                    })
                    .collect::<Result<Vec<_>, _>>()?
            }
            (None, Some(inline)) => inline
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|entry| {
                    let (slug, name) = entry.split_once('=').unwrap_or((entry, entry));
                    CatalogModel { slug: slug.trim().into(), name: name.trim().into() }
                })
                .collect(),
            (None, None) => vec![CatalogModel { slug: "rca-default".into(), name: "rca default".into() }],
        };
        Ok(StaticCatalog(models))
    }
}
