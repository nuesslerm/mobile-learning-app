//! The service the e2e tests ask: a real `rca-service` and a real `opencode serve` started on loopback for the run of the test binary,
//! so no test reaches a deployed backend. `rca_env()` is what points a `wf` at it.
use exec_testkit::{OpencodeServer, RcaService};
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

const SECRET: &str = "wf-offload-e2e";

struct Running {
    service: RcaService,
    opencode: OpencodeServer,
    ask_root: std::path::PathBuf,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static ENV: OnceLock<Vec<(&'static str, String)>> = OnceLock::new();

/// Statics are not dropped when the test process exits, so the two servers are stopped here.
extern "C" fn stop() {
    if let Ok(mut running) = RUNNING.lock() {
        if let Some(Running { service, opencode, ask_root }) = running.take() {
            drop(service);
            drop(opencode);
            let _ = std::fs::remove_dir_all(ask_root);
        }
    }
}

/// Points `command` (a `wf`) at the servers of this run.
pub(crate) fn point(command: &mut std::process::Command) -> &mut std::process::Command {
    command.envs(rca_env().iter().map(|(k, v)| (*k, v.as_str())))
}

/// The variables that make a `wf offload` talk to the servers of this run: the service's host, port and secret, no tailnet, and no vault
/// (a vault the run is scoped to would answer for `CHAT_AGENT_API_SECRET` with the deployed service's secret, not this run's).
pub(crate) fn rca_env() -> &'static [(&'static str, String)] {
    ENV.get_or_init(|| {
        let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/remote-chat-agent/opencode-server/server-config.json");
        let opencode = OpencodeServer::start(crate::parity::key(), &config);
        let ask_root = tempfile::Builder::new().prefix("wf-e2e-rca-").tempdir().expect("a directory for the asks").keep();
        let (asks, ledger) = (ask_root.join("asks"), ask_root.join("ledger.json"));
        let url = opencode.url();
        let path: OsString = std::env::var_os("PATH").unwrap_or_default();
        let service = RcaService::start_with_env(SECRET, &path, &[("OPENCODE_SERVER_URL", &url), ("RCA_ASK_ROOT", &asks.to_string_lossy()), ("RCA_LEDGER_FILE", &ledger.to_string_lossy())]);
        let addr = service.addr();
        *RUNNING.lock().unwrap() = Some(Running { service, opencode, ask_root });
        unsafe { libc::atexit(stop) };
        vec![("CHAT_AGENT_API_SECRET", SECRET.to_string()), ("RCA_HOST", addr.ip().to_string()), ("RCA_PORT", addr.port().to_string()), ("WF_TAILNET_SOCKS", String::new()), ("SECRETS_BACKEND", "env".to_string())]
    })
}
