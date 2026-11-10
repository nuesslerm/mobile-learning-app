//! The record-references trigger of `wf mimic`: the commit marker that asks the VM runner for a capture job (`trigger`), how a set's reference is
//! installed from data (`provision`), the chunked job itself (`job`, over a double of the checkpointed `record`) and the transport of a
//! recorded set to another checkout (`publish`, `fetch`, over a stub of the artifacts bucket).
use crate::support::*;
use std::path::{Path, PathBuf};

fn reporoot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const PROVISION: &str = "[provision]\n[provision.toolchain]\nnode = \"24\"\n[[provision.step]]\nname = \"build\"\nrun = [\"true\"]\n";

/// A checkout with sets of the given names: each a directory with a mimic.toml, a corpus and a provision block.
fn checkout(sets: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for s in sets {
        add_set(dir.path(), s, PROVISION);
    }
    dir
}

fn add_set(root: &Path, rel: &str, extra: &str) {
    let set = root.join(rel);
    std::fs::create_dir_all(&set).unwrap();
    std::fs::write(set.join("pin.json"), r#"{"version":"1"}"#).unwrap();
    std::fs::write(set.join("corpus.json"), "{}").unwrap();
    std::fs::write(set.join("mimic.toml"), format!("[reference]\ncommand = [\"true\"]\npin = \"pin.json\"\n[candidate.main]\ncommand = [\"true\"]\n{extra}")).unwrap();
}

fn trigger(root: &Path, message: &str) -> Run {
    mimic(&["trigger", "--root", &root.display().to_string(), "--message", message], &[])
}

fn names(r: &Run) -> Vec<String> {
    r.json["sets"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn a_record_marker_names_its_sets_chunk_and_fresh_and_a_replay_marker_only_its_sets() {
    // Given a checkout with three sets, one of them reachable by path only because two directories share its name
    let root = checkout(&["a/alpha", "b/beta", "c/gamma"]);
    add_set(root.path(), "d/beta", PROVISION);
    // When the head commit of a push carries a record marker with several sets, a chunk and fresh
    let r = trigger(root.path(), "re-record\n\n[record alpha b/beta gamma chunk=15 fresh]\n");
    // Then every set is a job, in order, with the chunk and fresh the marker gave and the node its provision block pins
    assert_eq!((r.code, r.json["go"].clone(), r.json["action"].clone()), (0, json!(true), json!("record")), "{}", r.json);
    assert_eq!((names(&r), r.json["chunk"].clone(), r.json["fresh"].clone()), (vec!["alpha".into(), "beta".into(), "gamma".into()], json!(15), json!(true)));
    let jobs = r.json["matrix"]["include"].as_array().unwrap();
    assert_eq!((jobs[1]["dir"].clone(), jobs[0]["node"].clone()), (json!("b/beta"), json!("24")));
    // When a record marker gives neither, and a replay marker names a recorded set
    let defaults = trigger(root.path(), "[record alpha]");
    std::fs::write(root.path().join("a/alpha/cassette.json"), "{}").unwrap();
    let replay = trigger(root.path(), "[replay alpha]");
    // Then the chunk is ten minutes and nothing starts over, and the replay is a job for the candidates of the set
    assert_eq!((defaults.json["chunk"].clone(), defaults.json["fresh"].clone()), (json!(10), json!(false)));
    assert_eq!((replay.code, replay.json["action"].clone(), replay.json["matrix"]["include"][0]["candidates"].clone()), (0, json!("replay"), json!(["main"])), "{}", replay.json);
}

#[test]
fn a_message_without_a_marker_starts_nothing() {
    // Given a checkout
    let root = checkout(&["a/alpha"]);
    for message in ["fix the recorder", "mention [recording] and [records alpha] in prose", "", "notes [record alpha", "we write [record alpha] or [replay alpha] after the subject", "subject\n\n  [record alpha]\n"] {
        // When a commit message that is not a marker is read
        let r = trigger(root.path(), message);
        // Then it succeeds and asks for no job
        assert_eq!((r.code, r.json["ok"].clone(), r.json["go"].clone(), r.json["matrix"]["include"].clone()), (0, json!(true), json!(false), json!([])), "{message:?}: {}", r.json);
    }
}

#[test]
fn a_malformed_marker_or_an_unknown_set_fails_with_the_reason() {
    // Given a checkout whose sets are alpha and gamma, gamma without a provision block, and a twin directory name
    let root = checkout(&["a/alpha", "x/twin", "y/twin"]);
    add_set(root.path(), "c/gamma", "");
    let cases = [
        ("[record]", "names no set"),
        ("[record alpha", "not closed"),
        ("[record alpha chunk=0]", "outside 1..="),
        ("[record alpha chunk=x]", "not a number"),
        ("[record alpha chunk=5 chunk=6]", "twice"),
        ("[record alpha]\n[replay alpha]", "more than one"),
        ("[replay alpha chunk=5]", "takes sets only"),
        ("[record ../etc]", "neither a set name"),
        ("[record nope]", "unknown set \"nope\""),
        ("[record twin]", "names 2 sets"),
        ("[record gamma]", "no [provision]"),
        ("[replay alpha]", "no cassette.json"),
    ];
    for (message, reason) in cases {
        // When the message is read
        let r = trigger(root.path(), message);
        // Then it is refused, the error says why (and an unknown set lists the known ones), and no job is planned
        assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)), "{message}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(reason), "{message}: {}", r.json["error"]);
        assert!(r.json["next"].as_str().is_some_and(|n| !n.is_empty()));
    }
    let unknown = trigger(root.path(), "[record nope]");
    assert!(unknown.json["error"].as_str().unwrap().contains("alpha"), "{}", unknown.json["error"]);
}

#[test]
fn every_set_of_the_repository_that_declares_provisioning_validates() {
    // Given every set of this repository whose loaded config has a provisioning block (inline or in a file), as the loader sees it
    let root = reporoot();
    let declared: Vec<PathBuf> = wf_mimic::marker::all_sets(&root).into_iter().filter(|d| wf_mimic::config::Config::load(&root.join(d).join("mimic.toml")).is_ok_and(|c| c.provision.is_some())).collect();
    // Then the T3 and Orca recording sets are among them, so the loop below cannot pass on a subset
    for app in ["clis/t3-serve/cassettes", "clis/orca-serve/cassettes"] {
        assert!(declared.iter().any(|d| d.starts_with(app)), "no set under {app} declares provisioning: {declared:?}");
    }
    for dir in declared {
        // When its block is checked
        let r = mimic(&["provision", &root.join(&dir).display().to_string(), "--check", "--root", &root.display().to_string()], &[]);
        // Then it loads: the schema, the pin it reads the commit from and the placeholders it uses are all valid
        assert_eq!((r.code, r.json["ok"].clone()), (0, json!(true)), "{}: {}", dir.display(), r.json);
        assert!(!r.json["steps"].as_array().unwrap().is_empty(), "{}", dir.display());
    }
}

fn bad_block(block: &str) -> Run {
    let root = tempfile::tempdir().unwrap();
    add_set(root.path(), "s", block);
    mimic(&["provision", &root.path().join("s").display().to_string(), "--check"], &[])
}

#[test]
fn a_provision_block_that_floats_a_ref_leaks_a_credential_or_misuses_a_placeholder_is_refused() {
    let step = "[[provision.step]]\nname = \"s\"\nrun = [\"true\"]\n";
    let cases = [
        (format!("[provision]\n[provision.source]\nrepo = \"https://example.test/o/r\"\n{step}"), "needs a commit or a pin"),
        (format!("[provision]\n[provision.source]\nrepo = \"https://example.test/o/r\"\ncommit = \"main\"\n{step}"), "40-hex"),
        (format!("[provision]\n[provision.source]\nrepo = \"http://example.test/o/r\"\ncommit = \"{}\"\n{step}", "a".repeat(40)), "https URL"),
        (format!("[provision]\n[provision.source]\nrepo = \"https://user:pw@example.test/o/r\"\ncommit = \"{}\"\n{step}", "a".repeat(40)), "no credentials"),
        (format!("[provision]\n[provision.env]\nGH_TOKEN = \"x\"\n{step}"), "looks like a credential"),
        (format!("[provision]\n[provision.env]\nROOT = \"{{nowhere}}\"\n{step}"), "placeholders are"),
        (format!("[provision]\n[provision.env]\nROOT = \"{{source}}\"\n{step}"), "no [source]"),
        ("[provision]\n[provision.toolchain]\nnode = \"latest\"\n".to_string() + step, "not a version"),
        ("[provision]\n[provision.toolchain]\nruby = \"3\"\n".to_string() + step, "not one the runner sets up"),
        (format!("[provision]\n[provision.env]\nPATH = \"/tmp/evil\"\n{step}"), "changes how programs are found"),
        (format!("[provision]\n[provision.env]\nLD_PRELOAD = \"/tmp/evil.so\"\n{step}"), "changes how programs are found"),
        ("[provision]\n[[provision.step]]\nname = \"s\"\nrun = [\"true\"]\n[provision.step.env]\nNODE_OPTIONS = \"--require /tmp/x\"\n".to_string(), "changes how programs are found"),
        (format!("[provision]\n[provision.env]\nHTTPS_PROXY = \"http://evil.test\"\n{step}"), "changes how programs are found"),
        ("[provision]\n".to_string(), "no [[step]]"),
        (format!("[provision]\n{step}{step}"), "used twice"),
        (format!("{step}\ntimeout_minutes = 0\n").replace("[[provision.step]]", "[provision]\n[[provision.step]]"), "timeout_minutes"),
    ];
    for (block, reason) in cases {
        // When a mimic.toml carries the block
        let r = bad_block(&block);
        // Then loading it fails and says what is wrong
        assert_eq!(r.code, 2, "{block}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(reason), "{block}: {}", r.json["error"]);
    }
}

struct Source {
    _dir: tempfile::TempDir,
    url_rewrite: (String, String),
    commit: String,
}

/// A local repository with one commit and the git settings that let `https://example.test/o/r` resolve to it.
fn source() -> Source {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").arg("-C").arg(dir.path()).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("marker.txt"), "pinned").unwrap();
    git(&["add", "."]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "one"]);
    git(&["config", "uploadpack.allowAnySHA1InWant", "true"]);
    let commit = git(&["rev-parse", "HEAD"]);
    let url_rewrite = (format!("url.file://{}.insteadOf", dir.path().display()), "https://example.test/o/r".to_string());
    Source { _dir: dir, url_rewrite, commit }
}

fn provision_with_source(src: &Source, work: &Path, steps: &str, extra_env: &[(&str, &Path)]) -> (tempfile::TempDir, Run) {
    let root = tempfile::tempdir().unwrap();
    add_set(root.path(), "s", &format!("[provision]\ndrivers = false\n[provision.toolchain]\nnode = \"{}\"\n[provision.source]\nrepo = \"https://example.test/o/r\"\ncommit = \"{}\"\n[provision.env]\nREF_ROOT = \"{{source}}\"\n{steps}", node_major(), src.commit));
    let (key, value) = (&src.url_rewrite.0, &src.url_rewrite.1);
    let (k, v) = (Path::new(key), Path::new(value));
    let mut env = vec![("GIT_CONFIG_COUNT", Path::new("1")), ("GIT_CONFIG_KEY_0", k), ("GIT_CONFIG_VALUE_0", v)];
    env.extend_from_slice(extra_env);
    let r = mimic(&["provision", &root.path().join("s").display().to_string(), "--work-root", &work.display().to_string()], &env);
    (root, r)
}

fn node_major() -> String {
    let out = std::process::Command::new("node").arg("--version").output().expect("node is on PATH");
    String::from_utf8_lossy(&out.stdout).trim().trim_start_matches('v').split('.').next().unwrap().to_string()
}

#[test]
fn provisioning_fetches_the_pinned_commit_runs_the_steps_without_credentials_and_reuses_a_finished_work_directory() {
    // Given a source repository, a step that records what it saw and one that writes beside the source
    let src = source();
    let work = tempfile::tempdir().unwrap();
    let steps = "[[provision.step]]\nname = \"see\"\nrun = [\"sh\", \"-c\", \"echo t=$DEPLOY_TOKEN > {work}/token.txt; echo run >> {work}/runs.txt; cat marker.txt > {work}/seen.txt\"]\n";
    // When it is provisioned with a credential in the caller's environment
    let (root, r) = provision_with_source(&src, work.path(), steps, &[("DEPLOY_TOKEN", Path::new("hunter2"))]);
    // Then the step ran in the source at the pinned commit and never saw the credential, and the block's env comes back resolved
    assert_eq!((r.code, r.json["cached"].clone()), (0, json!(false)), "{}\n{}", r.json, r.stderr);
    let dir = work.path().join("s");
    assert_eq!(std::fs::read_to_string(dir.join("seen.txt")).unwrap().trim(), "pinned");
    assert_eq!(std::fs::read_to_string(dir.join("token.txt")).unwrap().trim(), "t=");
    assert_eq!(r.json["env"]["REF_ROOT"], json!(dir.join("source").display().to_string()));
    assert!(!r.json.to_string().contains("hunter2") && !r.stderr.contains("hunter2"));
    // When the same block is provisioned again
    let again = mimic(&["provision", &root.path().join("s").display().to_string(), "--work-root", &work.path().display().to_string()], &[("GIT_CONFIG_COUNT", Path::new("1")), ("GIT_CONFIG_KEY_0", Path::new(&src.url_rewrite.0)), ("GIT_CONFIG_VALUE_0", Path::new(&src.url_rewrite.1))]);
    // Then nothing runs a second time
    assert_eq!((again.code, again.json["cached"].clone()), (0, json!(true)), "{}", again.json);
    assert_eq!(std::fs::read_to_string(dir.join("runs.txt")).unwrap().lines().count(), 1);
}

#[test]
fn a_failing_step_stops_provisioning_and_names_the_step_and_a_wrong_node_is_refused_before_anything_runs() {
    // Given a block whose second step fails
    let src = source();
    let work = tempfile::tempdir().unwrap();
    let steps = "[[provision.step]]\nname = \"fine\"\nrun = [\"true\"]\n[[provision.step]]\nname = \"broken\"\nrun = [\"sh\", \"-c\", \"exit 3\"]\n[[provision.step]]\nname = \"never\"\nrun = [\"sh\", \"-c\", \"touch {work}/never\"]\n";
    // When it is provisioned
    let (_root, r) = provision_with_source(&src, work.path(), steps, &[]);
    // Then it fails at that step with the exit code, and the steps after it did not run
    assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)), "{}", r.json);
    let error = r.json["error"].as_str().unwrap();
    assert!(error.contains("broken") && error.contains("exited 3"), "{error}");
    assert!(!work.path().join("s/never").exists());
    // When the set pins a Node that is not the one on PATH
    let root = tempfile::tempdir().unwrap();
    add_set(root.path(), "s", "[provision]\ndrivers = false\n[provision.toolchain]\nnode = \"1\"\n[[provision.step]]\nname = \"s\"\nrun = [\"touch\", \"{work}/ran\"]\n");
    let wrong = mimic(&["provision", &root.path().join("s").display().to_string(), "--work-root", &work.path().display().to_string()], &[]);
    // Then it is refused and nothing ran
    assert_eq!(wrong.code, 2);
    assert!(wrong.json["error"].as_str().unwrap().contains("pins node 1") || wrong.json["error"].as_str().unwrap().contains("node --version"), "{}", wrong.json["error"]);
    assert!(!work.path().join("s/ran").exists());
}

