//! `wf cache nx --why`: every task Nx ran in the command is explained against the earlier runs of that task, kept in the
//! verification-artifacts bucket (a stateful loopback double), and the run's own inputs are recorded for the next miss. Nx is a stub that
//! leaves a `run.json`; the hasher (`WF_NODE`) is a stub that prints the input table the test gives it; git is real, in a two-commit repo.
use crate::support::{create_stub, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

const SECRET: &str = "https://agent.secret-value-xyz.example";
const SHARD: &str = "verification/nx-inputs/lint.json";

/// The bucket: `PUT <par>/<key>` stores, `GET <read>/<key>` reads, under one prefix each.
struct Store {
    par: String,
    read: String,
    objects: Arc<Mutex<BTreeMap<String, String>>>,
}

impl Store {
    fn start() -> Store {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a loopback port");
        let port = server.server_addr().to_ip().expect("an ip address").port();
        let store = Store { par: format!("http://127.0.0.1:{port}/par/"), read: format!("http://127.0.0.1:{port}/o"), objects: Arc::default() };
        let held = store.objects.clone();
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                let url = request.url().to_string();
                let key = url.strip_prefix("/par/").or_else(|| url.strip_prefix("/o/")).unwrap_or_default().to_string();
                let found = held.lock().unwrap().get(&key).cloned();
                let reply = match (request.method().as_str(), found) {
                    ("PUT", _) => {
                        held.lock().unwrap().insert(key, body);
                        tiny_http::Response::from_data(Vec::new())
                    }
                    ("GET", Some(text)) => tiny_http::Response::from_data(text.into_bytes()),
                    _ => tiny_http::Response::from_data(Vec::new()).with_status_code(404),
                };
                let _ = request.respond(reply);
            }
        });
        store
    }

    fn seed(&self, shard: Value) {
        self.objects.lock().unwrap().insert(SHARD.to_string(), shard.to_string());
    }

    fn shard(&self) -> Value {
        serde_json::from_str(self.objects.lock().unwrap().get(SHARD).expect("the run recorded its inputs")).unwrap()
    }
}

/// A two-commit repository (the second changes `libraries/app/a.ts`), a home for Nx's `run.json`, and the two stubs.
struct Why {
    dir: tempfile::TempDir,
    store: Store,
    commits: (String, String),
}

impl Why {
    fn new() -> Why {
        let dir = tempfile::tempdir().expect("a temp dir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("libraries/app")).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git").args(args).current_dir(&repo).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q"]);
        std::fs::write(repo.join("libraries/app/a.ts"), "1").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "one"]);
        let first = git(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("libraries/app/a.ts"), "2").unwrap();
        git(&["commit", "-qam", "two"]);
        let second = git(&["rev-parse", "HEAD"]);
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        create_stub(&dir.path().join("bin/nx-run"), "mkdir -p \"$HOME/.nx/ws/cache\"\nprintf '%s' \"$RUN_JSON\" > \"$HOME/.nx/ws/cache/run.json\"\nexit \"${RUN_EXIT:-0}\"");
        create_stub(&dir.path().join("bin/hasher"), "cat \"$HASH_TABLE\"");
        Why { dir, store: Store::start(), commits: (first, second) }
    }

    fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    /// `wf cache nx --why -- nx-run` where Nx ran `app:lint` under `hash` (status 0, or 1 when it failed) and the hasher says these inputs.
    fn run(&self, hash: &str, inputs: Value, failed: bool) -> (RunResult, Value) {
        let table = self.dir.path().join("table.json");
        std::fs::write(&table, json!({ "runtime": {}, "tasks": { "app:lint": { "hash": hash, "inputs": inputs } } }).to_string()).unwrap();
        let run_json = json!({ "tasks": [{ "taskId": "app:lint", "hash": hash, "startTime": "2026-11-08T10:00:00.000Z", "endTime": "2026-11-08T10:00:02.000Z", "cacheStatus": "cache-miss", "status": i32::from(failed) }] }).to_string();
        let opts = RunOpts::default()
            .cwd(&self.repo())
            .bin_dir(&self.dir.path().join("bin"))
            .env("HOME", &self.dir.path().join("home").display().to_string())
            .env("WF_NODE", &self.dir.path().join("bin/hasher").display().to_string())
            .env("HASH_TABLE", &table.display().to_string())
            .env("RUN_JSON", &run_json)
            .env("VERIFICATION_ARTIFACTS_PAR_URL", &self.store.par)
            .env("VERIFICATION_ARTIFACTS_READ_URL", &self.store.read)
            .env("EXPO_PUBLIC_AGENT_BASE_URL", SECRET)
            .env("GITHUB_RUN_ID", "")
            .env("GITHUB_SHA", "")
            .timeout(std::time::Duration::from_secs(120));
        let r = run_wf(&["cache", "nx", "--why", "--", "nx-run"], &opts);
        let line = r.stderr.lines().find_map(|l| l.strip_prefix("wf: nx why ")).unwrap_or_else(|| panic!("no `wf: nx why` line: {r:?}"));
        let why: Value = serde_json::from_str(line).unwrap();
        (r, why)
    }

    fn earlier(&self, sha: &str, hash: &str, passed: bool, inputs: Value) -> Value {
        json!({ "schema": 1, "groups": {}, "tasks": { "app:lint": [{ "run": "7", "sha": sha, "ts": 1_700_000_000_000i64, "source": "ci", "hash": hash, "outcome": "ran", "passed": passed, "inputs": inputs, "npm": null, "runtime": {} }] } })
    }
}

