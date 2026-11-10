use std::io::{Read, Write};
use std::net::TcpListener;
use vm_ops_api::infrastructure::healthcheck::is_healthy;

/// A server on a loopback port that answers each connection with `reply` and records the request head of the first.
fn serve(reply: &'static str) -> (u16, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().expect("an address").port();
    let head = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("a connection");
        let mut buffer = [0u8; 2048];
        let read = stream.read(&mut buffer).unwrap_or(0);
        let _ = stream.write_all(reply.as_bytes());
        String::from_utf8_lossy(&buffer[..read]).to_string()
    });
    (port, head)
}

fn ok(body: &str) -> &'static str {
    Box::leak(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).into_boxed_str())
}

#[tokio::test]
async fn given_a_health_that_answers_200_when_checked_then_it_is_healthy_and_the_request_carried_the_raw_token() {
    let (port, head) = serve(ok(r#"{"ok":true}"#));

    let healthy = is_healthy(port, "seed-token").await;

    assert!(healthy);
    let head = head.join().expect("the server thread").to_lowercase();
    assert!(head.starts_with("get /health http/1.1") && head.contains("authorization: bearer seed-token"), "{head}");
}

#[tokio::test]
async fn given_a_failing_status_or_nobody_listening_when_checked_then_it_is_not_healthy() {
    // Given a 500 and a port nobody listens on
    let (broken, _) = serve("HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
    let closed = TcpListener::bind("127.0.0.1:0").expect("a port").local_addr().expect("an address").port();

    // When each is checked, Then none is healthy
    assert!(!is_healthy(broken, "t").await);
    assert!(!is_healthy(closed, "t").await);
}
