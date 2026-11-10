//! Flows through the public client (`Request`, `answers`, `fetch_local`, `download`) against a loopback service double.
#[path = "flow/fake_service.rs"]
mod fake_service;
#[path = "flow/streaming.rs"]
mod streaming;

use cli_http::{answers, decode_uri_component, download, encode_uri_component, fetch_local, form_encode, Request};
use fake_service::{closed_port, redirect, text, FakeService, TempDir};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

#[test]
fn given_a_service_when_a_caller_posts_reads_follows_and_downloads_then_each_exchange_arrives_and_answers_as_sent() {
    // Given a service with a JSON endpoint, a missing page, a redirect, an archive and a broken download
    let service = FakeService::start(|seen| match seen.url.as_str() {
        "/x" => text("{\"ok\":true}", 201),
        "/from" => redirect("/to"),
        "/to" => text("arrived", 200),
        "/archive" => text("archive", 200),
        "/broken" => text("", 500),
        "/status" => text("up", 200),
        _ => text("nope", 404),
    });
    let url = &service.url;

    // When a JSON body is posted with a header
    let posted = Request::post(&format!("{url}/x")).header("X-Secret", "s3").json(&serde_json::json!({ "a": 1 })).send().unwrap();
    // Then it arrives with its headers and the reply comes back
    let seen = service.next();
    assert_eq!((posted.status, posted.text().as_str()), (201, "{\"ok\":true}"));
    assert_eq!((seen.method.as_str(), seen.url.as_str(), seen.body.as_str()), ("POST", "/x", "{\"a\":1}"));
    assert_eq!(seen.header("x-secret"), Some("s3"));
    assert_eq!(seen.header("content-type"), Some("application/json"));

    // When a missing page is read with basic credentials, its name encoded as a URI component
    let segment = encode_uri_component("a b/é?");
    let missing = Request::get(&format!("{url}/{segment}")).basic_auth("user", "pw").send().unwrap();
    // Then the 404 is a response, not an error, and the credentials and the encoded name arrived
    let seen = service.next();
    assert_eq!((missing.status, missing.ok(), missing.text().as_str()), (404, false, "nope"));
    assert_eq!(seen.header("authorization"), Some("Basic dXNlcjpwdw=="));
    assert_eq!(seen.url, "/a%20b%2F%C3%A9%3F");
    assert_eq!(decode_uri_component(&seen.url[1..]), "a b/é?");
    assert_eq!(decode_uri_component("100%zz%E0%A4"), "100%zz\u{FFFD}", "an invalid escape is kept, bytes that are not UTF-8 are replaced");
    assert_eq!(form_encode("a b&c=é"), "a+b%26c%3D%C3%A9");

    // When a redirect is fetched as is, then followed
    let held = Request::get(&format!("{url}/from")).send().unwrap();
    let followed = Request::get(&format!("{url}/from")).follow_redirects().send().unwrap();
    // Then the 3xx comes back unless asked, and the target answers when followed
    assert_eq!((held.status, held.header("location")), (302, Some("/to")));
    assert_eq!((followed.status, followed.text().as_str()), (200, "arrived"));

    // When an archive and a broken download are saved
    let dir = TempDir::new("download");
    download(&format!("{url}/archive"), &dir.0.join("a"), Duration::from_secs(5)).unwrap();
    let failed = download(&format!("{url}/broken"), &dir.0.join("b"), Duration::from_secs(5));
    // Then the file holds the body and the failure leaves no file
    assert_eq!(std::fs::read_to_string(dir.0.join("a")).unwrap(), "archive");
    assert_eq!(failed.unwrap_err(), "HTTP 500");
    assert!(!dir.0.join("b").exists());

    // When the local probes ask a live endpoint and a missing one
    // Then the 2xx answers and its body is read, the 404 is neither
    assert!(answers(&format!("{url}/status"), Duration::from_secs(2)));
    assert_eq!(fetch_local(&format!("{url}/status"), Duration::from_secs(2)).as_deref(), Some("up"));
    assert!(!answers(&format!("{url}/gone"), Duration::from_secs(2)));
    assert_eq!(fetch_local(&format!("{url}/gone"), Duration::from_secs(2)), None);
}

#[test]
fn given_nothing_listening_or_a_stalled_service_when_a_caller_sends_then_it_is_an_error_and_the_probe_says_no() {
    // Given a port nothing listens on
    let closed = format!("http://127.0.0.1:{}", closed_port());
    // When a request and the probe go there
    // Then the request is an error and the probe says no
    assert!(Request::get(&format!("{closed}/")).timeout(Duration::from_secs(2)).send().is_err());
    assert!(!answers(&format!("{closed}/status"), Duration::from_secs(2)));

    // Given a service that holds its answer until the test releases it
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let service = FakeService::start(move |_| {
        let _ = held.lock().unwrap().recv_timeout(Duration::from_secs(5));
        text("late", 200)
    });
    // When the caller gives it 300 ms
    let error = Request::get(&service.url).timeout(Duration::from_millis(300)).send().unwrap_err();
    // Then the request reached the service and failed at the timeout
    let _ = service.next();
    release.send(()).unwrap();
    assert!(error.to_lowercase().contains("timeout") || error.to_lowercase().contains("timed out"), "{error}");
}
