//! What a case runs against: a server started for it from the pinned release (`SpawnLaunch`) or one that is already running (`UrlLaunch`).
use crate::driver::{Launch, Server};
use crate::hermetic::{refuse_candidate_only_steps, write_cli_script, Host};
use mimic_script_core::Transport;
use mimic_script_net::TcpTransport;
use serde_json::Value;
use std::cell::RefCell;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The credential in what `t3 auth pairing create --json` printed: its first string among the names a pairing code goes by.
pub fn credential_of(stdout: &str) -> Result<String, String> {
    let doc: Value = serde_json::from_str(stdout).map_err(|e| format!("the pairing command printed no JSON: {e}"))?;
    ["credential", "token", "code", "secret"].iter().find_map(|key| doc[key].as_str().map(String::from)).ok_or_else(|| "the pairing command printed no credential".to_string())
}

/// `MIMIC_FIXED_PORT` pins the port of every server (one at a time), so a port never reaches an answer; unset, a free one is asked for.
fn free_port() -> Result<u16, String> {
    if let Some(port) = std::env::var("MIMIC_FIXED_PORT").ok().and_then(|p| p.parse().ok()) {
        return Ok(port);
    }
    TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| e.to_string())
}

static HOSTS: AtomicU64 = AtomicU64::new(0);

/// A fresh server of the pinned release per start: a new home and workspace each time, so a project one run made is not there for the next.
pub struct SpawnLaunch {
    /// The unpacked runtime, holding the `t3` binary.
    pub runtime: PathBuf,
    pub ready_ms: u64,
    /// Scripted agent CLIs that come before the stubs on the server's `PATH`.
    pub cli_dir: Option<PathBuf>,
}

struct Spawned {
    addr: SocketAddr,
    workspace: String,
    binary: PathBuf,
    env: Vec<(String, String)>,
    root: PathBuf,
    cli: Option<PathBuf>,
    child: RefCell<Option<Child>>,
}

impl Server for Spawned {
    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn workspace(&self) -> String {
        self.workspace.clone()
    }

    fn mint(&self) -> Result<String, String> {
        let out = Command::new(&self.binary).args(["auth", "pairing", "create", "--json"]).env_clear().envs(self.env.iter().cloned()).output().map_err(|e| format!("pairing create: {e}"))?;
        if !out.status.success() {
            return Err(format!("pairing create failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        credential_of(&String::from_utf8_lossy(&out.stdout))
    }

    fn script_cli(&self, cli: &str, turns: &Value) -> Result<(), String> {
        let dir = self.cli.as_ref().ok_or("this host was given no directory of scripted agent CLIs")?;
        refuse_candidate_only_steps(turns)?;
        write_cli_script(dir, cli, turns)
    }

    fn stop(&self) {
        self.log_cli_starts();
        if let Some(mut child) = self.child.borrow_mut().take() {
            // the server writes into its home until it is gone: removing it first fails
            let _ = Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Spawned {
    /// A probe aid: the arguments of every start of a scripted agent CLI, before the host's directory is removed with them.
    fn log_cli_starts(&self) {
        let Some(dir) = &self.cli else { return };
        for cli in ["claude", "codex", "opencode"] {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("{cli}.invocations.jsonl"))) else { continue };
            for line in text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
                eprintln!("level=info source=mimic msg=host.cli.invocations cli={cli} argv={}", line["argv"]);
            }
        }
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Polls the server's environment document until it answers, or the child exits, or the time is up.
fn ready(addr: SocketAddr, child: &mut Child, ms: u64) -> Result<(), String> {
    let until = Instant::now() + Duration::from_millis(ms);
    let mut transport = TcpTransport::new(addr);
    while Instant::now() < until {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("the server exited with {status} before it served"));
        }
        let call = mimic_script_core::HttpCall { method: "GET".into(), path: "/.well-known/t3/environment".into(), headers: vec![], body: None };
        if transport.http(&call).is_ok_and(|r| (200..300).contains(&r.status)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(format!("the server did not serve {addr} within {ms} ms"))
}

impl Launch for SpawnLaunch {
    fn start(&mut self) -> Result<std::rc::Rc<dyn Server>, String> {
        let root = std::env::temp_dir().join(format!("mimic-host-{}-{}", std::process::id(), HOSTS.fetch_add(1, Ordering::Relaxed)));
        let host = Host::prepare(&root, self.cli_dir.as_deref())?;
        let port = free_port()?;
        let binary = self.runtime.join("t3");
        let mut child = Command::new(&binary)
            .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string(), "--no-browser"])
            .arg(&host.workspace)
            .env_clear()
            .envs(host.env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", binary.display()))?;
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().map_err(|e| format!("{e}"))?;
        if let Err(e) = ready(addr, &mut child, self.ready_ms) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&root);
            return Err(e);
        }
        Ok(std::rc::Rc::new(Spawned { addr, workspace: host.workspace.display().to_string(), binary, env: host.env, root, cli: host.cli, child: RefCell::new(Some(child)) }))
    }
}

/// A server somebody else started (the candidate chain of a replay): its address, its workspace and a command that prints a credential.
pub struct UrlLaunch {
    pub addr: SocketAddr,
    pub workspace: String,
    /// Run for each credential; its output is the credential or the JSON of `credential_of`.
    pub mint: Vec<String>,
    /// The directory of scripted agent CLIs the server's `PATH` starts first, when the caller has one: a `cli.script` writes into it.
    pub cli_dir: Option<PathBuf>,
}

struct Remote {
    addr: SocketAddr,
    workspace: String,
    mint: Vec<String>,
    cli_dir: Option<PathBuf>,
}

impl Server for Remote {
    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn workspace(&self) -> String {
        self.workspace.clone()
    }

    fn mint(&self) -> Result<String, String> {
        let (program, args) = self.mint.split_first().ok_or("the target names no mint command")?;
        let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
        if !out.status.success() {
            return Err(format!("{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(credential_of(&text).unwrap_or_else(|_| text.trim().to_string()))
    }

    fn script_cli(&self, cli: &str, turns: &Value) -> Result<(), String> {
        let dir = self.cli_dir.as_ref().ok_or("this server was given no directory of scripted agent CLIs (`--cli-dir`)")?;
        write_cli_script(dir, cli, turns)
    }

    fn stop(&self) {}
}

impl Launch for UrlLaunch {
    fn start(&mut self) -> Result<std::rc::Rc<dyn Server>, String> {
        Ok(std::rc::Rc::new(Remote { addr: self.addr, workspace: self.workspace.clone(), mint: self.mint.clone(), cli_dir: self.cli_dir.clone() }))
    }
}

#[cfg(test)]
mod tests {
    use super::credential_of;

    #[test]
    fn given_pairing_output_when_read_then_the_first_credential_key_wins() {
        assert_eq!(credential_of(r#"{"token":"t","credential":"c"}"#).as_deref(), Ok("c"));
        assert_eq!(credential_of(r#"{"code":"x"}"#).as_deref(), Ok("x"));
        assert!(credential_of(r#"{"other":1}"#).is_err());
        assert!(credential_of("plain").is_err());
    }
}
