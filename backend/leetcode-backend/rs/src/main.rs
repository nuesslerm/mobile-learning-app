use clap::Parser;
use leetcode_backend::api::{self, Settings};
use leetcode_backend::infrastructure::{self, config::Config};
use secrecy::ExposeSecret;
use service_kit::{heartbeat, shutdown};
use axum::serve::ListenerExt;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let config = Config::parse();
    // The flag lives in this binary so the image needs no curl.
    if config.healthcheck {
        return healthcheck(config.port).await;
    }
    let (Some(capture_base_url), Some(capture_secret), Some(admin), Some(bearer)) =
        (config.capture_base_url, config.capture_secret, config.admin_secret, config.bearer_token)
    else {
        return ExitCode::FAILURE;
    };

    let extra_secrets = [&capture_secret, &admin, &bearer].map(|secret| secret.expose_secret().to_string()).to_vec();
    let telemetry = service_kit::telemetry::init!("leetcode-backend", extra_secrets);

    let ports = infrastructure::ports(config.session_file, config.leetcode_origin, capture_base_url, capture_secret);
    let app = api::app(ports, Settings { commit: config.commit, bearer, admin, requests_per_minute: api::REQUESTS_PER_MINUTE });

    // All interfaces: the app container is reached from its compose network and from the tailscale sidecar.
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(port = config.port, error = %error, "server.bind.failed");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(host = "0.0.0.0", port = config.port, "server.listening");
    heartbeat::spawn("leetcode-backend.heartbeat");

    // Small writes (a handshake, a hello frame) must not wait on the peer's delayed ACK.
    let listener = listener.tap_io(|connection| {
        let _ = connection.set_nodelay(true);
    });
    let served = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown::requested()).await;
    if let Err(error) = &served {
        tracing::error!(error = %error, "server.failed");
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
