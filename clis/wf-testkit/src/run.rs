use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The binary under test: `WF_BIN`, else the one cargo built for the package whose tests run (`CARGO_BIN_EXE_<package>`, which
/// cargo sets at run time as it does at compile time): `wf` for `wf`, the thin `wf-<name>` for each `wf-<name>` crate.
pub fn wf_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("WF_BIN") {
        return PathBuf::from(path);
    }
    let package = std::env::var("CARGO_PKG_NAME").expect("cargo sets CARGO_PKG_NAME for a test it runs");
    let binary = format!("CARGO_BIN_EXE_{package}");
    PathBuf::from(std::env::var_os(&binary).unwrap_or_else(|| panic!("{binary} is not set: run the tests with `cargo test -p {package} --test flow`, or set WF_BIN")))
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Default, Clone)]
pub struct RunOpts {
    /// Injected as `--identifier <slot>` after the arguments.
    pub slot: Option<String>,
    /// Extra environment variables, merged on top of the inherited ones.
    pub env: HashMap<String, String>,
    /// Directory of stub executables, put first on PATH.
    pub bin_dir: Option<PathBuf>,
    /// Working directory of the spawned process.
    pub cwd: Option<PathBuf>,
    /// Start from HOME and PATH only instead of the test process's environment.
    pub clean_env: bool,
    /// Text written to the child's stdin.
    pub input: Option<String>,
    /// Kill the child after this long (default 15 s).
    pub timeout: Option<Duration>,
}

impl RunOpts {
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn bin_dir(mut self, dir: &Path) -> Self {
        self.bin_dir = Some(dir.to_path_buf());
        self
    }

    pub fn cwd(mut self, dir: &Path) -> Self {
        self.cwd = Some(dir.to_path_buf());
        self
    }

    pub fn input(mut self, text: &str) -> Self {
        self.input = Some(text.to_string());
        self
    }

    pub fn timeout(mut self, limit: Duration) -> Self {
        self.timeout = Some(limit);
        self
    }

    pub fn clean_env(mut self) -> Self {
        self.clean_env = true;
        self
    }
}

/// The first `node` on the test process's own PATH.
pub fn node_path() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join("node")).find(|candidate| candidate.is_file())
}

/// `node -e <script>` for a helper whose stdout a test parses: Nx runs tasks with FORCE_COLOR set, and node then colours a
/// logged number.
pub fn node_script(script: &str) -> Command {
    let mut command = Command::new(node_path().expect("node on PATH"));
    command.arg("-e").arg(script).env_remove("FORCE_COLOR");
    command
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Keeps the credentials and the tailnet of the machine running the tests away from `wf`: no real vault, VM, cloud or ingest is
/// reachable unless a test hands it a fake.
fn isolate(command: &mut Command) {
    static REGISTERED: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let registered = REGISTERED.get_or_init(|| {
        let text = std::fs::read_to_string(super::repo_root().join("clis/secrets/src/data/registry.json")).unwrap_or_default();
        let entries: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
        entries.values().filter_map(|e| e["envVar"].as_str().map(String::from)).collect()
    });
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        // the outer `wf test run` hands its Nx the build-cache shim: a fixture's own Nx must not replay from the real bucket
        if ["BWS_", "AWS_", "CLOUDSDK_", "NX_SELF_HOSTED_"].iter().any(|p| name.starts_with(p)) || ["GH_TOKEN", "GITHUB_TOKEN"].contains(&&*name) || registered.iter().any(|r| *r == name) {
            command.env_remove(&key);
        }
    }
    // the files the Actions runner hands a step (its summary page, outputs, env, path, state): a test's `wf` appending to them wrote
    // fixture reports into the real run's summary
    for channel in ["GITHUB_STEP_SUMMARY", "GITHUB_OUTPUT", "GITHUB_ENV", "GITHUB_PATH", "GITHUB_STATE"] {
        command.env_remove(channel);
    }
    // a `--fresh` outer Nx hands its tasks NX_SKIP_NX_CACHE=true: a fixture's own Nx would never replay under it
    for skip in ["NX_SKIP_NX_CACHE", "NX_DISABLE_NX_CACHE"] {
        command.env_remove(skip);
    }
    // CI's e2e job exports a speed; only a test that sets one runs faster than a user
    command.env_remove("WF_TEST_SPEED");
    command.env("WF_TAILNET_SOCKS", "127.0.0.1:1").env("SECRETS_BACKEND", "macos").env("SECRETS_LOG_BETTERSTACK", "false");
    // the developer's ~/.config/wf/.env holds real keys; a test that wants one gives wf its own HOME
    let real = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    command.env("CARGO_HOME", std::env::var_os("CARGO_HOME").map_or_else(|| real.join(".cargo"), PathBuf::from));
    command.env("RUSTUP_HOME", std::env::var_os("RUSTUP_HOME").map_or_else(|| real.join(".rustup"), PathBuf::from));
    let cache = real.join(".cache/ms-playwright");
    if std::env::var_os("PLAYWRIGHT_BROWSERS_PATH").is_none() && cache.is_dir() {
        command.env("PLAYWRIGHT_BROWSERS_PATH", cache);
    }
    command.env("HOME", test_home());
}

