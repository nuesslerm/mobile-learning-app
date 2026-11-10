use clap::Parser;
use secrecy::{ExposeSecret, SecretString};
use service_kit::shutdown;
use service_kit::telemetry::Destination;
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use vm_ops_api::api::{self, Limits, Ports as ApiPorts, Settings};
use vm_ops_api::infrastructure::{
    healthcheck, helpers, running_binary_sha, Config, DockerEngine, DockerSiblings, FileCooldownLedger, FileRunLog, OsProcessMemory, OtlpEventLog, ProcHostFiles, RandomIds, StdoutEventLog, SystemClock,
    TeeEventLog,
};
use vm_ops_api_core::application::heartbeat::Heartbeat;
use vm_ops_api_core::application::ports::{Clock, Containers, EventLog};
use vm_ops_api_core::application::startup;

/// The opencode-log dashboard, embedded so the image needs no file beside the binary.
const DASHBOARD: &[u8] = include_bytes!("../static/opencode-log/index.html");

// Raw bytes on stderr, not an event: telemetry has not started, and the integration suite pins Node's exact line.
const NO_AUTH: &[u8] = b"VM_OPS_API_AUTH_TOKEN is not set: refusing to start with no auth configured\n";

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    // The sibling scripts call the helper subcommands by name, so no option of the service itself is parsed for them.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some((name, rest)) = args.split_first().filter(|(name, _)| helpers::is_helper(name)) {
        return helper(name, rest).await;
    }
    if args.first().is_some_and(|flag| flag == "--healthcheck") {
        return healthcheck().await;
    }
    let config = Config::parse();
    let Some(token) = config.auth_token() else {
        let _ = std::io::stderr().write_all(NO_AUTH);
        return ExitCode::FAILURE;
    };
    let telemetry = service_kit::telemetry::init!("vm-ops-api", config.secrets());
    let code = serve(&config, token).await;
    let _ = tokio::task::spawn_blocking(move || telemetry.shutdown()).await;
    code
}

/// Compose's check of this container: its own port and token, from the environment alone, and only a 200 passes.
async fn healthcheck() -> ExitCode {
    let config = Config::parse_from(["vm-ops-api-rs"]);
    let token = config.auth_token().map(|token| token.expose_secret().to_string()).unwrap_or_default();
    let Ok(port) = config.port() else { return ExitCode::FAILURE };
    if healthcheck::is_healthy(port, &token).await {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn helper(name: &str, args: &[String]) -> ExitCode {
    let token = std::env::var("GITHUB_RAW_TOKEN").ok();
    let code = helpers::run(name, args, token.as_deref(), &mut std::io::stdout(), &mut std::io::stderr()).await;
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Every event is the JSON line on stdout; with an ingest in the environment it is exported too, in batches from a thread of its own.
fn event_log(config: &Config) -> (Arc<dyn EventLog>, Option<Arc<OtlpEventLog>>) {
    let stdout = Arc::new(StdoutEventLog::stdout().scrubbing(config.secrets()));
    let Some(destination) = Destination::from_env() else { return (stdout, None) };
    let source = service_kit::telemetry::source!("vm-ops-api");
    let otlp = Arc::new(OtlpEventLog::new(source, destination, config.host_label(), config.secrets(), stdout.clone()));
    (Arc::new(TeeEventLog::new(vec![stdout, otlp.clone()])), Some(otlp))
}

async fn serve(config: &Config, token: SecretString) -> ExitCode {
    let port = match config.port() {
        Ok(port) => port,
        Err(error) => {
            tracing::error!(error = %error, "config.invalid");
            return ExitCode::FAILURE;
        }
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let (log, otlp) = event_log(config);
    let ci_port = match config.ci_port() {
        Ok(port) => port,
        Err(error) => {
            tracing::error!(error = %error, "config.invalid");
            return ExitCode::FAILURE;
        }
    };
    let code = run(config, token, Ports { admin: port, ci: ci_port }, clock, log).await;
    if let Some(otlp) = otlp {
        let _ = tokio::task::spawn_blocking(move || otlp.flush(Duration::from_secs(2))).await;
    }
    code
}

/// The ports the service listens on.
struct Ports {
    admin: u16,
    ci: Option<u16>,
}

async fn bind(port: u16, listener: &str) -> Option<tokio::net::TcpListener> {
    // All interfaces: the container's own network namespace is the tailscale sidecar's, and no host port is published.
    match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(bound) => Some(bound),
        Err(error) => {
            tracing::error!(port, listener, error = %error, "server.bind.failed");
            None
        }
    }
}

async fn run(config: &Config, token: SecretString, ports: Ports, clock: Arc<dyn Clock>, log: Arc<dyn EventLog>) -> ExitCode {
    let auth = config.auth(token);
    startup::check_signing_key(log.as_ref(), &auth);
    let containers = match DockerEngine::new(&config.docker_socket()) {
        Ok(engine) => Arc::new(engine) as Arc<dyn Containers>,
        Err(error) => {
            tracing::error!(error = %error, "docker.client.failed");
            return ExitCode::FAILURE;
        }
    };
    let routers = api::routers(
        ApiPorts {
            clock: clock.clone(),
            log: log.clone(),
            containers: containers.clone(),
            host_files: Arc::new(ProcHostFiles::new(config.proc_root())),
            cooldowns: Arc::new(FileCooldownLedger::new(config.pool_cooldown_state_file(), log.clone())),
            run_log: Arc::new(FileRunLog::new(config.opencode_log_state_dir())),
            ids: Arc::new(RandomIds),
            siblings: Arc::new(DockerSiblings::default()),
        },
        Settings {
            auth,
            ci_auth: ports.ci.and(config.ci_auth()),
            build: config.build(running_binary_sha()),
            limits: Limits::NODE,
            dashboard: DASHBOARD,
            sibling_image: config.sibling_image(),
            redeploy: config.redeploy_env(),
        },
    );
    let Some(listener) = bind(ports.admin, "admin").await else { return ExitCode::FAILURE };
    let ci_listener = match (ports.ci, routers.ci) {
        (Some(port), Some(router)) => match bind(port, "ci").await {
            Some(bound) => Some((port, bound, router)),
            None => return ExitCode::FAILURE,
        },
        _ => None,
    };
    let heartbeat = Heartbeat::new(config.host_label(), clock.clone(), Arc::new(OsProcessMemory), log.clone());
    heartbeat.beat();
    startup::announce_listening(log.as_ref(), ports.admin, &config.host_label());
    tokio::spawn(async move { heartbeat.run().await });

    let admin = api::serve::serve(listener, routers.admin, shutdown::requested(), Limits::NODE);
    let ci = async {
        if let Some((port, bound, router)) = ci_listener {
            startup::announce_ci_listening(log.as_ref(), port);
            api::serve::serve(bound, router, shutdown::requested(), Limits::NODE).await;
        }
    };
    tokio::join!(admin, ci);
    ExitCode::SUCCESS
}
