use crate::{FileStore, HostFiles, Processes, System};
use rca_cli_driver::application::ports::Spawner;
use rca_service_core::application::runs::RunHosts;
use remote_chat_agent_core::application::ports::{Commands, WorkspaceFiles};
use remote_chat_agent_core::application::store::Store;
use remote_chat_agent_core::domain::types::Event;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio_stream::StreamExt;

#[tokio::test]
async fn given_a_command_that_prints_when_it_runs_then_its_stdout_comes_back() {
    let out = System.run("echo", &["hello"], None, None).await.expect("echo runs");
    assert_eq!(out.stdout.trim(), "hello");
}

#[tokio::test]
async fn given_a_command_that_fails_when_it_runs_then_the_error_names_the_command_and_its_stderr() {
    let err = System.run("sh", &["-c", "echo boom >&2; exit 3"], None, None).await.expect_err("sh exits 3");
    assert!(err.stderr.contains("boom"));
}

#[test]
fn given_a_missing_directory_when_a_file_is_appended_then_the_directory_is_made_and_the_bytes_land() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a/b/log.txt");
    HostFiles.append(&path, b"one\n").unwrap();
    HostFiles.append(&path, b"two\n").unwrap();
    assert_eq!(HostFiles.read(&path).unwrap(), b"one\ntwo\n");
}

#[test]
fn given_a_tree_when_its_files_are_listed_then_every_file_under_it_comes_with_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    HostFiles.write(&dir.path().join("x"), b"1").unwrap();
    HostFiles.create_dir(&dir.path().join("d")).unwrap();
    HostFiles.write(&dir.path().join("d/y"), b"2").unwrap();
    let mut found: Vec<_> = HostFiles.files_under(dir.path()).into_iter().map(|(p, b)| (p.file_name().unwrap().to_string_lossy().into_owned(), b)).collect();
    found.sort();
    assert_eq!(found, vec![("x".to_string(), b"1".to_vec()), ("y".to_string(), b"2".to_vec())]);
}

#[test]
fn given_a_thread_id_that_names_another_place_when_events_are_appended_then_it_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    assert!(store.append("../x", &[]).is_err());
    assert!(store.read_events("a/b", None).is_err());
}

#[test]
fn given_a_log_cut_mid_append_when_it_is_read_then_the_fragment_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let thread = dir.path().join("threads/t1");
    std::fs::create_dir_all(&thread).unwrap();
    std::fs::write(thread.join("events.ndjson"), "{\"sequence\": 1, \"truncat").unwrap();
    assert_eq!(store.read_events("t1", None).unwrap(), Vec::<Event>::new());
    assert_eq!(store.list_thread_ids().unwrap(), vec!["t1".to_string()]);
}

#[test]
fn given_the_hosts_when_one_is_named_as_a_port_then_it_is_a_run_hosts() {
    fn port(_: &dyn RunHosts) {}
    port(&crate::Hosts);
}

async fn run_script(script: &str, dir: &Path) -> (Vec<String>, Result<(), String>, Duration) {
    let started = Instant::now();
    let mut process = Processes::at("/unused").spawn("sh", &["-c".to_string(), script.to_string()], dir).await.expect("sh starts");
    let mut lines = Vec::new();
    while let Some(line) = process.lines.next().await {
        lines.push(line);
    }
    let ended = process.finish.await;
    (lines, ended, started.elapsed())
}

fn gone(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

#[tokio::test]
async fn given_a_cli_that_leaves_a_background_process_holding_its_output_when_it_runs_then_the_turn_ends_with_the_cli_and_nothing_is_left_behind() {
    // Given a CLI that prints a line, starts a long sleep with its stdout inherited, and exits 0
    let dir = tempfile::tempdir().unwrap();
    // When it runs
    let (lines, ended, took) = run_script("echo printed; sleep 30 & echo $! > child.pid; exit 0", dir.path()).await;
    // Then the process ends in seconds, not when the sleep does, with the line it printed and its own exit status
    assert_eq!((lines, ended), (vec!["printed".to_string()], Ok(())));
    assert!(took < Duration::from_secs(5), "{took:?}");
    // And the background process is gone
    let pid: i32 = std::fs::read_to_string(dir.path().join("child.pid")).unwrap().trim().parse().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !gone(pid) {
        assert!(Instant::now() < until, "the background process is still running");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn given_a_cli_that_prints_slowly_and_then_fails_when_it_runs_then_every_line_arrives_and_the_failure_names_its_stderr() {
    // Given a CLI that prints a line every 350 ms (longer than the quiet grace after an exit), then fails
    let dir = tempfile::tempdir().unwrap();
    // When it runs
    let (lines, ended, _) = run_script("for i in 1 2 3; do echo line$i; sleep 0.35; done; echo broke >&2; exit 3", dir.path()).await;
    // Then all lines arrived in order and the end carries the exit status and the stderr tail
    assert_eq!(lines, ["line1", "line2", "line3"]);
    assert!(ended.as_ref().is_err_and(|e| e.starts_with("exited with") && e.ends_with("broke")), "{ended:?}");
}

#[tokio::test]
async fn given_a_cli_that_leaves_a_process_flooding_its_output_when_it_runs_then_the_turn_still_ends_within_the_drain_cap() {
    // Given a CLI that prints a line, leaves `yes` writing to its stdout without pause, and exits 0
    let dir = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let mut process = Processes::at("/unused").spawn("sh", &["-c".to_string(), "echo printed; yes spam & exit 0".to_string()], dir.path()).await.expect("sh starts");
    // When it runs
    let mut first = None;
    while let Some(line) = process.lines.next().await {
        first.get_or_insert(line);
    }
    let ended = process.finish.await;
    // Then it ends in seconds with the first line and its own exit status
    assert_eq!((first.as_deref(), ended), (Some("printed"), Ok(())));
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
}