/// One empty home for every `wf` this test process starts.
fn test_home() -> &'static Path {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::Builder::new().prefix("wf-flow-home-").tempdir().expect("a temp home").keep())
}

/// With `WF_TEST_ARGV_LOG` naming a file, one line per spawn (test thread, start ms, end ms, first three argv words), written in
/// a single append when the run ends or the `Spawned` is dropped, so parallel tests never interleave.
struct ArgvLog {
    file: PathBuf,
    thread: String,
    start: i128,
    words: String,
}

impl ArgvLog {
    fn begin(args: &[&str]) -> Option<ArgvLog> {
        let file = PathBuf::from(std::env::var_os("WF_TEST_ARGV_LOG")?);
        let thread = std::thread::current().name().unwrap_or("-").to_string();
        Some(ArgvLog { file, thread, start: super::now_ms(), words: args.iter().take(3).map(|w| w.replace(['\t', '\n', '\r'], " ")).collect::<Vec<_>>().join(" ") })
    }
}

impl Drop for ArgvLog {
    fn drop(&mut self) {
        use std::io::Write;
        let line = format!("{}\t{}\t{}\t{}\n", self.thread, self.start, super::now_ms(), self.words);
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open(&self.file) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// Runs the `wf` binary once and waits for it, killing it after the timeout.
pub fn run_wf(args: &[&str], opts: &RunOpts) -> RunResult {
    let _log = ArgvLog::begin(args);
    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let path = match &opts.bin_dir {
        Some(dir) => format!("{}:{inherited_path}", dir.display()),
        None => inherited_path,
    };
    let mut command = Command::new(wf_binary());
    command.args(args);
    if let Some(slot) = &opts.slot {
        command.args(["--identifier", slot]);
    }
    if opts.clean_env {
        command.env_clear().env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    }
    isolate(&mut command);
    // A stubbed `node` on PATH must not run the legacy bundle behind a Rust wf: WF_NODE names the real one.
    command.env("PATH", path);
    if let Some(node) = node_path() {
        command.env("WF_NODE", node);
    }
    // The vault copy is off unless a test opts in, so no test reads or writes the real one and each sees its own fixture;
    // likewise no run posts a test-metrics row unless the test names a stub ORDS.
    command.env("WF_SECRETS_CACHE", "off").env("TEST_RUNS_ORDS_BASE_URL", "").env("WF_USAGE", "off").envs(&opts.env);
    if let Some(cwd) = &opts.cwd {
        command.current_dir(cwd);
    }
    command.stdin(if opts.input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap_or_else(|e| panic!("cannot start {}: {e}", wf_binary().display()));
    if let (Some(input), Some(mut stdin)) = (&opts.input, child.stdin.take()) {
        use std::io::Write;
        let _ = stdin.write_all(input.as_bytes());
    }
    let (stdout, stderr) = (drain(child.stdout.take().expect("piped")), drain(child.stderr.take().expect("piped")));
    let deadline = Instant::now() + opts.timeout.unwrap_or(Duration::from_secs(15));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(_) => break None,
        }
    };
    RunResult { stdout: stdout.join().unwrap_or_default(), stderr: stderr.join().unwrap_or_default(), exit_code: status.and_then(|s| s.code()).unwrap_or(1) }
}

/// A `wf` started and left running: its output (both streams) accumulates, and the test signals or waits for it.
pub struct Spawned {
    child: std::process::Child,
    out: std::sync::Arc<std::sync::Mutex<String>>,
    readers: Vec<std::thread::JoinHandle<()>>,
    _log: Option<ArgvLog>,
}

pub fn spawn_wf(args: &[&str], opts: &RunOpts) -> Spawned {
    spawn_bin(&wf_binary(), args, opts)
}

pub fn spawn_bin(bin: &Path, args: &[&str], opts: &RunOpts) -> Spawned {
    let log = ArgvLog::begin(args);
    let inherited_path = std::env::var("PATH").unwrap_or_default();
    let path = match &opts.bin_dir {
        Some(dir) => format!("{}:{inherited_path}", dir.display()),
        None => inherited_path,
    };
    let mut command = Command::new(bin);
    command.args(args).env_clear().env("PATH", path).env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    isolate(&mut command);
    if let Some(node) = node_path() {
        command.env("WF_NODE", node);
    }
    command.env("WF_SECRETS_CACHE", "off").env("TEST_RUNS_ORDS_BASE_URL", "").env("WF_USAGE", "off").envs(&opts.env);
    if let Some(cwd) = &opts.cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap_or_else(|e| panic!("cannot start {}: {e}", bin.display()));
    let out = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut readers = Vec::new();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().flatten() {
        let out = out.clone();
        readers.push(std::thread::spawn(move || {
            let mut pipe = pipe;
            let mut buf = [0u8; 1024];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        }));
    }
    Spawned { child, out, readers, _log: log }
}

impl Spawned {
    /// Everything written so far.
    pub fn out(&self) -> String {
        self.out.lock().unwrap().clone()
    }

