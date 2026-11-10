use serde_json::{json, Value};
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use vm_ops_api::infrastructure::FileRunLog;
use vm_ops_api_core::application::ports::{RunLog, RunMeta, StateError};

fn run_meta(text: &str) -> RunMeta {
    serde_json::from_str(text).expect("a JSON object")
}

/// A run's meta as the start route writes it: `JSON.stringify` of the fields in their order.
fn started(run_id: &str, prompt: &str, agent: &str, started_at: &str) -> String {
    json!({
        "runId": run_id, "prompt": prompt, "agent": agent, "caller": agent, "claudeSessionId": null, "model": "minimax/MiniMax-M3",
        "startedAt": started_at, "finishedAt": null, "status": "running", "ok": null, "tokens": null, "cost": null, "durationMs": null,
    })
    .to_string()
}

fn dir_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).expect("a dir").map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

fn write_run_file(dir: &Path, run_id: &str, file: &str, content: &str) {
    std::fs::create_dir_all(dir.join(run_id)).expect("a run dir");
    std::fs::write(dir.join(run_id).join(file), content).expect("a run file");
}

fn ids(runs: &[RunMeta]) -> Vec<String> {
    runs.iter().map(|run| run["runId"].as_str().expect("a run id").to_string()).collect()
}

#[tokio::test]
async fn given_no_log_directory_when_read_then_there_are_no_runs_no_meta_and_no_lines_and_nothing_is_created() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let state = dir.path().join("opencode-log");
    let log = FileRunLog::new(&state);

    assert_eq!(log.list_runs().await, Vec::<RunMeta>::new());
    assert_eq!(log.read_meta("run-1").await, None);
    assert_eq!(log.read_lines("run-1").await, Vec::<String>::new());

    assert!(!state.exists());
}

