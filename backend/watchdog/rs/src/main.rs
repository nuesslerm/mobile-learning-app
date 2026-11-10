use clap::Parser;
use service_kit::shutdown;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use watchdog_service::infrastructure::config::Config;
use watchdog_service::infrastructure::wiring::{self, Ports};
use watchdog_service::infrastructure::{DockerOnPath, SystemClock};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let config = Config::parse();
    let telemetry = service_kit::telemetry::init!("watchdog");
    let watchdog = wiring::watchdog(Ports { docker: Arc::new(DockerOnPath), clock: Arc::new(SystemClock), host_label: config.host_label });

    tokio::select! {
        () = watchdog.run(Duration::from_millis(config.interval_ms)) => {}
        () = shutdown::requested() => {}
    }
    let _ = tokio::task::spawn_blocking(move || telemetry.shutdown()).await;
    ExitCode::SUCCESS
}