    pub fn signal(&self, name: &str) {
        let _ = Command::new("kill").args([&format!("-{name}"), &self.child.id().to_string()]).status();
    }

    /// The exit code once the process ends (waits up to `within`); `None` if it is still running.
    pub fn wait(&mut self, within: Duration) -> Option<i32> {
        let deadline = Instant::now() + within;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    // what it wrote just before it ended may still be in the pipes (a loaded CI runner showed an empty error)
                    let drained = Instant::now() + Duration::from_secs(2);
                    while self.readers.iter().any(|r| !r.is_finished()) && Instant::now() < drained {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    return Some(status.code().unwrap_or(-1));
                }
                Ok(None) if Instant::now() >= deadline => return None,
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => return None,
            }
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Polls `pred` until it holds; panics after `within`.
pub fn wait_until(within: Duration, what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !pred() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_an_outer_fresh_nx_run_when_wf_is_spawned_then_the_nx_skip_switches_are_dropped() {
        // Given a test process an outer `--fresh` Nx run left NX_SKIP_NX_CACHE=true in
        std::env::set_var("NX_SKIP_NX_CACHE", "true");
        // When a command is isolated
        let mut command = Command::new("true");
        isolate(&mut command);
        std::env::remove_var("NX_SKIP_NX_CACHE");
        // Then the child inherits neither skip switch
        let dropped: Vec<_> = command.get_envs().filter(|(_, v)| v.is_none()).map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        assert!(dropped.contains(&"NX_SKIP_NX_CACHE".to_string()) && dropped.contains(&"NX_DISABLE_NX_CACHE".to_string()), "{dropped:?}");
    }
}
