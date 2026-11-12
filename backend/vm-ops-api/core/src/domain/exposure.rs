//! Which listener serves which route. vm-ops-api listens on two ports: the CI port carries only what a CI job needs, the admin
//! port carries everything (decision 6 of docs/plans/ci-scoped-vault-access/plan.md). A route a port does not serve is a 404 there.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Port {
    Ci,
    Admin,
}

impl Port {
    pub fn name(self) -> &'static str {
        match self {
            Port::Ci => "ci",
            Port::Admin => "admin",
        }
    }
}

/// The ports a route is served on. `Shared` is a declared exception: agent-shell and the sandbox reach the CI routes over the admin
/// port, so those routes answer on both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    CiOnly,
    AdminOnly,
    Shared,
}

impl Exposure {
    pub fn serves(self, port: Port) -> bool {
        matches!((self, port), (Exposure::Shared, _) | (Exposure::CiOnly, Port::Ci) | (Exposure::AdminOnly, Port::Admin))
    }
}

/// One route of the service: `id` is the name of its variant in the API's route enum, `path` its pattern with `{name}` for one
/// segment.
#[derive(Debug, Clone, Copy)]
pub struct RouteSpec {
    pub id: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub exposure: Exposure,
}

const fn spec(id: &'static str, method: &'static str, path: &'static str, exposure: Exposure) -> RouteSpec {
    RouteSpec { id, method, path, exposure }
}

use Exposure::{AdminOnly, CiOnly, Shared};

/// Every route, classified. Anything that is not clearly something a CI job calls (a redeploy of a tag CI already pushed, its smoke
/// test and status, health, the container and stats reads) is admin only.
pub const ROUTES: &[RouteSpec] = &[
    spec("Health", "GET", "/health", Shared),
    spec("Ps", "GET", "/services/{app}/ps", Shared),
    spec("Stats", "GET", "/stats", Shared),
    spec("RedeployStatus", "GET", "/redeploy/status", Shared),
    spec("AppRedeploy", "POST", "/services/{app}/redeploy", Shared),
    spec("AppSmoke", "POST", "/services/{app}/smoke", Shared),
    spec("AppSmokeStop", "POST", "/services/{app}/smoke/stop", Shared),
    spec("SelfRedeploy", "POST", "/redeploy", Shared),
    spec("SelfSmoke", "POST", "/redeploy/smoke", Shared),
    spec("SelfSmokeStop", "POST", "/redeploy/smoke/stop", Shared),
    // The wf build CI published, installed on the VM by a sibling: a hosted runner cannot write to the VM, and only CI calls this.
    spec("ClisInstall", "POST", "/clis/install", CiOnly),
    spec("Logs", "GET", "/services/{app}/logs", AdminOnly),
    spec("Retire", "POST", "/services/{app}/retire", AdminOnly),
    spec("AppPair", "POST", "/services/{app}/pair", AdminOnly),
    spec("Drill", "POST", "/drill", AdminOnly),
    spec("OpencodeDashboard", "GET", "/opencode-log", AdminOnly),
    spec("OpencodeRuns", "GET", "/opencode-log/api/runs", AdminOnly),
    spec("OpencodeRun", "GET", "/opencode-log/api/runs/{run}", AdminOnly),
    spec("OpencodeStart", "POST", "/opencode-log/{run}/start", AdminOnly),
    spec("OpencodeLine", "POST", "/opencode-log/{run}/line", AdminOnly),
    spec("OpencodeFinish", "POST", "/opencode-log/{run}/finish", AdminOnly),
    spec("ExecScriptStart", "POST", "/services/agent-shell/exec-script", AdminOnly),
    spec("ExecScriptStatus", "GET", "/services/agent-shell/exec-script/{job}", AdminOnly),
    spec("ExecScriptCancel", "POST", "/services/agent-shell/exec-script/{job}/cancel", AdminOnly),
    spec("VmLedger", "GET", "/services/agent-shell/vm-ledger/{name}", AdminOnly),
    spec("Artifact", "GET", "/services/agent-shell/artifact", AdminOnly),
    spec("Credentials", "POST", "/agent-shell/credentials", AdminOnly),
];