/// A stand-in for `wf-mimic`: its `record --help` lists the flags the checkpointed record has (or not, with `FAKE_NO_RESUME`), and each
/// `record <set> --resume --max-minutes n` call leaves a checkpoint, reports complete:false until `FAKE_DONE_AT` calls were made, then writes the set.
const FAKE: &str = r#"#!/bin/sh
[ "$1" = mimic ] || exit 2
case "$2" in
  record)
    if [ "$3" = --help ]; then
      [ -n "$FAKE_NO_RESUME" ] && echo "  --keep-raw" || echo "  --resume  --max-minutes <n>"
      exit 0
    fi
    set="$3"
    mkdir -p "$set/raw/checkpoint"
    echo "REF_HOME=$REF_HOME PATH_KEPT=$([ -n "$PATH" ] && echo yes)" > "$set/seen-env.txt"
    echo "$@" > "$set/args.txt"
    [ -n "$FAKE_FAIL" ] && { echo '{"ok":false,"error":"boom"}'; exit 3; }
    [ -n "$FAKE_CKPT_LEAK" ] && echo "Bearer abcdefgh12345" > "$set/raw/checkpoint/cap.jsonl"
    n=$(cat "$set/raw/count" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$set/raw/count"
    if [ "$n" -lt "${FAKE_DONE_AT:-1}" ]; then
      echo '{"ok":true,"set":"s","interactions":4,"complete":false,"nondeterministic":[],"warnings":[]}'
      exit 0
    fi
    echo '{"schema":"mimic-cassette/1"}' > "$set/cassette.json"
    echo "{\"id\":\"a\",\"output\":\"${FAKE_LEAK:-fine}\"}" > "$set/interactions.jsonl"
    echo '{"ok":true,"set":"s","interactions":8,"complete":true,"nondeterministic":["x"],"warnings":[]}'
    ;;
  verify)
    [ -n "$FAKE_ACCOUNTING" ] && { echo '{"ok":false,"sets":[{"checks":[],"problems":[{"field":"stability","message":"x is tagged nondeterministic but not listed"}]}]}'; exit 1; }
    [ -n "$FAKE_STALE" ] && { echo '{"ok":false,"sets":[{"checks":[{"name":"rules_hash","status":"stale"}],"problems":[{"field":"stability","message":"x"}]}]}'; exit 1; }
    echo '{"ok":true,"warnings":[]}' ;;
  *) exit 2 ;;
