//! The reporting service's binary: the HTTP API over the VM's health rows, series and plan files, wired here to its real adapters.
//! Its image keeps `cargo-chef` and the registry layer cache: sccache built it in 223 s cold and 99 s warm, against 174-192 s and 14 s (decision log, 26 Oct).
use axum::serve::ListenerExt;
use clap::Parser;
use reporting_core::application::{Heartbeat, NoHeartbeat, NoPublisher, RunnerSource, SamplePublisher};
use reporting_service::api::{App, Ports};
use reporting_service::infrastructure::config::Config;
use reporting_service::infrastructure::{DockerSocket, GithubRunner, HttpHeartbeat, OrdsStore, PlanFiles, ProcHost, SinkPublisher, SystemClock};
use service_kit::{heartbeat, shutdown};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let config = Config::parse();
    if config.healthcheck {
        return healthcheck(config.port).await;
    }
    let telemetry = service_kit::telemetry::init!("reporting-service");
    let code = serve(config).await;
    let _ = tokio::task::spawn_blocking(move || telemetry.shutdown()).await;
    code
}

async fn serve(config: Config) -> ExitCode {
    let store = match OrdsStore::connect(&config.ords_base_url, "health_rpt", &config.ords_password).await {
        Ok(store) => Arc::new(store),
        Err(error) => {
            tracing::error!(error = %error, "store.connect.failed");
            return ExitCode::FAILURE;
        }
    };
    let docker = match DockerSocket::new(&config.docker_socket) {
        Ok(docker) => Arc::new(docker),
        Err(error) => {
            tracing::error!(error = %error, "docker.client.failed");
            return ExitCode::FAILURE;
        }
    };
    let heartbeat: Arc<dyn Heartbeat> = match config.heartbeat_url.clone().filter(|url| !url.is_empty()).map(HttpHeartbeat::new) {
        Some(Ok(heartbeat)) => Arc::new(heartbeat),
        Some(Err(error)) => {
            tracing::error!(error = %error, "heartbeat.client.failed");
            return ExitCode::FAILURE;
        }
        None => Arc::new(NoHeartbeat),
    };
    let publisher: Arc<dyn SamplePublisher> = match config.metrics_host.as_deref().zip(config.metrics_token.as_deref()).and_then(|(host, token)| telemetry::Destination::from_host(host, token)) {
        Some(destination) => Arc::new(SinkPublisher::new(Arc::new(telemetry::OtlpMetricsSink::new(destination, "reporting-service")))),
        None => {
            tracing::info!("metrics.publish.off");
            Arc::new(NoPublisher)
        }
    };
    let runner: Option<Arc<dyn RunnerSource>> = match (config.runner_github_token.clone(), config.github_repo.clone(), config.runner_name.clone()) {
        (Some(token), Some(repo), Some(name)) => match GithubRunner::new("https://api.github.com", repo, name, token) {
            Ok(runner) => Some(Arc::new(runner)),
            Err(error) => {
                tracing::error!(error = %error, "runner.client.failed");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            tracing::info!("runner.sampling.off");
            None
        }
    };
    let app = App::new(Ports {
        store,
        docker,
        host: Arc::new(ProcHost),
        plans: Arc::new(PlanFiles::new(config.repo_root())),
        clock: Arc::new(SystemClock),
        commit: config.commit.clone(),
        policy: config.policy(),
        heartbeat,
        publisher,
        runner,
    });

    // All interfaces: the app container is reached from the tailscale sidecar's network namespace.
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(port = config.port, error = %error, "server.bind.failed");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(port = config.port, socket_path = %config.docker_socket.display(), "reporting.server.listening");
    heartbeat::spawn("reporting-service.heartbeat");
    let scheduler = app.start_sampling(config.intervals());

    let listener = listener.tap_io(|connection| {
        let _ = connection.set_nodelay(true);
    });
    let served = axum::serve(listener, app.router().into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown::requested()).await;
    scheduler.stop();
    if let Err(error) = &served {
        tracing::error!(error = %error, "server.failed");
    }
    if served.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The container healthcheck: success when the service on this host answers `/health`.
async fn healthcheck(port: u16) -> ExitCode {
    let answered = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success());
    if answered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
