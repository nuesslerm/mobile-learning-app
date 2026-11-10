//! The one place that names concrete adapters: reads the settings once, opens the state directory, builds the `Host`.
use crate::adapters::rca::RcaAgent;
use crate::adapters::{DirBundle, FileDevices, FileKeyPair, RegistryPrincipals};
use crate::transport::Shared;
use orca_core::{Area, Host, HostInfo};
use orca_crypto::ports::{load_or_create_keypair, mint_device};
use orca_crypto::{Entropy, SystemEntropy};
use orca_map_mobile::{BundleSource, MobileWebArea, NoBundle, RelayArea};
use orca_map_settings::{PairingArea, SettingsArea};
use orca_map_agent::AgentArea;
use orca_map_accounts::AccountsArea;
use orca_map_git::GitArea;
use orca_map_hosted::HostedArea;
use orca_map_schedules::SchedulesArea;
use orca_map_state::StateArea;
use orca_map_terminal::TerminalArea;
use orca_map_workspace::WorkspaceArea;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DEFAULT_PORT: u16 = 6768;
pub const DEFAULT_BIND: &str = "127.0.0.1";

pub struct Settings {
    pub state_dir: PathBuf,
    /// What the phone shows as this host's name.
    pub label: String,
    pub rca_url: Option<String>,
    /// `ORCA_RCA_BEARER`: the device bearer this server opens rca's agent API with (`wf offload bearers mint --label orca`).
    pub rca_bearer: Option<String>,
    /// `ORCA_MOBILE_WEB_DIR`: the directory holding the phone web page's `manifest.json` and assets. The host serves it and does not build it.
    pub web_dir: Option<PathBuf>,
}

/// Every area of the server. A mapping crate is added here and nowhere else.
pub fn areas(web: MobileWebArea) -> Vec<Box<dyn Area>> {
    vec![Box::new(SettingsArea), Box::new(PairingArea), Box::new(web), Box::new(RelayArea), Box::new(WorkspaceArea), Box::new(AgentArea::default()), Box::new(TerminalArea), Box::new(GitArea), Box::new(AccountsArea), Box::new(StateArea), Box::new(HostedArea), Box::new(SchedulesArea)]
}

/// The bundle source of a settings value: the directory, or none.
pub fn bundle_source(web_dir: Option<&Path>) -> Box<dyn BundleSource> {
    match web_dir {
        Some(dir) => Box::new(DirBundle(dir.to_path_buf())),
        None => Box::new(NoBundle),
    }
}

/// Node.js platform names, which the phone reads from `host.platform`.
pub fn platform() -> String {
    match std::env::consts::OS {
        "macos" => "darwin".into(),
        "windows" => "win32".into(),
        other => other.into(),
    }
}

pub fn host_info(label: &str) -> HostInfo {
    HostInfo { platform: platform(), machine_name: label.to_string(), app_version: env!("CARGO_PKG_VERSION").to_string() }
}

/// The runtime id persists in the state directory: a restart is the same host to a paired phone.
pub fn runtime_id(state_dir: &Path, entropy: &dyn Entropy) -> Result<String, String> {
    let path = state_dir.join("runtime-id");
    if let Ok(id) = std::fs::read_to_string(&path) {
        if !id.trim().is_empty() {
            return Ok(id.trim().to_string());
        }
    }
    let id = mint_device(entropy).id;
    std::fs::create_dir_all(state_dir).and_then(|()| std::fs::write(&path, &id)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(id)
}

/// The deploy smoke waits for the port a little over a minute: a first-ever tailnet node needs that long to join and resolve rca.
const RCA_ATTEMPTS: u32 = 30;
const RCA_PAUSE: std::time::Duration = std::time::Duration::from_secs(2);

pub struct Wired {
    pub shared: Shared,
    pub public_key: [u8; 32],
}

pub fn wire(settings: &Settings) -> Result<Wired, String> {
    let entropy: Arc<SystemEntropy> = Arc::new(SystemEntropy);
    let keys = load_or_create_keypair(&FileKeyPair(settings.state_dir.join("host-key.json")), entropy.as_ref()).map_err(|e| e.0)?;
    let agent: Arc<dyn orca_core::AgentApi> = match (&settings.rca_url, &settings.rca_bearer) {
        (Some(url), Some(bearer)) => {
            let rca = RcaAgent::new(url, bearer);
            rca.check_waiting(RCA_ATTEMPTS, RCA_PAUSE)?;
            Arc::new(rca)
        }
        (Some(_), None) => return Err("--rca-url needs ORCA_RCA_BEARER in the environment: a device bearer for rca's agent API (wf offload bearers mint --label orca)".into()),
        (None, _) => return Err("--rca-url (ORCA_RCA_URL) is required: the rca service this server fronts".into()),
    };
    let id = runtime_id(&settings.state_dir, entropy.as_ref())?;
    let devices = Arc::new(FileDevices::new(settings.state_dir.join("devices.json")));
    let web = MobileWebArea::new(bundle_source(settings.web_dir.as_deref()));
    if let Some(problem) = web.problem() {
        wf_kit::logfmt::mcp("warn", "orca-serve", "the mobile web bundle is not served", &[("error", problem.to_string())]);
    }
    let host = Host::new(id, areas(web), agent, Arc::new(RegistryPrincipals(devices.clone())), host_info(&settings.label))?;
    let public_key = keys.public;
    Ok(Wired { shared: Shared { host, keys: Arc::new(keys), devices, entropy, handshake_deadline: crate::transport::HANDSHAKE_DEADLINE }, public_key })
}