esac
"#;

fn fake(dir: &Path) -> PathBuf {
    let path = dir.join("fake-wf-mimic");
    std::fs::write(&path, FAKE).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

struct Job {
    dir: tempfile::TempDir,
    set: PathBuf,
    bin: PathBuf,
}

fn job() -> Job {
    let dir = tempfile::tempdir().unwrap();
    add_set(dir.path(), "s", PROVISION);
    let bin = fake(dir.path());
    Job { set: dir.path().join("s"), bin, dir }
}

impl Job {
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let set = self.set.display().to_string();
        let mut all = vec!["job", "record", set.as_str()];
        all.extend_from_slice(args);
        let mut e: Vec<(&str, &Path)> = vec![("WF_MIMIC_BIN", &self.bin)];
        e.extend(env.iter().map(|(k, v)| (*k, Path::new(*v))));
        mimic(&all, &e)
    }
    fn summary(&self) -> PathBuf {
        self.dir.path().join("summary.md")
    }
}

#[test]
fn a_record_job_runs_chunks_until_the_record_is_complete_then_drops_the_raw_capture_and_reports_counts_and_stability() {
    // Given a record that needs three bounded chunks
    let j = job();
    let summary = j.summary();
    // When the job runs with a ten minute chunk
    let r = j.run(&["--chunk", "10", "--summary", &summary.display().to_string(), "--run", "77"], &[("FAKE_DONE_AT", "3")]);
    // Then three chunks ran, the set is complete with its counts and stability, raw/ is gone and the set was verified
    assert_eq!((r.code, r.json["ok"].clone(), r.json["complete"].clone()), (0, json!(true), json!(true)), "{}\n{}", r.json, r.stderr);
    assert_eq!(r.json["chunks"].as_array().unwrap().len(), 3);
    assert_eq!((r.json["interactions"].clone(), r.json["nondeterministic"].clone(), r.json["stable_pct"].clone()), (json!(8), json!(1), json!(87.5)));
    assert!(!j.set.join("raw").exists(), "the raw capture is deleted before anything is uploaded");
    assert!(j.set.join("cassette.json").is_file());
    let md = std::fs::read_to_string(&summary).unwrap();
    assert!(md.contains("complete") && md.contains("87.5%") && md.contains("3 x 10 min"), "{md}");
}

