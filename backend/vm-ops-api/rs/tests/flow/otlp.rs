//! The real event sinks behind the router: an ingest that never answers must not hold a request, and the line still reaches stdout.
//! This is deliberate difference 6 of the plan: credentials replies without waiting on the export. Given/When/Then.
use super::harness::{world_with, Doubles};
use service_kit::telemetry::{Destination, Source};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vm_ops_api::infrastructure::{OtlpEventLog, StdoutEventLog, TeeEventLog};
use vm_ops_api_core::application::ports::{EventLog, ExecResult};
use vm_ops_api_core::domain::containers::DockerContainer;
use vm_ops_api_core::testing::{FakeContainers, FakeEventLog};

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())).to_string()
    }
}

#[tokio::test]
async fn given_an_ingest_that_never_answers_when_credentials_are_written_then_the_reply_does_not_wait_and_the_stdout_line_is_there() {
    // Given an ingest nobody accepts on, a stdout capture, and the two sinks teed as `main` tees them
    let ingest = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let destination = Destination::new(format!("http://{}", ingest.local_addr().expect("an address")), "ingest-token");
    let stdout = Captured::default();
    let stdout_log = Arc::new(StdoutEventLog::to(stdout.clone()));
    let problems: Arc<dyn EventLog> = Arc::new(FakeEventLog::default());
    let otlp = Arc::new(OtlpEventLog::new(Source::new("vm-ops-api", "0.1.0"), destination, "main", Vec::new(), problems));
    let log: Arc<dyn EventLog> = Arc::new(TeeEventLog::new(vec![stdout_log, otlp.clone()]));
    let shell = DockerContainer { id: "as1".into(), names: Some(vec!["/agent-shell".into()]), state: Some("running".into()), ..DockerContainer::default() };
    let containers = FakeContainers::new().with_app_containers("agent-shell", vec![shell]).with_exec(&[], vec![Ok(ExecResult::attached(Some(0), String::new()))]);
    let world = world_with(false, Doubles { containers, log: Some(log), ..Doubles::default() });

    // When credentials are posted
    let body = br#"{"tool":"codex","content":"eA=="}"#;
    let reply = tokio::time::timeout(Duration::from_secs(5), world.signed("POST", "/agent-shell/credentials", body)).await.expect("the reply waited for the export");

    // Then it is 200 with the file's path, and the event is on stdout already
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(stdout.text(), "{\"level\":\"info\",\"message\":\"agent_shell_credentials.written\",\"tool\":\"codex\"}\n");
    // And the export is still waiting on the ingest, which a flush gives up on when asked
    assert!(!otlp.flush(Duration::from_millis(200)));
}
