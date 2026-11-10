//! For each service's integration test: the service's built binary run as a black box. It listens on a free loopback port, works in a
//! temp dir of its own and sees only the environment the test gives it (no real secret, no telemetry endpoint), so every
//! dependency is whatever the test points it at: a fake server, a file in the temp dir, nothing. The process dies with the test.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const READY_WITHIN: Duration = Duration::from_secs(20);

pub struct Running {
    child: Child,
    dir: tempfile::TempDir,
    pub base: String,
    pub client: reqwest::Client,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).expect("no free loopback port")
}

impl Running {
    /// Starts `binary` with `env` (and `port_env` set to a free port), and waits until `ready_path` gets any HTTP answer (a listener that
    /// serves no health route still answers its 404).
    pub async fn start(binary: &str, port_env: &str, env: &[(&str, String)], ready_path: &str) -> Running {
        let dir = tempfile::tempdir().expect("no temp dir");
        let port = free_port();
        let (out, err) = (dir.path().join("stdout.log"), dir.path().join("stderr.log"));
        let mut command = Command::new(binary);
        command.env_clear().env("PATH", "/usr/bin:/bin").env("HOME", dir.path()).env(port_env, port.to_string()).current_dir(dir.path());
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command.stdout(Stdio::from(std::fs::File::create(&out).expect("no log file"))).stderr(Stdio::from(std::fs::File::create(&err).expect("no log file"))).spawn().unwrap_or_else(|e| panic!("cannot start {binary}: {e}"));
        let mut running = Running { child, dir, base: format!("http://127.0.0.1:{port}"), client: reqwest::Client::new() };
        let started = Instant::now();
        loop {
            if let Some(status) = running.child.try_wait().expect("cannot poll the service") {
                panic!("{binary} exited ({status}) before {ready_path} answered:\n{}", running.logs());
            }
            if running.client.get(format!("{}{ready_path}", running.base)).send().await.is_ok() {
                return running;
            }
            assert!(started.elapsed() < READY_WITHIN, "{binary} did not answer {ready_path} within {}s:\n{}", READY_WITHIN.as_secs(), running.logs());
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The service's temp dir: what a test writes there is what the service reads as its files.
    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// Its stdout and stderr so far, for a failure message.
    pub fn logs(&self) -> String {
        let read = |name: &str| std::fs::read_to_string(self.dir.path().join(name)).unwrap_or_default();
        format!("--- stdout\n{}\n--- stderr\n{}", read("stdout.log"), read("stderr.log"))
    }

    /// A GET: the status and the JSON body (null when it has none).
    pub async fn get(&self, path: &str) -> (u16, serde_json::Value) {
        let response = self.client.get(format!("{}{path}", self.base)).send().await.unwrap_or_else(|e| panic!("GET {path}: {e}\n{}", self.logs()));
        (response.status().as_u16(), response.json().await.unwrap_or(serde_json::Value::Null))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
