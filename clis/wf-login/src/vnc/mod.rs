//! `wf secret login listener`: the persistent auth-session listener that runs inside the login-browser container. It mints a
//! tokened `tailscale serve` link for a login (`POST /auth/start`), keeps the login browser presenting as a phone, takes
//! a browser-only credential off it when the human is done (`POST /auth/done`) and relays a credential file back to
//! agent-shell. Nothing else reaches the browser: the only ingress is the token-gated viewer behind `tailscale serve`.
pub mod descriptor;
mod desktop;
mod engine;
mod listener;
mod log;
mod relay;
mod serve;
mod view;

pub use relay::expand_tilde;
use descriptor::env_or_vault;
use std::net::TcpListener;
use std::sync::Arc;

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn required(name: &str) -> Result<String, i32> {
    env_or_vault(name).ok_or_else(|| {
        eprintln!("Missing required env var: {name}");
        1
    })
}

fn bind(addr: (&str, u16)) -> Result<TcpListener, i32> {
    TcpListener::bind(addr).map_err(|e| {
        wf_kit::wf_err!("wf: cannot bind {}:{}: {e}", addr.0, addr.1);
        1
    })
}

/// A deploy's smoke test runs a second copy beside the live listener, in the same tailnet node: it must not repoint the live route.
fn binds_route(smoke: Option<&str>) -> bool {
    smoke.is_none()
}

fn listen() -> Result<(), i32> {
    let trigger_secret = required("VNC_AUTH_TRIGGER_SECRET")?;
    let owner = required("VNC_AUTH_OWNER_CHAT_ID")?;
    let port: u16 = env_nonempty("VNC_AUTH_LISTENER_PORT").and_then(|p| p.parse().ok()).unwrap_or(8795);
    // `app` shares the tailscale sidecar's network namespace only: the CLI reaches the daemon through a shared socket.
    let socket = env_nonempty("TS_SOCKET");
    // An explicit override wins; else the node's own tailnet DNS name. An empty value (what compose passes when unset)
    // falls through to self-derivation.
    let Some(link_base) = env_nonempty("VNC_AUTH_LINK_BASE_URL").or_else(|| serve::resolve_base_url(socket.as_deref())) else {
        eprintln!("Could not determine link base URL: set VNC_AUTH_LINK_BASE_URL or ensure tailscale is up");
        return Err(1);
    };

    let gate_port: u16 = env_nonempty("VNC_AUTH_GATE_PORT").and_then(|p| p.parse().ok()).unwrap_or(3002);
    let gate = bind(("127.0.0.1", gate_port))?;
    let gate_port = gate.local_addr().map_or(gate_port, |a| a.port());

    let tokens = engine::Tokens::default();
    let launcher = Arc::new(desktop::Launcher::new(desktop::Config::from_env()));
    let viewer_launcher = launcher.clone();
    let deps = Arc::new(listener::Deps {
        trigger_secret,
        owners: owner.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        engine: engine::Engine::new(&link_base, launcher.clone(), tokens.clone()),
        launcher,
    });

    // `tailscale serve` points at this process's own gate (loopback, plain HTTP: nothing but tailscale reaches it), which
    // enforces the per-session link token and serves the viewer.
    if binds_route(env_nonempty("VNC_AUTH_SMOKE").as_deref()) {
        serve::bind_route(socket.as_deref(), &format!("http://127.0.0.1:{gate_port}")).map_err(|e| {
            eprintln!("{e}");
            1
        })?;
    }
    let control = bind(("0.0.0.0", port))?;
    let port = control.local_addr().map_or(port, |a| a.port());

    // The only place `wf` runs an async runtime: the gate and the control channel are long-lived servers.
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().map_err(|e| {
        wf_kit::wf_err!("wf: cannot start the listener runtime: {e}");
        1
    })?;
    runtime.block_on(async move {
        let (gate, control) = (into_tokio(gate)?, into_tokio(control)?);
        tokio::spawn(view::serve(gate, viewer_launcher, tokens));
        log::info("vnc_auth.gate.listening", &[("gatePort", gate_port.to_string())]);
        log::info("vnc_auth.listener.listening", &[("port", port.to_string())]);
        listener::serve(control, deps).await;
        Ok(())
    })
}

fn into_tokio(listener: TcpListener) -> Result<tokio::net::TcpListener, i32> {
    listener.set_nonblocking(true).and_then(|()| tokio::net::TcpListener::from_std(listener)).map_err(|e| {
        wf_kit::wf_err!("wf: cannot use the listening socket: {e}");
        1
    })
}

/// `wf secret login listener` (no arguments): runs until the process is stopped.
pub fn run_listener(args: &[String]) -> Option<i32> {
    wf_kit::args::parse_none(args, &[], &[])?;
    Some(listen().err().unwrap_or(0))
}

#[cfg(test)]
mod smoke_tests {
    use super::binds_route;

    #[test]
    fn given_a_smoke_copy_when_it_starts_then_it_leaves_the_live_tailnet_route_alone() {
        assert!(!binds_route(Some("1")));
        assert!(binds_route(None));
    }
}