#[tokio::test]
async fn given_a_meta_when_written_then_the_run_directory_holds_it_as_compact_json_in_its_key_order() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    let meta = run_meta(r#"{"runId":"run-1","prompt":"p","tokens":{"total":1500,"input":1200.0,"output":300},"cost":0.0000083,"durationMs":12.5,"ok":true,"model":null,"b":1e3}"#);

    log.write_meta("run-1", meta).await.expect("stored");

    let text = std::fs::read_to_string(dir.path().join("run-1/meta.json")).expect("the meta file");
    assert_eq!(text, r#"{"runId":"run-1","prompt":"p","tokens":{"total":1500,"input":1200,"output":300},"cost":0.0000083,"durationMs":12.5,"ok":true,"model":null,"b":1000}"#);
    let read = log.read_meta("run-1").await.expect("the meta");
    assert_eq!(read.keys().collect::<Vec<_>>(), ["runId", "prompt", "tokens", "cost", "durationMs", "ok", "model", "b"]);
    assert_eq!(read["cost"], json!(0.0000083));
}

#[tokio::test]
async fn given_a_stored_meta_when_a_new_one_is_written_then_it_replaces_the_old_whole_and_nothing_is_left_beside_it() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    log.write_meta("run-1", run_meta(r#"{"runId":"run-1","status":"running","prompt":"a long prompt that the next meta lacks"}"#)).await.expect("stored");

    log.write_meta("run-1", run_meta(r#"{"runId":"run-1","status":"finished"}"#)).await.expect("stored");

    assert_eq!(std::fs::read_to_string(dir.path().join("run-1/meta.json")).expect("the meta file"), r#"{"runId":"run-1","status":"finished"}"#);
    assert_eq!(dir_names(&dir.path().join("run-1")), ["meta.json"]);
}

#[tokio::test]
async fn given_a_reader_holding_the_old_meta_when_a_new_one_is_written_then_it_still_reads_the_old_content_because_the_file_is_replaced() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    log.write_meta("run-1", run_meta(r#"{"runId":"run-1","status":"running"}"#)).await.expect("stored");
    let mut reader = std::fs::File::open(dir.path().join("run-1/meta.json")).expect("the meta file");

    log.write_meta("run-1", run_meta(r#"{"runId":"run-1","status":"finished"}"#)).await.expect("stored");

    let mut seen = String::new();
    reader.read_to_string(&mut seen).expect("the old content");
    assert_eq!(seen, r#"{"runId":"run-1","status":"running"}"#);
}

#[tokio::test]
async fn given_a_meta_file_that_is_no_json_object_when_read_then_the_run_has_no_meta() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    for (n, content) in ["{oops", "[1]", "5", "null", "\"text\"", "", "\u{feff}{}"].into_iter().enumerate() {
        write_run_file(dir.path(), &format!("run-{n}"), "meta.json", content);

        assert_eq!(log.read_meta(&format!("run-{n}")).await, None, "{content:?}");
    }
    write_run_file(dir.path(), "run-only-lines", "lines.jsonl", "a\n");
    assert_eq!(log.read_meta("run-only-lines").await, None);
    assert_eq!(log.list_runs().await, Vec::<RunMeta>::new());
}

#[tokio::test]
async fn given_lines_when_appended_then_each_ends_with_a_newline_and_later_appends_follow_in_order() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());

    log.append_lines("run-1", &["first".to_string(), "second".to_string()]).await.expect("stored");
    log.append_lines("run-1", &["third".to_string()]).await.expect("stored");

    assert_eq!(std::fs::read_to_string(dir.path().join("run-1/lines.jsonl")).expect("the lines file"), "first\nsecond\nthird\n");
    assert_eq!(log.read_lines("run-1").await, ["first", "second", "third"]);
    assert_eq!(dir_names(&dir.path().join("run-1")), ["lines.jsonl"]);
}

#[tokio::test]
async fn given_no_lines_when_appended_then_nothing_is_created() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());

    log.append_lines("run-1", &[]).await.expect("nothing to store");

    assert_eq!(dir_names(dir.path()), Vec::<String>::new());
}

#[tokio::test]
async fn given_a_line_with_a_newline_or_nothing_in_it_when_appended_then_it_reads_back_as_the_lines_it_made() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());

    log.append_lines("run-1", &["a\nb".to_string(), String::new(), "c\r".to_string()]).await.expect("stored");

    assert_eq!(std::fs::read_to_string(dir.path().join("run-1/lines.jsonl")).expect("the lines file"), "a\nb\n\nc\r\n");
    assert_eq!(log.read_lines("run-1").await, ["a", "b", "c\r"]);
}

#[tokio::test]
async fn given_a_run_with_more_than_5000_lines_and_empty_ones_among_them_when_read_then_the_last_5000_non_empty_are_returned() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    let lines: Vec<String> = (0..5003).map(|n| format!("{{\"n\":{n}}}")).collect();
    let file = format!("\n{}\n\n{}\n", lines[..5001].join("\n"), lines[5001..].join("\n\n"));
    write_run_file(dir.path(), "run-a", "lines.jsonl", &file);
    write_run_file(dir.path(), "run-b", "meta.json", &started("run-b", "p", "researcher", "2026-10-21T09:00:00.000Z"));

    assert_eq!(log.read_lines("run-a").await, lines[3..]);
    assert_eq!(log.read_lines("run-b").await, Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_many_appends_at_once_to_one_run_when_read_then_every_line_is_there_whole() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = Arc::new(FileRunLog::new(dir.path()));
    let long = "x".repeat(20_000);

    let appenders: Vec<_> = (0..40)
        .map(|n| {
            let log = log.clone();
            let line = format!("{n}:{long}");
            tokio::spawn(async move { log.append_lines("run-1", &[line]).await })
        })
        .collect();
    for appender in appenders {
        appender.await.expect("the appender ran").expect("stored");
    }

    let mut read = log.read_lines("run-1").await;
    read.sort();
    let mut want: Vec<String> = (0..40).map(|n| format!("{n}:{long}")).collect();
    want.sort();
    assert_eq!(read, want);
}

#[tokio::test]
async fn given_runs_when_listed_then_they_come_newest_first_as_stored_and_what_is_no_run_is_skipped() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    let a = started("run-a", "Fix the Login bug", "implementer", "2026-10-20T10:00:00.000Z");
    let b = started("run-b", "Find the docs", "researcher", "2026-10-21T09:00:00.000Z");
    let c = started("run-c", "add a LOGIN test", "implementer", "2026-10-21T11:00:00.000Z");
    for (id, text) in [("run-b", &b), ("run-a", &a), ("run-c", &c)] {
        write_run_file(dir.path(), id, "meta.json", text);
    }
    write_run_file(dir.path(), "run-d", "lines.jsonl", "only lines\n");
    write_run_file(dir.path(), "run-e", "meta.json", "{oops");
    write_run_file(dir.path(), "run-f", "meta.json", "[1]");
    std::fs::write(dir.path().join("stray.txt"), "not a run\n").expect("a stray file");

    let runs = log.list_runs().await;

    let stored: Vec<String> = runs.iter().map(|run| serde_json::to_string(run).expect("json")).collect();
    assert_eq!(stored, [c, b, a]);
}

#[tokio::test]
async fn given_runs_that_started_together_or_with_no_start_when_listed_then_the_directory_order_breaks_the_tie_and_those_with_no_start_come_last() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    let same = "2026-10-21T09:00:00.000Z";
    for id in ["run-f", "run-c", "run-e", "run-b", "run-d", "run-a"] {
        write_run_file(dir.path(), id, "meta.json", &started(id, "p", "implementer", same));
    }
    write_run_file(dir.path(), "run-0", "meta.json", r#"{"runId":"run-0"}"#);
    write_run_file(dir.path(), "run-1", "meta.json", r#"{"runId":"run-1","startedAt":null}"#);
    write_run_file(dir.path(), "run-2", "meta.json", r#"{"runId":"run-2","startedAt":"2026-10-22T00:00:00.000Z"}"#);

    assert_eq!(ids(&log.list_runs().await), ["run-2", "run-a", "run-b", "run-c", "run-d", "run-e", "run-f", "run-0", "run-1"]);
}

#[tokio::test]
async fn given_a_run_directory_that_is_a_file_when_written_then_it_fails_with_nodes_mkdir_message() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    std::fs::write(dir.path().join("run-1"), "a file where the run dir goes").expect("a file");
    let blocked = format!("EEXIST: file already exists, mkdir '{}'", dir.path().join("run-1").display());

    assert_eq!(log.write_meta("run-1", run_meta(r#"{"runId":"run-1"}"#)).await, Err(StateError(blocked.clone())));
    assert_eq!(log.append_lines("run-1", &["a".to_string()]).await, Err(StateError(blocked)));

    let under = format!("ENOTDIR: not a directory, mkdir '{}'", dir.path().join("run-1/sub").display());
    assert_eq!(log.write_meta("run-1/sub", run_meta(r#"{"runId":"x"}"#)).await, Err(StateError(under)));
}

#[tokio::test]
async fn given_a_meta_or_lines_path_that_is_a_directory_when_written_then_it_fails_with_nodes_open_message_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());
    std::fs::create_dir_all(dir.path().join("run-1/meta.json")).expect("a dir where the meta file goes");
    std::fs::create_dir_all(dir.path().join("run-1/lines.jsonl")).expect("a dir where the lines file goes");
    let meta_path = dir.path().join("run-1/meta.json");
    let lines_path = dir.path().join("run-1/lines.jsonl");

    let meta_error = log.write_meta("run-1", run_meta(r#"{"runId":"run-1"}"#)).await;
    let lines_error = log.append_lines("run-1", &["a".to_string()]).await;

    assert_eq!(meta_error, Err(StateError(format!("EISDIR: illegal operation on a directory, open '{}'", meta_path.display()))));
    assert_eq!(lines_error, Err(StateError(format!("EISDIR: illegal operation on a directory, open '{}'", lines_path.display()))));
    assert_eq!(dir_names(&dir.path().join("run-1")), ["lines.jsonl", "meta.json"]);
}

#[tokio::test]
async fn given_a_whole_float_and_a_small_cost_when_a_meta_is_stored_then_the_reader_gets_the_value_back_exactly() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let log = FileRunLog::new(dir.path());

    log.write_meta("run-1", run_meta(r#"{"runId":"run-1","cost":0.000004,"durationMs":4567.0}"#)).await.expect("stored");

    let read = log.read_meta("run-1").await.expect("the meta");
    assert_eq!(Value::Object(read), json!({ "runId": "run-1", "cost": 0.000004, "durationMs": 4567 }));
    assert_eq!(std::fs::read_to_string(dir.path().join("run-1/meta.json")).expect("the meta file"), r#"{"runId":"run-1","cost":0.000004,"durationMs":4567}"#);
}
