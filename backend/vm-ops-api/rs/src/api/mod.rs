//! The HTTP API. Every request enters one fallback handler that runs Node's pipeline in Node's order (see `dispatch`), so the
//! router never decides what a path means. `app` is what `main` serves and what the flow tests drive.
use axum::routing::any;
use axum::Router;
use std::sync::Arc;
use std::time::Duration;
use vm_ops_api_core::application::access::Gate;
use vm_ops_api_core::application::agent_shell::AgentShell;
use vm_ops_api_core::application::drill::Drill;
use vm_ops_api_core::application::health::{Build, Health};
use vm_ops_api_core::application::logs::Logs;
use vm_ops_api_core::application::opencode_log::OpencodeLog;
use vm_ops_api_core::application::opencode_log_writes::RunWrites;
use vm_ops_api_core::application::ports::{Clock, Containers, EventLog, HostFiles, Ids, RunLog, Siblings};
use vm_ops_api_core::application::ps::Ps;
use vm_ops_api_core::application::redeploy::Redeploy;
use vm_ops_api_core::application::redeploy_status::RedeployStatus;
use vm_ops_api_core::application::scripts::redeploy::RedeployEnv;
use vm_ops_api_core::application::self_redeploy::{SelfRedeploy, SelfRedeployDeps};
use vm_ops_api_core::application::stats::Stats;
use vm_ops_api_core::domain::auth::AuthConfig;
use vm_ops_api_core::domain::drill::OWN_CONTAINER;
use vm_ops_api_core::domain::exposure::Port;

mod dispatch;
mod reply;
mod routes;
pub mod serve;

pub use serve::Limits;

/// What the API reaches outside itself through.
pub struct Ports {
    pub clock: Arc<dyn Clock>,
    pub log: Arc<dyn EventLog>,
    pub containers: Arc<dyn Containers>,
    pub host_files: Arc<dyn HostFiles>,
    pub run_log: Arc<dyn RunLog>,
    pub ids: Arc<dyn Ids>,
    pub siblings: Arc<dyn Siblings>,
}

/// What the process was told at start.
pub struct Settings {
    /// The admin port's credentials.
    pub auth: AuthConfig,
    /// The CI port's own credentials, a different seed; none leaves the CI port off.
    pub ci_auth: Option<AuthConfig>,
    pub build: Build,
    pub limits: Limits,
    /// The opencode-log dashboard page, embedded in the binary.
    pub dashboard: &'static [u8],
    /// The image this service runs, which its siblings run too: `<namespace>/vm-ops-api-rs:<tag>`.
    pub sibling_image: String,
    /// What the sibling scripts of a redeploy, a retire and the redeploy status are rendered from.
    pub redeploy: RedeployEnv,
}

/// The listener a request came in on: the routes it serves are the port's, and only the port's own seed lets a request in.
#[derive(Clone)]
struct Lane {
    port: Port,
    gate: Arc<Gate>,
}

/// One router per listener.
pub struct Routers {
    pub admin: Router,
    /// None unless the settings carry CI credentials.
    pub ci: Option<Router>,
}

#[derive(Clone)]
pub struct AppState {
    lane: Lane,
    health: Arc<Health>,
    log: Arc<dyn EventLog>,
    body_timeout: Duration,
    ps: Arc<Ps>,
    stats: Arc<Stats>,
    logs: Arc<Logs>,
    opencode_log: Arc<OpencodeLog>,
    dashboard: &'static [u8],
    agent_shell: Arc<AgentShell>,
    run_writes: Arc<RunWrites>,
    drill: Arc<Drill>,
    redeploy: Arc<Redeploy>,
    redeploy_status: Arc<RedeployStatus>,
    self_redeploy: Arc<SelfRedeploy>,
}

/// The admin router alone, as the tests that do not care about the CI port drive it.
pub fn app(ports: Ports, settings: Settings) -> Router {
    routers(ports, settings).admin
}

pub fn routers(ports: Ports, settings: Settings) -> Routers {
    let self_redeploy = Arc::new(SelfRedeploy::new(SelfRedeployDeps {
        siblings: ports.siblings.clone(),
        log: ports.log.clone(),
        clock: ports.clock.clone(),
        env: settings.redeploy.clone(),
    }));
    let agent_shell = Arc::new(AgentShell::new(ports.containers.clone(), ports.ids, ports.clock.clone(), ports.log.clone()));
    let run_writes = Arc::new(RunWrites::new(ports.run_log.clone(), ports.clock.clone()));
    let drill = Arc::new(Drill::new(ports.siblings.clone(), settings.sibling_image.clone(), OWN_CONTAINER.to_string()));
    let redeploy = Arc::new(Redeploy::new(ports.siblings.clone(), ports.log.clone(), ports.clock.clone(), settings.redeploy.clone(), settings.build.server_script_sha.clone()));
    let redeploy_status = Arc::new(RedeployStatus::new(ports.siblings.clone(), settings.redeploy));
    let (admin_gate, ci_gate) = (Arc::new(Gate::new(settings.auth, ports.clock.clone())), settings.ci_auth.map(|auth| Arc::new(Gate::new(auth, ports.clock.clone()))));
    let state = AppState {
        lane: Lane { port: Port::Admin, gate: admin_gate },
        health: Arc::new(Health::new(settings.build.clone(), ports.clock)),
        log: ports.log,
        body_timeout: settings.limits.body,
        ps: Arc::new(Ps::new(ports.containers.clone())),
        stats: Arc::new(Stats::new(ports.containers.clone(), ports.host_files)),
        logs: Arc::new(Logs::new(ports.containers, settings.build.debug_mode)),
        opencode_log: Arc::new(OpencodeLog::new(ports.run_log)),
        dashboard: settings.dashboard,
        agent_shell,
        run_writes,
        drill,
        redeploy,
        redeploy_status,
        self_redeploy,
    };
    let router = |state: AppState| Router::new().fallback(any(dispatch::dispatch)).with_state(state).layer(service_kit::trace::layer());
    let ci = ci_gate.map(|gate| router(AppState { lane: Lane { port: Port::Ci, gate }, ..state.clone() }));
    Routers { admin: router(state), ci }
}
