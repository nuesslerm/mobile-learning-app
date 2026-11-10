use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use tokio::task::JoinSet;
use vm_ops_api::infrastructure::FileCooldownLedger;
use vm_ops_api_core::application::ports::{CooldownLedger, Event, LedgerState, LedgerWrite, Object};
use vm_ops_api_core::testing::FakeEventLog;

/// A ledger as wf's cooldown mirror posts it, compact: keys out of order, a nested array, a fraction, an empty object and list.
const LEDGER: &str = r#"{"minimax/MiniMax-M3":{"until":1700000000000,"reasons":["cap",429],"backoff":0.5},"empty":{},"none":[]}"#;
/// The same ledger as the service stores it: `JSON.stringify(ledger, null, 2)` and a newline.
const LEDGER_FILE: &str = "{\n  \"minimax/MiniMax-M3\": {\n    \"until\": 1700000000000,\n    \"reasons\": [\n      \"cap\",\n      429\n    ],\n    \"backoff\": 0.5\n  },\n  \"empty\": {},\n  \"none\": []\n}\n";

fn object(text: &str) -> Object {
    serde_json::from_str(text).expect("a JSON object")
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).expect("a dir").map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

fn text_of(file: &Path) -> String {
    std::fs::read_to_string(file).expect("the file")
}

fn ledger_in(dir: &Path) -> (FileCooldownLedger, std::path::PathBuf, Arc<FakeEventLog>) {
    let file = dir.join("pool-cooldowns.json");
    let log = Arc::new(FakeEventLog::default());
    (FileCooldownLedger::new(&file, log.clone()), file, log)
}

#[tokio::test]
async fn given_no_file_or_one_that_is_no_object_when_the_ledger_is_read_then_it_is_empty_at_version_0() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());
    assert_eq!(ledger.read().await, LedgerState { ledger: Object::new(), version: 0 });

    for content in ["not json at all", "[1]\n", "5", "null", ""] {
        std::fs::write(&file, content).expect("a file");
        assert_eq!(ledger.read().await, LedgerState { ledger: Object::new(), version: 0 }, "{content:?}");
    }
}

#[tokio::test]
async fn given_a_ledger_at_the_current_version_when_written_then_it_is_stored_pretty_and_the_version_moves_on() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, log) = ledger_in(dir.path());

    let written = ledger.write(object(LEDGER), 0).await;

    assert_eq!(written, LedgerWrite { ok: true, version: 1 });
    assert_eq!(text_of(&file), LEDGER_FILE);
    let read = ledger.read().await;
    assert_eq!(read.version, 1);
    assert_eq!(serde_json::to_string(&read.ledger).expect("json"), LEDGER);
    assert_eq!(log.events(), []);
}

#[tokio::test]
async fn given_the_version_a_write_returned_when_written_again_then_the_version_keeps_moving_on() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());

    let first = ledger.write(object(r#"{"poolA":{}}"#), 0).await;
    let second = ledger.write(object(r#"{"poolA":{},"poolB":{}}"#), first.version).await;

    assert_eq!((first, second), (LedgerWrite { ok: true, version: 1 }, LedgerWrite { ok: true, version: 2 }));
    assert_eq!(text_of(&file), "{\n  \"poolA\": {},\n  \"poolB\": {}\n}\n");
    assert_eq!(ledger.read().await.version, 2);
}

#[tokio::test]
async fn given_a_stale_version_when_written_then_it_is_refused_with_the_current_version_and_the_file_is_untouched() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, log) = ledger_in(dir.path());
    assert!(ledger.write(object(LEDGER), 0).await.ok);

    for stale in [0, -1, 2, i64::MAX] {
        assert_eq!(ledger.write(object(r#"{"poolB":{}}"#), stale).await, LedgerWrite { ok: false, version: 1 }, "{stale}");
    }

    assert_eq!(text_of(&file), LEDGER_FILE);
    assert_eq!(ledger.read().await.version, 1);
    assert_eq!(log.events(), []);
}

#[tokio::test]
async fn given_a_stale_write_on_a_missing_file_when_refused_then_no_file_is_created() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());

    assert_eq!(ledger.write(object("{}"), -1).await, LedgerWrite { ok: false, version: 0 });

    assert!(!file.exists());
}

#[tokio::test]
async fn given_a_stored_ledger_when_a_new_process_reads_it_then_the_ledger_is_there_and_the_version_starts_at_0() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (first, file, _log) = ledger_in(dir.path());
    assert!(first.write(object(LEDGER), 0).await.ok);

    let restarted = FileCooldownLedger::new(&file, Arc::new(FakeEventLog::default()));

    let read = restarted.read().await;
    assert_eq!((read.version, serde_json::to_string(&read.ledger).expect("json")), (0, LEDGER.to_string()));
    assert_eq!(restarted.write(object("{}"), 1).await, LedgerWrite { ok: false, version: 0 });
    assert_eq!(restarted.write(object("{}"), 0).await, LedgerWrite { ok: true, version: 1 });
}

#[tokio::test]
async fn given_a_ledger_file_that_is_a_directory_when_written_then_it_is_refused_logged_and_the_version_stays() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, log) = ledger_in(dir.path());
    std::fs::create_dir(&file).expect("a dir where the ledger file goes");

    let refused = ledger.write(object(LEDGER), 0).await;

    assert_eq!(refused, LedgerWrite { ok: false, version: 0 });
    let err = format!("Error: EISDIR: illegal operation on a directory, open '{}'", file.display());
    assert_eq!(log.events(), [Event::error("cooldowns.persist_failed").with("err", err)]);
    assert_eq!(dir_names(dir.path()), ["pool-cooldowns.json"]);
    std::fs::remove_dir(&file).expect("the dir goes");
    assert_eq!(ledger.write(object(LEDGER), 0).await, LedgerWrite { ok: true, version: 1 });
}

