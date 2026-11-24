//! `wf ci proven` through the built binary, against a loopback fake of the Actions cache API and a throwaway git repository: a shard whose fingerprint a job marked after it
//! passed (a cache entry `proven-<stage>-<fingerprint>` on the run's own ref) is left out of the matrix; anything unprovable, unmarked or unreadable runs.
use crate::ci_timings::{line, served, wf};
use crate::support::{git_command, MockReply, MockServer};
use serde_json::{json, Value};
use std::path::Path;

const REF: &str = "refs/heads/dev";

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new() -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repo { dir };
        repo.git(&["init", "-q", "-b", "dev"]);
        repo.write(".github/workflows/pipeline.yml", "name: one\n");
        repo.write("nx.json", "{}\n");
        repo.write("crates/alpha/Cargo.toml", "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        repo.write("crates/beta/Cargo.toml", "[package]\nname = \"beta\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        repo.write("crates/meta/Cargo.toml", "[package]\nname = \"meta\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[package.metadata.flow]\ninputs = [\"../../docs/**\"]\n");
        repo
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
        let out = git_command().args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).current_dir(self.dir.path()).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "c"]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn path(&self) -> String {
        self.dir.path().display().to_string()
    }
}

fn crate_unit(name: &str) -> Value {
    json!({"kind": "integration", "crate": name, "dir": format!("crates/{name}"), "manifest": format!("crates/{name}/Cargo.toml"), "group": "svc"})
}

fn plan_file(dir: &Path, sha: &str, units: Vec<Value>) -> String {
    let path = dir.join(format!("plan-{sha}.json"));
    std::fs::write(&path, json!({"sha": sha, "stages": {"integration-tests": units}}).to_string()).unwrap();
    path.display().to_string()
}

fn ask(server: &MockServer, repo: &Repo, plan: &str, count: &str, extra: &[&str], env: &[(&str, &str)]) -> Value {
    let root = repo.path();
    let mut args = vec!["proven", "--stage", "integration-tests", "--count", count, "--plan", plan, "--repo", &root];
    args.extend(extra);
    let mut envs = vec![("GITHUB_REF", REF)];
    envs.extend(env);
    let r = wf(server, &args, &envs);
    line(&r)
}

fn caches(key: &str, git_ref: &str) -> Value {
    json!({"total_count": 1, "actions_caches": [{"id": 1, "key": key, "ref": git_ref, "version": "v"}]})
}

fn learned(repo: &Repo, plan: &str, count: &str) -> Vec<Value> {
    let server = served(&[]);
    ask(&server, repo, plan, count, &["--no-lookup"], &[])["shards"].as_array().unwrap().clone()
}

#[test]
fn given_a_shard_marked_on_this_ref_when_the_plan_asks_then_it_is_proven_and_the_others_run() {
    // Given a plan of two clean crates dealt over two shards, and a marker for the first shard's fingerprint on the run's ref
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha"), crate_unit("beta")]);
    let key = learned(&repo, &plan, "2")[0]["key"].as_str().unwrap().to_string();
    let server = served(&[(&format!("/repos/o/r/actions/caches?key={key}&per_page=100"), caches(&key, REF))]);

    // When the shards are asked about
    let out = ask(&server, &repo, &plan, "2", &[], &[]);

    // Then the first is proven and skipped, the second still runs, and the key carries the stage
    assert_eq!((out["proven"].clone(), out["run"].clone()), (json!([1]), json!([2])), "{out}");
    assert!(key.starts_with("proven-integration-tests-"), "{key}");
    assert_eq!(out["shards"][0]["proven"], json!(true), "{out}");
}

#[test]
fn given_a_marker_of_another_ref_when_the_plan_asks_then_nothing_is_proven() {
    // Given a marker for the shard's fingerprint that a pull request's run saved
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha")]);
    let key = learned(&repo, &plan, "1")[0]["key"].as_str().unwrap().to_string();
    let server = served(&[(&format!("/repos/o/r/actions/caches?key={key}&per_page=100"), caches(&key, "refs/pull/9/merge"))]);

    // When a push to dev asks
    let out = ask(&server, &repo, &plan, "1", &[], &[]);

    // Then the shard runs
    assert_eq!((out["proven"].clone(), out["run"].clone()), (json!([]), json!([1])), "{out}");
}