#[test]
fn a_record_job_that_runs_out_of_budget_keeps_its_checkpoint_and_says_how_to_resume() {
    // Given a record that never finishes
    let j = job();
    let summary = j.summary();
    // When the job has room for one one-minute chunk only
    let r = j.run(&["--chunk", "1", "--budget", "1", "--summary", &summary.display().to_string()], &[("FAKE_DONE_AT", "99")]);
    // Then it stops after that chunk, red, the checkpoint stays and the summary names the marker that resumes it
    assert_eq!((r.code, r.json["complete"].clone(), r.json["chunks"].as_array().map(Vec::len)), (1, json!(false), Some(1)), "{}", r.json);
    assert!(j.set.join("raw/checkpoint").is_dir());
    assert!(r.json["next"].as_str().unwrap().contains("[record s]"));
    assert!(std::fs::read_to_string(&summary).unwrap().contains("INCOMPLETE"));
    // When the job is told to start fresh
    let fresh = j.run(&["--chunk", "1", "--budget", "1", "--fresh"], &[("FAKE_DONE_AT", "1")]);
    // Then the checkpoint of the earlier run is gone and the record completes in one chunk
    assert_eq!((fresh.code, fresh.json["chunks"].as_array().map(Vec::len)), (0, Some(1)), "{}", fresh.json);
}

