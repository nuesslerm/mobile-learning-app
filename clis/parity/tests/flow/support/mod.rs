//! The world of the flow tests: a fixture tree copied to a temp directory, its reference made a git checkout at the pin, and
//! `wf-parity` (or `WF_BIN`) run on it.
pub use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Run {
    pub code: i32,
    pub json: Value,
    pub stdout: String,
}

pub fn binary() -> PathBuf {
    std::env::var_os("WF_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_wf-parity")))
}

/// `parity coverage --root <root> <args>`.
pub fn coverage(root: &Path, args: &[&str]) -> Run {
    let out = Command::new(binary()).args(["parity", "coverage", "--root"]).arg(root).args(args).current_dir(root).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or("null");
    let json = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {stdout}\n{}", String::from_utf8_lossy(&out.stderr)));
    Run { code: out.status.code().unwrap_or(-1), json, stdout }
}

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub app: &'static str,
}

impl Fixture {
    /// `tests/fixtures/<app>`, with `references/<reference>` a checkout whose head the pin names.
    pub fn at_pin(app: &'static str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(app), dir.path());
        let f = Fixture { dir, app };
        let reference = f.reference();
        git(&reference, &["init", "-q"]);
        git(&reference, &["add", "-A"]);
        git(&reference, &["commit", "-qm", "the pinned reference"]);
        let head = git(&reference, &["rev-parse", "HEAD"]);
        let mut pin = f.json(f.pin_file());
        pin["commit"] = head.into();
        std::fs::write(f.root().join(f.pin_file()), serde_json::to_string_pretty(&pin).unwrap()).unwrap();
        f
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn reference(&self) -> PathBuf {
        self.root().join(if self.app == "t3" { "references/t3code" } else { "references/orca" })
    }

    pub fn pin_file(&self) -> &'static str {
        if self.app == "t3" {
            "tooling/t3-conformance/t3-pin.json"
        } else {
            "tooling/orca-conformance/orca-pin.json"
        }
    }

    pub fn table(&self, suffix: &str) -> String {
        format!("docs/plans/rca-parity/coverage/{}{suffix}", self.app)
    }

    pub fn json(&self, rel: &str) -> Value {
        serde_json::from_str(&self.text(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    pub fn text(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    pub fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.root().join(rel), text).unwrap();
    }

    pub fn run(&self, args: &[&str]) -> Run {
        let mut all = vec!["--app", self.app];
        all.extend_from_slice(args);
        coverage(self.root(), &all)
    }

    /// The rows of the methods list, by id.
    pub fn methods(&self) -> std::collections::BTreeMap<String, Value> {
        self.json(&self.table("-methods.json"))["rows"].as_array().unwrap().iter().map(|r| (r["id"].as_str().unwrap().to_string(), r.clone())).collect()
    }

    /// The rows of the coverage table, by id.
    pub fn coverage_rows(&self) -> std::collections::BTreeMap<String, Value> {
        self.json(&self.table(".json"))["rows"].as_array().unwrap().iter().map(|r| (r["id"].as_str().unwrap().to_string(), r.clone())).collect()
    }
}

/// The rules of the problems a run reported, with their rows.
pub fn problems(run: &Run) -> Vec<(String, Vec<String>)> {
    run.json["problems"]
        .as_array()
        .map(|a| a.iter().map(|p| (p["rule"].as_str().unwrap_or("").to_string(), p["rows"].as_array().map(|r| r.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default())).collect())
        .unwrap_or_default()
}
