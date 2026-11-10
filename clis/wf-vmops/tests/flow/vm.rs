//! `wf vm exec`: a script run on the VM through vm-ops-api's start, poll and cancel job model. vm-ops-api is a fake behind a fake
//! tailnet (`wf_vmops::testing`).
use crate::support::run_wf;
use base64::Engine;
use serde_json::json;
use wf_vmops::testing::{answers, FakeVm};

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

#[test]
fn vm_exec_runs_a_script_on_the_vm_streams_its_output_and_brings_back_its_artifacts() {
    let vm = FakeVm::start();
    let dir = tempfile::Builder::new().prefix("deploy-exec-").tempdir().unwrap();
    let base = "http://vm-ops-api-main:8080/services/agent-shell";
    let exec = |args: &[&str], replies: &[String]| {
        vm.replies(replies);
        let mut full = vec!["vm", "exec"];
        full.extend_from_slice(args);
        run_wf(&full, &vm.opts().cwd(dir.path()))
    };
    let script = |body: &str| {
        let file = dir.path().join("job.sh");
        std::fs::write(&file, body).unwrap();
        file.display().to_string()
    };
    let url = |n: usize| format!("http://vm-ops-api-main:8080{}", vm.requests()[n - 1].url);
    let started = |job: &str| answers::reply(202, &answers::job_started(job));
    let finished = |job: &str, exit_code: i64, output: &str| answers::reply(200, &answers::job(job, Some(exit_code), output));
    let artifact = |path: &str, text: &str| answers::reply(200, &answers::artifact(path, &b64(text)));
    let [j1, j2, j3, j4, j5] = [1, 2, 3, 4, 5].map(answers::job_id);

    // Given a script, When it is started without waiting, Then its body is the POST body and the job id is printed
    let r = exec(&[&script("echo from-vm\n"), "--no-wait"], &[started(&j1)]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("{j1}\n")));
    assert_eq!((vm.requests()[0].method.as_str(), url(1), vm.requests()[0].body.as_str()), ("POST", format!("{base}/exec-script"), "echo from-vm\n"));
    // and a start answered anything but 202 fails naming the status: a 200 (Node never sends it; wf takes a job id only from
    // a 202), no agent-shell, and a step that failed in agent-shell, with its output
    let r = exec(&[&script("x"), "--no-wait"], &[answers::reply(200, &answers::job_started(&j1))]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains(&format!("returned HTTP 200: {{\"jobId\":\"{j1}\"}}")), "{}", r.stderr);
    let r = exec(&[&script("x"), "--no-wait"], &[answers::reply(503, &answers::agent_shell_not_running())]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("returned HTTP 503: {\"error\":\"agent-shell container is not running\"}"), "{}", r.stderr);
    let mkdir = json!({ "error": "mkdir failed inside agent-shell", "exitCode": 1, "output": "mkdir: can't create directory '/tmp/exec-jobs': No space left on device\n" });
    let r = exec(&[&script("x"), "--no-wait"], &[answers::reply(502, &mkdir)]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("returned HTTP 502: exitCode=1 error=mkdir failed inside agent-shell\n--- script output ---\nmkdir: can't create directory '/tmp/exec-jobs': No space left on device"), "{}", r.stderr);

    // Given a job that prints over two polls and exits 3, Then only new text streams, the job id goes to stderr and wf exits 3
    let r = exec(&[&script("run")], &[started(&j2), answers::reply(200, &answers::job(&j2, None, "hello ")), finished(&j2, 3, "hello world\n")]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (3, "hello world\n"));
    assert!(r.stderr.contains(&format!("jobId: {j2}\n")));
    assert_eq!((url(2), vm.requests().len()), (format!("{base}/exec-script/{j2}"), 3));

    // Given --out and artifacts, When the job fails, Then output, exit code and every artifact are saved and the exit code propagates
    let out = dir.path().join("out");
    let r = exec(
        &[&script("run"), "--out", &out.display().to_string(), "--artifact", "results/a.txt", "--artifact", "b-dir/c.txt"],
        &[started(&j3), finished(&j3, 2, "partial\n"), artifact("results/a.txt", "hello"), artifact("b-dir/c.txt", "world")],
    );
    assert_eq!(r.exit_code, 2);
    let read = |rel: &str| std::fs::read_to_string(out.join(rel)).unwrap();
    assert_eq!([read("output.log"), read("exit-code"), read("results/a.txt"), read("b-dir/c.txt")], ["partial\n", "2\n", "hello", "world"]);
    assert_eq!([url(3), url(4)], [format!("{base}/artifact?path=results%2Fa.txt"), format!("{base}/artifact?path=b-dir%2Fc.txt")]);
    // and an artifact path that climbs out is refused once the job has run
    let r = exec(&[&script("run"), "--out", &dir.path().join("o").display().to_string(), "--artifact", "../x"], &[started(&j4), finished(&j4, 0, "")]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: artifact path may not contain '..': ../x"), "{}", r.stderr);
    assert_eq!(vm.requests().len(), 2);

    // When a job is polled, cancelled or its whole log fetched, Then each goes to its route
    let status = answers::job(&j2, Some(0), "hi");
    let r = exec(&["--status", &j2], &[answers::reply(200, &status)]);
    assert_eq!((r.exit_code, r.stdout.clone(), url(1)), (0, format!("{}\n", serde_json::to_string_pretty(&status).unwrap()), format!("{base}/exec-script/{j2}")));
    let r = exec(&["--cancel", &j2], &[answers::reply(200, &answers::ok())]);
    assert_eq!((r.stdout.as_str(), vm.requests()[0].method.as_str(), url(1)), ("ok\n", "POST", format!("{base}/exec-script/{j2}/cancel")));
    let r = exec(&["--log", "70e3d788-fec3"], &[started(&j5), finished(&j5, 0, ""), artifact("job-logs/70e3d788-fec3.log", "line 1\nline 2\n")]);
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "line 1\nline 2\n"), "{}", r.stderr);
    assert!(vm.requests()[0].body.contains("cp /tmp/exec-jobs/70e3d788-fec3/output.log /home/agent/e2e-artifacts/job-logs/70e3d788-fec3.log"));
    assert_eq!(url(3), format!("{base}/artifact?path=job-logs%2F70e3d788-fec3.log"));

    // Given the removed worktree runner's options, When each is passed, Then it is an unknown option (exit 2) and no request is made
    for (args, option) in [(vec!["--e2e", "home", "--branch", "feat/x", "--out", "o"], "--e2e"), (vec!["--branch", "dev", "job.sh"], "--branch"), (vec!["--e2e=home"], "--e2e")] {
        let r = exec(&args, &[]);
        assert_eq!(r.exit_code, 2, "{args:?}: {}{}", r.stdout, r.stderr);
        assert!(r.stderr.contains(&format!("unexpected argument '{option}")), "{args:?}: {}", r.stderr);
        assert!(vm.requests().is_empty(), "{args:?}");
    }
    // and the help no longer names them, while the plain script, status, cancel and log paths it keeps are listed
    let help = run_wf(&["vm", "exec", "--help"], &vm.opts());
    assert_eq!(help.exit_code, 0, "{}{}", help.stdout, help.stderr);
    for gone in ["--e2e", "--branch", "dev-bridge", "verify-"] {
        assert!(!help.stdout.contains(gone), "{gone} is still in the help: {}", help.stdout);
    }
    for kept in ["--status", "--cancel", "--log", "--artifact", "--out", "--no-wait"] {
        assert!(help.stdout.contains(kept), "{kept} is gone from the help: {}", help.stdout);
    }

    // Given a command line that cannot run, Then it is refused before any call
    let big = script(&"x".repeat(65537));
    let missing = dir.path().join("missing.sh").display().to_string();
    let o = dir.path().join("o").display().to_string();
    let refused: Vec<(Vec<&str>, &str)> = vec![
        (vec![], "wf: wf vm exec needs a <file>, --status <jobId> or --cancel <jobId>"),
        (vec!["--log", "x; rm -rf /"], "not a job id"),
        (vec![&big, "--no-wait", "--artifact", "a.txt"], "wf: --artifact cannot be used with --no-wait (artifacts are fetched after the script exits)"),
        (vec![&big, "--out", &o], "wf: Script exceeds 65536-byte cap (was 65537 bytes)"),
        (vec![&missing], "no such file"),
    ];
    for (args, message) in refused {
        let r = exec(&args, &[]);
        assert_eq!(r.exit_code, 1, "{args:?}");
        assert!(r.stderr.contains(message), "{args:?}: {}", r.stderr);
        assert!(vm.requests().is_empty(), "{args:?}");
    }
}