#[test]
fn given_a_changed_shared_input_when_the_plan_asks_then_the_old_marker_no_longer_matches() {
    // Given a shard marked at one commit and a later commit that edits a workflow every test reads
    let repo = Repo::new();
    let first = repo.commit();
    let plan_one = plan_file(repo.dir.path(), &first, vec![crate_unit("alpha")]);
    let key = learned(&repo, &plan_one, "1")[0]["key"].as_str().unwrap().to_string();
    repo.write(".github/workflows/pipeline.yml", "name: two\n");
    let second = repo.commit();
    let plan_two = plan_file(repo.dir.path(), &second, vec![crate_unit("alpha")]);
    let server = served(&[(&format!("/repos/o/r/actions/caches?key={key}&per_page=100"), caches(&key, REF))]);

    // When the second commit's plan asks
    let out = ask(&server, &repo, &plan_two, "1", &[], &[]);

    // Then the fingerprint differs and the shard runs, while an unrelated file leaves it as it was
    assert_ne!(out["shards"][0]["key"], json!(key), "{out}");
    assert_eq!(out["run"], json!([1]), "{out}");
    repo.write("docs/notes.md", "unrelated\n");
    let third = repo.commit();
    let plan_three = plan_file(repo.dir.path(), &third, vec![crate_unit("alpha")]);
    assert_eq!(learned(&repo, &plan_three, "1")[0]["key"], out["shards"][0]["key"]);
}

#[test]
fn given_a_package_and_a_crate_with_flow_inputs_when_the_plan_asks_then_neither_can_be_proven_and_both_run() {
    // Given a shard of a package and a shard of a crate that declares package.metadata.flow inputs
    let repo = Repo::new();
    let sha = repo.commit();
    let package = json!({"kind": "integration", "package": "@x/pkg", "dir": "packages/pkg", "script": "test:integration"});
    let plan = plan_file(repo.dir.path(), &sha, vec![package, crate_unit("meta")]);
    let server = served(&[]);

    // When the shards are asked about
    let out = ask(&server, &repo, &plan, "2", &[], &[]);

    // Then no key exists for either, each says why, and both run without a request to the cache API
    let why: Vec<String> = out["shards"].as_array().unwrap().iter().map(|s| s["why"].as_str().unwrap().to_string()).collect();
    assert!(why.iter().any(|w| w.contains("is a package")), "{why:?}");
    assert!(why.iter().any(|w| w.contains("declares flow inputs")), "{why:?}");
    assert!(out["shards"].as_array().unwrap().iter().all(|s| s["key"].is_null()), "{out}");
    assert_eq!((out["proven"].clone(), out["run"].clone()), (json!([]), json!([1, 2])), "{out}");
    assert!(server.requests().is_empty(), "{:?}", server.requests());
}

#[test]
fn given_a_fresh_run_when_the_plan_asks_then_every_shard_runs_and_none_is_marked() {
    // Given a shard that is marked, and a run with the caches off
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha")]);
    let key = learned(&repo, &plan, "1")[0]["key"].as_str().unwrap().to_string();
    let server = served(&[(&format!("/repos/o/r/actions/caches?key={key}&per_page=100"), caches(&key, REF))]);
    let output = repo.dir.path().join("github-output");

    // When the plan asks with --fresh
    let out = ask(&server, &repo, &plan, "1", &["--fresh", "--github-output"], &[("GITHUB_OUTPUT", &output.display().to_string())]);

    // Then the shard runs, nothing was asked of the API, and the proof keys are empty so no job saves a marker
    assert_eq!((out["proven"].clone(), out["run"].clone()), (json!([]), json!([1])), "{out}");
    assert!(server.requests().is_empty());
    let written = std::fs::read_to_string(output).unwrap();
    assert!(written.contains("integration_tests_run_shards=[1]\n") && written.contains("integration_tests_proof_keys=[\"\",\"\"]\n"), "{written}");
}

#[test]
fn given_a_proven_shard_when_the_step_outputs_are_written_then_the_matrix_list_and_the_one_based_marker_keys_follow() {
    // Given two shards of which the first is marked
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha"), crate_unit("beta")]);
    let keys: Vec<String> = learned(&repo, &plan, "2").iter().map(|s| s["key"].as_str().unwrap().to_string()).collect();
    let server = served(&[(&format!("/repos/o/r/actions/caches?key={}&per_page=100", keys[0]), caches(&keys[0], REF))]);
    let output = repo.dir.path().join("github-output");

    // When the plan asks with --github-output
    ask(&server, &repo, &plan, "2", &["--github-output"], &[("GITHUB_OUTPUT", &output.display().to_string())]);

    // Then the run list holds shard 2 only, and the key list starts with an empty entry so the 1-based matrix shard indexes it
    let written = std::fs::read_to_string(output).unwrap();
    assert!(written.contains("integration_tests_run_shards=[2]\n"), "{written}");
    assert!(written.contains(&format!("integration_tests_proof_keys={}\n", json!(["", keys[0], keys[1]]))), "{written}");
}

#[test]
fn given_an_unreadable_cache_api_when_the_plan_asks_then_every_shard_runs_with_a_warning() {
    // Given an API that answers every cache listing with an error
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha")]);
    let server = MockServer::start(None);
    server.queue(vec![MockReply::json(500, json!({"message": "boom"}))], true);

    // When the plan asks
    let out = ask(&server, &repo, &plan, "1", &[], &[]);

    // Then the shard runs and the warning says the marker could not be read
    assert_eq!((out["ok"].clone(), out["run"].clone(), out["proven"].clone()), (json!(true), json!([1]), json!([])), "{out}");
    assert!(out["warnings"][0].as_str().unwrap().contains("HTTP 500"), "{out}");
}

