//! The scripted CLI as a test uses it: one program installed under the names `claude`, `codex` and `opencode` in a directory of its own, a
//! script for each, and the log of how each was started. The directory goes first on the `PATH` of the process under test, which starts
//! the program it would start for the real CLI.
use crate::script::{app_server_log_file, claude_log_file, claude_turns_file, log_file, script_file, Invocation, Received, Script, PROGRAMS};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Names a program that is already built, so a suite builds it once for everyone (CI, or a developer's shell).
pub const ENV_CLI: &str = "EXEC_TESTKIT_CLI";

pub struct ScriptedCli {
    dir: tempfile::TempDir,
}

/// `<target>/<profile>/deps/<test binary>` is where a test runs from, so the target directory is three levels up.
fn target_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return dir.into();
    }
    let exe = std::env::current_exe().expect("the test binary has a path");
    exe.ancestors().nth(3).expect("the test binary sits under a target directory").to_path_buf()
}

/// Builds the program into `<target>/exec-testkit`, a build directory of its own so the cargo that runs the tests (which holds the lock of its
/// own) is not waited for. A lock file keeps two test binaries that start together from building at once.
pub(crate) fn build(bin: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let build_dir = target_dir().join("exec-testkit");
    std::fs::create_dir_all(&build_dir).unwrap_or_else(|e| panic!("{}: {e}", build_dir.display()));
    let lock = std::fs::File::create(build_dir.join(".build.lock")).expect("a lock file");
    lock.lock().expect("the build lock");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "--locked", "--quiet", "--bin", bin, "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&build_dir)
        .current_dir(root)
        .status()
        .expect("cargo starts");
    assert!(status.success(), "building {bin} failed");
    build_dir.join("debug").join(bin)
}

/// The program `named` (the value of `EXEC_TESTKIT_CLI`) points at, or else the one `build` makes.
fn resolve(named: Option<OsString>, build: impl FnOnce() -> PathBuf) -> PathBuf {
    match named {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "{ENV_CLI}={} is no file", path.display());
            path
        }
        None => build(),
    }
}

/// The program, built once per process (or the one `EXEC_TESTKIT_CLI` names).
fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| resolve(std::env::var_os(ENV_CLI), || build("scripted-cli")))
}

/// A directory of its own with `binary` under each of `programs`. A hard link, so the program knows its directory from where it was started: in the
/// temporary directory, or beside the binary where that is on another file system. A copy is the last resort, and a copy another test thread's fork
/// still holds open for writing cannot be executed (`ETXTBSY`).
pub(crate) fn program_dir(prefix: &str, binary: &Path, programs: &[&str]) -> tempfile::TempDir {
    let linked = |dir: tempfile::TempDir| programs.iter().all(|program| std::fs::hard_link(binary, dir.path().join(program)).is_ok()).then_some(dir);
    let fresh = |at: Option<&Path>| match at {
        Some(at) => tempfile::Builder::new().prefix(prefix).tempdir_in(at).ok(),
        None => Some(tempfile::Builder::new().prefix(prefix).tempdir().expect("a directory for the scripted programs")),
    };
    if let Some(dir) = fresh(None).and_then(linked) {
        return dir;
    }
    if let Some(dir) = fresh(binary.parent()).and_then(linked) {
        return dir;
    }
    let dir = fresh(None).expect("a directory");
    for program in programs {
        let to = dir.path().join(program);
        std::fs::copy(binary, &to).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    }
    dir
}

