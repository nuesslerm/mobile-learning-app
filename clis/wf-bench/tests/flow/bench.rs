//! `wf bench`: timing a CLI binary, listing and comparing the committed app-bench suites, and running them (the build suite
//! and the A/B against a stub `pnpm` that plays `expo export`, the android suite on the lab device through a fake vm-ops-api; the
//! browser suites in a real Chromium are `integration/bench.rs`).
use crate::support::{create_stub, run_wf, RunOpts};
use crate::lab::{committed, median, readings, Lab, WEB_SCRIPT};
use wf_vmops::testing::{answers, FakeVm};
use regex::Regex;
use serde_json::{json, Value};
use std::process::Command;

#[test]
fn repo_bench_cli_times_every_startup_target_of_a_binary_and_counts_the_repository_s_footprint() {
    let dir = tempfile::Builder::new().prefix("wf-flow-bench-cli-").tempdir().unwrap();
    let subject = dir.path().join("subject");
    // `--help` fails, every other target succeeds
    create_stub(&subject, "for a in \"$@\"; do [ \"$a\" = \"--help\" ] && exit 3; done\nexit 0");
    let subject_path = subject.display().to_string();
    let bench = |args: &[&str]| {
        let mut full = vec!["bench", "cli", "--no-footprint"];
        full.extend_from_slice(args);
        run_wf(&full, &RunOpts::default().cwd(dir.path()).env("WF_BIN", &subject_path))
    };
    let row = |target: &str, runs: usize, failures: usize| Regex::new(&format!(r"^{} \| {runs} \| [\d.]+ \| [\d.]+ \| [\d.]+ \| [\d.]+ \| [\d.]+ \| {failures}$", regex::escape(target))).unwrap();
    let startup = [("--version", 0), ("--help", 2), ("secret --help", 2), ("offload ask --help", 2), ("ship --changed --help", 2), ("check --help", 2)];

    // When a binary is benched twice per target, Then one row per startup target reports its timings and counts its failures
    let r = bench(&["--runs", "2"]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let lines: Vec<&str> = r.stdout.split('\n').collect();
    assert!(lines[0].starts_with(&format!("bin: {subject_path}")));
    assert_eq!(lines[1], "target | runs | first ms | median ms | max ms | median RSS MB | max RSS MB | failures");
    for (i, (target, failures)) in startup.into_iter().enumerate() {
        assert!(row(target, 2, failures).is_match(lines[2 + i]), "line {}: {:?}", 2 + i, lines[2 + i]);
    }
    assert_eq!(lines.len(), 9);
    assert!(!r.stdout.contains("node_modules files"));

    // Given --live, Then the network and VM targets come after the startup ones
    let r = bench(&["--runs", "1", "--live"]);
    let targets: Vec<&str> = r.stdout.split('\n').skip(2).map(|l| l.split(" | ").next().unwrap()).collect();
    let at = |name: &str| targets.iter().position(|t| *t == name).unwrap_or_else(|| panic!("no {name} in {targets:?}"));
    assert!(at("offload ask --stats") > at("check --help") && targets.contains(&"ship --changed"));

    // Given --out, Then the JSON report holds every target without a footprint
    let out = dir.path().join("report.json");
    assert_eq!(bench(&["--runs", "1", "--out", &out.display().to_string()]).exit_code, 0);
    let report: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(report["bin"], subject_path);
    let argvs: Vec<String> = report["results"].as_array().unwrap().iter().map(|x| x["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(argvs, startup.map(|(t, _)| t.to_string()));
    assert_eq!((report["results"][0]["runs"].clone(), report["results"][0]["failures"].clone(), report["results"][1]["failures"].clone()), (1.into(), 0.into(), 1.into()));
    assert!(report.get("footprint").is_none());

    // Given a bad --runs or a binary that does not exist, Then it fails instead of reporting made-up numbers
    let r = bench(&["--runs", "0"]);
    assert!(r.exit_code == 1 && r.stderr.contains("--runs must be a positive integer, got: 0"), "{}", r.stderr);
    assert_ne!(run_wf(&["bench", "cli", "--runs", "1"], &RunOpts::default().cwd(dir.path()).env("WF_BIN", &dir.path().join("missing").display().to_string())).exit_code, 0);

    // Given a tree with a bundle and node_modules, When the footprint is counted, Then the bundle size and the entry count are reported
    let tree = dir.path().join("tree");
    std::fs::create_dir_all(tree.join("clis/alpha/bundle")).unwrap();
    std::fs::write(tree.join("clis/alpha/bundle/index.js"), vec![b'x'; 1024 * 1024]).unwrap();
    std::fs::create_dir_all(tree.join("clis/beta/src")).unwrap();
    std::fs::create_dir_all(tree.join("node_modules/pkg/lib")).unwrap();
    std::fs::write(tree.join("node_modules/pkg/lib/a.js"), "").unwrap();
    std::fs::write(tree.join("node_modules/pkg/package.json"), "").unwrap();
    let r = run_wf(&["bench", "cli", "--runs", "1"], &RunOpts::default().cwd(&tree).env("WF_BIN", "/bin/true"));
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    // pkg, pkg/lib, pkg/lib/a.js, pkg/package.json
    assert!(r.stdout.contains("bundle alpha/index.js: 1.00 MB\n") && !r.stdout.contains("beta") && r.stdout.ends_with("node_modules files: 4\n"), "{}", r.stdout);
}

const SUITES: [&str; 5] = ["build", "memory", "runtime", "startup", "android"];

fn suite_metrics(suite: &str) -> Vec<String> {
    committed("scenarios", suite)["metrics"].as_object().unwrap().keys().cloned().collect()
}

#[test]
fn repo_bench_lists_and_compares_the_committed_suites_against_their_baselines() {
    let bench = |args: &[&str]| {
        let mut full = vec!["bench"];
        full.extend_from_slice(args);
        run_wf(&full, &RunOpts::default().clean_env())
    };

    // When the suites are listed, Then each shows its metrics and the commit of its initial baseline
    let r = bench(&["list"]);
    assert_eq!(r.exit_code, 0);
    let expected: Vec<String> = SUITES.iter().map(|s| format!("{s}: {} | initial {}", suite_metrics(s).join(", "), &committed("baselines", s)["initial"]["commit"].as_str().unwrap()[..9])).collect();
    assert_eq!(r.stdout.trim_end().split('\n').map(str::to_string).collect::<Vec<_>>(), expected);

    // When all are compared, Then every metric of every suite gets one verdict line under its suite's header, and the exit code follows the verdicts
    let r = bench(&["compare"]);
    let out = r.stdout.trim_end().to_string();
    let lines: Vec<&str> = out.split('\n').collect();
    let header = Regex::new(r"^(PASS|FAIL) ").unwrap();
    let headers: Vec<&&str> = lines.iter().filter(|l| header.is_match(l)).collect();
    assert_eq!(headers.iter().map(|l| l.split(' ').nth(1).unwrap()).collect::<Vec<_>>(), SUITES);
    assert!(headers.iter().all(|l| l.ends_with("(current vs initial baseline)")));
    let all: Vec<String> = SUITES.iter().flat_map(|s| suite_metrics(s)).collect();
    assert_eq!(lines.len(), headers.len() + all.len());
    for m in &all {
        assert!(lines.iter().any(|l| Regex::new(&format!(r"^  \S+\s+{}\s+ref=", regex::escape(m))).unwrap().is_match(l)), "no verdict line for {m}");
    }
    assert_eq!(r.exit_code, if headers.iter().all(|l| l.starts_with("PASS")) { 0 } else { 1 });

    // Given one suite, Then ratios have three decimals and a dash for a zero reference; --budgets says so in the header
    let r = bench(&["compare", "--suite", "memory"]);
    assert!(Regex::new(r"^(PASS|FAIL) memory \(current vs initial baseline\)\n").unwrap().is_match(&r.stdout), "{}", r.stdout);
    assert!(Regex::new(r"(?m) x\d+\.\d{3}(\s|$)").unwrap().is_match(&r.stdout) && Regex::new(r"(?m) x-(\s|$)").unwrap().is_match(&r.stdout) && !r.stdout.contains("build"));
    assert!(Regex::new(r"^(PASS|FAIL) runtime \(current vs initial baseline, budgets on\)\n").unwrap().is_match(&bench(&["compare", "--suite=runtime", "--budgets"]).stdout));

    // Given an unknown suite, Then it exits 1 naming it with nothing on stdout
    let r = bench(&["compare", "--suite", "nope"]);
    assert_eq!((r.exit_code, r.stdout.as_str(), r.stderr.as_str()), (1, "", "unknown suite: nope\nwf: unknown suite: nope\n\n"));

    // Given --help or -h, Then the usage prints and it exits 0; with no command the usage goes to stdout and it exits 1
    for flag in ["--help", "-h"] {
        let r = bench(&[flag]);
        assert!(r.exit_code == 0 && r.stdout.starts_with("wf bench <command> [options]\n\nCommands:\n") && r.stdout.contains("  list                       Show suites"), "{flag}");
    }
    let r = bench(&[]);
    assert_eq!((r.exit_code, r.stderr.as_str()), (1, "wf: passthrough command failed with exit code 1\n"));
    assert!(r.stdout.starts_with("wf bench <command> [options]\n"));
}

#[test]
fn repo_bench_runs_the_build_suite_from_the_export_records_baselines_once_and_flags_regressions() {
    let lab = Lab::new();

    // Given a stub expo export, When the build suite runs twice for web and iOS, Then each metric comes from the export's log and output
    lab.scenario("build", json!({ "cwd": lab.app_rel, "platform": "web", "env": {}, "platforms": ["web", "ios"] }), 3);
    let r = lab.bench(&["build", "--runs", "2"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(Regex::new(r"^build: 2 run\(s\) at [0-9a-f]{9}$").unwrap().is_match(r.stdout.lines().next().unwrap()), "{}", r.stdout);
    let m = readings(&r.stdout);
    for (name, value, unit) in [("webModuleCount", 120.0, "modules"), ("tamaguiPluginMs", 1600.0, "ms"), ("metroBundleMs", 143.0, "ms"), ("jsBytesRaw", WEB_SCRIPT.len() as f64, "bytes"), ("iosModuleCount", 90.0, "modules"), ("iosBundleMs", 800.0, "ms"), ("iosBundleBytes", 4096.0, "bytes")] {
        assert_eq!((median(&m, name), m[name].unit.as_str()), (value, unit), "{name}");
    }
    let gzip = Command::new("gzip").arg("-c").arg(lab.root.path().join("entry.js")).output().unwrap().stdout.len() as f64;
    // another deflate implementation: the size of a 57-byte script may differ by a few bytes
    assert!((median(&m, "jsBytesGzip") - gzip).abs() <= 16.0);
    for name in ["coldExportMs", "warmExportMs", "iosColdExportMs", "iosWarmExportMs"] {
        assert!(m[name].unit == "ms" && m[name].median > 0.0, "{name}");
    }

    // Given no baseline, Then a current record is refused, an initial one is written for both blocks, and a second initial is refused
    lab.scenario("build", json!({ "cwd": lab.app_rel, "platform": "web", "env": {} }), 1);
    let r = lab.bench(&["build", "--runs", "1", "--record", "current"], &[]);
    assert!(r.exit_code != 0 && r.stderr.contains("record the initial baseline for build before a current one") && !lab.baseline("build").exists(), "{}", r.stderr);
    let first = lab.bench(&["build", "--runs", "1", "--record", "initial"], &[]);
    assert_eq!(first.exit_code, 0, "{}", first.stderr);
    let written = std::fs::read_to_string(lab.baseline("build")).unwrap();
    let file: Value = serde_json::from_str(&written).unwrap();
    assert_eq!((file["suite"].clone(), file["initial"]["runs"].clone(), file["initial"]["metrics"]["webModuleCount"]["values"].clone()), (json!("build"), json!(1), json!([120])));
    assert!(Regex::new(r"^[0-9a-f]{40}$").unwrap().is_match(file["initial"]["commit"].as_str().unwrap()));
    assert_eq!(file["current"], file["initial"]);
    let second = lab.bench(&["build", "--runs", "1", "--record", "initial"], &[]);
    assert!(second.exit_code != 0 && second.stderr.contains("baseline for build already has an initial block; it is never overwritten"), "{}", second.stderr);
    assert_eq!(std::fs::read_to_string(lab.baseline("build")).unwrap(), written);

    // Given a baseline far better than the result, Then a regression is reported and wf exits 1
    lab.pin("build", "webModuleCount", 10.0);
    let r = lab.bench(&["build", "--runs", "1"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(Regex::new(r"(?m)^FAIL build \(result vs current baseline\)$").unwrap().is_match(&r.stdout), "{}", r.stdout);
    assert!(Regex::new(r"regressed\s+webModuleCount\s+ref=10 now=120 x12\.000 worse than 10 beyond \d+%").unwrap().is_match(&r.stdout), "{}", r.stdout);
    // and recording both replaces the current block, keeps the initial one, and does not fail the run
    assert_eq!(lab.bench(&["build", "--runs", "1", "--record", "both"], &[]).exit_code, 0);
    let both: Value = serde_json::from_str(&std::fs::read_to_string(lab.baseline("build")).unwrap()).unwrap();
    assert_eq!((both["initial"]["metrics"]["webModuleCount"]["median"].as_f64(), both["current"]["metrics"]["webModuleCount"]["median"].as_f64()), (Some(10.0), Some(120.0)));

    // Given the metric marked not gated, Then a worse result is information, not a regression
    let path = lab.data.join("scenarios/build.json");
    let mut scenario: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    scenario["metrics"]["webModuleCount"]["gate"] = json!(false);
    scenario["metrics"]["webModuleCount"]["gateNote"] = json!("its initial was read by another tool");
    std::fs::write(&path, scenario.to_string()).unwrap();
    lab.pin("build", "webModuleCount", 10.0);
    let r = lab.bench(&["build", "--runs", "1"], &[]);
    assert!(!r.stdout.contains("regressed   webModuleCount"), "{}", r.stdout);
    assert!(Regex::new(r"info\s+webModuleCount\s+ref=10 now=120 x12\.000 not gated: its initial was read by another tool").unwrap().is_match(&r.stdout), "{}", r.stdout);

    // Given a candidate arm that exports slower, When two A/B pairs run, Then the arms alternate and the candidate won no pair
    let r = lab.bench(&["ab", "--env", "STUB_CANDIDATE=1", "--env", "CANDIDATE_SLOW=1", "--pairs", "2"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let arm = Regex::new(r"^ {2}(baseline|candidate) [\d.]+ s$").unwrap();
    assert_eq!(r.stdout.lines().filter(|l| arm.is_match(l)).map(|l| l.trim().split(' ').next().unwrap()).collect::<Vec<_>>(), ["baseline", "candidate", "baseline", "candidate"]);
    assert!(Regex::new(r"(?m)^ab web: baseline median [\d.]+ s, candidate median [\d.]+ s, ratio [\d.]+, candidate faster in 0 of 2 pairs$").unwrap().is_match(&r.stdout), "{}", r.stdout);
    // and without --env the candidate arm is refused
    let r = lab.bench(&["ab", "--pairs", "1"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("ab needs --env KEY=VALUE for the candidate arm"), "{}", r.stderr);
}

/// One cold start as the device script prints it: the launch stamp, the JS marker 5.512 s later, and the app's and the device's memory.
const COLD_START: &str = "@@RUN\n@@T0 1760368120000\n1760368125.512  3815  3841 I ReactNativeJS: Running \"main\", {\"initialProps\":{}}\n\
           Java Heap:    11928                          46292\n\
         Native Heap:    76720                          79884\n\
                Code:    59628                         149208\n\
           TOTAL PSS:   193782            TOTAL RSS:   310416      TOTAL SWAP (KB):        0\n\
 Used RAM: 1,224,882K (  436,846K used pss +   788,036K kernel)\n@@CRASHES 0\n";

#[test]
fn repo_bench_cold_starts_the_lab_apk_on_the_android_lab_device_through_the_vm() {
    let lab = Lab::new();
    let vm = FakeVm::start();
    let params = json!({ "container": "rdl-bench", "port": 5556, "package": "com.x.app", "apkUrl": "https://example.test/lab.apk", "settleSeconds": 1, "teardown": true });
    lab.scenario("android", params, 2);
    let job = |n: u16, output: &str, exit: i64| {
        let id = answers::job_id(n);
        [answers::reply(202, &answers::job_started(&id)), answers::reply(200, &answers::job(&id, Some(exit), output))]
    };
    let script = |n: usize| vm.requests()[n].body.clone();

    // Given the device comes up and two cold starts print their stamps and memory, When the android suite runs twice
    let mut replies: Vec<String> = job(1, "rdl-bench up\n", 0).to_vec();
    replies.extend(job(2, &format!("@@SCREEN Physical size: 432x1200 Physical density: 192\n{COLD_START}{COLD_START}@@APK 67169659\n"), 0));
    replies.extend(job(3, "rdl-bench down\n", 0));
    vm.replies(&replies);
    let r = lab.bench(&["android", "--runs", "2"], &[("WF_TAILNET_SOCKS", &vm.tailnet.address()), ("VM_OPS_API_AUTH_TOKEN", wf_vmops::testing::VM_SEED), ("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", &vm.private_one_line)]);
    // Then the device was booted, the APK installed once and cold-started per run with its JS marker awaited, and the device taken down
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(script(0).contains("rdl-bench") && script(0).contains("5556"), "{}", script(0));
    let runs = script(2);
    assert!(runs.contains("docker cp $apk rdl-bench:/data/local/tmp/lab.apk") && runs.contains("seq 1 2") && runs.contains("am force-stop com.x.app"), "{runs}");
    assert!(runs.matches("pm install").count() == 1 && runs.contains("logcat -v epoch -e 'Running \"main\"' -m 1") && !runs.contains("am start -W"), "{runs}");
    assert!(script(4).contains("docker rm -f rdl-bench"), "{}", script(4));
    // and the cold start, the app's memory parts, the device's RAM, crashes and the APK size are the suite's metrics
    let m = readings(&r.stdout);
    for (name, value) in [("coldStartMs", 5512.0), ("pssBytes", 193782.0 * 1024.0), ("rssBytes", 310416.0 * 1024.0), ("javaHeapBytes", 11928.0 * 1024.0), ("nativeHeapBytes", 76720.0 * 1024.0), ("codeBytes", 59628.0 * 1024.0), ("deviceUsedBytes", 1_224_882.0 * 1024.0), ("crashes", 0.0), ("apkBytes", 67169659.0)] {
        assert_eq!(median(&m, name), value, "{name}");
    }

    // Given a run whose app never logged its marker, Then the suite fails with the tail of the device's output
    let mut replies: Vec<String> = job(4, "", 0).to_vec();
    replies.extend(job(5, "@@RUN\n@@T0 1760368120000\nError: Activity class does not exist\n", 0));
    replies.extend(job(6, "", 0));
    vm.replies(&replies);
    let r = lab.bench(&["android", "--runs", "1"], &[("WF_TAILNET_SOCKS", &vm.tailnet.address()), ("VM_OPS_API_AUTH_TOKEN", wf_vmops::testing::VM_SEED), ("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", &vm.private_one_line)]);
    assert!(r.exit_code != 0 && format!("{}{}", r.stdout, r.stderr).contains("did not log") && format!("{}{}", r.stdout, r.stderr).contains("Activity class"), "{}{}", r.stdout, r.stderr);
}
