//! `wf bucket state` through the built binary, against a bare repository standing in for GitHub: each job is its own clone, so
//! two writers race over the ref as two runners would.
use crate::support::{last_json, run_wf, RunOpts};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Remote {
    dir: tempfile::TempDir,
}

impl Remote {
    fn new() -> Remote {
        let dir = tempfile::tempdir().expect("a temp dir");
        git(dir.path(), &["init", "--bare", "--quiet", "origin.git"]);
        Remote { dir }
    }

    fn bare(&self) -> PathBuf {
        self.dir.path().join("origin.git")
    }

    fn job(&self, name: &str) -> PathBuf {
        let job = self.dir.path().join(name);
        std::fs::create_dir(&job).expect("a job dir");
        git(&job, &["init", "--quiet"]);
        git(&job, &["remote", "add", "origin", self.bare().to_str().unwrap()]);
        job
    }
}

fn state(job: &Path, args: &[&str], input: Option<&str>) -> (i32, serde_json::Value) {
    let mut opts = RunOpts { cwd: Some(job.to_path_buf()), input: input.map(str::to_string), ..RunOpts::default() };
    opts.timeout = Some(std::time::Duration::from_secs(60));
    let r = run_wf(&[&["bucket", "state"], args].concat(), &opts);
    (r.exit_code, last_json(&r))
}

#[test]
fn given_no_state_ref_when_a_file_is_read_then_it_is_not_found_and_not_an_error() {
    let remote = Remote::new();
    let job = remote.job("reader");

    let (code, out) = state(&job, &["read", "eas/latest-dev.json"], None);

    assert_eq!((code, out["ok"].as_bool(), out["found"].as_bool()), (0, Some(true), Some(false)));
}

#[test]
fn given_a_written_file_when_another_job_reads_it_then_it_gets_the_body_and_a_missing_path_is_not_found() {
    let remote = Remote::new();
    let (writer, reader) = (remote.job("writer"), remote.job("reader"));
    let (code, written) = state(&writer, &["write", "eas/latest-dev.json", "--message", "pointer"], Some("{\"group\":\"g1\"}"));
    assert_eq!(code, 0, "{written}");

    let (_, found) = state(&reader, &["read", "eas/latest-dev.json"], None);
    let (_, missing) = state(&reader, &["read", "eas/latest-e2e.json"], None);

    assert_eq!(found["content"], "{\"group\":\"g1\"}");
    assert_eq!(missing["found"], false);
    assert_eq!(git(&remote.bare(), &["log", "--format=%s", "refs/wf/state"]), "pointer");
    assert_eq!(git(&remote.bare(), &["for-each-ref", "--format=%(refname)"]), "refs/wf/state");
}

#[test]
fn given_two_jobs_writing_different_files_at_once_when_both_finish_then_neither_file_is_lost() {
    let remote = Remote::new();
    let jobs: Vec<PathBuf> = (0..4).map(|i| remote.job(&format!("job{i}"))).collect();

    let threads: Vec<_> = jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let job = job.clone();
            std::thread::spawn(move || state(&job, &["write", &format!("test-runs/{i}.json")], Some(&format!("{{\"n\":{i}}}"))))
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().expect("a writer thread")).collect();

    assert!(results.iter().all(|(code, _)| *code == 0), "{results:?}");
    let reader = remote.job("reader");
    for i in 0..4 {
        let (_, out) = state(&reader, &["read", &format!("test-runs/{i}.json")], None);
        assert_eq!(out["content"], format!("{{\"n\":{i}}}"), "file {i}");
    }
}

#[test]
fn given_a_path_that_climbs_out_when_it_is_written_then_it_is_refused_and_nothing_is_pushed() {
    let remote = Remote::new();
    let job = remote.job("writer");

    let (code, out) = state(&job, &["write", "../escape.json"], Some("x"));

    assert_eq!((code, out["ok"].as_bool()), (1, Some(false)));
    assert_eq!(git(&remote.bare(), &["for-each-ref"]), "");
}

