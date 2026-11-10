//! A throwaway checkout seeded from `fixture.json`, driven through `cli::run_in` with its root handed in (the double
//! for `git rev-parse`). The golden tree under `golden/` is what the TypeScript `sync` wrote for the same fixture.
use agent_config_sync::cli::{run_in, Outcome};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

pub fn here() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/flow")
}

pub struct Checkout {
    pub root: PathBuf,
}

impl Checkout {
    /// The fixture, with `overrides` written over it.
    pub fn new(overrides: &[(&str, &str)]) -> Checkout {
        let root = std::env::temp_dir().join(format!("acs-flow-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&root);
        let checkout = Checkout { root };
        let files: Value = serde_json::from_str(&fs::read_to_string(here().join("fixture.json")).unwrap()).unwrap();
        for (rel, content) in files.as_object().unwrap() {
            checkout.write(rel, content.as_str().unwrap());
        }
        for (rel, content) in overrides {
            checkout.write(rel, content);
        }
        fs::create_dir_all(checkout.root.join(".claude")).unwrap();
        checkout
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.path(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap()
    }

    /// The command line, as the binary and `wf agent-config sync` run it.
    pub fn run(&self, args: &[&str]) -> Outcome {
        self.run_with_stdin(args, "")
    }

    pub fn run_with_stdin(&self, args: &[&str], stdin: &str) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        let root = self.root.to_string_lossy().into_owned();
        run_in(&args, &mut stdin.as_bytes(), &|| root.clone())
    }

    /// The PreToolUse hook's answer to a call of `tool`: exit code and stdout.
    pub fn guard(&self, tool: &str) -> (i32, String) {
        let out = self.run_with_stdin(&["mcp-guard"], &format!("{{\"tool_name\":\"{tool}\",\"tool_input\":{{}}}}"));
        (out.code, out.stdout)
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn golden_paths() -> Vec<String> {
    fs::read_to_string(here().join("golden-paths.txt")).unwrap().lines().map(str::to_string).collect()
}

pub fn golden(path: &str, root: &Path) -> String {
    fs::read_to_string(here().join("golden").join(path)).unwrap().replace("{ROOT}", root.to_str().unwrap())
}
