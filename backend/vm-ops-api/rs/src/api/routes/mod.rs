//! The routes, one variant each: which request reaches it and what it answers. The dispatcher owns the pipeline around them.
use crate::api::reply::Reply;
use crate::api::AppState;
use vm_ops_api_core::application::ports::ContainersError;
use vm_ops_api_core::domain::exposure::{spec_of, Exposure};
use vm_ops_api_core::domain::request::Target;

mod agent_shell;
mod clis_install;
mod drill;
mod health;
mod logs;
mod opencode_log;
mod opencode_log_writes;
mod ps;
mod redeploy;
mod redeploy_status;
mod self_redeploy;
mod stats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteId {
    Health,
    Ps,
    Stats,
    Logs,
    OpencodeDashboard,
    OpencodeRuns,
    OpencodeRun,
    OpencodeStart,
    OpencodeLine,
    OpencodeFinish,
    ExecScriptStart,
    ExecScriptStatus,
    ExecScriptCancel,
    VmLedger,
    Artifact,
    Credentials,
    Drill,
    AppRedeploy,
    AppSmoke,
    AppSmokeStop,
    Retire,
    AppPair,
    RedeployStatus,
    SelfRedeploy,
    SelfSmoke,
    SelfSmokeStop,
    ClisInstall,
}

impl RouteId {
    /// A public route runs with no body read and no auth.
    pub fn is_public(self) -> bool {
        matches!(self, RouteId::OpencodeDashboard | RouteId::OpencodeRuns | RouteId::OpencodeRun)
    }

    /// The key of this route in the core's route table, which says which ports serve it.
    fn spec_id(self) -> &'static str {
        match self {
            RouteId::Health => "Health",
            RouteId::Ps => "Ps",
            RouteId::Stats => "Stats",
            RouteId::Logs => "Logs",
            RouteId::OpencodeDashboard => "OpencodeDashboard",
            RouteId::OpencodeRuns => "OpencodeRuns",
            RouteId::OpencodeRun => "OpencodeRun",
            RouteId::OpencodeStart => "OpencodeStart",
            RouteId::OpencodeLine => "OpencodeLine",
            RouteId::OpencodeFinish => "OpencodeFinish",
            RouteId::ExecScriptStart => "ExecScriptStart",
            RouteId::ExecScriptStatus => "ExecScriptStatus",
            RouteId::ExecScriptCancel => "ExecScriptCancel",
            RouteId::VmLedger => "VmLedger",
            RouteId::Artifact => "Artifact",
            RouteId::Credentials => "Credentials",
            RouteId::Drill => "Drill",
            RouteId::AppRedeploy => "AppRedeploy",
            RouteId::AppSmoke => "AppSmoke",
            RouteId::AppSmokeStop => "AppSmokeStop",
            RouteId::Retire => "Retire",
            RouteId::AppPair => "AppPair",
            RouteId::RedeployStatus => "RedeployStatus",
            RouteId::SelfRedeploy => "SelfRedeploy",
            RouteId::SelfSmoke => "SelfSmoke",
            RouteId::SelfSmokeStop => "SelfSmokeStop",
            RouteId::ClisInstall => "ClisInstall",
                        }
    }

    pub fn exposure(self) -> Exposure {
        spec_of(self.spec_id()).map_or(Exposure::AdminOnly, |spec| spec.exposure)
    }

    /// A signed-only route takes the signed rotating token and refuses the legacy raw one with the same 401.
    pub fn is_signed_only(self) -> bool {
        self == RouteId::Drill
    }
}

/// What a handler reads of the request: its target and its body.
pub struct Call<'a> {
    pub target: &'a Target,
    pub body: &'a [u8],
}

/// A handler that could not answer; its message becomes the `detail` of the 502.
pub struct Failure(pub String);

impl From<ContainersError> for Failure {
    fn from(error: ContainersError) -> Self {
        Failure(error.to_string())
    }
}

/// `/services/<app>/<action>`, where segments are matched as they stand on the wire and empty ones are dropped.
fn app_route(target: &Target, action: &str) -> bool {
    matches!(target.parts.as_slice(), [services, _, last] if services == "services" && last == action)
}

