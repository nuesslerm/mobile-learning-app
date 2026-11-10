//! The scripted CLI as a test uses it: one program installed under the names `claude`, `codex` and `opencode` in a directory of its own, a
//! script for each, and the log of how each was started. The directory goes first on the `PATH` of the process under test, which starts
//! the program it would start for the real CLI.
use crate::script::{log_file, script_file, Invocation, Script, PROGRAMS};
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

impl ScriptedCli {
    /// A directory of its own with the program under each name. A hard link, so the program knows its directory from where it was started; a
    /// copy where the directories are on different file systems.
    pub fn install() -> ScriptedCli {
        let dir = tempfile::Builder::new().prefix("scripted-cli-").tempdir().expect("a directory for the scripted CLIs");
        for program in PROGRAMS {
            let to = dir.path().join(program);
            if std::fs::hard_link(binary(), &to).is_err() {
                std::fs::copy(binary(), &to).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
            }
        }
        ScriptedCli { dir }
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
        let path = self.dir().join(script_file(program));
        std::fs::write(&path, serde_json::to_string_pretty(script).expect("a script is JSON")).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
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
