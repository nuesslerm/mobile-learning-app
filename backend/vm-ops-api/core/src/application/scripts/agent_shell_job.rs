//! POSIX sh run inside agent-shell. A job is `setsid sh -c WRAPPER sh <jobDir> <backstopSec>`, so its whole process tree shares the
//! group whose id is written to `<jobDir>/pid`.

/// Exit code 143 marks a job ended by TERM, from the backstop or from a cancel.
pub const AGENT_SHELL_JOB_WRAPPER: &str = concat!(
    r#"mypid=$$; "#,
    r#"echo "$mypid" > "$1/pid"; "#,
    r#"trap 'echo 143 > "$1/exit-code" 2>/dev/null; exit 143' TERM; "#,
    r#"sh "$1/script.sh" > "$1/output.log" 2>&1 & "#,
    r#"script_pid=$!; "#,
    r#"( sleep "$2"; kill -TERM -"$mypid" 2>/dev/null ) & "#,
    r#"wait "$script_pid"; "#,
    r#"code=$?; "#,
    r#"trap - TERM; "#,
    r#"echo "$code" > "$1/exit-code"; "#,
    r#"kill -TERM -"$mypid" 2>/dev/null"#,
);

/// docker stop's default grace: long enough for a client to unwind, short enough not to hold a queue.
pub const JOB_CANCEL_GRACE_SEC: u32 = 10;

/// `$1` = jobDir, `$2` = grace seconds. TERM the whole group, then KILL whatever outlives the grace.
pub const AGENT_SHELL_JOB_CANCEL: &str = concat!(
    r#"pgid=$(cat "$1/pid" 2>/dev/null) || exit 0; "#,
    r#"[ -n "$pgid" ] || exit 0; "#,
    r#"kill -TERM "-$pgid" 2>/dev/null || exit 0; "#,
    r#"waited=0; "#,
    r#"while [ "$waited" -lt "$2" ] && kill -0 "-$pgid" 2>/dev/null; do sleep 1; waited=$((waited + 1)); done; "#,
    r#"if kill -0 "-$pgid" 2>/dev/null; then kill -KILL "-$pgid" 2>/dev/null; fi; "#,
    r#"exit 0"#,
);

/// `$1` = jobDir. Prints the log's byte count, the marker, then the exit code once the job has one. A group that is gone with no
/// exit code was KILLed before its wrapper could write one (a frozen build client outlives the cancel grace), and a job dir that is
/// gone was lost with its container (agent-shell was recreated mid-job): report 137 for both instead of leaving the job `running`
/// forever.
pub const AGENT_SHELL_JOB_STATUS: &str = concat!(
    r#"if [ ! -d "$1" ]; then echo 0; printf "\n---EXITCODE---\n"; echo 137; exit 0; fi; "#,
    r#"wc -c < "$1/output.log" 2>/dev/null || echo 0; "#,
    r#"printf "\n---EXITCODE---\n"; "#,
    r#"if [ -f "$1/exit-code" ]; then cat "$1/exit-code"; "#,
    r#"else pgid=$(cat "$1/pid" 2>/dev/null); "#,
    r#"[ -z "$pgid" ] || kill -0 "-$pgid" 2>/dev/null || echo 137; fi"#,
);

pub const AGENT_SHELL_JOB_STATUS_MARKER: &str = "\n---EXITCODE---\n";

pub fn agent_shell_status_argv(job_dir: &str) -> Vec<String> {
    ["sh", "-c", AGENT_SHELL_JOB_STATUS, "sh", job_dir].map(String::from).to_vec()
}

/// The argv that starts a job's wrapper detached: it takes the job's dir and the backstop in seconds.
pub fn agent_shell_job_argv(job_dir: &str, backstop_sec: u32) -> Vec<String> {
    ["setsid", "sh", "-c", AGENT_SHELL_JOB_WRAPPER, "sh", job_dir, &backstop_sec.to_string()].map(String::from).to_vec()
}

pub fn agent_shell_cancel_argv(job_dir: &str, grace_sec: u32) -> Vec<String> {
    ["sh", "-c", AGENT_SHELL_JOB_CANCEL, "sh", job_dir, &grace_sec.to_string()].map(String::from).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn status_of(dir: &std::path::Path) -> String {
        let argv = agent_shell_status_argv(&dir.display().to_string());
        let out = Command::new(&argv[0]).args(&argv[1..]).output().expect("sh runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn given_a_job_dir_that_is_gone_when_its_status_is_read_then_the_job_is_lost_with_137_not_running() {
        // Given a job dir that never existed here, as after agent-shell was recreated mid-job
        let gone = std::env::temp_dir().join("wf-no-such-job-dir");

        // When its status is read
        let status = status_of(&gone);

        // Then it ends with exit code 137
        assert_eq!(status, "0\n\n---EXITCODE---\n137\n");
    }

    #[test]
    fn given_a_running_job_when_its_status_is_read_then_no_exit_code_follows_the_marker() {
        // Given a job dir whose wrapper started (a pid of a live group) and has written no exit code
        let dir = std::env::temp_dir().join(format!("wf-running-job-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("output.log"), "hello").unwrap();
        let mut group = Command::new("setsid").args(["sleep", "30"]).spawn().expect("a live group");
        std::fs::write(dir.join("pid"), group.id().to_string()).unwrap();

        // When its status is read
        let status = status_of(&dir);
        let _ = group.kill();
        let _ = group.wait();
        let _ = std::fs::remove_dir_all(&dir);

        // Then it reports the log size and no exit code
        assert_eq!(status, "5\n\n---EXITCODE---\n");
    }
}
