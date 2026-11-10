//! Where a job, a ledger, an artifact and a credential live on agent-shell's disk, which names and paths a request may use, and how
//! a job's status text reads. A request-echoed name reaches a shell, so every check is an allow-list.
use crate::domain::js::{self, JsString};

/// A job that runs longer than this is TERMed by its own wrapper.
pub const EXEC_SCRIPT_BACKSTOP_SEC: u32 = 1800;
pub const JOBS_ROOT: &str = "/tmp/exec-jobs";
pub const EXIT_MARKER: &str = "\n---EXITCODE---\n";
/// How much of a job's output a status keeps, in UTF-16 units.
pub const STATUS_OUTPUT_UNITS: usize = 4000;
/// The clean-up every start runs first: job dirs older than a day.
pub const JOBS_CLEANUP: &str = "find /tmp/exec-jobs -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {} + 2>/dev/null || true";
/// A job's status: its log, the marker, then the exit code once the wrapper wrote one.
pub const JOB_STATUS_SCRIPT: &str = r#"cat "$1/output.log" 2>/dev/null; printf "\n---EXITCODE---\n"; cat "$1/exit-code" 2>/dev/null"#;

const LEDGER_ROOT: &str = "/workspace/vm-ledger";
const ARTIFACT_ROOT: &str = "/home/agent/e2e-artifacts";

/// agent-shell's `agent` user (uid 1001) has its runtime HOME on the /workspace volume, not /home/agent.
pub const AGENT_SHELL_UID: u32 = 1001;
pub const AGENT_SHELL_HOME: &str = "/workspace/home";

/// Each credential tool and where its file goes under [`AGENT_SHELL_HOME`], in the order `knownTools` lists them.
pub const CREDENTIAL_PATHS: [(&str, &str); 5] = [
    ("codex", ".codex/auth.json"),
    ("claude", ".claude/.credentials.json"),
    ("gcloud", ".config/gcloud/credentials.db"),
    ("gemini", ".gemini/oauth_creds.json"),
    ("opencode", ".opencode/data/auth.json"),
];

pub fn known_tools() -> Vec<&'static str> {
    CREDENTIAL_PATHS.iter().map(|(tool, _)| *tool).collect()
}

/// The file of a tool, relative to [`AGENT_SHELL_HOME`]; none for any other name, whatever every object inherits.
pub fn credential_path(tool: &str) -> Option<&'static str> {
    CREDENTIAL_PATHS.iter().find(|(known, _)| *known == tool).map(|(_, path)| *path)
}

pub fn job_dir(job_id: &str) -> String {
    format!("{JOBS_ROOT}/{job_id}")
}

/// The content type of a ledger the service may cat, or none for any other name.
pub fn ledger_content_type(name: &str) -> Option<&'static str> {
    match name {
        "opencode-dispatch-runs.jsonl" => Some("text/plain; charset=utf-8"),
        "pool-cooldowns.json" => Some("application/json; charset=utf-8"),
        _ => None,
    }
}

pub fn ledger_file(name: &str) -> String {
    format!("{LEDGER_ROOT}/{name}")
}

/// `^[A-Za-z0-9_\-./]+$` with no `..` in it.
pub fn is_valid_artifact_path(path: &str) -> bool {
    !path.is_empty() && !path.contains("..") && path.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/'))
}

/// Prints the file's base64, or `NOT_ALLOWED` for a path that resolves outside the artifact dir and `NOT_FOUND` for no file.
pub fn artifact_guard_script(relative_path: &str) -> String {
    [
        format!("F=\"{ARTIFACT_ROOT}/{relative_path}\""),
        "REAL=$(readlink -f \"$F\" 2>/dev/null)".to_string(),
        format!("case \"$REAL\" in {ARTIFACT_ROOT}/*) ;; *) echo NOT_ALLOWED; exit 1;; esac"),
        "[ -f \"$F\" ] || { echo NOT_FOUND; exit 1; }".to_string(),
        "base64 -w0 \"$F\"".to_string(),
    ]
    .join("\n")
}

/// What a status exec printed, read.
#[derive(Debug, Clone, PartialEq)]
pub struct JobStatus {
    /// No exit code yet: the job is running, or has no files, which reads the same.
    pub running: bool,
    /// `Number.parseInt` of the code text: NaN when the text is no number.
    pub exit_code: f64,
    pub output: JsString,
}