impl ScriptedCli {
    /// A directory of its own with the program under each name (`program_dir`).
    pub fn install() -> ScriptedCli {
        ScriptedCli { dir: program_dir("scripted-cli-", binary(), &PROGRAMS) }
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// A `PATH` that finds the scripted programs before any other.
    pub fn path(&self) -> OsString {
        let mut paths = vec![self.dir().to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        std::env::join_paths(paths).expect("a PATH")
    }

    /// What `program` does from its next start on, and from the start of its log: earlier starts are forgotten.
    pub fn script(&self, program: &str, script: &Script) {
        assert!(PROGRAMS.contains(&program), "{program} is not one of {PROGRAMS:?}");
        let _ = std::fs::remove_file(self.dir().join(log_file(program)));
        let _ = std::fs::remove_file(self.dir().join(app_server_log_file(program)));
        let _ = std::fs::remove_file(self.dir().join(format!("{program}.app-server.starts")));
        let _ = std::fs::remove_file(self.dir().join(claude_log_file(program)));
        let _ = std::fs::remove_file(self.dir().join(claude_turns_file(program)));
        for file in [crate::opencode_live::http_log_file(program), crate::opencode_live::password_file(program), format!("{program}.sessions.json"), format!("{program}.serve-turns")] {
            let _ = std::fs::remove_file(self.dir().join(file));
        }
        let path = self.dir().join(script_file(program));
        std::fs::write(&path, serde_json::to_string_pretty(script).expect("a script is JSON")).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }

    /// Every message the scripted app-server of `program` was sent since its script was set, oldest first, with the start it was sent to and that
    /// process's id: a process that went on across turns shows one id, a restart another.
    pub fn app_server_log(&self, program: &str) -> Vec<Received> {
        let path = self.dir().join(app_server_log_file(program));
        let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
        text.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {line}: {e}", path.display()))).collect()
    }

    /// Every message the live scripted `claude` was sent since its script was set, oldest first, with the start it was sent to and that process's id.
    pub fn claude_log(&self, program: &str) -> Vec<Received> {
        let path = self.dir().join(claude_log_file(program));
        let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
        text.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {line}: {e}", path.display()))).collect()
    }

    /// The text of every message of the person the live scripted `claude` was sent since its script was set, oldest first: what its turns were asked.
    pub fn claude_prompts(&self, program: &str) -> Vec<String> {
        self.claude_log(program).into_iter().filter(|r| r.message["type"] == "user").map(|r| r.message["message"]["content"][0]["text"].as_str().unwrap_or_default().to_string()).collect()
    }

    /// Every request the scripted `opencode serve` received since its script was set, oldest first, as `{start, method, path, body, authorized}`: the
    /// credential itself is never in it.
    pub fn opencode_requests(&self, program: &str) -> Vec<Value> {
        let path = self.dir().join(crate::opencode_live::http_log_file(program));
        let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
        text.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {line}: {e}", path.display()))).collect()
    }

    /// The password the last start of the scripted `opencode serve` was given: what must be found nowhere the service or a client can read.
    pub fn opencode_password(&self, program: &str) -> Option<String> {
        std::fs::read_to_string(self.dir().join(crate::opencode_live::password_file(program))).ok()
    }

    /// Every start of `program` since its script was set, oldest first.
    pub fn invocations(&self, program: &str) -> Vec<Invocation> {
        let path = self.dir().join(log_file(program));
        let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
        text.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {line}: {e}", path.display()))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_prebuilt_program_named_or_not_when_resolved_then_the_named_one_is_used_without_a_build_and_none_builds() {
        // Given a file standing for a prebuilt program
        let dir = tempfile::tempdir().unwrap();
        let prebuilt = dir.path().join("scripted-cli");
        std::fs::write(&prebuilt, "").unwrap();
        // When EXEC_TESTKIT_CLI names it, Then it is the program and nothing is built
        assert_eq!(resolve(Some(prebuilt.clone().into_os_string()), || panic!("built though a program was named")), prebuilt);
        // And when it names nothing, the build's program is
        assert_eq!(resolve(None, || PathBuf::from("/built/scripted-cli")), PathBuf::from("/built/scripted-cli"));
        // And a name that is no file stops the run saying which variable
        let missing = dir.path().join("missing");
        let stopped = std::panic::catch_unwind(|| resolve(Some(missing.into_os_string()), PathBuf::new)).unwrap_err();
        assert!(stopped.downcast_ref::<String>().is_some_and(|message| message.contains("EXEC_TESTKIT_CLI=") && message.contains("is no file")));
    }
}
