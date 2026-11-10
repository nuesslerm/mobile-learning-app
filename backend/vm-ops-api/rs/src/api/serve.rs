//! The HTTP/1 server, with Node's slow-client limits: `axum::serve` has none, and this service reads a body before credentials.
//! hyper's parser differs from Node's at the edges (stricter on a raw `<`, `>` or backtick in a target, looser on bare LF).
use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use std::future::Future;
use std::io::ErrorKind;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

/// How long the server gives a client.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// A request's head, and an idle connection's wait for the next request (Node: `headersTimeout`).
    pub head: Duration,
    /// A request's body once its head is in (Node: `requestTimeout`, which also counts the head).
    pub body: Duration,
    /// How long a stop waits for requests in flight before it drops them.
    pub drain: Duration,
}

impl Limits {
    pub const NODE: Limits = Limits { head: Duration::from_secs(60), body: Duration::from_secs(300), drain: Duration::from_secs(5) };
}

/// Serves `router` on `listener` until `stop` resolves, then lets requests in flight finish for at most `limits.drain`.
pub async fn serve(listener: TcpListener, router: Router, stop: impl Future<Output = ()>, limits: Limits) {
    let (stopped, stop_seen) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            () = &mut stop => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    connections.spawn(connection(stream, router.clone(), limits.head, stop_seen.clone()));
                }
                Err(error) => accept_failed(&error).await,
            },
        }
    }
    let _ = stopped.send(true);
    let drained = async { while connections.join_next().await.is_some() {} };
    if tokio::time::timeout(limits.drain, drained).await.is_err() {
        tracing::warn!(open = connections.len(), "http.drain_expired");
    }
}

async fn connection(stream: TcpStream, router: Router, head: Duration, mut stop: watch::Receiver<bool>) {
    let _ = stream.set_nodelay(true);
    let served = http1::Builder::new().timer(TokioTimer::new()).header_read_timeout(head).serve_connection(TokioIo::new(stream), TowerToHyperService::new(router));
    tokio::pin!(served);
    // `wait_for` hands back a lock guard, which must not live across the awaits below
    let stopped = async {
        let _ = stop.wait_for(|stopped| *stopped).await;
    };
    let outcome = tokio::select! {
        outcome = served.as_mut() => outcome,
        () = stopped => {
            served.as_mut().graceful_shutdown();
            served.await
        }
    };
    if let Err(error) = outcome {
        tracing::debug!(error = %error, "http.connection_ended");
    }
}

/// A connection the client dropped before it was accepted is the client's; anything else, out of file descriptors say, is waited out.
async fn accept_failed(error: &std::io::Error) {
    if matches!(error.kind(), ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused) {
        return;
    }
    tracing::warn!(error = %error, "http.accept_failed");
    tokio::time::sleep(Duration::from_secs(1)).await;
}