#[test]
fn given_a_long_history_when_a_job_reads_the_pointer_then_it_gets_the_newest_body_and_its_checkout_is_left_alone() {
    let remote = Remote::new();
    let (writer, reader) = (remote.job("writer"), remote.job("reader"));
    for n in 1..=4 {
        let (code, out) = state(&writer, &["write", "eas/latest-dev.json", "--message", &format!("pointer {n}")], Some(&format!("{{\"group\":\"g{n}\"}}")));
        assert_eq!(code, 0, "{out}");
    }
    let (code, big) = state(&writer, &["write", "ci/ci.sqlite.sealed", "--message", "sealed"], Some(&"x".repeat(200_000)));
    assert_eq!(code, 0, "{big}");

    let (code, found) = state(&reader, &["read", "eas/latest-dev.json"], None);

    assert_eq!((code, found["content"].as_str()), (0, Some("{\"group\":\"g4\"}")));
    assert_eq!(git(&reader, &["for-each-ref"]), "", "the read must not write refs into the checkout");
    assert!(!reader.join(".git/shallow").exists(), "the read must not make the checkout shallow");
}

#[test]
fn given_a_state_ref_without_the_pointer_when_it_is_read_then_it_is_not_found() {
    let remote = Remote::new();
    let (writer, reader) = (remote.job("writer"), remote.job("reader"));
    let (code, out) = state(&writer, &["write", "ci/other.json"], Some("{}"));
    assert_eq!(code, 0, "{out}");

    let (code, out) = state(&reader, &["read", "eas/latest-dev.json"], None);

    assert_eq!((code, out["ok"].as_bool(), out["found"].as_bool()), (0, Some(true), Some(false)));
}

#[test]
fn given_a_remote_that_never_answers_when_a_file_is_read_then_the_read_fails_within_its_timeout_and_says_so() {
    let silent = std::net::TcpListener::bind("127.0.0.1:0").expect("a listener");
    let port = silent.local_addr().expect("an address").port();
    let held = std::thread::spawn(move || silent.incoming().take(8).filter_map(Result::ok).collect::<Vec<_>>());
    let dir = tempfile::tempdir().expect("a temp dir");
    git(dir.path(), &["init", "--quiet"]);
    git(dir.path(), &["remote", "add", "origin", &format!("http://127.0.0.1:{port}/repo.git")]);
    let mut opts = RunOpts { cwd: Some(dir.path().to_path_buf()), timeout: Some(std::time::Duration::from_secs(40)), ..RunOpts::default() };
    opts.env.insert("WF_STATE_READ_TIMEOUT_S".into(), "2".into());
    let started = std::time::Instant::now();

    let r = run_wf(&["bucket", "state", "read", "eas/latest-dev.json"], &opts);

    let out = last_json(&r);
    assert_eq!((r.exit_code, out["ok"].as_bool()), (1, Some(false)));
    assert!(out["error"].as_str().unwrap_or("").contains("did not finish within 2 s"), "{out}");
    assert!(started.elapsed() < std::time::Duration::from_secs(30), "took {:?}", started.elapsed());
    drop(held);
}

#[test]
fn given_a_write_when_it_lands_then_the_output_times_each_phase_so_a_slow_push_names_its_cause() {
    // Given an empty remote and a job with a pointer to write
    let remote = Remote::new();
    let job = remote.job("writer");

    // When the pointer is written
    let (code, out) = state(&job, &["write", "eas/latest-dev.json", "--message", "pointer"], Some("{\"group\":\"g1\"}"));

    // Then the line carries the attempts and a non-negative second count for the fetch, the commit and the push
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["timing"]["attempts"], 1, "{out}");
    for phase in ["fetchS", "commitS", "pushS"] {
        assert!(out["timing"][phase].as_f64().is_some_and(|s| s >= 0.0), "{phase}: {out}");
    }
}