#[test]
fn a_masked_set_with_a_token_shaped_value_is_held_back_and_a_record_without_resume_fails_naming_the_flag() {
    // Given a record whose masked output still holds a bearer token
    let j = job();
    // When the job completes it
    let r = j.run(&[], &[("FAKE_LEAK", "Bearer abcdefgh12345")]);
    // Then the set is held back with the file and line, so it is never published
    assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{}", r.json);
    assert!(r.json["leaks"][0].as_str().unwrap().contains("interactions.jsonl:1 has a Bearer token"), "{}", r.json["leaks"]);
    // Given a wf-mimic whose record has no --resume yet
    let old = j.run(&[], &[("FAKE_NO_RESUME", "1")]);
    // Then the job fails at once and the message names the missing flags
    assert_eq!(old.code, 2);
    let error = old.json["error"].as_str().unwrap();
    assert!(error.contains("--resume") && error.contains("--max-minutes"), "{error}");
}

#[test]
fn a_record_job_passes_the_provision_env_to_the_record_alone_and_keeps_the_stability_check_unless_the_set_opts_out() {
    // Given a set whose provision block has an env, and the default of no opt-out
    let j = job();
    let work = j.dir.path().join("work");
    std::fs::write(j.set.join("mimic.toml"), format!("{}{}", std::fs::read_to_string(j.set.join("mimic.toml")).unwrap(), "[provision.env]\nREF_HOME = \"{work}/home\"\n")).unwrap();
    let work_root = work.display().to_string();
    // When the job records it
    let r = j.run(&["--work-root", &work_root], &[]);
    // Then the record saw the resolved variable, was not told to allow nondeterminism, and the job wrote nothing to a GITHUB_ENV
    assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
    assert_eq!(std::fs::read_to_string(j.set.join("seen-env.txt")).unwrap().trim(), format!("REF_HOME={}/s/home PATH_KEPT=yes", work.display()));
    let args = std::fs::read_to_string(j.set.join("args.txt")).unwrap();
    assert!(args.contains("--recorded-on ci") && !args.contains("--allow-nondeterministic"), "{args}");
    // When the set opts out of the check and adds a flag of its own through [record]
    std::fs::write(j.set.join("mimic.toml"), format!("{}{}", std::fs::read_to_string(j.set.join("mimic.toml")).unwrap(), "[record]\nallow_nondeterministic = true\nargs = [\"--keep-raw\"]\n")).unwrap();
    let opted = j.run(&["--fresh"], &[]);
    // Then the record call carries both
    assert_eq!(opted.code, 0, "{}\n{}", opted.json, opted.stderr);
    let args = std::fs::read_to_string(j.set.join("args.txt")).unwrap();
    assert!(args.contains("--allow-nondeterministic") && args.contains("--keep-raw"), "{args}");
    // When a set's [record] args try to override a flag the job sets itself
    std::fs::write(j.set.join("mimic.toml"), std::fs::read_to_string(j.set.join("mimic.toml")).unwrap().replace("--keep-raw", "--recorded-on")).unwrap();
    let refused = j.run(&[], &[]);
    // Then the config is refused
    assert_eq!(refused.code, 2, "{}", refused.json);
    assert!(refused.json["error"].as_str().unwrap().contains("sets itself"), "{}", refused.json["error"]);
}

