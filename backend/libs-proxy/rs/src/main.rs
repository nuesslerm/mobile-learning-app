//! The proxy's binary and composition root: it wires the config, the OCI reader and the router together.
use clap::Parser;
use libs_proxy::api::App;
use libs_proxy::infrastructure::{Config, OciObjectRead};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let config = Config::parse();
    if config.healthcheck {
        return healthcheck(config.port).await;
    }
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).with_writer(std::io::stderr).init();
    serve(config).await
}

async fn serve(config: Config) -> ExitCode {
    let (Some(par_url), Some(base_url)) = (config.read_par_url, config.base_url) else {
        tracing::error!("config.missing");
        return ExitCode::FAILURE;
    };
    let objects = match OciObjectRead::new(&par_url) {
        Ok(objects) => Arc::new(objects),
        Err(error) => {
            tracing::error!(error = %error.without_url(), "bucket.client.failed");
            return ExitCode::FAILURE;
        }
    };
    let app = App::new(objects, &base_url, std::env::var("GIT_COMMIT").ok().as_deref());
    // All interfaces: the app container is reached from the tailscale sidecar's network namespace.
    let listener = match tokio::net::TcpListener::bind((config.bind.as_str(), config.port)).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(port = config.port, error = %error, "server.bind.failed");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(port = config.port, "libs.server.listening");
    match axum::serve(listener, app.router()).with_graceful_shutdown(shutdown()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = %error, "server.failed");
            ExitCode::FAILURE
        }
    }
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
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
