use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use vm_ops_api::infrastructure::DockerSiblings;
use vm_ops_api_core::application::ports::{SiblingError, SiblingResult, Siblings};

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|word| (*word).to_string()).collect()
}

fn path_of(dirs: &[&Path]) -> std::ffi::OsString {
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(dirs.iter().map(|dir| dir.to_path_buf()).chain(std::env::split_paths(&rest))).expect("a PATH")
}

/// `await FILE` in a script waits for the test to create FILE, and gives up after 20 s so a failed test leaves nothing running.
const AWAIT: &str = "await() { n=0; while [ ! -e \"$1\" ] && [ \"$n\" -lt 1000 ]; do sleep 0.02; n=$((n+1)); done; }";

/// A `docker` that is a shell script with the body the test gives, first on the PATH of the adapter it hands out.
struct FakeDocker {
    dir: TempDir,
}

impl FakeDocker {
    fn new(body: &str) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        std::fs::create_dir(dir.path().join("bin")).expect("a bin dir");
        let script = dir.path().join("bin/docker");
        std::fs::write(&script, format!("#!/bin/sh\n[ \"$1\" = --probe ] && exit 0\n{AWAIT}\n{body}\n")).expect("the script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("executable");
        wait_until_it_starts(&script);
        Self { dir }
    }