#[test]
fn a_set_that_allows_nondeterminism_is_published_when_only_its_stability_accounting_is_stale_and_never_when_anything_else_is() {
    // Given a set that allows nondeterminism, whose verify fails on the stability allowlist alone
    let j = job();
    std::fs::write(j.set.join("mimic.toml"), format!("{}[record]\nallow_nondeterministic = true\n", std::fs::read_to_string(j.set.join("mimic.toml")).unwrap())).unwrap();
    // When the job completes the record
    let r = j.run(&[], &[("FAKE_ACCOUNTING", "1")]);
    // Then it is not held back, and says the allowlist is for the commit to settle
    assert_eq!((r.code, r.json["ok"].clone(), r.json["verify"]["accounting_deferred"].clone()), (0, json!(true), json!(true)), "{}", r.json);
    // When a freshness check fails as well
    let stale = j.run(&["--fresh"], &[("FAKE_STALE", "1")]);
    // Then the set is held back
    assert_eq!((stale.code, stale.json["ok"].clone(), stale.json["verify"]["accounting_deferred"].clone()), (1, json!(false), json!(false)), "{}", stale.json);
    // and a set that does not allow nondeterminism is held back on the accounting too
    let strict = job();
    let held = strict.run(&[], &[("FAKE_ACCOUNTING", "1")]);
    assert_eq!((held.code, held.json["ok"].clone()), (1, json!(false)), "{}", held.json);
}

#[test]
fn a_checkpoint_is_resumed_only_by_the_record_it_belongs_to_and_is_kept_for_upload_only_when_the_run_ran_out_of_budget_cleanly() {
    // Given a record that ran out of budget and left a checkpoint with state in it
    let j = job();
    let first = j.run(&["--chunk", "1", "--budget", "1"], &[("FAKE_DONE_AT", "99")]);
    assert_eq!((first.code, first.json["incomplete"].clone(), first.json["checkpoint_upload"].clone()), (1, json!(true), json!(true)), "{}", first.json);
    std::fs::write(j.set.join("raw/checkpoint/state.txt"), "mine").unwrap();
    // When the same record runs again with nothing changed
    let again = j.run(&["--chunk", "1", "--budget", "1"], &[("FAKE_DONE_AT", "99")]);
    // Then the checkpoint is resumed: its state is still there and nothing warns
    assert!(j.set.join("raw/checkpoint/state.txt").is_file());
    assert_eq!(again.json["warnings"], json!([]), "{}", again.json);
    // When the corpus changed since that checkpoint
    std::fs::write(j.set.join("corpus.json"), r#"{"changed":true}"#).unwrap();
    let stale = j.run(&["--chunk", "1", "--budget", "1"], &[("FAKE_DONE_AT", "99")]);
    // Then the checkpoint is dropped and the record starts over, and the job says so
    assert!(!j.set.join("raw/checkpoint/state.txt").exists());
    assert!(stale.json["warnings"][0].as_str().unwrap().contains("dropped"), "{}", stale.json);
    // When a leg fails, the job does not call it incomplete, so no checkpoint is uploaded
    let failed = j.run(&[], &[("FAKE_FAIL", "1")]);
    assert_eq!(failed.code, 2, "{}", failed.json);
    assert!(failed.json["incomplete"].is_null(), "{}", failed.json);
    // When an unfinished record's checkpoint holds a token-shaped value
    let leaky = j.run(&["--fresh", "--chunk", "1", "--budget", "1"], &[("FAKE_DONE_AT", "99"), ("FAKE_CKPT_LEAK", "1")]);
    // Then it is incomplete but must not be uploaded, and the file is named
    assert_eq!((leaky.json["incomplete"].clone(), leaky.json["checkpoint_upload"].clone()), (json!(true), json!(false)), "{}", leaky.json);
    assert!(leaky.json["checkpoint_leaks"][0].as_str().unwrap().contains("cap.jsonl:1 has a Bearer token"), "{}", leaky.json);
}

#[test]
fn a_provision_file_named_from_the_repository_root_is_found_from_any_depth() {
    // Given a checkout with a shared provision file and a set deep in the tree that names it from the root
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".git")).unwrap();
    std::fs::create_dir_all(root.path().join("shared")).unwrap();
    std::fs::write(root.path().join("shared/p.toml"), "[toolchain]\nnode = \"24\"\n[[step]]\nname = \"s\"\nrun = [\"true\"]\n").unwrap();
    add_set(root.path(), "a/b/c/d/deep", "");
    let toml = root.path().join("a/b/c/d/deep/mimic.toml");
    std::fs::write(&toml, format!("provision_file = \"//shared/p.toml\"\n{}", std::fs::read_to_string(&toml).unwrap())).unwrap();
    // When the set's provisioning is checked
    let r = mimic(&["provision", &root.path().join("a/b/c/d/deep").display().to_string(), "--check"], &[]);
    // Then the shared block is found, and a node toolchain brings the repository's drivers with it
    assert_eq!((r.code, r.json["steps"].clone()), (0, json!(["drivers", "s"])), "{}", r.json);
}

#[test]
fn a_replay_job_tabulates_every_candidate_and_is_red_only_when_one_differs() {
    // Given the toy set, recorded, with a candidate that behaves and one that changes a value
    let w = World::new();
    w.record();
    let toy = toy().display().to_string();
    w.write("mimic.toml", &format!("[reference]\ncommand = [{toy:?}]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n[candidate.same]\ncommand = [{toy:?}]\n[candidate.change]\ncommand = [{toy:?}, \"--tweak\", \"change-value\"]\n"));
    let summary = w.dir.path().join("summary.md");
    // When every candidate is replayed
    let set = w.arg();
    let r = mimic(&["job", "replay", &set, "--summary", &summary.display().to_string(), "--out", &w.dir.path().join("out").display().to_string()], &[]);
    // Then the table has a row per candidate, the differing one with its first difference, and the run is red
    assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{}\n{}", r.json, r.stderr);
    let md = std::fs::read_to_string(&summary).unwrap();
    assert!(md.contains("| same | none |") && md.contains("| change | drift |") && md.contains("| change |"), "{md}");
    assert!(w.dir.path().join("out/toy-set.replay.json").is_file());
    // When only the candidate that behaves is replayed
    let clean = mimic(&["job", "replay", &set, "--candidate", "same"], &[]);
    // Then the run is green
    assert_eq!((clean.code, clean.json["ok"].clone()), (0, json!(true)), "{}", clean.json);
}