#[tokio::test]
async fn given_a_directory_that_cannot_be_made_when_the_ledger_is_written_then_it_is_refused_with_the_mkdir_error_logged() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let blocker = dir.path().join("state");
    std::fs::write(&blocker, "a file where the state dir goes").expect("a file");
    let log = Arc::new(FakeEventLog::default());
    let ledger = FileCooldownLedger::new(blocker.join("pool-cooldowns.json"), log.clone());

    assert_eq!(ledger.write(object("{}"), 0).await, LedgerWrite { ok: false, version: 0 });

    let err = format!("Error: EEXIST: file already exists, mkdir '{}'", blocker.display());
    assert_eq!(log.events(), [Event::error("cooldowns.persist_failed").with("err", err)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_writers_racing_at_one_version_when_they_write_then_exactly_one_wins() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());
    let ledger = Arc::new(ledger);

    let writers: Vec<_> = (0..16)
        .map(|n| {
            let ledger = ledger.clone();
            tokio::spawn(async move { (n, ledger.write(object(&format!("{{\"writer-{n}\":{{}}}}")), 0).await) })
        })
        .collect();
    let mut results = Vec::new();
    for writer in writers {
        results.push(writer.await.expect("the writer ran"));
    }

    let winners: Vec<_> = results.iter().filter(|(_, write)| write.ok).collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert!(results.iter().all(|(_, write)| write.version == 1), "{results:?}");
    assert_eq!(text_of(&file), format!("{{\n  \"writer-{}\": {{}}\n}}\n", winners[0].0));
    assert_eq!(ledger.read().await.version, 1);
    assert_eq!(dir_names(dir.path()), ["pool-cooldowns.json"]);
}

#[tokio::test]
async fn given_writers_started_together_at_one_version_when_they_write_then_exactly_one_wins() {
    // One thread: every writer reaches the version check before the first write lands, unless the check is guarded.
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());
    let ledger = Arc::new(ledger);

    let mut writers = JoinSet::new();
    for n in 0..8 {
        let ledger = ledger.clone();
        writers.spawn(async move { (n, ledger.write(object(&format!("{{\"writer-{n}\":{{}}}}")), 0).await) });
    }
    let mut results = Vec::new();
    while let Some(done) = writers.join_next().await {
        results.push(done.expect("the writer ran"));
    }

    let winners: Vec<_> = results.iter().filter(|(_, write)| write.ok).collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert!(results.iter().all(|(_, write)| write.version == 1), "{results:?}");
    assert_eq!(text_of(&file), format!("{{\n  \"writer-{}\": {{}}\n}}\n", winners[0].0));
}

#[tokio::test]
async fn given_numbers_when_a_ledger_is_stored_then_they_are_written_as_javascript_writes_them() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());

    assert!(ledger.write(object(r#"{"p":{"cost":0.0000012,"whole":1.0,"exp":1e3,"big":12345678901234567890,"2":"index","b":"named","1":"index"}}"#), 0).await.ok);

    assert_eq!(
        text_of(&file),
        "{\n  \"p\": {\n    \"1\": \"index\",\n    \"2\": \"index\",\n    \"cost\": 0.0000012,\n    \"whole\": 1,\n    \"exp\": 1000,\n    \"big\": 12345678901234567000,\n    \"b\": \"named\"\n  }\n}\n"
    );
}

#[tokio::test]
async fn given_a_directory_that_is_not_there_when_the_ledger_is_written_then_it_is_made() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let file = dir.path().join("state/deeper/pool-cooldowns.json");
    let ledger = FileCooldownLedger::new(&file, Arc::new(FakeEventLog::default()));

    assert!(ledger.write(object("{}"), 0).await.ok);

    assert_eq!(text_of(&file), "{}\n");
}

#[tokio::test]
async fn given_a_reader_holding_the_old_ledger_when_it_is_written_then_it_still_reads_the_old_content_because_the_file_is_replaced() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let (ledger, file, _log) = ledger_in(dir.path());
    assert!(ledger.write(object(r#"{"a":{}}"#), 0).await.ok);
    let mut reader = std::fs::File::open(&file).expect("the file");

    assert!(ledger.write(object(r#"{"b":{}}"#), 1).await.ok);

    let mut seen = String::new();
    reader.read_to_string(&mut seen).expect("the old content");
    assert_eq!(seen, "{\n  \"a\": {}\n}\n");
    assert_eq!(text_of(&file), "{\n  \"b\": {}\n}\n");
}