    fn siblings(&self) -> DockerSiblings {
        DockerSiblings::with_path(path_of(&[&self.dir.path().join("bin")]))
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// A thread that forks while the script is open for writing makes its first exec fail with ETXTBSY; wait that out.
fn wait_until_it_starts(script: &Path) {
    for _ in 0..500 {
        match std::process::Command::new(script).arg("--probe").status() {
            Ok(_) => return,
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("the fake docker does not start: {error}"),
        }
    }
    panic!("the fake docker stayed busy");
}

async fn wait_for(file: &Path, ready: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let read = std::fs::read_to_string(file).unwrap_or_default();
        if ready(&read) {
            return read;
        }
        assert!(Instant::now() < deadline, "{} holds {read:?}", file.display());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_text(file: &Path, text: &str) {
    wait_for(file, |read| read == text).await;
}

async fn wait_for_line(file: &Path) -> String {
    wait_for(file, |read| read.ends_with('\n')).await.trim_end().to_string()
}

/// The process group and the session of a live process.
fn group_and_session(pid: &str) -> (i64, i64) {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("a live process");
    let fields: Vec<&str> = stat.rsplit_once(") ").expect("a stat line").1.split(' ').collect();
    (fields[2].parse().expect("a group"), fields[3].parse().expect("a session"))
}

fn release(file: &Path) {
    std::fs::write(file, "").expect("the release file");
}

#[tokio::test]
async fn given_a_docker_when_run_with_awkward_arguments_then_each_reaches_it_as_one_argument_and_its_streams_and_exit_code_come_back() {
    let docker = FakeDocker::new("echo \"argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done; echo err >&2; exit 7");

    let result = docker.siblings().run(&words(&["run", "--rm", "-e", "A=b c", "", "$HOME", "x\ny"])).await;

    let stdout = "argc=7\n[run]\n[--rm]\n[-e]\n[A=b c]\n[]\n[$HOME]\n[x\ny]\n";
    assert_eq!(result, SiblingResult { code: Some(7), stdout: stdout.to_string(), stderr: "err\n".to_string() });
}

#[tokio::test]
async fn given_a_docker_that_exits_zero_when_run_then_the_code_is_zero_and_the_streams_stay_apart() {
    let docker = FakeDocker::new("echo out; echo warn >&2");

    let result = docker.siblings().run(&words(&["ps"])).await;

    assert_eq!(result, SiblingResult::exited(0, "out\n", "warn\n"));
}

#[tokio::test]
async fn given_a_docker_killed_by_a_signal_when_run_then_there_is_no_exit_code_and_what_it_wrote_is_kept() {
    let docker = FakeDocker::new("echo partial; kill -9 $$");

    let result = docker.siblings().run(&words(&["ps"])).await;

    assert_eq!(result, SiblingResult { code: None, stdout: "partial\n".to_string(), stderr: String::new() });
}

#[tokio::test]
async fn given_invalid_utf8_on_both_streams_when_run_then_each_bad_sequence_is_one_replacement_character() {
    let docker = FakeDocker::new("printf 'a\\377b\\342\\202'; printf 'e\\342\\202' >&2");

    let result = docker.siblings().run(&words(&["ps"])).await;

    assert_eq!((result.stdout.as_str(), result.stderr.as_str()), ("a\u{fffd}b\u{fffd}", "e\u{fffd}"));
}

#[tokio::test]
async fn given_more_output_than_a_pipe_holds_on_both_streams_when_run_then_all_of_it_is_captured_uncut() {
    let docker = FakeDocker::new(
        "head -c 100000 /dev/zero | tr '\\0' e >&2; head -c 100000 /dev/zero | tr '\\0' o\nhead -c 100000 /dev/zero | tr '\\0' E >&2; head -c 100000 /dev/zero | tr '\\0' O",
    );

    let result = tokio::time::timeout(Duration::from_secs(30), docker.siblings().run(&words(&["logs"]))).await.expect("no deadlock on a full pipe");

    assert_eq!(result.code, Some(0));
    assert!(result.stdout == format!("{}{}", "o".repeat(100_000), "O".repeat(100_000)), "stdout is {} bytes", result.stdout.len());
    assert!(result.stderr == format!("{}{}", "e".repeat(100_000), "E".repeat(100_000)), "stderr is {} bytes", result.stderr.len());
}

#[tokio::test]
async fn given_a_background_process_that_keeps_the_streams_open_when_run_then_the_result_waits_for_its_output() {
    let docker = FakeDocker::new("(sleep 0.3; echo late) & echo early; exit 3");

    let result = docker.siblings().run(&words(&["compose", "up"])).await;

    assert_eq!(result, SiblingResult::exited(3, "early\nlate\n", ""));
}

#[tokio::test]
async fn given_a_docker_that_reads_its_input_when_run_then_the_input_stays_open_and_empty() {
    let docker = FakeDocker::new("timeout 0.3 cat; echo \"status=$?\"");

    let result = docker.siblings().run(&words(&["run", "-i"])).await;

    assert_eq!(result.stdout, "status=124\n", "a closed input would end the cat at once with status 0");
}

#[tokio::test]
async fn given_the_callers_environment_when_run_then_the_command_inherits_it_and_the_working_directory() {
    let docker = FakeDocker::new("pwd; env");
    let valid_name = |name: &str| name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let skipped = ["PATH", "PWD", "OLDPWD", "SHLVL", "_"];
    let inherited: Vec<String> = std::env::vars().filter(|(name, value)| valid_name(name) && !skipped.contains(&name.as_str()) && !value.contains('\n')).map(|(name, value)| format!("{name}={value}")).collect();

    let result = docker.siblings().run(&words(&["info"])).await;

    let (directory, environment) = result.stdout.split_once('\n').expect("a directory line first");
    let seen: Vec<&str> = environment.lines().collect();
    assert!(!inherited.is_empty());
    let missing: Vec<&String> = inherited.iter().filter(|line| !seen.contains(&line.as_str())).collect();
    assert!(missing.is_empty(), "not inherited: {missing:?}");
    assert_eq!(std::fs::canonicalize(directory).expect("a directory"), std::fs::canonicalize(std::env::current_dir().expect("a directory")).expect("a directory"));
}

#[tokio::test]
async fn given_no_docker_on_the_path_when_run_then_it_reports_code_minus_one_with_nodes_error_text() {
    let empty = tempfile::tempdir().expect("a temp dir");

    let result = DockerSiblings::with_path(empty.path()).run(&words(&["ps"])).await;

    assert_eq!(result, SiblingResult { code: Some(-1), stdout: String::new(), stderr: "Error: spawn docker ENOENT".to_string() });
}

#[tokio::test]
async fn given_a_docker_without_the_execute_bit_when_run_then_it_is_eacces_unless_a_later_path_entry_has_one() {
    let blocked = tempfile::tempdir().expect("a temp dir");
    std::fs::write(blocked.path().join("docker"), "#!/bin/sh\necho blocked\n").expect("the file");
    let fine = FakeDocker::new("echo later");

    let alone = DockerSiblings::with_path(blocked.path()).run(&words(&["ps"])).await;
    let then_fine = DockerSiblings::with_path(path_of(&[blocked.path(), &fine.dir.path().join("bin")])).run(&words(&["ps"])).await;

    assert_eq!(alone, SiblingResult { code: Some(-1), stdout: String::new(), stderr: "Error: spawn docker EACCES".to_string() });
    assert_eq!(then_fine, SiblingResult::exited(0, "later\n", ""));
}

#[tokio::test]
async fn given_a_run_in_flight_when_another_starts_then_it_finishes_first_and_neither_waits_for_the_other() {
    let docker = FakeDocker::new("if [ \"$1\" = slow ]; then echo up > \"$3\"; await \"$2\"; echo slow-done; else echo fast-done; fi");
    let siblings = Arc::new(docker.siblings());
    let (go, started) = (docker.file("go"), docker.file("started"));
    let slow_args = words(&["slow", go.to_str().expect("utf-8"), started.to_str().expect("utf-8")]);
    let slow = tokio::spawn({
        let siblings = siblings.clone();
        async move { siblings.run(&slow_args).await }
    });
    wait_for_line(&started).await;

    let fast = tokio::time::timeout(Duration::from_secs(10), siblings.run(&words(&["fast"]))).await.expect("the fast run is not held up by the slow one");

    assert_eq!(fast.stdout, "fast-done\n");
    assert!(!slow.is_finished());
    release(&go);
    assert_eq!(slow.await.expect("joined").stdout, "slow-done\n");
}

#[tokio::test]
async fn given_a_run_in_flight_when_its_process_is_inspected_then_it_shares_the_callers_group_and_session() {
    let docker = FakeDocker::new("echo $$ > \"$1\"; await \"$2\"");
    let siblings = Arc::new(docker.siblings());
    let (pid_file, go) = (docker.file("pid"), docker.file("go"));
    let args = words(&[pid_file.to_str().expect("utf-8"), go.to_str().expect("utf-8")]);
    let run = tokio::spawn({
        let siblings = siblings.clone();
        async move { siblings.run(&args).await }
    });

    let pid = wait_for_line(&pid_file).await;
    let (command, caller) = (group_and_session(&pid), group_and_session(&std::process::id().to_string()));

    release(&go);
    run.await.expect("joined");
    assert_eq!(command, caller);
}

#[tokio::test]
async fn given_a_detached_sibling_when_spawned_then_each_argument_reaches_docker_as_given() {
    let docker = FakeDocker::new("out=$1; shift; { echo \"argc=$#\"; for a in \"$@\"; do echo \"[$a]\"; done; echo done; } > \"$out\"");
    let out = docker.file("argv");

    let started = docker.siblings().spawn_detached(&words(&[out.to_str().expect("utf-8"), "run", "--pull", "always", "--rm", "-d", "a b", "", "$HOME"]));

    assert_eq!(started, Ok(()));
    wait_for_text(&out, "argc=8\n[run]\n[--pull]\n[always]\n[--rm]\n[-d]\n[a b]\n[]\n[$HOME]\ndone\n").await;
}

#[tokio::test]
async fn given_a_detached_sibling_when_it_runs_then_it_leads_a_session_of_its_own_with_stdio_on_dev_null_in_the_callers_directory() {
    let docker = FakeDocker::new(
        "out=$1; fds=$(readlink /proc/$$/fd/0 /proc/$$/fd/1 /proc/$$/fd/2); { echo $$; sed 's/.*) //' /proc/$$/stat | cut -d' ' -f3,4; echo \"$fds\"; pwd; echo done; } > \"$out\"",
    );
    let out = docker.file("facts");

    docker.siblings().spawn_detached(&words(&[out.to_str().expect("utf-8")])).expect("started");

    let facts = wait_for(&out, |read| read.ends_with("done\n")).await;
    let lines: Vec<&str> = facts.lines().collect();
    let pid = lines[0];
    assert_eq!(lines[1], format!("{pid} {pid}"), "its own process group and session");
    assert_eq!(&lines[2..5], ["/dev/null", "/dev/null", "/dev/null"]);
    assert_eq!(std::fs::canonicalize(lines[5]).expect("a directory"), std::fs::canonicalize(std::env::current_dir().expect("a directory")).expect("a directory"));
    assert_ne!(group_and_session(&std::process::id().to_string()).1.to_string(), pid);
}

#[tokio::test]
async fn given_a_detached_sibling_when_the_spawning_task_ends_then_it_was_not_waited_on_and_finishes_alone() {
    let docker = FakeDocker::new("await \"$2\"; echo finished > \"$1\"");
    let (out, go) = (docker.file("out"), docker.file("go"));
    let siblings = docker.siblings();
    let args = words(&[out.to_str().expect("utf-8"), go.to_str().expect("utf-8")]);

    let started = tokio::spawn(async move { siblings.spawn_detached(&args) }).await.expect("the task ended");

    assert_eq!(started, Ok(()));
    assert!(!out.exists(), "the call returned while the sibling was still blocked");
    release(&go);
    wait_for_text(&out, "finished\n").await;
}

#[test]
fn given_a_detached_sibling_when_the_runtime_that_started_it_shuts_down_then_it_still_finishes() {
    let docker = FakeDocker::new("await \"$2\"; echo finished > \"$1\"");
    let (out, go) = (docker.file("out"), docker.file("go"));
    let siblings = docker.siblings();
    let args = words(&[out.to_str().expect("utf-8"), go.to_str().expect("utf-8")]);
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");

    let started = runtime.block_on(async { siblings.spawn_detached(&args) });
    runtime.shutdown_background();

    assert_eq!(started, Ok(()));
    release(&go);
    let waiter = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    waiter.block_on(wait_for_text(&out, "finished\n"));
}

#[tokio::test]
async fn given_no_docker_on_the_path_when_spawned_detached_then_it_is_an_error_with_nodes_message_and_nothing_crashes() {
    let empty = tempfile::tempdir().expect("a temp dir");

    let started = DockerSiblings::with_path(empty.path()).spawn_detached(&words(&["run", "-d"]));

    assert_eq!(started, Err(SiblingError("spawn docker ENOENT".to_string())));
}
