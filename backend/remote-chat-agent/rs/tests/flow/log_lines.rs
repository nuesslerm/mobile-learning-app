//! The lines the service writes at its console sink, pinned: a refactor of `application::log` may not change one silently. Each line is
//! what a journey through the router makes the sink print, in `tracing`'s own format (event name, then the `meta` object).
use crate::harness::{post, service, Env, Options};
use serde_json::json;
use std::io::Write;
use std::sync::{Mutex, Once};

static SINK: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static INSTALL: Once = Once::new();

struct Sink;

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        SINK.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Starts a fresh capture of the console sink; the flows of this binary take turns, so the lines are this flow's.
fn capture() {
    INSTALL.call_once(|| {
        let subscriber = tracing_subscriber::fmt().with_ansi(false).without_time().with_target(false).with_writer(|| Sink).finish();
        tracing::subscriber::set_global_default(subscriber).expect("no other subscriber in the flow binary");
    });
    SINK.lock().unwrap().clear();
}

fn printed(event: &str) -> Vec<String> {
    let text = String::from_utf8(SINK.lock().unwrap().clone()).unwrap();
    text.lines().filter(|line| line.contains(&format!(" {event} ")) || line.ends_with(&format!(" {event}"))).map(str::to_string).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_startup_a_refused_socket_and_a_failed_run_when_the_service_logs_then_the_sink_prints_each_event_with_its_fields_as_pinned() {
    // Given the sink captured and a service that starts with no thread to recover
    let env = Env::process(&[("OPENCODE_LADDER_IGNORE_STATS", "1")]);
    capture();
    let repo = env.dir("repo");
    std::fs::create_dir(repo.join(".git")).unwrap();
    let svc = service(&env, Options { ledger_error: Some("POOL_COOLDOWNS_ORDS_BASE_URL must be set"), ..Options::default() });
    // When a socket is opened with a wrong token and a run is requested with no usable ledger
    let served = svc.serve();
    assert_eq!(served.connect("?token=wrong").await.err().map(|(status, _)| status), Some(401));
    let cwd = repo.to_string_lossy().into_owned();
    svc.send(post("/runs").secret().json(json!({ "prompt": "p", "cwd": cwd }))).await;
    // Then each event is one line: level, event name, and the meta object with the typed values
    assert_eq!(printed("agent_threads.started"), vec![r#" INFO agent_threads.started meta={"interrupted":0}"#.to_string()]);
    assert_eq!(printed("ws.upgrade.unauthorized"), vec![r#" WARN request{otel.name=GET /ws http.method=GET http.route=/ws}: ws.upgrade.unauthorized meta={"pathname":"/ws","remoteAddress":"127.0.0.1:40001"}"#.to_string()]);
    let ledger = format!(r#"ERROR request{{otel.name=POST /runs http.method=POST http.route=/runs}}: runs.ledger_unavailable meta={{"cwd":{},"why":"POOL_COOLDOWNS_ORDS_BASE_URL must be set"}}"#, json!(cwd));
    assert_eq!(printed("runs.ledger_unavailable").iter().map(|l| l.trim_start().to_string()).collect::<Vec<_>>(), vec![ledger]);
}
