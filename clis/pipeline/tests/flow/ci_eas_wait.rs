//! The step of e2e-tests.yml that waits for the run's own EAS Update job, run as the bash the runner would run, with `gh` and `sleep` replaced by stubs:
//! the step's answer is what the Android update units read as `WF_EAS_UPDATE_RESULT`.
use crate::support::repo_root;
use std::path::{Path, PathBuf};
use yaml_rust2::{Yaml, YamlLoader};

const STEP: &str = "Wait for this run's EAS Update";

const RATE_LIMITED: &str = "1|{\\n\\t\"message\": \"API rate limit exceeded for installation.\",\\n\\t\"status\": \"403\"\\n}\\n";

fn step_script() -> String {
    let text = std::fs::read_to_string(repo_root().join(".github/workflows/e2e-tests.yml")).unwrap();
    let docs = YamlLoader::load_from_str(&text).unwrap();
    let Yaml::Hash(jobs) = &docs[0]["jobs"] else { panic!("no jobs") };
    jobs.values()
        .filter_map(|job| job["steps"].as_vec())
        .flatten()
        .find(|step| step["name"].as_str() == Some(STEP))
        .and_then(|step| step["run"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("no step named {STEP}"))
}

struct Waited {
    dir: PathBuf,
}

impl Waited {
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }
}

/// Runs the step; the Actions API answers one `exit|stdout` line of `answers` per call (the last one again once they run out).
fn run_step(answers: &[&str]) -> Waited {
    let dir = tempfile::tempdir().unwrap().keep();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(dir.join("answers"), answers.join("\n")).unwrap();
    stub(&bin, "gh", "echo x >> \"$STUB_DIR/gh.calls\"\nn=$(wc -l < \"$STUB_DIR/gh.calls\")\ntotal=$(wc -l < \"$STUB_DIR/answers\")\ntotal=$((total + 1))\n[ \"$n\" -le \"$total\" ] || n=$total\nline=$(sed -n \"${n}p\" \"$STUB_DIR/answers\")\nprintf '%b' \"${line#*|}\"\nexit \"${line%%|*}\"");
    stub(&bin, "sleep", "echo \"$*\" >> \"$STUB_DIR/sleep.calls\"");
    let script = dir.join("step.sh");
    std::fs::write(&script, step_script()).unwrap();
    let status = std::process::Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-e", "-o", "pipefail"])
        .arg(&script)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("STUB_DIR", &dir)
        .env("GITHUB_REPOSITORY", "o/r")
        .env("GITHUB_RUN_ID", "44905124372")
        .env("GITHUB_OUTPUT", dir.join("output"))
        .env("GITHUB_ENV", dir.join("env"))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("bash runs");
    assert!(status.success(), "the step failed: {status}");
    Waited { dir }
}

fn stub(bin: &Path, name: &str, body: &str) {
    let path = bin.join(name);
    std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

#[test]
fn given_the_api_refuses_two_polls_with_its_error_body_when_the_job_then_ends_then_the_step_answers_that_conclusion_and_nothing_else() {
    // Given the Actions API refuses the first two polls (the rate limit prints its JSON error on stdout), reports the job still running, then success
    // When the step waits
    let waited = run_step(&[RATE_LIMITED, RATE_LIMITED, "0|", "0|success\\n"]);

    // Then the error text never reaches the step's outputs, which hold the conclusion alone, and the step polled until it had one
    assert_eq!(waited.read("output"), "result=success\n");
    assert_eq!(waited.read("env"), "WF_EAS_UPDATE_RESULT=success\n");
    assert_eq!(waited.read("gh.calls").lines().count(), 4);
    assert_eq!(waited.read("sleep.calls").lines().count(), 3);
}

#[test]
fn given_the_job_ended_in_failure_when_the_step_waits_then_it_answers_failure_at_once() {
    // Given the job's conclusion is failure on the first poll
    // When the step waits
    let waited = run_step(&["0|failure\\n"]);

    // Then the answer is failure, after one poll and no sleep
    assert_eq!(waited.read("output"), "result=failure\n");
    assert_eq!((waited.read("gh.calls").lines().count(), waited.read("sleep.calls").lines().count()), (1, 0));
}

#[test]
fn given_the_api_never_names_a_conclusion_when_the_step_waits_then_it_gives_up_with_timed_out_and_a_clean_output() {
    // Given every poll is refused with the rate limit's error body
    // When the step waits through all its polls
    let waited = run_step(&[RATE_LIMITED]);

    // Then the step still succeeds, and answers timed_out without a line of the error in its outputs
    assert_eq!(waited.read("output"), "result=timed_out\n");
    assert_eq!(waited.read("env"), "WF_EAS_UPDATE_RESULT=timed_out\n");
    assert_eq!(waited.read("gh.calls").lines().count(), 150);
}
