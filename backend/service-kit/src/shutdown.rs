//! The signal a service shuts down on: Ctrl-C or SIGTERM (what `docker stop` sends).

use tokio::signal::unix::{signal, SignalKind};

/// Resolves when the process is asked to stop. Hand it to `axum::serve(...).with_graceful_shutdown(...)`.
pub async fn requested() {
    let terminate = signal(SignalKind::terminate());
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = async {
            match terminate {
                Ok(mut stream) => { stream.recv().await; }
                Err(_) => std::future::pending::<()>().await,
            }
        } => {}
    }
}