/// `/services/<app>/smoke/stop`.
fn smoke_stop_route(target: &Target) -> bool {
    matches!(target.parts.as_slice(), [services, _, smoke, stop] if services == "services" && smoke == "smoke" && stop == "stop")
}

/// The first route whose method equals the request's exactly and whose path matches.
pub fn resolve(method: &str, target: &Target) -> Option<RouteId> {
    match method {
        "GET" if target.is_path("/health") => Some(RouteId::Health),
        "GET" if app_route(target, "ps") => Some(RouteId::Ps),
        "GET" if target.is_path("/stats") => Some(RouteId::Stats),
        "GET" if app_route(target, "logs") => Some(RouteId::Logs),
        "GET" if opencode_log::is_dashboard(target) => Some(RouteId::OpencodeDashboard),
        "GET" if target.is_path("/opencode-log/api/runs") => Some(RouteId::OpencodeRuns),
        "GET" if opencode_log::run_id_of_detail(target).is_some() => Some(RouteId::OpencodeRun),
        "GET" if target.is_path("/redeploy/status") => Some(RouteId::RedeployStatus),
        "POST" if app_route(target, "redeploy") => Some(RouteId::AppRedeploy),
        "POST" if app_route(target, "smoke") => Some(RouteId::AppSmoke),
        "POST" if smoke_stop_route(target) => Some(RouteId::AppSmokeStop),
        "POST" if app_route(target, "retire") => Some(RouteId::Retire),
        "POST" if app_route(target, "pair") => Some(RouteId::AppPair),
        "POST" if opencode_log_writes::is_write(target, opencode_log_writes::Write::Start) => Some(RouteId::OpencodeStart),
        "POST" if opencode_log_writes::is_write(target, opencode_log_writes::Write::Line) => Some(RouteId::OpencodeLine),
        "POST" if opencode_log_writes::is_write(target, opencode_log_writes::Write::Finish) => Some(RouteId::OpencodeFinish),
        "POST" if target.is_path("/services/agent-shell/exec-script") => Some(RouteId::ExecScriptStart),
        "GET" if agent_shell::is_job_status(target) => Some(RouteId::ExecScriptStatus),
        "POST" if agent_shell::is_job_cancel(target) => Some(RouteId::ExecScriptCancel),
        "GET" if agent_shell::is_ledger(target) => Some(RouteId::VmLedger),
        "GET" if target.is_path("/services/agent-shell/artifact") => Some(RouteId::Artifact),
        "POST" if target.is_path("/agent-shell/credentials") => Some(RouteId::Credentials),
        "POST" if target.is_path("/drill") => Some(RouteId::Drill),
        "POST" if target.is_path("/redeploy") => Some(RouteId::SelfRedeploy),
        "POST" if target.is_path("/redeploy/smoke") => Some(RouteId::SelfSmoke),
        "POST" if target.is_path("/redeploy/smoke/stop") => Some(RouteId::SelfSmokeStop),
        "POST" if target.is_path("/clis/install") => Some(RouteId::ClisInstall),
        _ => None,
    }
}

