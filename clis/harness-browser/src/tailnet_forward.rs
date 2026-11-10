//! A localhost listener that dials a target through a SOCKS5 proxy (tailscaled's userspace listener, default
//! 127.0.0.1:1055) and pipes bytes both ways: a transparent, protocol-agnostic tunnel for plain HTTP and WebSockets.
//! One listener per `host:port` for the life of the daemon.
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::{Mutex, OnceLock};
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use tokio_socks::TargetAddr;

const DEFAULT_SOCKS_PORT: u16 = 1055;

fn active() -> &'static Mutex<HashMap<String, u16>> {
    static ACTIVE: OnceLock<Mutex<HashMap<String, u16>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// One small runtime for every forwarder of this process, started by the first one.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(1).thread_name("wf-forward").enable_all().build().expect("a one-worker runtime starts"))
}

fn socks_port() -> u16 {
    std::env::var("WF_SOCKS_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_SOCKS_PORT)
}

/// The proxy resolves the name itself (tailscaled's userspace listener does), so the target goes over as a domain.
async fn forward(mut client: TcpStream, socks: u16, host: String, port: u16) {
    let Ok(mut upstream) = Socks5Stream::connect(("127.0.0.1", socks), TargetAddr::Domain(host.into(), port)).await else { return };
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
}

async fn accept(listener: tokio::net::TcpListener, socks: u16, host: String, port: u16) {
    while let Ok((client, _)) = listener.accept().await {
        tokio::spawn(forward(client, socks, host.clone(), port));
    }
}

/// `(local port, reused)`: starts the listener for `host:port`, or returns the running one. A requested local port that
/// something else already holds counts as reused, as the Node daemon reports it.
pub fn ensure(host: &str, port: u16, local_port: Option<u16>) -> Result<(u16, bool), String> {
    let key = format!("{host}:{port}");
    if let Some(existing) = active().lock().map_err(|_| "forward registry poisoned")?.get(&key) {
        return Ok((*existing, true));
    }
    let listener = match TcpListener::bind(("127.0.0.1", local_port.unwrap_or(0))) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => return Ok((local_port.unwrap_or(0), true)),
        Err(e) => return Err(format!("cannot listen on 127.0.0.1:{}: {e}", local_port.unwrap_or(0))),
    };
    let bound = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let _guard = runtime().enter();
    let listener = tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
    runtime().spawn(accept(listener, socks_port(), host.to_string(), port));
    active().lock().map_err(|_| "forward registry poisoned")?.insert(key, bound);
    Ok((bound, false))
}
