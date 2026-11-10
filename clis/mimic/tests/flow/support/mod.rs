//! The world of the flow tests: a temp directory holding one cassette set (mimic.toml, pin, rules, corpus) whose reference is the toy
//! program `mimic-toy`, and `wf-mimic` (or `WF_BIN`, the full wf through its router) run on it.
pub use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Run {
    pub code: i32,
    pub json: Value,
    pub stderr: String,
}

pub fn toy() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mimic-toy"))
}

fn binary() -> PathBuf {
    std::env::var_os("WF_BIN").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_wf-mimic")))
}

/// `args` run as `<binary> mimic <args>` with no stdin, so a verb that prompted would hang the test; `wf` finds wf-mimic on its PATH.
pub fn mimic(args: &[&str], env: &[(&str, &Path)]) -> Run {
    let beside = Path::new(env!("CARGO_BIN_EXE_wf-mimic")).parent().expect("a directory").display().to_string();
    let path = format!("{beside}:{}", std::env::var("PATH").unwrap_or_default());
    let mut command = Command::new(binary());
    command.arg("mimic").args(args).env("PATH", path).env_remove("WF_TEST_OUTPUT_DIR").stdin(Stdio::null());
    // hermetic: a vault of the machine running the tests must never be reached (a publish would write to the real bucket)
    for (k, _) in std::env::vars_os().filter(|(k, _)| k.to_str().is_some_and(|k| k.starts_with("BWS_") || k.starts_with("VERIFICATION_ARTIFACTS_") || k == "SECRETS_BACKEND")) {
        command.env_remove(k);
    }
    for (k, v) in env {
        command.env(k, v);
    }
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or("null");
    let json = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {stdout}\n{}", String::from_utf8_lossy(&out.stderr)));
    Run { code: out.status.code().unwrap_or(-1), json, stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

pub const RULES: &str = r#"{"schema":"mimic-rules/1","rules":[
  {"kind":"uuid-ordinal"},
  {"kind":"timestamp","keys":[]},
  {"kind":"token","pattern":"[0-9a-f]{32}\\.[0-9a-f]{32}","placeholder":"<token>"},
  {"kind":"sequence","keys":["sequence"]}]}"#;

pub fn corpus(extra: &[Value]) -> Value {
    let mut generators = vec![
        json!({"kind": "corpus", "name": "echo", "cases": [{"id": "echo-1", "row": "rpc:echo", "input": {"a": "x", "b": 2}}]}),
        json!({"kind": "corpus", "name": "session", "op": "session", "cases": [{"id": "session-1", "row": "rpc:session", "input": {"user": "u"}}]}),
        json!({"kind": "corpus", "name": "reject", "op": "reject", "cases": [{"id": "reject-1", "input": {}}]}),
        json!({"kind": "seeded", "name": "drawn", "count": 3, "template": {"a": "", "n": 0},
               "fields": [{"pointer": "/a", "gen": "text", "min_len": 3, "max_len": 6}, {"pointer": "/n", "gen": "int", "min": 0, "max": 9}], "mutators": ["empty"]}),
    ];
    generators.extend(extra.iter().cloned());
    json!({"schema": "mimic-corpus/1", "seed": 7, "generators": generators})
}

pub struct World {
    pub dir: tempfile::TempDir,
    pub set: PathBuf,
}

impl World {
    pub fn new() -> Self {
        Self::with_corpus(&[])
    }

    pub fn with_corpus(extra: &[Value]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let set = dir.path().join("toy-set");
        std::fs::create_dir_all(&set).unwrap();
        let w = Self { dir, set };
        w.write("pin.json", r#"{"version":"toy-1.0"}"#);
        w.write("rules.json", RULES);
        w.write("corpus.json", &corpus(extra).to_string());
        let toy = toy().display().to_string();
        let candidate = |tweak: &str| format!("command = [{toy:?}, \"--tweak\", \"{tweak}\"]\n");
        w.write(
            "mimic.toml",
            &format!(
                "[reference]\ncommand = [{toy:?}]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n\n[candidate.same]\ncommand = [{toy:?}]\n\n\
                 [candidate.drop]\n{}\n[candidate.extra]\n{}\n[candidate.change]\n{}\n[candidate.missing]\ncommand = [\"/nonexistent/driver\"]\n",
                candidate("drop-key"),
                candidate("extra-key"),
                candidate("change-value")
            ),
        );
        w
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.set.join(rel)
    }

    pub fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.path(rel), text).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    pub fn arg(&self) -> String {
        self.set.display().to_string()
    }

    /// `mimic <verb> <set> <args>`.
    pub fn run(&self, verb: &str, args: &[&str]) -> Run {
        let set = self.arg();
        let mut all = vec![verb, set.as_str()];
        all.extend_from_slice(args);
        mimic(&all, &[])
    }

    pub fn record(&self) -> Run {
        let r = self.run("record", &[]);
        assert_eq!((r.code, r.json["ok"].clone()), (0, json!(true)), "record: {}\n{}", r.json, r.stderr);
        r
    }

    /// The interactions of the written cassette, by line.
    pub fn interactions(&self) -> Vec<Value> {
        self.read("interactions.jsonl").lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }
}
