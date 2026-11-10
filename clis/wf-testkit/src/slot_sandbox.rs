use super::{run_wf, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;


/// A throwaway git repo (branch `main`), an isolated registry (`WF_REGISTRY_DIR`) and a directory of argv-recording stub tools, the
/// setting of the slot-scoped `wf android` suites. Port of the per-file `beforeEach` of the TypeScript suites.
pub struct SlotSandbox {
    _root: tempfile::TempDir,
    pub repo: PathBuf,
    pub reg_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl SlotSandbox {
    pub fn new(prefix: &str) -> Self {
        let root = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        let (repo, reg_dir, bin_dir) = (root.path().join("repo"), root.path().join("reg"), root.path().join("bin"));
        for dir in [&repo, &reg_dir, &bin_dir] {
            std::fs::create_dir(dir).unwrap();
        }
        assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(&repo).status().unwrap().success());
        assert!(Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]).current_dir(&repo).status().unwrap().success());
        let sandbox = Self { _root: root, repo, reg_dir, bin_dir };
        sandbox.seed(&json!({}));
        sandbox
    }

    pub fn seed(&self, slots: &Value) {
        let registry = json!({ "version": 3, "slots": slots, "devices": { "ios": {}, "android": {} } });
        std::fs::write(self.reg_dir.join("registry.json"), format!("{}\n", serde_json::to_string_pretty(&registry).unwrap())).unwrap();
    }

    /// An executable `#!/bin/sh` stub named `name` with the given body.
    pub fn stub(&self, name: &str, body: &str) {
        let path = self.bin_dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Writes a fixture file under the repo and returns its path as text.
    pub fn file(&self, rel: &str, contents: impl AsRef<[u8]>) -> String {
        let path = self.repo.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path.display().to_string()
    }

    /// The lines a stub appended to `<bin>/<name>.log`.
    pub fn calls(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.bin_dir.join(format!("{name}.log"))).map(|s| s.trim().lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
    }

    /// Forgets what a stub recorded so far (its `.log`, `.n` counter and `.stdin`), so the next step of a journey starts clean.
    pub fn forget(&self, name: &str) {
        for suffix in ["log", "n", "stdin"] {
            let _ = std::fs::remove_file(self.bin_dir.join(format!("{name}.{suffix}")));
        }
    }

    /// Runs `wf <args>` in the repo with the stubs first on PATH; `ts=<seconds>` at the start of a log line reads `ts=T`.
    pub fn run(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut opts = RunOpts::default().bin_dir(&self.bin_dir).cwd(&self.repo).env("WF_REGISTRY_DIR", &self.reg_dir.display().to_string()).env("ANDROID_SERIAL", "emulator-5554");
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        let ts = Regex::new(r"(?m)^ts=\d+").unwrap();
        let mut result = run_wf(args, &opts);
        result.stdout = ts.replace_all(&result.stdout, "ts=T").into_owned();
        result.stderr = ts.replace_all(&result.stderr, "ts=T").into_owned();
        result
    }
}
