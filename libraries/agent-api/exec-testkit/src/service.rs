//! The real rca service as a test of what talks to it starts it: the `rca-service` binary, built from `backend/remote-chat-agent` once per
//! process (or the one `RCA_SERVICE_BIN` names), on a free port with a secret, a home directory of its own and a `PATH` that finds the
//! scripted CLIs before any other. It is the only way a mapper reaches an agent API, so what the mapper's trace says it did, this process did.
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Names an `rca-service` that is already built, so a suite builds it once for everyone (CI, or a developer's shell).
pub const ENV_BIN: &str = "RCA_SERVICE_BIN";

/// How long the service may take to answer `/health` after it starts.
const START_WAIT: Duration = Duration::from_secs(60);

/// `<target>/<profile>/deps/<test binary>` is where a test runs from, so the target directory is three levels up.
fn target_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return dir.into();
    }
    let exe = std::env::current_exe().expect("the test binary has a path");
    exe.ancestors().nth(3).expect("the test binary sits under a target directory").to_path_buf()
}

/// Builds `rca-service` from its own workspace into `<target>/rca-service-build`, a build directory of its own so the cargo that runs the tests
/// (which holds the lock of its own) is not waited for. A lock file keeps two test binaries that start together from building at once.
fn build() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../backend/remote-chat-agent/Cargo.toml");
    let build_dir = target_dir().join("rca-service-build");
    std::fs::create_dir_all(&build_dir).unwrap_or_else(|e| panic!("{}: {e}", build_dir.display()));
    let lock = std::fs::File::create(build_dir.join(".build.lock")).expect("a lock file");
    lock.lock().expect("the build lock");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo).args(["build", "--locked", "--quiet", "-p", "rca-service", "--bin", "rca-service", "--manifest-path"]).arg(&manifest).arg("--target-dir").arg(&build_dir).status().expect("cargo starts");
    assert!(status.success(), "building rca-service failed");
    build_dir.join("debug").join("rca-service")
}

/// The program `named` (the value of `RCA_SERVICE_BIN`) points at, or else the one `build` makes.
fn resolve(named: Option<OsString>, build: impl FnOnce() -> PathBuf) -> PathBuf {
    match named {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "{ENV_BIN}={} is no file", path.display());
            path
        }
        None => build(),
    }
}

fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| resolve(std::env::var_os(ENV_BIN), build))
}

/// A running `rca-service`, stopped when dropped; its output is in `logs`.
pub struct RcaService {
    child: Child,
    addr: SocketAddr,
    secret: String,
    home: tempfile::TempDir,
}

/// One HTTP exchange with `Connection: close`: the status and the JSON body (null when there is none).
fn http(addr: SocketAddr, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> std::io::Result<(u16, Value)> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    stream.write_all(format!("{request}\r\n{body}").as_bytes())?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok((status, serde_json::from_str(body).unwrap_or(Value::Null)))
}

impl RcaService {
    /// Starts the service with `secret` (the executor channel's and the operator's) and `path` as the `PATH` its CLIs are found on, and
    /// returns once `/health` answers. It knows nothing else of the environment: its home is a directory of its own.
    pub fn start(secret: &str, path: &OsString) -> RcaService {
        RcaService::start_with_env(secret, path, &[])
    }

    /// `start`, with the environment variables `env` set as well (such as the address of the model API the service writes commit messages with).
    pub fn start_with_env(secret: &str, path: &OsString, env: &[(&str, &str)]) -> RcaService {
        let port = TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).expect("a free port").port();
        let home = tempfile::Builder::new().prefix("rca-service-home-").tempdir().expect("a home for the service");
        let log = std::fs::File::create(home.path().join("service.log")).expect("a log file");
        let child = Command::new(binary())
            .env_clear()
            .env("PATH", path)
            .env("HOME", home.path())
            // the directory a CLI of a turn starts in: the service's own, as no workspace of the service is a model's to work in
            .env("RCA_EXEC_ROOT", home.path())
            .env("RCA_PROVIDERS_ROOT", home.path().join("providers"))
            .env("PORT", port.to_string())
            .env("CHAT_AGENT_API_SECRET", secret)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(log.try_clone().expect("a log handle"))
            .stderr(log)
            .spawn()
            .expect("rca-service starts");
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut service = RcaService { child, addr, secret: secret.to_string(), home };
        let deadline = Instant::now() + START_WAIT;
        loop {
            if let Ok(Some(status)) = service.child.try_wait() {
                panic!("rca-service ended with {status} before it served /health:\n{}", service.logs());
            }
            if http(addr, "GET", "/health", &[], "").is_ok_and(|(status, _)| status == 200) {
                return service;
            }
            assert!(Instant::now() < deadline, "rca-service did not serve /health within {START_WAIT:?}:\n{}", service.logs());
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// A device bearer minted as an operator mints one, with the service secret: its token.
    pub fn mint(&self, label: &str) -> String {
        let (status, body) = http(self.addr, "POST", "/agent/v1/bearers", &[("x-bot-secret", &self.secret), ("Content-Type", "application/json")], &json!({"label": label}).to_string()).expect("the service answers the mint");
        assert_eq!(status, 201, "the mint answered {status}: {body}\n{}", self.logs());
        body["token"].as_str().expect("the mint answers the token").to_string()
    }

    /// Everything the service has printed so far.
    pub fn logs(&self) -> String {
        std::fs::read_to_string(self.home.path().join("service.log")).unwrap_or_default()
    }

    /// The service's home directory, where its state lives and where a CLI it starts runs.
    pub fn home(&self) -> &Path {
        self.home.path()
    }
}

impl Drop for RcaService {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_prebuilt_service_named_or_not_when_resolved_then_the_named_one_is_used_without_a_build_and_none_builds() {
        // Given a file standing for a prebuilt service
        let dir = tempfile::tempdir().unwrap();
        let prebuilt = dir.path().join("rca-service");
        std::fs::write(&prebuilt, "").unwrap();
        // When RCA_SERVICE_BIN names it, Then it is the program and nothing is built
        assert_eq!(resolve(Some(prebuilt.clone().into_os_string()), || panic!("built though a program was named")), prebuilt);
        // And when it names nothing, the build's program is
        assert_eq!(resolve(None, || PathBuf::from("/built/rca-service")), PathBuf::from("/built/rca-service"));
        // And a name that is no file stops the run saying which variable
        let stopped = std::panic::catch_unwind(|| resolve(Some(dir.path().join("missing").into_os_string()), PathBuf::new)).unwrap_err();
        assert!(stopped.downcast_ref::<String>().is_some_and(|message| message.contains("RCA_SERVICE_BIN=") && message.contains("is no file")));
    }
}
