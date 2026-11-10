use axum::serve::ListenerExt;
use capture_core::application::Capture;
use capture_core::domain::config::{CaptureConfig, Mode};
use capture_core::domain::outcome::Outcome;
use capture_service::api;
use capture_service::infrastructure::config::{Config, ModeArg};
use capture_service::infrastructure::ports;
use clap::Parser;
use service_kit::{heartbeat, secrets, shutdown};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    // Before the flags are read: a variable the container was not given may come from the vault.
    secrets::load().await;
    let config = Config::parse();
    if config.healthcheck {
        return healthcheck(config.port).await;
    }
    let (Some(backend_base_url), Some(admin), Some(notify_base_url), Some(notify_secret)) =
        (config.backend_base_url, config.admin_secret, config.notify_base_url, config.notify_secret)
    else {
        return ExitCode::FAILURE;
    };

    let extra_secrets = secrets::env_vars().iter().filter_map(|name| std::env::var(name).ok()).filter(|value| value.chars().count() >= 8).collect();
    let telemetry = service_kit::telemetry::init!("leetcode-auth-cookie-capture", extra_secrets);

    let capture_config = CaptureConfig {
        mode: match config.mode {
            ModeArg::Local => Mode::Local,
            ModeArg::Remote => Mode::Remote,
        },
        novnc_port: config.novnc_port,
        capture_timeout: Duration::from_millis(config.timeout_ms),
        poll_interval: Duration::from_millis(config.poll_interval_ms),
        login_url: config.login_url,
    };
    let capture = Arc::new(Capture::new(capture_config, ports(backend_base_url, admin, notify_base_url, notify_secret, config.wf_bin.as_deref())));

    let code = if config.once { once(&capture).await } else { listen(capture, config.trigger_secret, config.port).await };
    let _ = tokio::task::spawn_blocking(move || telemetry.shutdown()).await;
    code
}

/// One attempt and out: the exit code says whether the cookie was pushed.
async fn once(capture: &Capture) -> ExitCode {
    match capture.run().await {
        Ok(Outcome::Succeeded) => ExitCode::SUCCESS,
        Ok(Outcome::AlreadyActive { expires_at }) => {
            tracing::error!(expires_at, "capture.already_active");
            ExitCode::FAILURE
        }
        Ok(outcome) => {
            tracing::error!(outcome = ?outcome, "capture.attempt.unsuccessful");
            ExitCode::FAILURE
        }
        Err(error) => {
            tracing::error!(error = %error, "capture.attempt.not_started");
            ExitCode::FAILURE
        }
    }
}

async fn listen(capture: Arc<Capture>, trigger_secret: Option<secrecy::SecretString>, port: u16) -> ExitCode {
    let Some(trigger_secret) = trigger_secret else { return ExitCode::FAILURE };
    let app = api::app(capture, trigger_secret);

    // All interfaces: the app container is reached from the tailscale sidecar's network namespace.
    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(port, error = %error, "server.bind.failed");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(host = "0.0.0.0", port, "listener.listening");
    heartbeat::spawn("leetcode-auth-cookie-capture.heartbeat");

    let listener = listener.tap_io(|connection| {
        let _ = connection.set_nodelay(true);
    });
    let served = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown::requested()).await;
    if let Err(error) = &served {
        tracing::error!(error = %error, "server.failed");
    }
    if served.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The container healthcheck: success when the listener on this host answers `/` with anything (a 404 is an answer).
async fn healthcheck(port: u16) -> ExitCode {
    let answered = reqwest::Client::new().get(format!("http://127.0.0.1:{port}/")).timeout(Duration::from_secs(3)).send().await.is_ok();
    if answered {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