pub async fn handle(route: RouteId, call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match route {
        RouteId::Health => Ok(health::answer(state)),
        RouteId::Ps => ps::answer(call, state).await,
        RouteId::Stats => stats::answer(state).await,
        RouteId::Logs => logs::answer(call, state).await,
        RouteId::OpencodeDashboard => Ok(opencode_log::dashboard(state)),
        RouteId::OpencodeRuns => Ok(opencode_log::runs(call, state).await),
        RouteId::OpencodeRun => Ok(opencode_log::run(call, state).await),
        RouteId::OpencodeStart => opencode_log_writes::answer(opencode_log_writes::Write::Start, call, state).await,
        RouteId::OpencodeLine => opencode_log_writes::answer(opencode_log_writes::Write::Line, call, state).await,
        RouteId::OpencodeFinish => opencode_log_writes::answer(opencode_log_writes::Write::Finish, call, state).await,
        RouteId::ExecScriptStart => agent_shell::start(call, state).await,
        RouteId::ExecScriptStatus => agent_shell::status(call, state).await,
        RouteId::ExecScriptCancel => agent_shell::cancel(call, state).await,
        RouteId::VmLedger => agent_shell::ledger(call, state).await,
        RouteId::Artifact => agent_shell::artifact(call, state).await,
        RouteId::Credentials => agent_shell::credentials(call, state).await,
        RouteId::Drill => drill::answer(call, state),
        RouteId::AppRedeploy => Ok(redeploy::app(call, state).await),
        RouteId::AppSmoke => Ok(redeploy::smoke(call, state).await),
        RouteId::AppSmokeStop => Ok(redeploy::smoke_stop(call, state).await),
        RouteId::Retire => Ok(redeploy::retire(call, state).await),
        RouteId::AppPair => Ok(redeploy::pair(call, state).await),
        RouteId::RedeployStatus => Ok(redeploy_status::answer(state).await),
        RouteId::SelfRedeploy => Ok(self_redeploy::answer(call, state).await),
        RouteId::SelfSmoke => Ok(self_redeploy::smoke(call, state).await),
        RouteId::SelfSmokeStop => Ok(self_redeploy::smoke_stop(call, state).await),
        RouteId::ClisInstall => clis_install::answer(call, state).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vm_ops_api_core::domain::exposure::{matches, ROUTES};

    const ALL: [RouteId; 27] = [
        RouteId::Health,
        RouteId::Ps,
        RouteId::Stats,
        RouteId::Logs,
        RouteId::OpencodeDashboard,
        RouteId::OpencodeRuns,
        RouteId::OpencodeRun,
        RouteId::OpencodeStart,
        RouteId::OpencodeLine,
        RouteId::OpencodeFinish,
        RouteId::ExecScriptStart,
        RouteId::ExecScriptStatus,
        RouteId::ExecScriptCancel,
        RouteId::VmLedger,
        RouteId::Artifact,
        RouteId::Credentials,
        RouteId::Drill,
        RouteId::AppRedeploy,
        RouteId::AppSmoke,
        RouteId::AppSmokeStop,
        RouteId::Retire,
        RouteId::AppPair,
        RouteId::RedeployStatus,
        RouteId::SelfRedeploy,
        RouteId::SelfSmoke,
        RouteId::SelfSmokeStop,
        RouteId::ClisInstall,
    ];

    #[test]
    fn given_the_route_table_when_each_pattern_is_resolved_then_it_reaches_the_route_that_names_it() {
        assert_eq!(ALL.len(), ROUTES.len());
        for spec in ROUTES {
            let path = spec.path.replace("{app}", "leetcode-backend").replace("{run}", "run-1").replace("{job}", "0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab").replace("{name}", "pool-cooldowns.json");
            let route = resolved(spec.method, &path).unwrap_or_else(|| panic!("{} {path} reaches no route", spec.method));
            assert_eq!(route.spec_id(), spec.id, "{} {path}", spec.method);
            assert!(matches(spec, spec.method, &path));
        }
    }

    #[test]
    fn given_every_route_when_its_exposure_is_read_then_the_table_lists_it() {
        for route in ALL {
            assert!(spec_of(route.spec_id()).is_some(), "{route:?} is not in the route table");
        }
    }

    fn resolved(method: &str, raw: &str) -> Option<RouteId> {
        resolve(method, &Target::parse(raw).expect("a target"))
    }

    #[test]
    fn given_health_when_resolved_then_only_a_get_of_the_exact_pathname_reaches_it() {
        assert_eq!(resolved("GET", "/health"), Some(RouteId::Health));
        assert_eq!(resolved("GET", "/health?a=1"), Some(RouteId::Health));
        assert_eq!(resolved("GET", "/x/../health"), Some(RouteId::Health));
        for (method, raw) in [("HEAD", "/health"), ("OPTIONS", "/health"), ("POST", "/health"), ("get", "/health"), ("GET", "/health/"), ("GET", "//health")] {
            assert_eq!(resolved(method, raw), None, "{method} {raw}");
        }
    }
}
