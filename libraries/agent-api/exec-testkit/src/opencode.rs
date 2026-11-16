//! A real `opencode serve` on a free loopback port, for the rca service's `POST /runs` to talk to (`OPENCODE_SERVER_URL`): it finds `opencode`
//! on the `PATH`, gets the provider key through its own environment only (never argv or a file) and a home of its own, and is stopped when dropped.
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND_ATTEMPTS: usize = 5;
const START_WAIT: Duration = Duration::from_secs(90);

pub struct OpencodeServer {
    child: Child,
    addr: SocketAddr,
    home: tempfile::TempDir,
}

/// The `serve` command: loopback only, the key and the config as environment variables of the child.
fn command(program: &Path, key: &str, config: &Path, home: &Path, port: u16) -> Command {
    let mut command = Command::new(program);
    command
        .args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
        .env("HOME", home)
        .env("OPENCODE_API_KEY", key)
        .env("OPENCODE_CONFIG", config)
        .env("npm_config_cache", std::env::temp_dir().join("exec-testkit-opencode-npm-cache"))
        .current_dir(home)
        .stdin(Stdio::null());
    command
}

impl OpencodeServer {
    /// Starts the `opencode` of the `PATH`; returns once the server answers `GET /app`.
    pub fn start(key: &str, config: &Path) -> OpencodeServer {
        OpencodeServer::start_program(Path::new("opencode"), key, config)
    }

    /// `start` with the program named, so a test starts a stand-in.
    pub fn start_program(program: &Path, key: &str, config: &Path) -> OpencodeServer {
        for _ in 0..BIND_ATTEMPTS {
            match OpencodeServer::try_start(program, key, config) {
                Ok(server) => return server,
                Err(log) if log.contains("EADDRINUSE") || log.contains("in use") => continue,
                Err(log) => panic!("{log}"),
            }
        }
        panic!("opencode serve could not bind a free port in {BIND_ATTEMPTS} attempts")
    }

    fn try_start(program: &Path, key: &str, config: &Path) -> Result<OpencodeServer, String> {
        let port = TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).expect("a free port").port();
        let home = tempfile::Builder::new().prefix("opencode-home-").tempdir().expect("a home for opencode");
        let log = std::fs::File::create(home.path().join("opencode.log")).expect("a log file");
        let child = command(program, key, config, home.path(), port).stdout(log.try_clone().expect("a log handle")).stderr(log).spawn().unwrap_or_else(|e| panic!("{} starts: {e}", program.display()));
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut server = OpencodeServer { child, addr, home };
        let deadline = Instant::now() + START_WAIT;
        loop {
            if let Ok(Some(status)) = server.child.try_wait() {
                return Err(format!("opencode serve ended with {status} before it answered:\n{}", server.logs()));
            }
            // any status counts: the server may sit behind a password
            if crate::service::http(addr, "GET", "/app", &[], "").is_ok() {
                return Ok(server);
            }
            if Instant::now() >= deadline {
                return Err(format!("opencode serve did not answer within {START_WAIT:?}:\n{}", server.logs()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// `http://127.0.0.1:<port>`, the value of the service's `OPENCODE_SERVER_URL`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn logs(&self) -> String {
        std::fs::read_to_string(self.home.path().join("opencode.log")).unwrap_or_default()
    }
}

impl Drop for OpencodeServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn given_a_key_and_a_config_when_started_with_a_stand_in_then_it_serves_app_on_loopback_and_the_key_is_only_in_the_childs_environment() {
        // Given a stand-in `opencode` that records its argv and environment, then serves on the port it was given
        let dir = tempfile::tempdir().unwrap();
        let stand_in = dir.path().join("opencode");
        let devserver = crate::devserver::binary();
        std::fs::write(&stand_in, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}/argv'\nenv > '{0}/env'\nPORT=\"$5\" exec '{1}'\n", dir.path().display(), devserver.display())).unwrap();
        std::fs::set_permissions(&stand_in, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = dir.path().join("server-config.json");
        // When it is started with a key and a config
        let server = OpencodeServer::start_program(&stand_in, "sk-test-not-a-secret", &config);
        // Then it answers on a loopback URL
        assert!(server.url().starts_with("http://127.0.0.1:"), "{}", server.url());
        // And the argv names the loopback host and the port, and not the key
        let argv = std::fs::read_to_string(dir.path().join("argv")).unwrap();
        assert!(argv.starts_with("serve\n--hostname\n127.0.0.1\n--port\n"), "{argv}");
        assert!(argv.trim_end().ends_with(&server.url().rsplit(':').next().unwrap().to_string()), "{argv}");
        assert!(!argv.contains("sk-test-not-a-secret"));
        // And the child's environment holds the key and the config
        let env = std::fs::read_to_string(dir.path().join("env")).unwrap();
        assert!(env.lines().any(|l| l == "OPENCODE_API_KEY=sk-test-not-a-secret"), "{env}");
        assert!(env.lines().any(|l| l == format!("OPENCODE_CONFIG={}", config.display())), "{env}");
        // And no file the server keeps holds the key
        assert!(!server.logs().contains("sk-test-not-a-secret"));
    }
}