#[test]
fn given_crates_with_integration_inputs_or_an_outside_program_when_the_plan_asks_then_neither_can_be_proven() {
    // Given a crate that declares package.metadata.integration and a crate whose tests spawn a docker command
    let repo = Repo::new();
    repo.write("crates/uses/Cargo.toml", "[package]\nname = \"uses\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[package.metadata.integration]\nuses = [\"other\"]\n");
    repo.write("crates/spawn/Cargo.toml", "[package]\nname = \"spawn\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    repo.write("crates/spawn/tests/e2e.rs", "fn t() { std::process::Command::new(\"docker\"); }\n");
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("uses"), crate_unit("spawn")]);
    let server = served(&[]);

    // When the shards are asked about
    let out = ask(&server, &repo, &plan, "2", &[], &[]);

    // Then both run, each saying which input the key leaves out
    let why: Vec<String> = out["shards"].as_array().unwrap().iter().map(|s| s["why"].as_str().unwrap().to_string()).collect();
    assert!(why.iter().any(|w| w.contains("declares integration inputs")), "{why:?}");
    assert!(why.iter().any(|w| w.contains("spawns a program")), "{why:?}");
    assert_eq!(out["run"], json!([1, 2]), "{out}");
}

#[test]
fn given_a_workspace_with_an_unreadable_path_dependency_when_the_plan_asks_then_nothing_is_proven() {
    // Given a workspace whose member names a path dependency that does not exist, so the crate graph cannot be read
    let repo = Repo::new();
    repo.write("Cargo.toml", "[workspace]\nmembers = [\"crates/alpha\"]\n");
    repo.write("crates/alpha/Cargo.toml", "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ngone = { path = \"../gone\" }\n");
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha")]);
    let server = served(&[]);

    // When the shard is asked about
    let out = ask(&server, &repo, &plan, "1", &[], &[]);

    // Then it has no key and runs
    assert!(out["shards"][0]["why"].as_str().unwrap().contains("cannot read the crate graph"), "{out}");
    assert!(out["shards"][0]["key"].is_null(), "{out}");
    assert_eq!(out["run"], json!([1]), "{out}");
}

#[test]
fn given_a_shard_with_no_unit_when_the_plan_asks_then_it_gets_no_runner_even_on_a_fresh_run() {
    // Given one crate dealt over two shards, so one shard holds nothing
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![crate_unit("alpha")]);
    let server = served(&[]);
    let output = repo.dir.path().join("github-output");

    // When the plan asks with --fresh
    let out = ask(&server, &repo, &plan, "2", &["--fresh", "--github-output"], &[("GITHUB_OUTPUT", &output.display().to_string())]);

    // Then only the shard with the crate runs, the empty one is skipped and says why, and it has no marker key
    let run = out["run"].as_array().unwrap().clone();
    let skipped = out["proven"].as_array().unwrap().clone();
    assert_eq!((run.len(), skipped.len()), (1, 1), "{out}");
    let empty = skipped[0].as_u64().unwrap() as usize - 1;
    assert!(out["shards"][empty]["why"].as_str().unwrap().contains("no unit"), "{out}");
    let written = std::fs::read_to_string(output).unwrap();
    assert!(written.contains(&format!("integration_tests_run_shards={}\n", json!(run))), "{written}");
    assert!(server.requests().is_empty(), "{:?}", server.requests());
}

#[test]
fn given_every_shard_empty_when_the_plan_asks_then_no_shard_runs() {
    // Given a stage with no unit over two shards
    let repo = Repo::new();
    let sha = repo.commit();
    let plan = plan_file(repo.dir.path(), &sha, vec![]);
    let server = served(&[]);

    // When the plan asks
    let out = ask(&server, &repo, &plan, "2", &[], &[]);

    // Then the run list is empty
    assert_eq!((out["proven"].clone(), out["run"].clone()), (json!([1, 2]), json!([])), "{out}");
}

#[test]
fn given_a_plan_without_the_stage_when_the_plan_asks_then_no_shard_is_skipped_as_empty() {
    // Given a valid plan that lacks the stage key
    let repo = Repo::new();
    let sha = repo.commit();
    let path = repo.dir.path().join(format!("plan-{sha}.json"));
    std::fs::write(&path, json!({"sha": sha, "stages": {}}).to_string()).unwrap();
    let server = served(&[]);

    // When the plan asks
    let out = ask(&server, &repo, &path.display().to_string(), "2", &["--no-lookup"], &[]);

    // Then no shard is reported proven as empty
    assert_eq!(out["proven"].clone(), json!([]), "{out}");
}
