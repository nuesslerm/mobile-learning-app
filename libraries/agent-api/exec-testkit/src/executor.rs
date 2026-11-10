//! The real executor as a test of what talks to it starts it: the `rca-exec` binary, built from `clis/` once per process, serving a git
//! repository and attached to an rca service the way an operator attaches one (`rca-exec serve`). It is the only way a flow reaches the
//! workspace, so a file that exists in the repository was written by this process.
use crate::repo::TempRepo;
use serde_json::Value;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

/// Names an `rca-exec` that is already built, so a suite builds it once for everyone (CI, or a developer's shell).
pub const ENV_BIN: &str = "RCA_EXEC_BIN";

/// The variable the bearer is given in; `rca-exec serve` reads it by the name `--secret-env` gives.
const BEARER_ENV: &str = "RCA_EXEC_TESTKIT_BEARER";

/// How long the executor may take to attach.
const ATTACH_WAIT: Duration = Duration::from_secs(20);

/// `<target>/<profile>/deps/<test binary>` is where a test runs from, so the target directory is three levels up.
fn target_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return dir.into();
    }
    let exe = std::env::current_exe().expect("the test binary has a path");
    exe.ancestors().nth(3).expect("the test binary sits under a target directory").to_path_buf()
}

/// Builds `rca-exec` from the `clis` workspace into `<target>/rca-exec-build`, a build directory of its own so the cargo that runs the tests
/// (which holds the lock of its own) is not waited for. A lock file keeps two test binaries that start together from building at once.
fn build() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../clis/Cargo.toml");
    let build_dir = target_dir().join("rca-exec-build");
    std::fs::create_dir_all(&build_dir).unwrap_or_else(|e| panic!("{}: {e}", build_dir.display()));
    let lock = std::fs::File::create(build_dir.join(".build.lock")).expect("a lock file");
    lock.lock().expect("the build lock");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo).args(["build", "--locked", "--quiet", "-p", "rca-exec", "--bin", "rca-exec", "--manifest-path"]).arg(&manifest).arg("--target-dir").arg(&build_dir).status().expect("cargo starts");
    assert!(status.success(), "building rca-exec failed");
    build_dir.join("debug").join("rca-exec")
}

/// The program `named` (the value of `RCA_EXEC_BIN`) points at, or else the one `build` makes.
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

/// A running `rca-exec serve`, stopped when dropped.
pub struct RcaExec {
    child: Child,
    lines: mpsc::Receiver<Value>,
}

impl RcaExec {
    /// Starts the executor for `workspace` on `repo`, attached to the rca service at `rca_url` (`http://host:port`) with `bearer`, and returns
    /// once it says it is attached. It runs git as the repository's tests do (no configuration of the machine) and knows nothing else of the
    /// environment but `PATH`.
    pub fn serve(rca_url: &str, workspace: &str, repo: &TempRepo, bearer: &str) -> RcaExec {
        RcaExec::serve_with(rca_url, workspace, repo, bearer, &[])
    }

    /// Like `serve`, with more flags for `rca-exec serve` (`--worktree-base <dir>`, `--browse-allow <dirs>`, `--executor <id>`).
    pub fn serve_with(rca_url: &str, workspace: &str, repo: &TempRepo, bearer: &str, flags: &[&str]) -> RcaExec {
        RcaExec::serve_on(rca_url, workspace, repo, bearer, flags, &std::env::var_os("PATH").unwrap_or_default())
    }

    /// Like `serve_with`, with the executor's `PATH` set (the fake `gh` and `glab` of `HostedClis::path`).
    pub fn serve_on(rca_url: &str, workspace: &str, repo: &TempRepo, bearer: &str, flags: &[&str], path: &std::ffi::OsStr) -> RcaExec {
        RcaExec::serve_with_home(rca_url, workspace, repo, bearer, flags, path, None)
    }

    /// Like `serve_on`, with the executor's `HOME` set to `home` (whose skills directories `skill_scan` then reads).
    pub fn serve_with_home(rca_url: &str, workspace: &str, repo: &TempRepo, bearer: &str, flags: &[&str], path: &std::ffi::OsStr, home: Option<&std::path::Path>) -> RcaExec {
        let mut child = Command::new(binary())
            .args(["serve", "--rca-url", rca_url, "--workspace", workspace, "--root"])
            .arg(repo.path())
            .args(["--secret-env", BEARER_ENV])
            .args(flags)
            .env_clear()
            .envs(repo.git_env())
            .env("PATH", path)
            .envs(home.map(|h| ("HOME", h)))
            .env(BEARER_ENV, bearer)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("rca-exec starts");
        let stdout = child.stdout.take().expect("rca-exec has a stdout");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if tx.send(value).is_err() {
                        return;
                    }
                }
            }
        });
        let executor = RcaExec { child, lines };
        let first = executor.lines.recv_timeout(ATTACH_WAIT).unwrap_or_else(|e| panic!("rca-exec did not say it attached to {rca_url}: {e}"));
        assert_eq!((first["ok"].clone(), first["event"].clone()), (Value::Bool(true), Value::String("attached".into())), "rca-exec did not attach to {rca_url}: {first}");
        executor
    }

    /// Whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for RcaExec {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_prebuilt_executor_named_or_not_when_resolved_then_the_named_one_is_used_without_a_build_and_none_builds() {
        // Given a file standing for a prebuilt executor
        let dir = tempfile::tempdir().unwrap();
        let prebuilt = dir.path().join("rca-exec");
        std::fs::write(&prebuilt, "").unwrap();
        // When RCA_EXEC_BIN names it, Then it is the program and nothing is built
        assert_eq!(resolve(Some(prebuilt.clone().into_os_string()), || panic!("built though a program was named")), prebuilt);
        // And when it names nothing, the build's program is
        assert_eq!(resolve(None, || PathBuf::from("/built/rca-exec")), PathBuf::from("/built/rca-exec"));
        // And a name that is no file stops the run saying which variable
        let stopped = std::panic::catch_unwind(|| resolve(Some(dir.path().join("missing").into_os_string()), PathBuf::new)).unwrap_err();
        assert!(stopped.downcast_ref::<String>().is_some_and(|message| message.contains("RCA_EXEC_BIN=") && message.contains("is no file")));
    }
}
