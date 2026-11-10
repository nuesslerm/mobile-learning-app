//! `wf vm net up [--hostname <name>] [--backend <name>]`: join this machine to the tailnet, idempotently. A sandbox
//! reaches the VM services only over the tailnet, and `tailscaled` does not survive a container restart; a stopped daemon
//! reads like a missing capability, so the state is classified from `tailscale status` before anything is joined.
use regex::Regex;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const DEFAULT_HOSTNAME: &str = "sandbox-agent";
const SOCKS_PORT: u16 = 1055;
const DAEMON_LOG: &str = "/tmp/tailscaled.log";
const MAX_ATTEMPTS: usize = 30;
const DELAY: Duration = Duration::from_secs(1);

struct Fail(String);

#[derive(PartialEq)]
enum State {
    Joined,
    LoggedOut,
    DaemonDown,
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(program: &str, args: &[&str]) -> Ran {
    match Command::new(program).args(args).stdin(Stdio::null()).output() {
        Ok(o) => Ran { code: o.status.code().unwrap_or(1), stdout: String::from_utf8_lossy(&o.stdout).into_owned(), stderr: String::from_utf8_lossy(&o.stderr).into_owned() },
        Err(e) => Ran { code: 1, stdout: String::new(), stderr: e.to_string() },
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(bin)).find(|f| f.is_file()))
}

fn classify(r: &Ran) -> State {
    let text = format!("{}\n{}", r.stdout, r.stderr);
    if Regex::new(r"(?i)failed to connect to local tailscaled").unwrap().is_match(&text) {
        State::DaemonDown
    } else if Regex::new(r"(?im)^logged out").unwrap().is_match(&text) {
        State::LoggedOut
    } else if r.code == 0 {
        State::Joined
    } else {
        State::LoggedOut
    }
}

fn status() -> Ran {
    run("tailscale", &["status"])
}

/// Whether this node reports itself online, or None when that cannot be read.
fn status_online() -> Option<bool> {
    let r = run("tailscale", &["status", "--json"]);
    if r.code != 0 || r.stdout.is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&r.stdout).ok()?["Self"]["Online"].as_bool()
}

fn start_daemon() {
    use std::os::unix::process::CommandExt;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(DAEMON_LOG);
    let mut cmd = Command::new("tailscaled");
    cmd.args(["--tun=userspace-networking", &format!("--socks5-server=localhost:{SOCKS_PORT}"), &format!("--outbound-http-proxy-listen=localhost:{SOCKS_PORT}")]).stdin(Stdio::null()).process_group(0);
    match log.and_then(|l| l.try_clone().map(|c| (l, c))) {
        Ok((a, b)) => {
            cmd.stdout(a).stderr(b);
        }
        Err(_) => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    let _ = cmd.spawn();
}

fn wait_for(ready: impl Fn() -> bool) -> bool {
    for _ in 0..MAX_ATTEMPTS {
        if ready() {
            return true;
        }
        std::thread::sleep(DELAY);
    }
    ready()
}

fn backend(explicit: Option<&str>) -> String {
    let nonempty = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    nonempty(explicit.map(str::to_string)).or_else(|| nonempty(std::env::var("SECRETS_BACKEND").ok())).unwrap_or_else(|| if cfg!(target_os = "macos") { "macos".into() } else { "bitwarden".into() })
}

fn ensure_joined(hostname: &str, backend_flag: Option<&str>) -> Result<(bool, bool), Fail> {
    let missing: Vec<&str> = ["tailscale", "tailscaled"].into_iter().filter(|b| which(b).is_none()).collect();
    if !missing.is_empty() {
        return Err(Fail(format!("{} not found on PATH. Both are pinned in mise.toml and provisioned by `.claude/setup.sh` — run `pnpm run setup` to reprovision.", missing.join(" and "))));
    }
    let mut started = false;
    if classify(&status()) == State::DaemonDown {
        start_daemon();
        if !wait_for(|| classify(&status()) != State::DaemonDown) {
            return Err(Fail("tailscaled did not start accepting connections. Check its log for why it exited.".into()));
        }
        started = true;
    }
    // Re-read rather than reusing the pre-start result: a daemon restarted with an intact state dir comes back joined.
    if classify(&status()) == State::Joined {
        if status_online() == Some(false) {
            let _ = run("pkill", &["-x", "tailscaled"]);
            start_daemon();
            if !wait_for(|| status_online() == Some(true)) {
                return Err(Fail("tailscaled restarted but never reconnected to the coordination server -- check its log (/tmp/tailscaled.log) for why.".into()));
            }
            started = true;
        }
        return Ok((started, true));
    }
    let key = secrets::secret_from("TS_VM_AUTH_KEY", &backend(backend_flag)).ok().flatten().filter(|k| !k.is_empty());
    let Some(key) = key else {
        return Err(Fail("TS_VM_AUTH_KEY did not resolve from the vault, so there is no way to join. On a non-macOS host the default secrets backend is `macos`, which silently resolves nothing — pass `--backend bitwarden` or set SECRETS_BACKEND=bitwarden.".into()));
    };
    let r = run("tailscale", &["up", &format!("--authkey={key}"), &format!("--hostname={hostname}"), "--accept-dns=false", "--timeout=120s"]);
    if r.code != 0 {
        let stderr = r.stderr.trim();
        return Err(Fail(format!("tailscale up failed: {}", if stderr.is_empty() { "no stderr" } else { stderr })));
    }
    Ok((started, false))
}

pub fn up(args: &[String]) -> Option<i32> {
    let p = wf_kit::args::parse_none(args, &["hostname", "backend"], &[])?;
    let hostname = p.opts.get("hostname").map_or(DEFAULT_HOSTNAME, String::as_str);
    match ensure_joined(hostname, p.opts.get("backend").map(String::as_str)) {
        Ok((started, already)) => {
            if started {
                println!("started tailscaled (userspace, socks5 on localhost:{SOCKS_PORT})");
            }
            println!("{}", if already { format!("already on the tailnet as {hostname} — nothing to do") } else { format!("joined the tailnet as {hostname}") });
            println!("Reach tailnet services through the proxy, e.g.\n  curl --proxy socks5h://localhost:{SOCKS_PORT} http://vm-ops-api-main:8080/health");
            Some(0)
        }
        Err(Fail(m)) => {
            wf_kit::wf_err!("wf: {m}");
            Some(1)
        }
    }
}
