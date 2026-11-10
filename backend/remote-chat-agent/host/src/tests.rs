use crate::{FileStore, HostFiles, System};
use rca_service_core::application::runs::RunHosts;
use remote_chat_agent_core::application::ports::{Commands, WorkspaceFiles};
use remote_chat_agent_core::application::store::Store;
use remote_chat_agent_core::domain::types::Event;

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
