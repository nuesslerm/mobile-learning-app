//! `wf ci plan tags` and `wf ci plan mimic` through the built binary: the commit tags that shape a run and the capture marker that starts a
//! reference-capture job, with the step outputs the workflows read. The plan of the tests and the legs is held by `pipeline.rs` and `deploy`.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PROVISION: &str = "[provision]\n[provision.toolchain]\nnode = \"24\"\n[[provision.step]]\nname = \"build\"\nrun = [\"true\"]\n";

fn reporoot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn add_set(root: &Path, rel: &str, extra: &str) {
    let set = root.join(rel);
    std::fs::create_dir_all(&set).unwrap();
    std::fs::write(set.join("pin.json"), r#"{"version":"1"}"#).unwrap();
    std::fs::write(set.join("corpus.json"), "{}").unwrap();
    std::fs::write(set.join("mimic.toml"), format!("[reference]\ncommand = [\"true\"]\npin = \"pin.json\"\n[candidate.main]\ncommand = [\"true\"]\n{extra}")).unwrap();
}

/// A checkout with sets of the given names: each a directory with a mimic.toml, a corpus and a provision block.
fn checkout(sets: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for s in sets {
        add_set(dir.path(), s, PROVISION);
    }
    dir
}

struct Plan {
    run: RunResult,
    json: Value,
    output: String,
}

fn out(plan: &Plan, key: &str) -> Option<String> {
    plan.output.lines().find_map(|l| l.strip_prefix(&format!("{key}=")).map(str::to_string))
}

/// `wf ci plan mimic --root <root> --message <message> --github-output`
fn mimic(root: &Path, message: &str) -> Plan {
    let outdir = tempfile::tempdir().unwrap();
    let gh = outdir.path().join("gh-output");
    let run = run_wf(&["ci", "plan", "mimic", "--root", &root.display().to_string(), "--message", message, "--github-output"], &RunOpts::default().env("GITHUB_OUTPUT", &gh.display().to_string()));
    let json = serde_json::from_str(run.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", run.stdout, run.stderr));
    Plan { run, json, output: std::fs::read_to_string(gh).unwrap_or_default() }
}

fn names(p: &Plan) -> Vec<String> {
    p.json["sets"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn a_record_marker_names_its_sets_chunk_and_fresh_and_a_replay_marker_only_its_sets() {
    // Given a checkout with three sets, one of them reachable by path only because two directories share its name
    let root = checkout(&["a/alpha", "b/beta", "c/gamma"]);
    add_set(root.path(), "d/beta", PROVISION);
    // When the head commit of a push carries a record marker with several sets, a chunk and fresh
    let r = mimic(root.path(), "re-record\n\n[record alpha b/beta gamma chunk=15 fresh]\n");
    // Then every set is a job, in order, with the chunk and fresh the marker gave and the node its provision block pins
    assert_eq!((r.run.exit_code, r.json["go"].clone(), r.json["action"].clone()), (0, json!(true), json!("record")), "{}", r.json);
    assert_eq!((names(&r), r.json["chunk"].clone(), r.json["fresh"].clone()), (vec!["alpha".into(), "beta".into(), "gamma".into()], json!(15), json!(true)));
    let jobs = r.json["matrix"]["include"].as_array().unwrap();
    assert_eq!((jobs[1]["dir"].clone(), jobs[0]["node"].clone(), jobs[0]["drivers"].clone()), (json!("b/beta"), json!("24"), json!(true)));
    // and the step outputs carry the same plan for the jobs that read it
    assert_eq!((out(&r, "action"), out(&r, "chunk"), out(&r, "fresh"), out(&r, "go")), (Some("record".into()), Some("15".into()), Some("true".into()), Some("true".into())));
    assert_eq!(serde_json::from_str::<Value>(&out(&r, "matrix").unwrap()).unwrap(), r.json["matrix"]);
    // When a record marker gives neither, and a replay marker names a recorded set
    let defaults = mimic(root.path(), "[record alpha]");
    std::fs::write(root.path().join("a/alpha/cassette.json"), "{}").unwrap();
    let replay = mimic(root.path(), "[replay alpha]");
    // Then the chunk is ten minutes and nothing starts over, and the replay is a job for the candidates of the set
    assert_eq!((defaults.json["chunk"].clone(), defaults.json["fresh"].clone()), (json!(10), json!(false)));
    assert_eq!((replay.run.exit_code, replay.json["action"].clone(), replay.json["matrix"]["include"][0]["candidates"].clone()), (0, json!("replay"), json!(["main"])), "{}", replay.json);
}

#[test]
fn a_replay_marker_with_rows_hands_the_prefixes_to_the_capture_and_one_without_hands_none() {
    // Given a recorded set, and the workflow that runs the capture
    let root = checkout(&["a/alpha"]);
    std::fs::write(root.path().join("a/alpha/cassette.json"), "{}").unwrap();
    // When a replay marker names two row prefixes, and another names none
    let sliced = mimic(root.path(), "[replay alpha rows=orca.settings.,orca.state.get]");
    let whole = mimic(root.path(), "[replay alpha]");
    // Then the plan carries the prefixes as one comma-joined value in the JSON and the step outputs, and the whole replay an empty one
    assert_eq!((sliced.run.exit_code, sliced.json["rows"].clone(), out(&sliced, "rows")), (0, json!("orca.settings.,orca.state.get"), Some("orca.settings.,orca.state.get".into())), "{}", sliced.json);
    assert_eq!((whole.run.exit_code, whole.json["rows"].clone(), out(&whole, "rows")), (0, json!(""), Some(String::new())));
    // And the capture leg of the workflow hands that output to `wf mimic capture --rows`
    let text = std::fs::read_to_string(reporoot().join(".github/workflows/mimic-record.yml")).unwrap();
    assert!(text.contains("ROWS: ${{ needs.plan.outputs.rows }}") && text.contains("flags=\"--rows $ROWS\""), "the capture leg does not pass rows to --rows");
}

#[test]
fn a_message_without_a_marker_plans_no_mimic_run() {
    // Given a checkout
    let root = checkout(&["a/alpha"]);
    for message in ["fix the recorder", "mention [recording] and [records alpha] in prose", "", "notes [record alpha", "we write [record alpha] or [replay alpha] after the subject", "subject\n\n  [record alpha]\n"] {
        // When a commit message that is not a marker is planned
        let r = mimic(root.path(), message);
        // Then it succeeds, asks for no job, and the step outputs say go=false with an empty matrix
        assert_eq!((r.run.exit_code, r.json["ok"].clone(), r.json["go"].clone(), r.json["matrix"]["include"].clone()), (0, json!(true), json!(false), json!([])), "{message:?}: {}", r.json);
        assert_eq!((out(&r, "go"), out(&r, "action"), out(&r, "matrix")), (Some("false".into()), Some(String::new()), Some("{\"include\":[]}".into())), "{message:?}: {}", r.output);
    }
}

#[test]
fn a_malformed_marker_or_an_unknown_set_is_refused_with_the_rule_it_breaks() {
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
        ("[replay alpha chunk=5]", "takes sets and rows= only"),
        ("[replay alpha fresh]", "takes sets and rows= only"),
        ("[record alpha rows=M5]", "rows= belongs to [replay"),
        ("[replay alpha rows=M5 rows=M6]", "rows= is given twice"),
        ("[replay alpha rows=M5,]", "not a row prefix"),
        ("[replay alpha rows=a;b]", "not a row prefix"),
        ("[record ../etc]", "neither a set name"),
        ("[record nope]", "unknown set \"nope\""),
        ("[record twin]", "names 2 sets"),
        ("[record gamma]", "no [provision]"),
        ("[replay alpha]", "no cassette.json"),
    ];
    for (message, reason) in cases {
        // When the message is planned
        let r = mimic(root.path(), message);
        // Then it is refused, the error says why (and an unknown set lists the known ones), no job is planned and no output is written
        assert_eq!((r.run.exit_code, r.json["ok"].clone()), (2, json!(false)), "{message}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(reason), "{message}: {}", r.json["error"]);
        assert!(r.json["next"].as_str().is_some_and(|n| !n.is_empty()));
        assert_eq!(r.output, "", "{message}: a refused marker wrote outputs");
    }
    let unknown = mimic(root.path(), "[record nope]");
    assert!(unknown.json["error"].as_str().unwrap().contains("alpha"), "{}", unknown.json["error"]);
}

#[test]
fn a_set_with_a_broken_mimic_toml_or_two_provision_sources_is_refused_naming_the_set() {
    // Given a checkout whose set has a mimic.toml that is not TOML, and another that names both an inline block and a provision_file
    let root = checkout(&["a/broken", "b/twice"]);
    std::fs::write(root.path().join("a/broken/mimic.toml"), "[reference\n").unwrap();
    std::fs::write(root.path().join("b/twice/mimic.toml"), format!("provision_file = \"p.toml\"\n[reference]\ncommand = [\"true\"]\n{PROVISION}")).unwrap();
    for (message, reason) in [("[record broken]", "set broken"), ("[record twice]", "both [provision] and provision_file")] {
        // When it is planned
        let r = mimic(root.path(), message);
        // Then it is refused with the set and the reason
        assert_eq!(r.run.exit_code, 2, "{message}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(reason), "{message}: {}", r.json["error"]);
    }
}

#[test]
fn every_set_of_the_repository_that_can_record_is_planned_with_its_provision_block() {
    // Given this repository, whose sets that declare provisioning are the ones a record marker can name
    let root = reporoot();
    for app in ["clis/t3-serve/cassettes", "clis/orca-serve/cassettes"] {
        let sets: Vec<PathBuf> = std::fs::read_dir(root.join(app)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("mimic.toml").is_file()).collect();
        for set in sets.iter().filter(|s| std::fs::read_to_string(s.join("mimic.toml")).is_ok_and(|t| t.contains("provision"))) {
            let rel = set.strip_prefix(&root).unwrap().display().to_string();
            // When a record marker names it by path
            let r = mimic(&root, &format!("[record {rel}]"));
            // Then it is a job with its provisioning, or refused for a missing reference or corpus, never for the block
            let error = r.json["error"].as_str().unwrap_or("");
            assert!(r.run.exit_code == 0 || !error.contains("provision"), "{rel}: {}", r.json);
        }
    }
}

/// Every `<prefix>.<key>` the workflow text reads.
fn keys_read(text: &str, prefix: &str) -> std::collections::BTreeSet<String> {
    regex::Regex::new(&format!(r"{}\.([A-Za-z_]+)", regex::escape(prefix))).unwrap().captures_iter(text).map(|c| c[1].to_string()).collect()
}

#[test]
fn every_key_the_workflows_read_from_the_plan_is_written_by_wf_ci_plan_mimic_for_a_record_and_a_replay() {
    // Given the workflow that runs the capture, and a checkout with a recorded set that has a provision block
    let text = std::fs::read_to_string(reporoot().join(".github/workflows/mimic-record.yml")).unwrap();
    let root = checkout(&["a/alpha"]);
    std::fs::write(root.path().join("a/alpha/cassette.json"), "{}").unwrap();
    for message in ["[record alpha]", "[replay alpha]"] {
        // When the marker is planned
        let r = mimic(root.path(), message);
        assert_eq!(r.run.exit_code, 0, "{message}: {}", r.json);
        // Then every needs.plan.outputs.<x> is a step output of the plan, and every matrix.<x> a key of each job
        for key in keys_read(&text, "needs.plan.outputs") {
            assert!(out(&r, &key).is_some(), "{message}: the workflow reads needs.plan.outputs.{key}, wf ci plan mimic does not write it: {}", r.output);
        }
        let job = r.json["matrix"]["include"][0].as_object().unwrap();
        for key in keys_read(&text, "matrix") {
            assert!(job.contains_key(&key), "{message}: the workflow reads matrix.{key}, the plan does not emit it: {job:?}");
        }
    }
    // And the workflows call the plan through the verb and no other marker parser is left
    for file in ["mimic-record.yml", "track-contracts.yml"] {
        let text = std::fs::read_to_string(reporoot().join(".github/workflows").join(file)).unwrap();
        assert!(text.contains("wf ci plan mimic --file - --github-output"), "{file} does not plan through wf ci plan mimic");
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

/// A repository whose head commit has the message, and `wf ci plan tags` over it.
fn tags(message: &str, extra: &[&str]) -> Plan {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let ok = crate::support::git_command().args(args).current_dir(dir.path()).status().unwrap().success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", message]);
    let gh = dir.path().join("gh-output");
    let mut args = vec!["ci", "plan", "tags", "--github-output"];
    args.extend_from_slice(extra);
    let run = run_wf(&args, &RunOpts::default().cwd(dir.path()).env("GITHUB_OUTPUT", &gh.display().to_string()));
    let json = serde_json::from_str(run.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", run.stdout, run.stderr));
    Plan { run, json, output: std::fs::read_to_string(gh).unwrap_or_default() }
}

#[test]
fn a_commit_without_tags_runs_every_job_plans_what_changed_and_keeps_the_caches() {
    // Given a commit message with no tag, When it is planned
    let r = tags("fix a thing", &[]);
    // Then every test job runs, nothing is forced and the caches stay on
    assert_eq!(r.run.exit_code, 0, "{}", r.json);
    assert_eq!(r.output, "run_flow=true\nrun_unit=true\nrun_service=true\nrun_browser=true\nrun_android=true\nforce_tests=\nforce_ship=\nfresh=false\n");
    assert_eq!(r.json["services"], json!(["changed"]));
}

#[test]
fn the_ci_services_and_cache_tags_limit_the_jobs_force_services_and_drop_the_caches() {
    // Given a commit limited to two jobs, naming two services next to changed, with the caches off
    let r = tags("x\n\n[ci:flow,unit] [services:changed,vm-ops-api,@lib/logging] [cache:no]", &[]);
    // Then only those jobs run, the named services are forced (changed is not a name) and the run is fresh
    assert_eq!(out(&r, "run_flow").as_deref(), Some("true"));
    assert_eq!((out(&r, "run_service").as_deref(), out(&r, "run_browser").as_deref(), out(&r, "run_android").as_deref()), (Some("false"), Some("false"), Some("false")));
    assert_eq!((out(&r, "force_tests").as_deref(), out(&r, "force_ship").as_deref(), out(&r, "fresh").as_deref()), (Some("vm-ops-api,@lib/logging"), Some("--also vm-ops-api,@lib/logging"), Some("true")), "{}", r.output);
    // When the commit asks for all, and the caller adds --fresh and a service
    let all = tags("[services:all]", &["--fresh", "--services", "deploy;rm"]);
    // Then everything is forced, the run is fresh, and the name that is no name is ignored with a warning
    assert_eq!((out(&all, "force_tests").as_deref(), out(&all, "force_ship").as_deref(), out(&all, "fresh").as_deref()), (Some("all"), Some("--force"), Some("true")));
    assert_eq!(all.json["warnings"], json!(["[services:] ignores the name 'deploy;rm'"]));
}

#[test]
fn given_a_plan_when_the_shard_keys_are_asked_then_each_shard_has_a_key_from_its_crates_the_same_in_every_commit_and_none_when_empty() {
    // Given an integration plan of three crates, and the same crates planned again for another commit
    let unit = |c: &str| serde_json::json!({"kind": "integration", "crate": c, "group": "svc"});
    let plan = |sha: &str, crates: &[&str]| serde_json::json!({"sha": sha, "stages": {"integration-tests": crates.iter().map(|c| unit(c)).collect::<Vec<_>>()}});
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, p: serde_json::Value| {
        let path = dir.path().join(name);
        std::fs::write(&path, p.to_string()).unwrap();
        path.display().to_string()
    };
    let (first, second, other) = (write("a.json", plan("aaa", &["alpha", "beta", "gamma"])), write("b.json", plan("bbb", &["alpha", "beta", "gamma"])), write("c.json", plan("ccc", &["alpha", "delta", "gamma"])));
    let keys = |file: &str, count: &str| -> Vec<String> {
        let r = run_wf(&["ci", "plan", "shard-keys", "--stage", "integration-tests", "--count", count, "--plan", file], &RunOpts::default());
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        serde_json::from_str(r.stdout.trim()).unwrap()
    };

    // When the keys are asked for two shards
    let (a, b, c) = (keys(&first, "2"), keys(&second, "2"), keys(&other, "2"));

    // Then a commit that plans the same crates gets the same keys, so its shards queue behind the earlier commit's
    assert_eq!(a, b);
    // And a different set of crates gets a different key somewhere, so it runs at once
    assert_ne!(a, c);
    // And two shards of three crates hold different crates, and a shard with no crate is named none
    assert_ne!(a[0], a[1]);
    assert!(keys(&first, "6").contains(&"none".to_string()));
}

#[test]
fn given_two_commits_when_one_changes_a_crates_files_then_its_shard_key_changes_and_an_unrelated_change_leaves_it() {
    // Given a repo with two crates and a plan for each commit
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(repo.path()).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    for dir in ["clis/alpha", "clis/beta"] {
        std::fs::create_dir_all(repo.path().join(dir)).unwrap();
        std::fs::write(repo.path().join(dir).join("lib.rs"), "v1").unwrap();
    }
    let commit = |msg: &str| {
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", msg]);
        git(&["rev-parse", "HEAD"])
    };
    let first = commit("one");
    std::fs::write(repo.path().join("clis/alpha/lib.rs"), "v2").unwrap();
    let fixed = commit("fix alpha");
    std::fs::write(repo.path().join("clis/beta/lib.rs"), "v2").unwrap();
    let unrelated = commit("change beta");
    let unit = |c: &str| json!({"kind": "integration", "crate": c, "dir": format!("clis/{c}"), "manifest": "clis/Cargo.toml", "group": "svc"});
    let dir = tempfile::tempdir().unwrap();
    let keys = |sha: &str, only: &str| -> Vec<String> {
        let path = dir.path().join(format!("{sha}.json"));
        std::fs::write(&path, json!({"sha": sha, "stages": {"integration-tests": [unit(only)]}}).to_string()).unwrap();
        let r = run_wf(&["ci", "plan", "shard-keys", "--stage", "integration-tests", "--count", "1", "--plan", &path.display().to_string(), "--repo", &repo.path().display().to_string()], &RunOpts::default());
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        serde_json::from_str(r.stdout.trim()).unwrap()
    };

    // When alpha's shard is keyed at each commit
    let (a1, a2, a3) = (keys(&first, "alpha"), keys(&fixed, "alpha"), keys(&unrelated, "alpha"));

    // Then the fix to alpha is another key (it starts at once) and a change to beta alone is the same key as before (it queues and replays)
    assert_ne!(a1, a2);
    assert_eq!(a2, a3);
}

#[test]
fn given_a_plan_with_flow_and_typecheck_units_when_the_typecheck_shard_keys_are_asked_then_every_shard_has_a_distinct_key_and_the_same_plan_gives_the_same_keys() {
    // Given a plan of three flow crates and six typecheck crates
    let unit = |kind: &str, c: &str| json!({"kind": kind, "crate": c});
    let flow: Vec<Value> = ["alpha", "beta", "gamma"].iter().map(|c| unit("flow", c)).collect();
    let typecheck: Vec<Value> = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"].iter().map(|c| unit("unit", c)).collect();
    let plan = json!({"sha": "aaa", "stages": {"flow-tests": flow, "typecheck-lint-unit-tests": typecheck}});
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plan.json");
    std::fs::write(&file, plan.to_string()).unwrap();
    let keys = || -> Vec<String> {
        let r = run_wf(&["ci", "plan", "shard-keys", "--stage", "typecheck-lint-unit-tests", "--count", "3", "--plan", &file.display().to_string()], &RunOpts::default());
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        serde_json::from_str(r.stdout.trim()).unwrap()
    };
    // When the keys are asked twice
    let (a, b) = (keys(), keys());
    // Then each of the three shards holds units and has a key of its own, and the second ask is the first
    assert!(a.iter().all(|k| k != "none"), "{a:?}");
    assert_eq!(a.iter().collect::<std::collections::BTreeSet<_>>().len(), 3);
    assert_eq!(a, b);
}