/// A stand-in for the artifacts bucket: PUT stores a body under its path, GET serves it, 404 otherwise; `corrupt` flips what GET answers for
/// one path to prove the hash check.
struct Bucket {
    base: String,
    puts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
}

fn bucket(corrupt: Option<&'static str>) -> Bucket {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", server.server_addr().to_ip().unwrap().port());
    let puts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>> = Default::default();
    let store = puts.clone();
    std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let path = req.url().to_string();
            if req.method() == &tiny_http::Method::Put {
                let mut body = Vec::new();
                std::io::Read::read_to_end(req.as_reader(), &mut body).unwrap();
                store.lock().unwrap().insert(path, body);
                let _ = req.respond(tiny_http::Response::empty(200));
            } else {
                let found = store.lock().unwrap().get(&path).cloned();
                let _ = match found {
                    Some(mut body) => {
                        if corrupt.is_some_and(|c| path.ends_with(c)) {
                            body.extend_from_slice(b" ");
                        }
                        req.respond(tiny_http::Response::from_data(body))
                    }
                    None => req.respond(tiny_http::Response::empty(404)),
                };
            }
        }
    });
    Bucket { base, puts }
}

fn bucket_env(b: &Bucket) -> (String, String) {
    (format!("{}/par/", b.base), format!("{}/par", b.base))
}

#[test]
fn a_recorded_set_published_from_one_checkout_is_fetched_whole_into_another_without_its_raw_capture() {
    // Given a recorded toy set with a raw capture beside it, and a stub bucket
    let w = World::new();
    w.record();
    std::fs::create_dir_all(w.path("raw")).unwrap();
    w.write("raw/capture.jsonl", "{\"secret\":\"unmasked\"}\n");
    let b = bucket(None);
    let (par, read) = bucket_env(&b);
    let env = [("VERIFICATION_ARTIFACTS_PAR_URL", Path::new(&par)), ("VERIFICATION_ARTIFACTS_READ_URL", Path::new(&read)), ("NO_PROXY", Path::new("127.0.0.1")), ("no_proxy", Path::new("127.0.0.1"))];
    // When the set is published as run 42
    let p = mimic(&["publish", &w.arg(), "--run", "42", "--root", &w.dir.path().display().to_string()], &env);
    // Then every kept file is in the bucket and raw/ is not, and the output never shows the write link
    assert_eq!((p.code, p.json["ok"].clone()), (0, json!(true)), "{}\n{}", p.json, p.stderr);
    let keys: Vec<String> = b.puts.lock().unwrap().keys().cloned().collect();
    assert!(keys.iter().any(|k| k.ends_with("/42/toy-set/cassette.json")) && keys.iter().any(|k| k.ends_with("/42/manifest.json")) && keys.iter().any(|k| k.ends_with("/latest/toy-set.json")), "{keys:?}");
    assert!(!keys.iter().any(|k| k.contains("raw")), "{keys:?}");
    assert!(!p.json.to_string().contains(&par) && !p.stderr.contains(&par));
    // When another checkout fetches the run, by id and as latest
    let other = tempfile::tempdir().unwrap();
    let f = mimic(&["fetch", "42", "--root", &other.path().display().to_string()], &env);
    let latest = mimic(&["fetch", "latest", "--set", "toy-set", "--root", &other.path().display().to_string()], &env);
    // Then the set is there byte for byte, without raw/, and it verifies
    assert_eq!((f.code, f.json["ok"].clone(), latest.code), (0, json!(true), 0), "{}\n{}", f.json, f.stderr);
    for file in ["cassette.json", "interactions.jsonl", "rules.json", "corpus.json", "mimic.toml"] {
        assert_eq!(std::fs::read(other.path().join("toy-set").join(file)).unwrap(), std::fs::read(w.path(file)).unwrap(), "{file}");
    }
    assert!(!other.path().join("toy-set/raw").exists());
}

