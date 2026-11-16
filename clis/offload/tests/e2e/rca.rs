//! The services the e2e tests ask: a real `rca-service` and a real `opencode serve` started on loopback for the run of the test binary,
//! so no test reaches a deployed backend. `Model::Scripted` puts the `fake-model` behind opencode (no network, no key); `Model::Live` puts
//! the provider behind it (the key of `parity::key()`), for the tests that compare a real model's wording. Each is started on first use.
use exec_testkit::{FakeModel, OpencodeServer, RcaService};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

const SECRET: &str = "wf-offload-e2e";

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Model {
    Scripted,
    Live,
}

struct Running {
    service: RcaService,
    opencode: OpencodeServer,
    model: Option<FakeModel>,
    ask_root: std::path::PathBuf,
}

static RUNNING: Mutex<Vec<Running>> = Mutex::new(Vec::new());
static ENV: Mutex<Option<HashMap<Model, &'static [(&'static str, String)]>>> = Mutex::new(None);
static EXIT_HOOK: OnceLock<()> = OnceLock::new();

/// Statics are not dropped when the test process exits, so the servers are stopped here.
extern "C" fn stop() {
    if let Ok(mut running) = RUNNING.lock() {
        for Running { service, opencode, model, ask_root } in running.drain(..) {
            drop(service);
            drop(opencode);
            drop(model);
            let _ = std::fs::remove_dir_all(ask_root);
        }
    }
}

/// Points `command` (a `wf`) at the servers of this run that answer with `model`.
pub(crate) fn point(command: &mut std::process::Command, model: Model) -> &mut std::process::Command {
    command.envs(rca_env(model).iter().map(|(k, v)| (*k, v.as_str())))
}

/// The variables that make a `wf offload` talk to the servers of this run: the service's host, port and secret, no tailnet, and no vault
/// (a vault the run is scoped to would answer for `CHAT_AGENT_API_SECRET` with the deployed service's secret, not this run's).
pub(crate) fn rca_env(model: Model) -> &'static [(&'static str, String)] {
    let mut envs = ENV.lock().unwrap();
    envs.get_or_insert_with(HashMap::new).entry(model).or_insert_with(|| Box::leak(start(model).into_boxed_slice()))
}

fn start(model: Model) -> Vec<(&'static str, String)> {
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend/remote-chat-agent/opencode-server/server-config.json");
    let (fake, opencode) = match model {
        Model::Scripted => {
            let fake = FakeModel::start();
            let opencode = OpencodeServer::start_scripted(&fake.base_url(), &config);
            (Some(fake), opencode)
        }
        Model::Live => (None, OpencodeServer::start(crate::parity::key(), &config)),
    };
    let ask_root = tempfile::Builder::new().prefix("wf-e2e-rca-").tempdir().expect("a directory for the asks").keep();
    let (asks, ledger) = (ask_root.join("asks"), ask_root.join("ledger.json"));
    let url = opencode.url();
    let path: OsString = std::env::var_os("PATH").unwrap_or_default();
    let service = RcaService::start_with_env(SECRET, &path, &[("OPENCODE_SERVER_URL", &url), ("RCA_ASK_ROOT", &asks.to_string_lossy()), ("RCA_LEDGER_FILE", &ledger.to_string_lossy())]);
    let addr = service.addr();
    RUNNING.lock().unwrap().push(Running { service, opencode, model: fake, ask_root });
    EXIT_HOOK.get_or_init(|| unsafe {
        libc::atexit(stop);
    });
    vec![("CHAT_AGENT_API_SECRET", SECRET.to_string()), ("RCA_HOST", addr.ip().to_string()), ("RCA_PORT", addr.port().to_string()), ("WF_TAILNET_SOCKS", String::new()), ("SECRETS_BACKEND", "env".to_string())]
}