pub fn spec_of(id: &str) -> Option<&'static RouteSpec> {
    ROUTES.iter().find(|spec| spec.id == id)
}

/// Whether `method` and the pathname `path` (no query) are `spec`'s route, a `{name}` standing for one non-empty segment.
pub fn matches(spec: &RouteSpec, method: &str, path: &str) -> bool {
    let (mut pattern, mut actual) = (spec.path.split('/'), path.split('/'));
    spec.method == method
        && loop {
            match (pattern.next(), actual.next()) {
                (None, None) => break true,
                (Some(p), Some(a)) if (p.starts_with('{') && !a.is_empty()) || p == a => {}
                _ => break false,
            }
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn given_the_route_table_when_it_is_read_then_each_route_is_listed_once_and_only_the_ci_calls_are_shared() {
        let ids: HashSet<&str> = ROUTES.iter().map(|spec| spec.id).collect();
        let shapes: HashSet<(&str, &str)> = ROUTES.iter().map(|spec| (spec.method, spec.path)).collect();
        assert_eq!((ids.len(), shapes.len()), (ROUTES.len(), ROUTES.len()));
        let shared: Vec<&str> = ROUTES.iter().filter(|spec| spec.exposure == Exposure::Shared).map(|spec| spec.id).collect();
        let expected = ["Health", "Ps", "Stats", "RedeployStatus", "AppRedeploy", "AppSmoke", "AppSmokeStop", "SelfRedeploy", "SelfSmoke", "SelfSmokeStop"];
        assert_eq!(shared, expected);
    }

    #[test]
    fn given_a_route_that_is_not_shared_when_asked_which_ports_serve_it_then_exactly_one_does() {
        for spec in ROUTES.iter().filter(|spec| spec.exposure != Exposure::Shared) {
            let served: Vec<Port> = [Port::Ci, Port::Admin].into_iter().filter(|port| spec.exposure.serves(*port)).collect();
            assert_eq!(served.len(), 1, "{}", spec.id);
        }
        assert!([Port::Ci, Port::Admin].into_iter().all(|port| Exposure::Shared.serves(port)));
    }

    #[test]
    fn given_the_exec_script_routes_when_classified_then_they_are_admin_only_and_no_route_builds_an_image() {
        let found: Vec<&RouteSpec> = ROUTES.iter().filter(|spec| spec.path.contains("exec-script")).collect();
        assert_eq!(found.len(), 3, "the agent-shell exec-script routes");
        assert!(ROUTES.iter().all(|spec| !spec.path.contains("build") && !spec.id.to_lowercase().contains("build")));
        for spec in found {
            assert_eq!(spec.exposure, Exposure::AdminOnly, "{}", spec.id);
            assert!(!spec.exposure.serves(Port::Ci), "{}", spec.id);
        }
    }

    #[test]
    fn given_every_agent_shell_route_when_classified_then_the_ci_port_serves_none() {
        for spec in ROUTES.iter().filter(|spec| spec.path.contains("agent-shell") && !spec.path.contains("{app}")) {
            assert!(!spec.exposure.serves(Port::Ci), "{}", spec.id);
        }
    }

    #[test]
    fn given_a_pattern_when_matched_then_a_placeholder_takes_one_non_empty_segment() {
        let ps = spec_of("Ps").expect("Ps is listed");
        assert!(matches(ps, "GET", "/services/leetcode-backend/ps"));
        for (method, path) in [("POST", "/services/x/ps"), ("GET", "/services//ps"), ("GET", "/services/x/y/ps"), ("GET", "/services/x/ps/")] {
            assert!(!matches(ps, method, path), "{method} {path}");
        }
    }
}
