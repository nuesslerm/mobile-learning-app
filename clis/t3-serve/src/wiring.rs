//! The one place that names concrete adapters: reads the settings once, opens the store, builds the `Service`.
use crate::adapters::rca::RcaAgent;
use crate::adapters::{NoAgent, OsIds, ProcHost, StaticCatalog, StderrLog, SystemClock};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use t3_core::ports::{AgentApi, AuthStore, Clock, EventStore, Ids, Log, MetaStore, ModelCatalog, StorageProbe};
use t3_core::usecases::engine::Engine;
use t3_core::{EnvironmentInfo, Parts, Service};
use t3_store::SqliteStore;

pub const DEFAULT_PORT: u16 = 3774;
pub const DEFAULT_BIND: &str = "127.0.0.1";

pub struct Settings {
    pub state_dir: PathBuf,
    pub label: String,
    pub catalog_file: Option<PathBuf>,
    pub catalog_inline: Option<String>,
    pub rca_url: Option<String>,
    /// `T3_RCA_BEARER`: the device bearer this mapper opens rca's agent API with (`wf offload bearers mint`), the only credential it holds for rca.
    pub rca_bearer: Option<String>,
    /// `--rca-workspace`: the rca workspace whose executor works where the projects' files are.
    pub rca_workspace: Option<String>,
}

pub fn open_store(state_dir: &Path) -> Result<Arc<SqliteStore>, String> {
    std::fs::create_dir_all(state_dir).map_err(|e| format!("state dir {}: {e}", state_dir.display()))?;
    SqliteStore::open(&state_dir.join("state.sqlite")).map(Arc::new).map_err(|e| format!("store: {}", e.0))
}

/// The environment id persists in the store, so a restart is the same environment to a paired client.
pub fn environment_id(meta: &dyn MetaStore, ids: &dyn Ids) -> Result<String, String> {
    if let Some(id) = meta.get("environment-id").map_err(|e| e.0)? {
        return Ok(id);
    }
    let h = hex_of(&ids.random_bytes(16));
    let id = format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]);
    meta.set("environment-id", &id).map_err(|e| e.0)?;
    Ok(id)
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn platform() -> (String, String) {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    (std::env::consts::OS.to_string(), arch.to_string())
}

pub struct Wired {
    pub service: Service,
    pub environment_id: String,
}

pub fn service(settings: &Settings, store: Arc<SqliteStore>) -> Result<Wired, String> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let ids: Arc<dyn Ids> = Arc::new(OsIds);
    let log: Arc<dyn Log> = Arc::new(StderrLog);
    let catalog: Arc<dyn ModelCatalog> = Arc::new(StaticCatalog::load(settings.catalog_file.as_deref(), settings.catalog_inline.as_deref())?);
    let id = environment_id(store.as_ref(), ids.as_ref())?;
    let (os, arch) = platform();
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "/".into());
    let engine = Arc::new(Engine::open(store.clone() as Arc<dyn EventStore>, clock.clone(), ids.clone()).map_err(|e| format!("event log: {}", e.0))?);
    let env = EnvironmentInfo { id: id.clone(), label: settings.label.clone(), os, arch, server_version: env!("CARGO_PKG_VERSION").to_string(), cwd };
    let agent: Arc<dyn AgentApi> = match (&settings.rca_url, &settings.rca_bearer, &settings.rca_workspace) {
        (Some(url), Some(bearer), Some(workspace)) => {
            let rca = RcaAgent::new(url, bearer, workspace)?;
            rca.check_agent_api()?;
            Arc::new(rca)
        }
        (Some(_), None, _) => return Err("--rca-url needs T3_RCA_BEARER in the environment: a device bearer for rca's agent API (wf offload bearers mint --label t3)".into()),
        (Some(_), Some(_), None) => return Err("--rca-url needs --rca-workspace (T3_RCA_WORKSPACE): the rca workspace whose executor works where the projects' files are".into()),
        _ => Arc::new(NoAgent),
    };
    let service = Service::new(Parts { env, auth: store.clone() as Arc<dyn AuthStore>, clock, ids, log, catalog, engine, agent, meta: store.clone() as Arc<dyn MetaStore>, storage: store as Arc<dyn StorageProbe>, host: Arc::new(ProcHost::default()) });
    service.resume_runs();
    Ok(Wired { service, environment_id: id })
}