impl JobStatus {
    /// Splits at the last marker; text with no marker is all output of a job that is still running.
    pub fn read(raw: &str) -> Self {
        let (output, code_text) = match raw.rfind(EXIT_MARKER) {
            Some(at) => (&raw[..at], raw[at + EXIT_MARKER.len()..].trim_matches(js::is_space)),
            None => (raw, ""),
        };
        let running = code_text.is_empty();
        let exit_code = if running { f64::NAN } else { js::parse_int(code_text) };
        Self { running, exit_code, output: JsString::from(output).last(STATUS_OUTPUT_UNITS) }
    }

    /// `null` while running and when the code is no number.
    pub fn exit_code_json(&self) -> String {
        js::json_number(self.exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_known_tools_when_looked_up_then_only_those_five_have_a_path_and_object_names_have_none() {
        assert_eq!(known_tools(), ["codex", "claude", "gcloud", "gemini", "opencode"]);
        assert_eq!(credential_path("opencode"), Some(".opencode/data/auth.json"));
        for name in ["toString", "constructor", "__proto__", "hasOwnProperty", "", "Claude"] {
            assert_eq!(credential_path(name), None, "{name}");
        }
    }

    #[test]
    fn given_artifact_paths_when_validated_then_safe_characters_pass_and_a_double_dot_never_does() {
        for path in ["a.txt", "runs/r1/shot.png", "a-b_c/d.e", "/abs", "x/"] {
            assert!(is_valid_artifact_path(path), "{path}");
        }
        for path in ["", "a/../b", "..", "a b", "a;b", "a%20b", "a\n", "\u{e9}", "a..b"] {
            assert!(!is_valid_artifact_path(path), "{path:?}");
        }
    }

    #[test]
    fn given_the_guard_script_when_rendered_then_it_is_the_one_node_runs() {
        assert_eq!(
            artifact_guard_script("runs/r1/shot.png"),
            "F=\"/home/agent/e2e-artifacts/runs/r1/shot.png\"\nREAL=$(readlink -f \"$F\" 2>/dev/null)\ncase \"$REAL\" in /home/agent/e2e-artifacts/*) ;; *) echo NOT_ALLOWED; exit 1;; esac\n[ -f \"$F\" ] || { echo NOT_FOUND; exit 1; }\nbase64 -w0 \"$F\""
        );
    }

    #[test]
    fn given_status_text_when_read_then_it_splits_at_the_last_marker_and_an_empty_code_means_running() {
        let running = JobStatus::read(&format!("step 1{EXIT_MARKER}"));
        assert_eq!((running.running, running.exit_code_json(), running.output.to_text_lossy()), (true, "null".to_string(), "step 1".to_string()));
        let done = JobStatus::read(&format!("step 1{EXIT_MARKER}step 2{EXIT_MARKER}0\n"));
        assert_eq!((done.running, done.exit_code_json(), done.output.to_text_lossy()), (false, "0".to_string(), format!("step 1{EXIT_MARKER}step 2")));
        let unknown = JobStatus::read(EXIT_MARKER);
        assert_eq!((unknown.running, unknown.output.to_text_lossy()), (true, String::new()));
        let bare = JobStatus::read("garbage");
        assert_eq!((bare.running, bare.output.to_text_lossy()), (true, "garbage".to_string()));
    }

    #[test]
    fn given_a_code_that_is_no_number_or_a_long_output_when_read_then_the_code_is_null_and_the_last_4000_units_stay() {
        let odd = JobStatus::read(&format!("out{EXIT_MARKER}abc"));
        assert_eq!((odd.running, odd.exit_code_json()), (false, "null".to_string()));
        let text = format!("{}\u{1f600}{}{EXIT_MARKER}0", "x".repeat(999), "y".repeat(3999));
        let cut = JobStatus::read(&text);
        assert_eq!(cut.output.to_json(), format!("\"\\ude00{}\"", "y".repeat(3999)));
        assert_eq!(JobStatus::read(&format!("a{EXIT_MARKER}137")).exit_code_json(), "137");
    }
}
