//! The replay double of `gh` and `glab`: one program (`fake-hosted`) installed under both names in a directory of its own, the rules it
//! answers by (an argv, and what the real CLI prints for it) and the log of how it was started. The directory goes first on the `PATH`
//! of the executor under test. Nothing here reads a token or reaches a network.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const PROGRAMS: [&str; 2] = ["gh", "glab"];

/// Names a built `fake-hosted` so a suite builds it once for everyone.
pub const ENV_HOSTED: &str = "EXEC_TESTKIT_HOSTED";

/// Output of `times` copies of `text`, to stdout, after the rule's own output: what a CLI does with a huge diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flood {
    pub text: String,
    pub times: u64,
}

/// What the program prints and does when it is started with exactly `args`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub program: String,
    pub args: Vec<String>,
    /// The rule answers every start whose arguments begin with `args`, the first rule in order that does (a chain test that checks what the
    /// start was given from its log, not the argv the plan builds).
    #[serde(default)]
    pub prefix: bool,
    /// The text the start must be given on stdin; a start with other text matches no rule.
    #[serde(default)]
    pub stdin: Option<String>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub exit_code: i32,
    /// Waits this long after printing, before it exits.
    #[serde(default)]
    pub sleep_ms: u64,
    #[serde(default)]
    pub flood: Option<Flood>,
    /// Stays alive after printing until it is killed.
    #[serde(default)]
    pub hang: bool,
    /// A directory created before anything is printed, as `repo clone` makes its destination as it starts.
    #[serde(default)]
    pub mkdir: Option<String>,
}

/// One start of the program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Start {
    pub program: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub stdin: String,
    /// The environment variables the rules file names in `log_env`, none for one that was not set.
    pub env: std::collections::BTreeMap<String, Option<String>>,
    /// Whether a rule answered it.
    pub matched: bool,
}

/// The file in the program's directory that holds the rules of `program`.
pub fn rules_file(program: &str) -> String {
    format!("{program}.rules.json")
}

pub fn log_file(program: &str) -> String {
    format!("{program}.starts.jsonl")
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Rules {
    pub rules: Vec<Rule>,
    /// Environment variables whose values the log keeps.
    #[serde(default)]
    pub log_env: Vec<String>,
}

fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| match std::env::var_os(ENV_HOSTED) {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "{ENV_HOSTED}={} is no file", path.display());
            path
        }
        None => crate::cli::build("fake-hosted"),
    })
}

pub struct HostedClis {
    dir: tempfile::TempDir,
}

impl HostedClis {
    /// A directory of its own with the program under `gh` and `glab` (hard links, so it knows its directory from where it was started).
    pub fn install() -> HostedClis {
        let dir = tempfile::Builder::new().prefix("fake-hosted-").tempdir().expect("a directory for the fake gh and glab");
        for program in PROGRAMS {
            let to = dir.path().join(program);
            if std::fs::hard_link(binary(), &to).is_err() {
                std::fs::copy(binary(), &to).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
            }
        }
        HostedClis { dir }
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// A `PATH` that finds the fake programs before any other.
    pub fn path(&self) -> OsString {
        let mut paths = vec![self.dir().to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        std::env::join_paths(paths).expect("a PATH")
    }

    /// What each program answers from its next start on, and from the start of its log: earlier starts are forgotten. Every program gets a
    /// rules file, so one with no rule answers every start as unmatched.
    pub fn rules(&self, rules: &[Rule], log_env: &[&str]) {
        for program in PROGRAMS {
            let _ = std::fs::remove_file(self.dir().join(log_file(program)));
            let mine = Rules { rules: rules.iter().filter(|r| r.program == program).cloned().collect(), log_env: log_env.iter().map(|s| s.to_string()).collect() };
            let path = self.dir().join(rules_file(program));
            std::fs::write(&path, serde_json::to_string_pretty(&mine).expect("rules are JSON")).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }

    /// The rules a fixture lists, as JSON objects of `Rule`.
    pub fn replay(&self, rules: &[Value], log_env: &[&str]) {
        let rules: Vec<Rule> = rules.iter().map(|r| serde_json::from_value(r.clone()).unwrap_or_else(|e| panic!("{r}: {e}"))).collect();
        self.rules(&rules, log_env);
    }

    /// The rules of a cassette interaction (its `call`, `stdin`, `stdout`, `stderr` and `exit` steps), as `rules` sets them.
    pub fn play(&self, interaction: &crate::cassette::Interaction, log_env: &[&str]) {
        self.rules(&interaction.rules(), log_env);
    }

    /// Every start of `program` since its rules were set, oldest first.
    pub fn starts(&self, program: &str) -> Vec<Start> {
        let path = self.dir().join(log_file(program));
        let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
        text.lines().map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {line}: {e}", path.display()))).collect()
    }

    /// Every start of either program, in the order of their logs (`gh` first): `[program, args...]` each.
    pub fn argvs(&self) -> Vec<Vec<String>> {
        PROGRAMS.iter().flat_map(|p| self.starts(p).into_iter().map(|s| std::iter::once(s.program).chain(s.argv).collect())).collect()
    }
}
