use rca_service::{agent_api, Accounts, AttachmentParts, ScheduleParts};
use rca_service::api::app::{App, Config};
use rca_service::infrastructure::device_bearers::BearerFile;
use rca_service::api::routes;
use rca_service::compose;
use rca_service_core::application::ports::Ports;
use rca_service_core::application::runs;
use rca_service_core::application::startup;
use rca_service_core::application::log;
use rca_service::infrastructure::env::ProcessEnv;
use rca_service::infrastructure::{ords, outbound, secrets};
use remote_chat_agent_host::{Documents, HostFiles, Hosts, Processes, System};
use remote_chat_agent_opencode::Http;
use rca_service_core::application::environment::Environment as _;
use service_kit::{heartbeat, shutdown};
use axum::serve::ListenerExt;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    // The compose healthcheck runs this binary, so the image needs no curl.
    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        let port = ProcessEnv.settings().port;
        return healthcheck(port).await;
    }
    // The order the Node entry has: the vault fills what the container was not given, logging is wired, then the gate, then the port.
    secrets::load().await;
    let extra_secrets = secrets::env_vars().iter().filter_map(|name| ProcessEnv.var(name)).filter(|value| value.chars().count() >= 8).collect();
    let telemetry = service_kit::telemetry::init!("remote-chat-agent", extra_secrets);
    if let Err(message) = startup::check(ProcessEnv.settings().agent_cli.as_deref()) {
        eprintln!("remote-chat-agent: {message}");
        return ExitCode::FAILURE;
    }
    let config = Config::from_env(&ProcessEnv);
    let port = config.port;
    // All interfaces: the app container is reached from its compose network and from the tailscale sidecar.
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("server.bind.failed", port = port, error = e.to_string());
            return ExitCode::FAILURE;
        }
    };
    log::info!("server.listening", host = "0.0.0.0", port = port);
    let settings = ProcessEnv.settings();
    let spawner = Arc::new(Processes::at(settings.providers_root.clone()));
    let outbound = Arc::new(outbound::Reqwest::default());
    let commands = Arc::new(System);
    let env = Arc::new(ProcessEnv);
    let client = compose::opencode_client(Arc::new(Http::from_settings(&settings)), env.clone());
    let (orch, gateway) = compose::orchestration(&config, &client, env.as_ref());
    let ports = Ports { spawner: spawner.clone(), client, orch, gateway, commands: commands.clone(), outbound: outbound.clone(), documents: Arc::new(Documents::new(commands.clone(), &settings)), speech: Arc::new(remote_chat_agent_speech::Speech::new(outbound.clone(), commands.clone(), env.clone())), ledgers: Arc::new(ords::OrdsLedgers::from_settings(&settings)), files: Arc::new(HostFiles), hosts: Arc::new(Hosts), env };
    let state = Arc::new(rca_service::infrastructure::agent_state::StateFile::at(ProcessEnv.home_dir().join(".local/state/rca/agent-state.json")));
    let bearers = Arc::new(BearerFile::at(ProcessEnv.home_dir().join(".local/state/rca/device-bearers.json")));
    let schedule_parts = ScheduleParts::real(ProcessEnv.home_dir().join(".local/state/rca/schedules.json"));
    let attachment_parts = AttachmentParts::real(ProcessEnv.home_dir().join(".local/state/rca/attachments"));
    let agent_api = agent_api(&config, spawner, state, bearers, Accounts::real(&settings, outbound), schedule_parts, attachment_parts);
    let app = Arc::new(App::new(config, ports, agent_api));
    app.start();
    heartbeat::spawn("remote-chat-agent.heartbeat");
    let schedules = Arc::clone(&app.schedules);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(5));
        loop {
            every.tick().await;
            schedules.tick().await;
        }
    });
    let reaper = Arc::clone(&app);
    tokio::spawn(async move {
        let mut hourly = tokio::time::interval(Duration::from_secs(60 * 60));
        loop {
            hourly.tick().await;
            let (client, fs, handle) = (reaper.opencode.clone(), reaper.files.clone(), tokio::runtime::Handle::current());
            let _ = tokio::task::spawn_blocking(move || runs::reap(&handle, &client, fs.as_ref())).await;
        }
    });

    // Small writes (a handshake, a hello frame) must not wait on the peer's delayed ACK.
    let listener = listener.tap_io(|connection| {
        let _ = connection.set_nodelay(true);
    });
    let served = axum::serve(listener, routes::router(app).into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown::requested()).await;
    if let Err(error) = &served {
        log::error!("server.failed", error = error.to_string());
    }
    let _ = tokio::task::spawn_blocking(move || telemetry.shutdown()).await;
    if served.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The container healthcheck: success when the service on this host answers `/health`.
async fn healthcheck(port: u16) -> ExitCode {
    let answered = reqwest::Client::new().get(format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(3)).send().await.is_ok_and(|response| response.status().is_success());
    if answered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
