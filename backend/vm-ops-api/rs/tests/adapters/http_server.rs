use secrecy::SecretString;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use vm_ops_api::api::serve::serve;
use vm_ops_api::api::{self, Limits, Ports, Settings};
use vm_ops_api_core::application::health::Build;
use vm_ops_api_core::domain::auth::AuthConfig;
use vm_ops_api_core::testing::{FakeClock, FakeContainers, FakeCooldownLedger, FakeEventLog, FakeHostFiles, FakeIds, FakeRunLog, FakeSiblings};

const ASSERTED_WITHIN: Duration = Duration::from_secs(5);

struct Server {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

async fn start(limits: Limits) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free loopback port");
    let addr = listener.local_addr().expect("a bound address");
    let build = Build { commit: "c".into(), host: "main".into(), debug_mode: false, server_script_sha: "s".into(), image_tag: "t".into() };
    let ports = Ports {
        clock: Arc::new(FakeClock::at(0)),
        log: Arc::new(FakeEventLog::default()),
        containers: Arc::new(FakeContainers::new()),
        host_files: Arc::new(FakeHostFiles::default()),
        cooldowns: Arc::new(FakeCooldownLedger::default()),
        run_log: Arc::new(FakeRunLog::default()),
        ids: Arc::new(FakeIds::default()),
        siblings: Arc::new(FakeSiblings::new()),
    };
    let settings = Settings {
        auth: AuthConfig::new(SecretString::from("seed"), "", false),
        ci_auth: None,
        build,
        limits,
        dashboard: b"",
        sibling_image: "ns/vm-ops-api-rs:t".into(),
        redeploy: Default::default(),
    };
    let router = api::app(ports, settings);
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(serve(listener, router, async move { let _ = stopped.await; }, limits));
    Server { addr, stop, task }
}

async fn connect(server: &Server) -> TcpStream {
    TcpStream::connect(server.addr).await.expect("the server accepts")
}

/// What the server sends until it ends the connection, and how long that took; the test fails if it is still open after 5 s.
async fn until_closed(stream: &mut TcpStream) -> (String, Duration) {
    let started = Instant::now();
    let mut received = Vec::new();
    let ended = tokio::time::timeout(ASSERTED_WITHIN, stream.read_to_end(&mut received)).await;
    assert!(ended.is_ok(), "the connection was still open after {ASSERTED_WITHIN:?}; received {:?}", String::from_utf8_lossy(&received));
    (String::from_utf8_lossy(&received).to_string(), started.elapsed())
}

fn limits(head_ms: u64, body_ms: u64, drain_ms: u64) -> Limits {
    Limits { head: Duration::from_millis(head_ms), body: Duration::from_millis(body_ms), drain: Duration::from_millis(drain_ms) }
}

#[tokio::test]
async fn given_a_complete_request_when_it_arrives_over_a_socket_then_the_router_answers_it() {
    // Given a server with generous limits
    let server = start(limits(60_000, 60_000, 500)).await;
    let mut client = connect(&server).await;

    // When a request with no credentials is sent and the connection asked to close after it
    client.write_all(b"GET /health HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n").await.expect("sent");
    let (reply, _) = until_closed(&mut client).await;

    // Then the gate answers it
    assert!(reply.starts_with("HTTP/1.1 401 Unauthorized\r\n"), "{reply}");
    assert!(reply.ends_with(r#"{"error":"unauthorized"}"#), "{reply}");
}

#[tokio::test]
async fn given_a_client_that_never_finishes_the_request_head_when_the_head_limit_passes_then_the_server_ends_the_connection() {
    // Given a server that gives a head 300 ms
    let server = start(limits(300, 60_000, 500)).await;
    let mut client = connect(&server).await;

    // When a client sends half a head and then nothing
    client.write_all(b"GET /health HTTP/1.1\r\nhost: x\r\nx-slow: a").await.expect("sent");
    let (received, waited) = until_closed(&mut client).await;

    // Then the connection ends after the limit, with no answer, and not before it
    assert_eq!(received, "");
    assert!(waited >= Duration::from_millis(250), "closed after only {waited:?}");
}

#[tokio::test]
async fn given_an_idle_connection_when_the_head_limit_passes_then_the_server_ends_it() {
    // Given a connection that has had one answer and is kept alive
    let server = start(limits(300, 60_000, 500)).await;
    let mut client = connect(&server).await;
    client.write_all(b"GET /health HTTP/1.1\r\nhost: x\r\n\r\n").await.expect("sent");
    let mut first = [0u8; 12];
    client.read_exact(&mut first).await.expect("the first answer");
    assert_eq!(&first, b"HTTP/1.1 401");

    // When it sends nothing more
    let (rest, waited) = until_closed(&mut client).await;

    // Then it is ended after the limit
    assert!(rest.ends_with(r#"{"error":"unauthorized"}"#), "{rest}");
    assert!(waited >= Duration::from_millis(250), "closed after only {waited:?}");
}

#[tokio::test]
async fn given_a_body_that_stops_arriving_when_the_body_limit_passes_then_the_answer_is_408_and_the_connection_ends() {
    // Given a server that gives a body 300 ms
    let server = start(limits(60_000, 300, 500)).await;
    let mut client = connect(&server).await;

    // When a client promises 10 bytes, sends 3 and stops
    client.write_all(b"POST /exec HTTP/1.1\r\nhost: x\r\ncontent-length: 10\r\n\r\nabc").await.expect("sent");
    let (reply, waited) = until_closed(&mut client).await;

    // Then it is answered 408 with the connection closing, after the limit and not before it
    assert!(reply.starts_with("HTTP/1.1 408 Request Timeout\r\n"), "{reply}");
    assert!(reply.contains("connection: close\r\n"), "{reply}");
    assert!(reply.ends_with(r#"{"error":"request timeout"}"#), "{reply}");
    assert!(waited >= Duration::from_millis(250), "answered after only {waited:?}");
}

#[tokio::test]
async fn given_requests_still_open_when_the_server_is_told_to_stop_then_it_ends_within_the_drain_and_closes_them() {
    // Given a server with a long body limit and a 300 ms drain, a half-sent POST and an idle kept-alive connection
    let server = start(limits(60_000, 60_000, 300)).await;
    let mut stalled = connect(&server).await;
    stalled.write_all(b"POST /exec HTTP/1.1\r\nhost: x\r\ncontent-length: 10\r\n\r\nabc").await.expect("sent");
    let mut idle = connect(&server).await;
    idle.write_all(b"GET /health HTTP/1.1\r\nhost: x\r\n\r\n").await.expect("sent");
    let mut first = [0u8; 12];
    idle.read_exact(&mut first).await.expect("the idle connection's answer");

    // When the server is told to stop
    let stopped_at = Instant::now();
    server.stop.send(()).expect("the server is still running");
    let ended = tokio::time::timeout(ASSERTED_WITHIN, server.task).await;

    // Then it ends after the drain, not after the 60 s body limit, and both clients find their connection closed
    assert!(ended.is_ok(), "the server was still running 5 s after the stop");
    assert!(stopped_at.elapsed() < Duration::from_secs(3), "stopped after {:?}", stopped_at.elapsed());
    for (what, client) in [("the half-sent POST", &mut stalled), ("the idle connection", &mut idle)] {
        let mut received = Vec::new();
        let closed = tokio::time::timeout(ASSERTED_WITHIN, client.read_to_end(&mut received)).await;
        assert!(closed.is_ok(), "{what} is still open");
    }
}