const FILES: &str = "app:libraries/app/**/*,!libraries/app/target/**";
const ENV: &str = "env:EXPO_PUBLIC_AGENT_BASE_URL";

#[test]
fn a_miss_with_a_changed_file_input_names_the_file_that_changed_between_the_two_commits() {
    // Given an earlier CI run of the task at the first commit, and this run at the second, where the app's files digest differently
    let w = Why::new();
    w.store.seed(w.earlier(&w.commits.0, "H0", true, json!({ FILES: "1001", ENV: "5" })));

    // When `wf cache nx --why` runs a command whose Nx ran the task under a new hash
    let (r, why) = w.run("H1", json!({ FILES: "1002", ENV: "5" }), false);

    // Then the miss is an input change that names the file git says changed, and the earlier run it is judged against
    assert_eq!(r.exit_code, 0, "{r:?}");
    let miss = &why["misses"][0];
    assert_eq!((miss["task"].as_str(), miss["verdict"].as_str()), (Some("app:lint"), Some("input-changed")), "{why}");
    assert_eq!(miss["changed"][0]["kind"], "files", "{why}");
    assert_eq!(miss["changed"][0]["files"], json!(["libraries/app/a.ts"]), "{why}");
    assert!(miss["because"].as_str().unwrap().contains("libraries/app/a.ts"), "{why}");
    assert_eq!((miss["against"]["run"].as_str(), miss["against"]["sha"].as_str()), (Some("7"), Some(w.commits.0.as_str())), "{why}");
}

#[test]
fn a_miss_with_a_changed_env_var_names_the_variable_and_never_its_value() {
    // Given an earlier run whose environment digest differs from this run's, the files being the same
    let w = Why::new();
    w.store.seed(w.earlier(&w.commits.1, "H0", true, json!({ FILES: "1001", ENV: "5" })));

    // When the command runs, with the variable set to a secret-looking value
    let (r, why) = w.run("H1", json!({ FILES: "1001", ENV: "6" }), false);

    // Then the variable is named, and neither what is printed nor what is recorded holds the value
    let miss = &why["misses"][0];
    assert_eq!((miss["changed"][0]["kind"].as_str(), miss["changed"][0]["name"].as_str()), (Some("env"), Some("EXPO_PUBLIC_AGENT_BASE_URL")), "{why}");
    assert!(miss["because"].as_str().unwrap().contains("env var EXPO_PUBLIC_AGENT_BASE_URL"), "{why}");
    assert!(!r.stderr.contains("secret-value-xyz") && !w.store.shard().to_string().contains("secret-value-xyz"), "{r:?}");
}

#[test]
fn a_task_no_earlier_run_recorded_was_never_stored_and_this_run_records_it() {
    // Given a bucket with no history of the task
    let w = Why::new();

    // When the command runs
    let (_, why) = w.run("H1", json!({ FILES: "1001", ENV: "5" }), false);

    // Then the miss is never-stored, and the bucket now holds this run's inputs for the next miss
    assert_eq!(why["misses"][0]["verdict"], "never-stored", "{why}");
    assert_eq!(why["recorded"], true, "{why}");
    let kept = &w.store.shard()["tasks"]["app:lint"][0];
    assert_eq!((kept["hash"].as_str(), kept["inputs"][FILES].as_str(), kept["sha"].as_str()), (Some("H1"), Some("1001"), Some(w.commits.1.as_str())));
}

#[test]
fn a_miss_on_a_hash_an_earlier_run_stored_has_expired() {
    // Given an earlier run that passed under the very hash this run misses on
    let w = Why::new();
    w.store.seed(w.earlier(&w.commits.0, "H1", true, json!({ FILES: "1001", ENV: "5" })));

    // When the command runs
    let (_, why) = w.run("H1", json!({ FILES: "1001", ENV: "5" }), false);

    // Then the cache is said to have lost an entry it once had
    assert_eq!((why["misses"][0]["verdict"].as_str(), why["misses"][0]["stored"].as_bool()), (Some("expired"), Some(true)), "{why}");
}

#[test]
fn a_failed_run_says_it_stored_nothing_and_so_does_the_earlier_failure_it_repeats() {
    // Given an earlier run that failed under the hash this run computes too
    let w = Why::new();
    w.store.seed(w.earlier(&w.commits.0, "H1", false, json!({ FILES: "1001", ENV: "5" })));

    // When the command runs and its task fails again
    let (_, why) = w.run("H1", json!({ FILES: "1001", ENV: "5" }), true);

    // Then neither run stored anything under the hash
    let miss = &why["misses"][0];
    assert_eq!((miss["verdict"].as_str(), miss["stored"].as_bool()), (Some("never-stored"), Some(false)), "{why}");
    assert!(miss["because"].as_str().unwrap().contains("stored nothing"), "{why}");
    assert!(miss["thisRun"].as_str().unwrap().contains("stored nothing"), "{why}");
}
