//! The record-references verbs of `wf mimic`: how a set's reference is
//! installed from data (the provision phase of `capture`), the chunked capture itself (`capture record`, over a double of the checkpointed `record`) and the transport of a
//! recorded set to another checkout (the publish phase of `capture`, `fetch`, over a bare repository standing in for origin).
use crate::support::*;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Every directory under `root` that holds a mimic.toml, from `root`.
fn set_dirs(root: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() && !["node_modules", "target", ".git", "references", "dist", "tmp", ".nx"].iter().any(|s| name == *s) {
                walk(root, &e.path(), out);
            } else if kind.is_file() && name == "mimic.toml" {
                out.push(dir.strip_prefix(root).unwrap_or(dir).to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

fn reporoot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A block with one step that counts its runs, and no toolchain: the capture tests provision it (phase one) before the double records.
const JOB_BLOCK: &str = "[provision]\ndrivers = false\n[[provision.step]]\nname = \"count\"\nrun = [\"sh\", \"-c\", \"echo run >> {work}/runs.txt\"]\n";

fn add_set(root: &Path, rel: &str, extra: &str) {
    let set = root.join(rel);
    std::fs::create_dir_all(&set).unwrap();
    std::fs::write(set.join("pin.json"), r#"{"version":"1"}"#).unwrap();
    std::fs::write(set.join("corpus.json"), "{}").unwrap();
    std::fs::write(set.join("mimic.toml"), format!("[reference]\ncommand = [\"true\"]\npin = \"pin.json\"\n[candidate.main]\ncommand = [\"true\"]\n{extra}")).unwrap();
}

#[test]
fn every_set_of_the_repository_that_declares_provisioning_validates() {
    // Given every set of this repository whose loaded config has a provisioning block (inline or in a file), as the loader sees it
    let root = reporoot();
    let declared: Vec<PathBuf> = set_dirs(&root).into_iter().filter(|d| wf_mimic::config::Config::load(&root.join(d).join("mimic.toml")).is_ok_and(|c| c.provision.is_some())).collect();
    // Then the T3 and Orca recording sets are among them, so the loop below cannot pass on a subset
    for app in ["clis/t3-serve/cassettes", "clis/orca-serve/cassettes"] {
        assert!(declared.iter().any(|d| d.starts_with(app)), "no set under {app} declares provisioning: {declared:?}");
    }
    for dir in declared {
        // When its block is checked
        let r = mimic(&["capture", "record", &root.join(&dir).display().to_string(), "--check", "--root", &root.display().to_string()], &[]);
        // Then it loads: the schema, the pin it reads the commit from and the placeholders it uses are all valid
        assert_eq!((r.code, r.json["ok"].clone()), (0, json!(true)), "{}: {}", dir.display(), r.json);
        assert!(!r.json["provision"]["steps"].as_array().unwrap().is_empty(), "{}", dir.display());
    }
}

fn bad_block(block: &str) -> Run {
    let root = tempfile::tempdir().unwrap();
    add_set(root.path(), "s", block);
    mimic(&["capture", "record", &root.path().join("s").display().to_string(), "--check"], &[])
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
    let r = mimic(&["capture", "record", &root.path().join("s").display().to_string(), "--provision", "only", "--work-root", &work.display().to_string()], &env);
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
    assert_eq!((r.code, r.json["provision"]["state"].clone()), (0, json!("ran")), "{}\n{}", r.json, r.stderr);
    let dir = work.path().join("s");
    assert_eq!(std::fs::read_to_string(dir.join("seen.txt")).unwrap().trim(), "pinned");
    assert_eq!(std::fs::read_to_string(dir.join("token.txt")).unwrap().trim(), "t=");
    assert_eq!(r.json["provision"]["env"]["REF_ROOT"], json!(dir.join("source").display().to_string()));
    assert!(!r.json.to_string().contains("hunter2") && !r.stderr.contains("hunter2"));
    // When the same block is provisioned again
    let again = mimic(&["capture", "record", &root.path().join("s").display().to_string(), "--provision", "only", "--work-root", &work.path().display().to_string()], &[("GIT_CONFIG_COUNT", Path::new("1")), ("GIT_CONFIG_KEY_0", Path::new(&src.url_rewrite.0)), ("GIT_CONFIG_VALUE_0", Path::new(&src.url_rewrite.1))]);
    // Then nothing runs a second time
    assert_eq!((again.code, again.json["provision"]["state"].clone()), (0, json!("cached")), "{}", again.json);
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
    let wrong = mimic(&["capture", "record", &root.path().join("s").display().to_string(), "--provision", "only", "--work-root", &work.path().display().to_string()], &[]);
    // Then it is refused and nothing ran
    assert_eq!(wrong.code, 2);
    assert!(wrong.json["error"].as_str().unwrap().contains("pins node 1") || wrong.json["error"].as_str().unwrap().contains("node --version"), "{}", wrong.json["error"]);
    assert!(!work.path().join("s/ran").exists());
}

/// A stand-in for `wf-mimic`: its `record --help` lists the flags the checkpointed record has (or not, with `MIMIC_FAKE_NO_RESUME`), and each
/// `record <set> --resume --max-minutes n` call leaves a checkpoint, reports complete:false until `MIMIC_FAKE_DONE_AT` calls were made, then writes the set.
const FAKE: &str = r#"#!/bin/sh
[ "$1" = mimic ] || exit 2
case "$2" in
  record)
    if [ "$3" = --help ]; then
      [ -n "$MIMIC_FAKE_NO_RESUME" ] && echo "  --keep-raw" || echo "  --resume  --max-minutes <n>"
      exit 0
    fi
    set="$3"
    mkdir -p "$set/raw/checkpoint"
    echo "REF_HOME=$REF_HOME PATH_KEPT=$([ -n "$PATH" ] && echo yes)" > "$set/seen-env.txt"
    echo "$@" > "$set/args.txt"
    [ -n "$MIMIC_FAKE_FAIL" ] && { echo '{"ok":false,"error":"boom"}'; exit 3; }
    [ -n "$MIMIC_FAKE_CKPT_LEAK" ] && echo "Bearer abcdefgh12345" > "$set/raw/checkpoint/cap.jsonl"
    n=$(cat "$set/raw/count" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$set/raw/count"
    if [ "$n" -lt "${MIMIC_FAKE_DONE_AT:-1}" ]; then
      echo '{"ok":true,"set":"s","interactions":4,"complete":false,"nondeterministic":[],"warnings":[]}'
      exit 0
    fi
    echo '{"schema":"mimic-cassette/1"}' > "$set/cassette.json"
    echo "{\"id\":\"a\",\"output\":\"${MIMIC_FAKE_LEAK:-fine}\"}" > "$set/interactions.jsonl"
    echo '{"ok":true,"set":"s","interactions":8,"complete":true,"nondeterministic":["x"],"warnings":[]}'
    ;;
  verify)
    [ -n "$MIMIC_FAKE_ACCOUNTING" ] && { echo '{"ok":false,"sets":[{"checks":[],"problems":[{"field":"stability","message":"x is tagged nondeterministic but not listed"}]}]}'; exit 1; }
    [ -n "$MIMIC_FAKE_STALE" ] && { echo '{"ok":false,"sets":[{"checks":[{"name":"rules_hash","status":"stale"}],"problems":[{"field":"stability","message":"x"}]}]}'; exit 1; }
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
    add_set(dir.path(), "s", JOB_BLOCK);
    let bin = fake(dir.path());
    Job { set: dir.path().join("s"), bin, dir }
}

impl Job {
    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let set = self.set.display().to_string();
        let work = self.dir.path().join("work").display().to_string();
        let mut all = vec!["capture", "record", set.as_str()];
        if !args.contains(&"--work-root") {
            all.extend(["--work-root", work.as_str()]);
        }
        all.extend_from_slice(args);
        let mut e: Vec<(&str, &Path)> = vec![("WF_MIMIC_BIN", &self.bin)];
        e.extend(env.iter().map(|(k, v)| (*k, Path::new(*v))));
        mimic(&all, &e)
    }
    fn work(&self) -> PathBuf {
        self.dir.path().join("work/s")
    }
    /// The record with the publish phase, run inside a checkout whose `origin` is the remote the state ref is pushed to.
    fn publish_from(&self, cwd: &Path, run: &str) -> Run {
        let (set, work, root) = (self.set.display().to_string(), self.dir.path().join("work").display().to_string(), self.dir.path().display().to_string());
        mimic_in(Some(cwd), &["capture", "record", &set, "--work-root", &work, "--publish", "--run", run, "--root", &root], &[("WF_MIMIC_BIN", &self.bin)])
    }
    fn summary(&self) -> PathBuf {
        self.dir.path().join("summary.md")
    }
}

#[test]
fn a_record_capture_runs_chunks_until_the_record_is_complete_then_drops_the_raw_capture_and_reports_counts_and_stability() {
    // Given a record that needs three bounded chunks
    let j = job();
    let summary = j.summary();
    // When the job runs with a ten minute chunk
    let r = j.run(&["--chunk", "10", "--summary", &summary.display().to_string(), "--run", "77"], &[("MIMIC_FAKE_DONE_AT", "3")]);
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
fn a_record_capture_that_runs_out_of_budget_keeps_its_checkpoint_and_says_how_to_resume() {
    // Given a record that never finishes
    let j = job();
    let summary = j.summary();
    // When the job has room for one one-minute chunk only
    let r = j.run(&["--chunk", "1", "--budget", "1", "--summary", &summary.display().to_string()], &[("MIMIC_FAKE_DONE_AT", "99")]);
    // Then it stops after that chunk, red, the checkpoint stays and the summary names the marker that resumes it
    assert_eq!((r.code, r.json["complete"].clone(), r.json["chunks"].as_array().map(Vec::len)), (1, json!(false), Some(1)), "{}", r.json);
    assert!(j.set.join("raw/checkpoint").is_dir());
    assert!(r.json["next"].as_str().unwrap().contains("[record s]"));
    assert!(std::fs::read_to_string(&summary).unwrap().contains("INCOMPLETE"));
    // When the job is told to start fresh
    let fresh = j.run(&["--chunk", "1", "--budget", "1", "--fresh"], &[("MIMIC_FAKE_DONE_AT", "1")]);
    // Then the checkpoint of the earlier run is gone and the record completes in one chunk
    assert_eq!((fresh.code, fresh.json["chunks"].as_array().map(Vec::len)), (0, Some(1)), "{}", fresh.json);
}

#[test]
fn a_masked_set_with_a_token_shaped_value_is_held_back_and_a_record_without_resume_fails_naming_the_flag() {
    // Given a record whose masked output still holds a bearer token
    let j = job();
    // When the job completes it
    let r = j.run(&[], &[("MIMIC_FAKE_LEAK", "Bearer abcdefgh12345")]);
    // Then the set is held back with the file and line, so it is never published
    assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{}", r.json);
    assert!(r.json["leaks"][0].as_str().unwrap().contains("interactions.jsonl:1 has a Bearer token"), "{}", r.json["leaks"]);
    // Given a wf-mimic whose record has no --resume yet
    let old = j.run(&[], &[("MIMIC_FAKE_NO_RESUME", "1")]);
    // Then the job fails at once and the message names the missing flags
    assert_eq!(old.code, 2);
    let error = old.json["error"].as_str().unwrap();
    assert!(error.contains("--resume") && error.contains("--max-minutes"), "{error}");
}

#[test]
fn a_record_capture_passes_the_provision_env_to_the_record_alone_and_keeps_the_stability_check_unless_the_set_opts_out() {
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
fn the_provision_phase_is_skipped_on_a_matching_marker_and_only_provisions_without_capturing() {
    // Given a set whose provision step counts its runs, and a capture that provisions on the way (the default, auto)
    let j = job();
    let first = j.run(&[], &[]);
    // Then the step ran once, the record followed, and the output names both phases
    assert_eq!((first.code, first.json["provision"]["state"].clone(), first.json["captured"].clone(), first.json["schema"].clone()), (0, json!("ran"), json!(true), json!("mimic-capture/1")), "{}\n{}", first.json, first.stderr);
    assert_eq!(std::fs::read_to_string(j.work().join("runs.txt")).unwrap().lines().count(), 1);
    // When the same capture runs again
    let again = j.run(&["--fresh"], &[]);
    // Then the marker matched: the phase was skipped and only the capture ran
    assert_eq!((again.code, again.json["provision"]["state"].clone()), (0, json!("cached")), "{}", again.json);
    assert_eq!(std::fs::read_to_string(j.work().join("runs.txt")).unwrap().lines().count(), 1);
    // When --provision only runs on a fresh work root
    let other = job();
    let only = other.run(&["--provision", "only"], &[]);
    // Then the reference is provisioned and nothing is captured: the double of `record` was never called
    assert_eq!((only.code, only.json["provision"]["state"].clone(), only.json["captured"].clone()), (0, json!("ran"), json!(false)), "{}\n{}", only.json, only.stderr);
    assert!(other.work().join("runs.txt").is_file() && !other.set.join("args.txt").exists());
    assert!(only.json["next"].as_str().unwrap().contains("--provision skip"), "{}", only.json["next"]);
    // When --provision skip then captures on that marker
    let skipped = other.run(&["--provision", "skip"], &[]);
    // Then the phase is skipped, the record ran, and the step did not run again
    assert_eq!((skipped.code, skipped.json["provision"]["state"].clone(), skipped.json["captured"].clone()), (0, json!("skipped"), json!(true)), "{}\n{}", skipped.json, skipped.stderr);
    assert_eq!(std::fs::read_to_string(other.work().join("runs.txt")).unwrap().lines().count(), 1);
}

#[test]
fn provision_skip_refuses_a_missing_or_stale_marker_and_names_the_command_that_provisions() {
    // Given a set that was never provisioned
    let j = job();
    // When a capture is told to skip the phase
    let missing = j.run(&["--provision", "skip"], &[]);
    // Then it is refused before any capture, naming the command
    assert_eq!(missing.code, 2, "{}", missing.json);
    let (error, next) = (missing.json["error"].as_str().unwrap(), missing.json["next"].as_str().unwrap());
    assert!(error.contains("--provision skip") && error.contains("marker"), "{error}");
    assert!(next.starts_with("wf mimic capture record ") && next.ends_with("--provision only"), "{next}");
    assert!(!j.set.join("args.txt").exists());
    // Given the set provisioned, and then its block changed
    assert_eq!(j.run(&["--provision", "only"], &[]).code, 0);
    assert_eq!(j.run(&["--provision", "skip"], &[]).code, 0);
    let toml = j.set.join("mimic.toml");
    std::fs::write(&toml, format!("{}[[provision.step]]\nname = \"more\"\nrun = [\"true\"]\n", std::fs::read_to_string(&toml).unwrap())).unwrap();
    // When a capture skips the phase against the changed block
    let stale = j.run(&["--provision", "skip"], &[]);
    // Then the marker is stale and the capture is refused the same way
    assert_eq!(stale.code, 2, "{}", stale.json);
    assert!(stale.json["next"].as_str().unwrap().ends_with("--provision only"), "{}", stale.json["next"]);
    // And an unknown mode is refused
    assert_eq!(j.run(&["--provision", "maybe"], &[]).code, 2);
}

#[test]
fn the_provisioning_children_hold_no_key_shaped_variable_and_the_capture_holds_the_blocks_env() {
    // Given a set whose block has an env and a step that dumps its environment, and a caller that holds credentials
    let j = job();
    let toml = j.set.join("mimic.toml");
    std::fs::write(&toml, std::fs::read_to_string(&toml).unwrap().replace("run = [\"sh\", \"-c\", \"echo run >> {work}/runs.txt\"]", "run = [\"sh\", \"-c\", \"env > {work}/step-env.txt\"]") + "[provision.env]\nREF_HOME = \"{work}/home\"\n").unwrap();
    // When the capture runs
    let r = j.run(&[], &[("DEPLOY_TOKEN", "hunter2"), ("MODEL_API_KEY", "k-1"), ("BWS_ACCESS_TOKEN", "b-1")]);
    assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
    // Then what provisioning ran saw the block's env and no variable named like a credential
    let seen = std::fs::read_to_string(j.work().join("step-env.txt")).unwrap();
    let names: Vec<&str> = seen.lines().filter_map(|l| l.split_once('=')).map(|(k, _)| k).collect();
    assert!(names.contains(&"REF_HOME"), "{names:?}");
    let keyed: Vec<&&str> = names.iter().filter(|n| ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "API_KEY"].iter().any(|k| n.contains(k)) || n.ends_with("_KEY") || n.starts_with("BWS_")).collect();
    assert!(keyed.is_empty(), "{keyed:?}");
    assert!(!seen.contains("hunter2") && !seen.contains("k-1"));
    // And the capture saw the block's [env] resolved
    assert_eq!(std::fs::read_to_string(j.set.join("seen-env.txt")).unwrap().trim(), format!("REF_HOME={}/home PATH_KEPT=yes", j.work().display()));
}

#[test]
fn a_set_that_allows_nondeterminism_is_published_when_only_its_stability_accounting_is_stale_and_never_when_anything_else_is() {
    // Given a set that allows nondeterminism, whose verify fails on the stability allowlist alone
    let j = job();
    std::fs::write(j.set.join("mimic.toml"), format!("{}[record]\nallow_nondeterministic = true\n", std::fs::read_to_string(j.set.join("mimic.toml")).unwrap())).unwrap();
    // When the job completes the record
    let r = j.run(&[], &[("MIMIC_FAKE_ACCOUNTING", "1")]);
    // Then it is not held back, and says the allowlist is for the commit to settle
    assert_eq!((r.code, r.json["ok"].clone(), r.json["verify"]["accounting_deferred"].clone()), (0, json!(true), json!(true)), "{}", r.json);
    // When a freshness check fails as well
    let stale = j.run(&["--fresh"], &[("MIMIC_FAKE_STALE", "1")]);
    // Then the set is held back
    assert_eq!((stale.code, stale.json["ok"].clone(), stale.json["verify"]["accounting_deferred"].clone()), (1, json!(false), json!(false)), "{}", stale.json);
    // and a set that does not allow nondeterminism is held back on the accounting too
    let strict = job();
    let held = strict.run(&[], &[("MIMIC_FAKE_ACCOUNTING", "1")]);
    assert_eq!((held.code, held.json["ok"].clone()), (1, json!(false)), "{}", held.json);
}

#[test]
fn a_checkpoint_is_resumed_only_by_the_record_it_belongs_to_and_is_kept_for_upload_only_when_the_run_ran_out_of_budget_cleanly() {
    // Given a record that ran out of budget and left a checkpoint with state in it
    let j = job();
    let first = j.run(&["--chunk", "1", "--budget", "1"], &[("MIMIC_FAKE_DONE_AT", "99")]);
    assert_eq!((first.code, first.json["incomplete"].clone(), first.json["checkpoint_upload"].clone()), (1, json!(true), json!(true)), "{}", first.json);
    std::fs::write(j.set.join("raw/checkpoint/state.txt"), "mine").unwrap();
    // When the same record runs again with nothing changed
    let again = j.run(&["--chunk", "1", "--budget", "1"], &[("MIMIC_FAKE_DONE_AT", "99")]);
    // Then the checkpoint is resumed: its state is still there and nothing warns
    assert!(j.set.join("raw/checkpoint/state.txt").is_file());
    assert_eq!(again.json["warnings"], json!([]), "{}", again.json);
    // When the corpus changed since that checkpoint
    std::fs::write(j.set.join("corpus.json"), r#"{"changed":true}"#).unwrap();
    let stale = j.run(&["--chunk", "1", "--budget", "1"], &[("MIMIC_FAKE_DONE_AT", "99")]);
    // Then the checkpoint is dropped and the record starts over, and the job says so
    assert!(!j.set.join("raw/checkpoint/state.txt").exists());
    assert!(stale.json["warnings"][0].as_str().unwrap().contains("dropped"), "{}", stale.json);
    // When a leg fails, the job does not call it incomplete, so no checkpoint is uploaded
    let failed = j.run(&[], &[("MIMIC_FAKE_FAIL", "1")]);
    assert_eq!(failed.code, 2, "{}", failed.json);
    assert!(failed.json["incomplete"].is_null(), "{}", failed.json);
    // When an unfinished record's checkpoint holds a token-shaped value
    let leaky = j.run(&["--fresh", "--chunk", "1", "--budget", "1"], &[("MIMIC_FAKE_DONE_AT", "99"), ("MIMIC_FAKE_CKPT_LEAK", "1")]);
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
    let r = mimic(&["capture", "record", &root.path().join("a/b/c/d/deep").display().to_string(), "--check"], &[]);
    // Then the shared block is found, and a node toolchain brings the repository's drivers with it
    assert_eq!((r.code, r.json["provision"]["steps"].clone()), (0, json!(["drivers", "s"])), "{}", r.json);
}

#[test]
fn a_replay_capture_tabulates_every_candidate_and_is_red_only_when_one_differs() {
    // Given the toy set, recorded, with a candidate that behaves and one that changes a value
    let w = World::new();
    w.record();
    let toy = toy().display().to_string();
    w.write("mimic.toml", &format!("[reference]\ncommand = [{toy:?}]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n[candidate.same]\ncommand = [{toy:?}]\n[candidate.change]\ncommand = [{toy:?}, \"--tweak\", \"change-value\"]\n"));
    let summary = w.dir.path().join("summary.md");
    // When every candidate is replayed
    let set = w.arg();
    let r = mimic(&["capture", "replay", &set, "--summary", &summary.display().to_string(), "--out", &w.dir.path().join("out").display().to_string()], &[]);
    // Then the table has a row per candidate, the differing one with its first difference, and the run is red
    assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{}\n{}", r.json, r.stderr);
    let md = std::fs::read_to_string(&summary).unwrap();
    assert!(md.contains("| same | none |") && md.contains("| change | drift |") && md.contains("| change |"), "{md}");
    assert!(w.dir.path().join("out/toy-set.replay.json").is_file());
    // And the JSON names each differing interaction of a candidate with its first difference, and lists none for the one that behaves
    let by = |name: &str| r.json["candidates"].as_array().unwrap().iter().find(|c| c["candidate"] == name).unwrap_or_else(|| panic!("{}", r.json)).clone();
    assert_eq!(by("same")["differences"], json!([]), "{}", r.json);
    let listed = by("change")["differences"].as_array().unwrap().clone();
    assert!(!listed.is_empty() && listed.iter().all(|d| d["id"].is_string() && d["first"]["path"].is_string() && d["label"].is_string()), "{}", r.json);
    // When only the candidate that behaves is replayed
    let clean = mimic(&["capture", "replay", &set, "--candidate", "same"], &[]);
    // Then the run is green
    assert_eq!((clean.code, clean.json["ok"].clone()), (0, json!(true)), "{}", clean.json);
}

/// A bare repository standing in for GitHub's `origin`, and clones of it that run `wf mimic` the way two runners and an agent's checkout do.
struct Remote {
    dir: tempfile::TempDir,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl Remote {
    fn new() -> Remote {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--bare", "--quiet", "origin.git"]);
        Remote { dir }
    }

    fn bare(&self) -> PathBuf {
        self.dir.path().join("origin.git")
    }

    fn clone_named(&self, name: &str) -> PathBuf {
        let job = self.dir.path().join(name);
        std::fs::create_dir(&job).unwrap();
        git(&job, &["init", "--quiet"]);
        git(&job, &["remote", "add", "origin", self.bare().to_str().unwrap()]);
        job
    }
}

#[test]
fn a_recorded_set_published_by_the_capture_is_fetched_whole_into_another_checkout_without_its_raw_capture() {
    // Given a record that leaves a raw capture beside the set, and a runner and an agent checkout sharing one origin
    let j = job();
    let remote = Remote::new();
    let (runner, agent) = (remote.clone_named("runner"), remote.clone_named("agent"));
    // When the capture records it as run 42 and publishes it in the same call
    let p = j.publish_from(&runner, "42");
    // Then every kept file is on the state ref of origin and raw/ is not
    assert_eq!((p.code, p.json["ok"].clone(), p.json["publish"]["ok"].clone()), (0, json!(true), json!(true)), "{}\n{}", p.json, p.stderr);
    assert!(p.json["next"].as_str().unwrap().starts_with("wf mimic fetch 42"), "{}", p.json["next"]);
    let files = git(&remote.bare(), &["ls-tree", "-r", "--name-only", "refs/wf/state"]);
    assert!(files.contains("records/sets/s/cassette.json") && files.contains("records/runs/42.json") && files.contains("records/latest/s.json"), "{files}");
    assert!(!files.contains("raw"), "{files}");
    // When another checkout fetches the run, by id and as latest
    let other = tempfile::tempdir().unwrap();
    let f = mimic_in(Some(&agent), &["fetch", "42", "--root", &other.path().display().to_string()], &[]);
    let latest = mimic_in(Some(&agent), &["fetch", "latest", "--set", "s", "--root", &other.path().display().to_string()], &[]);
    // Then the set is there byte for byte, without raw/
    assert_eq!((f.code, f.json["ok"].clone(), latest.code), (0, json!(true), 0), "{}\n{}", f.json, f.stderr);
    for file in ["cassette.json", "interactions.jsonl", "corpus.json", "mimic.toml", "pin.json"] {
        assert_eq!(std::fs::read(other.path().join("s").join(file)).unwrap(), std::fs::read(j.set.join(file)).unwrap(), "{file}");
    }
    assert!(!other.path().join("s/raw").exists());
}

#[test]
fn a_record_held_back_or_without_the_publish_flag_publishes_nothing_and_a_refused_push_is_a_warning() {
    // Given a record whose masked output holds a bearer token, a clean record run without --publish, and a checkout whose origin refuses the push
    let held = job();
    let plain = job();
    let remote = Remote::new();
    let runner = remote.clone_named("runner");
    let broken = job();
    let lone = tempfile::tempdir().unwrap();
    git(lone.path(), &["init", "--quiet"]);
    git(lone.path(), &["remote", "add", "origin", "/nonexistent/origin.git"]);
    // When each is captured with or without the phase
    let leak = {
        let (set, work, root) = (held.set.display().to_string(), held.dir.path().join("work").display().to_string(), held.dir.path().display().to_string());
        mimic_in(Some(&runner), &["capture", "record", &set, "--work-root", &work, "--publish", "--run", "1", "--root", &root], &[("WF_MIMIC_BIN", &held.bin), ("MIMIC_FAKE_LEAK", Path::new("Bearer abcdefgh12345"))])
    };
    let unflagged = plain.run(&["--run", "2"], &[]);
    let refused = broken.publish_from(lone.path(), "3");
    // Then the held-back record is red and publishes nothing, the unflagged one has no publish phase, and the refused push leaves the record green with the reason
    assert_eq!((leak.code, leak.json["ok"].clone(), leak.json["publish"].clone()), (1, json!(false), Value::Null), "{}", leak.json);
    assert_eq!(git(&remote.bare(), &["for-each-ref", "refs/wf"]), "");
    assert_eq!((unflagged.code, unflagged.json["publish"].clone()), (0, Value::Null), "{}", unflagged.json);
    assert_eq!((refused.code, refused.json["ok"].clone(), refused.json["publish"]["ok"].clone()), (0, json!(true), json!(false)), "{}\n{}", refused.json, refused.stderr);
    assert!(refused.json["warnings"][0].as_str().unwrap().contains("not published"), "{}", refused.json["warnings"]);
    // And --publish with a replay is refused, naming the phase it goes with
    let w = World::new();
    let bad = mimic(&["capture", "replay", &w.arg(), "--publish"], &[]);
    assert_eq!(bad.code, 2, "{}", bad.json);
    assert!(bad.json["error"].as_str().unwrap().contains("--publish"), "{}", bad.json["error"]);
}

#[test]
fn a_fetched_file_that_does_not_match_its_manifest_hash_is_refused_and_an_unknown_run_says_so() {
    // Given a published run whose interactions file was altered on the ref after the manifest was written
    let j = job();
    let remote = Remote::new();
    let (runner, agent) = (remote.clone_named("runner"), remote.clone_named("agent"));
    let p = j.publish_from(&runner, "7");
    assert_eq!(p.code, 0, "{}", p.json);
    git(&runner, &["fetch", "--quiet", "origin", "refs/wf/state:refs/wf/state"]);
    let tip = git(&runner, &["rev-parse", "refs/wf/state"]);
    let index = runner.join(".git/tamper-index");
    let tamper = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).env("GIT_INDEX_FILE", &index).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").current_dir(&runner).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    tamper(&["read-tree", &tip]);
    let blob = {
        let mut child = std::process::Command::new("git").args(["hash-object", "-w", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).current_dir(&runner).spawn().unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"altered\n").unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).trim().to_string()
    };
    tamper(&["update-index", "--cacheinfo", &format!("100644,{blob},records/sets/s/interactions.jsonl")]);
    let tree = tamper(&["write-tree"]);
    let commit = tamper(&["commit-tree", &tree, "-p", &tip, "-m", "altered"]);
    git(&runner, &["push", "--quiet", "origin", &format!("{commit}:refs/wf/state")]);
    // When it is fetched
    let other = tempfile::tempdir().unwrap();
    let f = mimic_in(Some(&agent), &["fetch", "7", "--root", &other.path().display().to_string()], &[]);
    // Then the hash mismatch stops it, and the altered file is not written
    assert_eq!(f.code, 2, "{}", f.json);
    assert!(f.json["error"].as_str().unwrap().contains("does not match the hash"), "{}", f.json["error"]);
    assert!(!other.path().join("s/interactions.jsonl").exists());
    // When a run that was never published is fetched
    let none = mimic_in(Some(&agent), &["fetch", "404", "--root", &other.path().display().to_string()], &[]);
    // Then it says the run does not exist
    assert_eq!(none.code, 2);
    assert!(none.json["error"].as_str().unwrap().contains("does not exist"), "{}", none.json["error"]);
}

#[test]
fn a_run_a_newer_one_replaced_keeps_no_files_and_the_fetch_names_the_newest() {
    // Given two runs of the same set published one after the other
    let j = job();
    let remote = Remote::new();
    let (runner, agent) = (remote.clone_named("runner"), remote.clone_named("agent"));
    for run in ["1", "2"] {
        let p = j.publish_from(&runner, run);
        assert_eq!(p.code, 0, "{}", p.json);
    }
    // When the older run is fetched
    let other = tempfile::tempdir().unwrap();
    let old = mimic_in(Some(&agent), &["fetch", "1", "--root", &other.path().display().to_string()], &[]);
    // Then it is refused and points at the newest run, which fetches
    assert_eq!(old.code, 2, "{}", old.json);
    assert!(old.json["error"].as_str().unwrap().contains("no longer the newest"), "{}", old.json["error"]);
    let newest = mimic_in(Some(&agent), &["fetch", "latest", "--set", "s", "--root", &other.path().display().to_string()], &[]);
    assert_eq!((newest.code, newest.json["run"].clone()), (0, json!("2")), "{}", newest.json);
}

#[test]
fn a_record_capture_names_the_path_that_differed_between_the_two_stability_passes_of_a_nondeterministic_interaction() {
    // Given a set whose reference answers a case with a number that changes on every run, and that tolerates nondeterminism
    let w = World::with_corpus(&[json!({"kind": "corpus", "name": "wobble", "op": "wobble", "cases": [{"id": "wobble-1", "input": {}}]})]);
    w.write("mimic.toml", &format!("{}[record]\nallow_nondeterministic = true\n", std::fs::read_to_string(w.path("mimic.toml")).unwrap()));
    let bin = PathBuf::from(std::env::var_os("WF_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_wf-mimic").into()));
    // When the record job runs against the real wf-mimic
    let r = mimic(&["capture", "record", &w.set.display().to_string()], &[("WF_MIMIC_BIN", &bin)]);
    // Then its output names the interaction and the path, with the kind of each side and no value
    let d = r.json["differences"]["wobble-1"].as_array().unwrap_or_else(|| panic!("{}\n{}", r.json, r.stderr));
    assert_eq!(d.len(), 1, "{}", r.json["differences"]);
    assert!(d[0]["path"].as_str().unwrap().ends_with("/n") && d[0]["first"] == "number", "{}", d[0]);
    assert_eq!(r.json["nondeterministic"], json!(1));
}
