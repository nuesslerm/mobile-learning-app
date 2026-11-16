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