#[test]
fn a_fetched_file_that_does_not_match_its_manifest_hash_is_refused_and_an_unknown_run_says_so() {
    // Given a published run whose interactions file is altered in transit
    let w = World::new();
    w.record();
    let b = bucket(Some("interactions.jsonl"));
    let (par, read) = bucket_env(&b);
    let env = [("VERIFICATION_ARTIFACTS_PAR_URL", Path::new(&par)), ("VERIFICATION_ARTIFACTS_READ_URL", Path::new(&read)), ("NO_PROXY", Path::new("127.0.0.1")), ("no_proxy", Path::new("127.0.0.1"))];
    let p = mimic(&["publish", &w.arg(), "--run", "7", "--root", &w.dir.path().display().to_string()], &env);
    assert_eq!(p.code, 0, "{}", p.json);
    // When it is fetched
    let other = tempfile::tempdir().unwrap();
    let f = mimic(&["fetch", "7", "--root", &other.path().display().to_string()], &env);
    // Then the hash mismatch stops it, and the altered file is not written
    assert_eq!(f.code, 2, "{}", f.json);
    assert!(f.json["error"].as_str().unwrap().contains("does not match the hash"), "{}", f.json["error"]);
    assert!(!other.path().join("toy-set/interactions.jsonl").exists());
    // When a run that was never published is fetched
    let none = mimic(&["fetch", "404", "--root", &other.path().display().to_string()], &env);
    // Then it says the run does not exist
    assert_eq!(none.code, 2);
    assert!(none.json["error"].as_str().unwrap().contains("does not exist"), "{}", none.json["error"]);
}

fn workflow() -> String {
    std::fs::read_to_string(reporoot().join(".github/workflows/record-references.yml")).expect("the record-references workflow")
}

/// Every `<prefix>.<key>` the workflow text reads.
fn keys_read(text: &str, prefix: &str) -> std::collections::BTreeSet<String> {
    regex::Regex::new(&format!(r"{}\.([A-Za-z_]+)", regex::escape(prefix))).unwrap().captures_iter(text).map(|c| c[1].to_string()).collect()
}

#[test]
fn every_key_the_workflow_reads_from_the_plan_is_emitted_by_the_trigger_for_a_record_and_a_replay_plan() {
    // Given the workflow file and a checkout with a recorded set that has a provision block
    let text = workflow();
    let root = checkout(&["a/alpha"]);
    std::fs::write(root.path().join("a/alpha/cassette.json"), "{}").unwrap();
    for message in ["[record alpha]", "[replay alpha]"] {
        // When the trigger plans the marker
        let r = trigger(root.path(), message);
        assert_eq!(r.code, 0, "{message}: {}", r.json);
        // Then every needs.plan.outputs.<x> is a top-level field of the plan, and every matrix.<x> a key of each job
        for key in keys_read(&text, "needs.plan.outputs") {
            assert!(r.json.get(&key).is_some(), "{message}: the workflow reads needs.plan.outputs.{key}, the trigger does not emit it: {}", r.json);
        }
        let job = r.json["matrix"]["include"][0].as_object().unwrap();
        for key in keys_read(&text, "matrix") {
            assert!(job.contains_key(&key), "{message}: the workflow reads matrix.{key}, the trigger does not emit it: {job:?}");
        }
    }
    // And the workflow's plan step reads those top-level fields through jq under the same names
    for key in ["action", "chunk", "fresh", "matrix"] {
        assert!(text.contains(&format!("jq -c .{key} ")) || text.contains(&format!("jq -r .{key} ")), "the plan step does not read .{key}");
    }
}

#[test]
fn the_workflow_gate_starts_only_on_the_marker_forms_the_parser_accepts() {
    // Given the gate condition of the workflow that starts it: the router of the commit-tag triggers
    let text = std::fs::read_to_string(reporoot().join(".github/workflows/scheduled-triggers.yml")).expect("the scheduled-triggers workflow");
    for word in ["record", "replay"] {
        // Then it matches a marker at the start of the message and at the start of a later line, as the parser does, and not anywhere in a line
        assert!(text.contains(&format!("startsWith(github.event.head_commit.message, '[{word} ')")), "{word}: no start-of-message form");
        assert!(text.contains(&format!("format('{{0}}[{word} ', fromJSON('\"\\n\"'))")), "{word}: no start-of-line form");
        assert!(!text.contains(&format!("contains(github.event.head_commit.message, '[{word} ')")), "{word}: a mid-line match would start the run for prose");
    }
}

#[test]
fn a_record_job_names_the_path_that_differed_between_the_two_stability_passes_of_a_nondeterministic_interaction() {
    // Given a set whose reference answers a case with a number that changes on every run, and that tolerates nondeterminism
    let w = World::with_corpus(&[json!({"kind": "corpus", "name": "wobble", "op": "wobble", "cases": [{"id": "wobble-1", "input": {}}]})]);
    w.write("mimic.toml", &format!("{}[record]\nallow_nondeterministic = true\n", std::fs::read_to_string(w.path("mimic.toml")).unwrap()));
    let bin = PathBuf::from(std::env::var_os("WF_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_wf-mimic").into()));
    // When the record job runs against the real wf-mimic
    let r = mimic(&["job", "record", &w.set.display().to_string()], &[("WF_MIMIC_BIN", &bin)]);
    // Then its output names the interaction and the path, with the kind of each side and no value
    let d = r.json["differences"]["wobble-1"].as_array().unwrap_or_else(|| panic!("{}\n{}", r.json, r.stderr));
    assert_eq!(d.len(), 1, "{}", r.json["differences"]);
    assert!(d[0]["path"].as_str().unwrap().ends_with("/n") && d[0]["first"] == "number", "{}", d[0]);
    assert_eq!(r.json["nondeterministic"], json!(1));
}
