//! Flow tests of the control surface: each request enters through the router `main` serves, built by `api::app` from ports whose
//! outside world is doubled (see `flow/harness.rs`). Given/When/Then.
#[path = "flow/drill.rs"]
mod drill;
#[path = "flow/harness.rs"]
mod harness;
#[path = "flow/otlp.rs"]
mod otlp;
#[path = "flow/ports.rs"]
mod ports;
#[path = "flow/reads.rs"]
mod reads;
#[path = "flow/redeploy.rs"]
mod redeploy;
#[path = "flow/self_redeploy.rs"]
mod self_redeploy;
#[path = "flow/writes.rs"]
mod writes;

use harness::{legacy_headers, signature, signed_headers, today, world, DAY, NOON, SEED};
use vm_ops_api_core::application::ports::Event;
use vm_ops_api_core::domain::auth::derive_rotating_token;

const UNAUTHORIZED: &str = r#"{"error":"unauthorized"}"#;
const NOT_FOUND: &str = r#"{"error":"not found"}"#;

#[tokio::test]
async fn given_a_signed_request_when_health_is_read_then_it_reports_the_build_in_node_s_key_order_as_plain_json() {
    // Given the service, started at noon on 4 Oct 2026
    let world = world(false);
    // When an hour passes and /health is read, signed
    world.clock.set(NOON + 3_600_000);
    let reply = world.signed("GET", "/health", b"").await;
    // Then it is 200 with exactly the eight keys in order and both times as ISO strings
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(
        reply.body,
        r#"{"ok":true,"commit":"0123456789abcdef","startedAt":"2026-10-04T12:00:00.000Z","host":"main","debugMode":false,"time":"2026-10-04T13:00:00.000Z","serverScriptSha":"0123456789ab","imageTag":"sha-1234567"}"#
    );
    // And the reply says exactly application/json, carries its length and no CORS header
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_eq!(reply.header("content-length"), Some(reply.body.len().to_string().as_str()));
    assert!(!reply.headers.iter().any(|(key, _)| key.starts_with("access-control-")), "{:?}", reply.headers);
}

#[tokio::test]
async fn given_the_legacy_raw_token_when_health_is_read_then_it_passes_unless_signed_auth_is_required() {
    // Given the service in its migration posture
    let open = world(false);
    // When /health is read with the raw seed, Then it is 200
    assert_eq!(open.send("GET", "/health", &legacy_headers(), b"").await.status, 200);
    // Given the service that requires signed auth
    let strict = world(true);
    // When /health is read with the raw seed, Then it is 401, and a signed read is still 200
    let refused = strict.send("GET", "/health", &legacy_headers(), b"").await;
    assert_eq!((refused.status, refused.body.as_str()), (401, UNAUTHORIZED));
    assert_eq!(strict.signed("GET", "/health", b"").await.status, 200);
}

#[tokio::test]
async fn given_no_credentials_or_the_wrong_ones_when_health_is_read_then_it_is_401() {
    // Given the service, and the headers of a valid signed request
    let world = world(false);
    let signed = signed_headers("GET", "/health", b"", NOON, &today());
    let signature_only = signed[1..].to_vec();
    let mut wrong_case = vec![("authorization".to_string(), format!("bearer {}", derive_rotating_token(SEED, &today())))];
    wrong_case.extend(signature_only.clone());
    let attempts = [
        ("no Authorization", Vec::new()),
        ("signature headers only", signature_only),
        ("a lowercase bearer scheme", wrong_case),
        ("Basic", vec![("authorization".to_string(), "Basic c2VlZA==".to_string())]),
        ("a rotating token without a signature", vec![("authorization".to_string(), format!("Bearer {}", derive_rotating_token(SEED, &today())))]),
        ("the token of two days ago, signed", signed_headers("GET", "/health", b"", NOON, "2026-10-02")),
    ];
    // When /health is read with each, Then each is 401 unauthorized
    for (what, headers) in attempts {
        let reply = world.send("GET", "/health", &headers, b"").await;
        assert_eq!((reply.status, reply.body.as_str()), (401, UNAUTHORIZED), "{what}");
    }
}

#[tokio::test]
async fn given_the_token_of_yesterday_when_the_clock_moves_a_day_on_then_the_gate_stops_taking_it() {
    // Given the service at noon, and a request signed with the token of 3 Oct
    let world = world(false);
    let yesterday = signed_headers("GET", "/health", b"", NOON, "2026-10-03");
    // When it is sent, Then it passes
    assert_eq!(world.send("GET", "/health", &yesterday, b"").await.status, 200);
    // When the clock moves a day on and a request is signed for that moment with the same token
    world.clock.set(NOON + DAY);
    let stale = signed_headers("GET", "/health", b"", NOON + DAY, "2026-10-03");
    // Then it is 401
    assert_eq!(world.send("GET", "/health", &stale, b"").await.status, 401);
}

#[tokio::test]
async fn given_an_unknown_path_when_requested_then_it_is_401_without_auth_and_404_with_it() {
    // Given the service
    let world = world(false);
    // When an unknown path is requested without auth, Then it is 401, so it never reveals which routes exist
    let hidden = world.send("GET", "/nowhere", &[], b"").await;
    assert_eq!((hidden.status, hidden.body.as_str()), (401, UNAUTHORIZED));
    // When it is requested signed, or with the raw token, Then it is 404 not found
    assert_eq!(world.signed("GET", "/nowhere", b"").await.body, NOT_FOUND);
    assert_eq!(world.signed("POST", "/nowhere?x=1", b"").await.status, 404);
    assert_eq!(world.send("GET", "/nowhere", &legacy_headers(), b"").await.body, NOT_FOUND);
    // And a removed family of routes is the same unknown path
    for path in ["/desktop/resize", "/bootstrap-token", "/betterstack/logs", "/trunk/deploy"] {
        assert_eq!(world.send("GET", path, &[], b"").await.status, 401, "{path}");
        assert_eq!(world.signed("GET", path, b"").await.status, 404, "{path}");
    }
}

#[tokio::test]
async fn given_whole_path_routes_when_the_path_differs_in_form_or_method_then_no_route_matches() {
    // Given the service, whose GET /health is a whole-path route
    let world = world(false);
    // When /health is requested with a trailing slash, by HEAD, by OPTIONS or by POST
    // Then no route matches: 401 without auth, 404 signed
    for (method, target) in [("GET", "/health/"), ("HEAD", "/health"), ("OPTIONS", "/health"), ("POST", "/health")] {
        assert_eq!(world.send(method, target, &[], b"").await.status, 401, "{method} {target}");
        assert_eq!(world.signed(method, target, b"").await.status, 404, "{method} {target}");
    }
}

#[tokio::test]
async fn given_a_target_starting_with_two_slashes_when_signed_over_it_then_the_first_segment_is_the_host() {
    // Given the service
    let world = world(false);
    // When //health is requested, Then its first segment is a host and its pathname is /: a signature over / is the only one
    // that holds, and it finds no route
    let over_host_path = signed_headers("GET", "/", b"", NOON, &today());
    let reply = world.send("GET", "//health", &over_host_path, b"").await;
    assert_eq!((reply.status, reply.body.as_str()), (404, NOT_FOUND));
    let over_raw = signed_headers("GET", "//health", b"", NOON, &today());
    assert_eq!(world.send("GET", "//health", &over_raw, b"").await.status, 401);
}

#[tokio::test]
async fn given_a_target_with_dot_segments_or_a_bare_question_mark_when_signed_then_the_normalised_form_is_what_is_signed() {
    // Given the service
    let world = &world(false);
    let send = |target: &'static str, signed_path: &str| {
        let headers = signed_headers("GET", signed_path, b"", NOON, &today());
        async move { world.send("GET", target, &headers, b"").await.status }
    };
    // When /x/../health is signed as /health, Then it passes; signed as sent, it is 401
    assert_eq!(send("/x/../health", "/health").await, 200);
    assert_eq!(send("/x/../health", "/x/../health").await, 401);
    // When /health? is signed without its bare ?, Then it passes; signed with it, it is 401
    assert_eq!(send("/health?", "/health").await, 200);
    assert_eq!(send("/health?", "/health?").await, 401);
    // When /health?a=1 is signed over another query, Then it is 401
    assert_eq!(send("/health?a=1", "/health?a=1").await, 200);
    assert_eq!(send("/health?a=1", "/health?a=2").await, 401);
}

#[tokio::test]
async fn given_a_timestamp_when_it_is_59_or_61_seconds_off_then_59_passes_and_61_does_not() {
    // Given the service at noon
    let world = world(false);
    // When a signed read carries a timestamp that is off by each of these
    // Then 59 s either way passes and 61 s is 401
    for (offset, want) in [(-59_000, 200), (59_000, 200), (-61_000, 401), (61_000, 401)] {
        let headers = signed_headers("GET", "/health", b"", NOON + offset, &today());
        assert_eq!(world.send("GET", "/health", &headers, b"").await.status, want, "a timestamp {offset} ms off");
    }
}

#[tokio::test]
async fn given_a_timestamp_header_in_a_form_number_reads_when_signed_over_that_text_then_it_passes() {
    // Given the service at noon
    let world = world(false);
    // When the timestamp is sent as 1791115200000e0, +1791115200000 or its hex, each signed over exactly that text
    for form in [format!("{NOON}e0"), format!("{NOON}.0"), format!("+{NOON}"), format!("0x{NOON:x}")] {
        let headers = vec![
            ("authorization".to_string(), format!("Bearer {}", derive_rotating_token(SEED, &today()))),
            ("x-signature".to_string(), signature("GET", "/health", &form, b"")),
            ("x-signature-timestamp".to_string(), form.clone()),
        ];
        // Then each passes
        assert_eq!(world.send("GET", "/health", &headers, b"").await.status, 200, "the timestamp header {form:?}");
    }
}

#[tokio::test]
async fn given_a_body_of_exactly_65536_bytes_when_posted_signed_then_it_is_read_and_the_path_is_unknown() {
    // Given the service, and a body at the cap
    let world = world(false);
    let body = vec![b'a'; 65_536];
    // When it is posted signed over its own hash, Then the body was read and let in: 404 for the unknown path
    let headers = signed_headers("POST", "/nowhere", &body, NOON, &today());
    let reply = world.send("POST", "/nowhere", &headers, &body).await;
    assert_eq!((reply.status, reply.body.as_str()), (404, NOT_FOUND));
}

#[tokio::test]
async fn given_a_body_over_65536_bytes_when_posted_then_it_is_413_before_auth_is_looked_at() {
    // Given the service, and a body one byte over the cap
    let world = world(false);
    let body = vec![b'a'; 65_537];
    // When it is posted with no auth at all, and when it is posted signed
    let bare = world.send("POST", "/nowhere", &[], &body).await;
    let headers = signed_headers("POST", "/nowhere", &body, NOON, &today());
    let signed = world.send("POST", "/nowhere", &headers, &body).await;
    // Then both are 413 with the cap in the message, never 401
    for reply in [bare, signed] {
        assert_eq!((reply.status, reply.body.as_str()), (413, r#"{"error":"body exceeds 65536 bytes"}"#));
        assert_eq!(reply.header("content-type"), Some("application/json"));
    }
}

#[tokio::test]
async fn given_a_body_that_is_not_utf8_when_signed_over_its_decoding_then_the_service_hashes_the_decoding() {
    // Given the service, and a body with a byte that is no UTF-8
    let world = world(false);
    let body = b"a\xffb";
    // When it is signed over U+FFFD in that byte's place, as the service decodes it
    let headers = signed_headers("POST", "/nowhere", "a\u{fffd}b".as_bytes(), NOON, &today());
    // Then it is let in: 404 for the unknown path
    assert_eq!(world.send("POST", "/nowhere", &headers, body).await.status, 404);
}

#[tokio::test]
async fn given_a_target_that_is_no_url_when_requested_then_it_is_500_invalid_url_and_the_event_names_the_request() {
    // Given the service
    let world = world(false);
    // When a target that is no URL against http://localhost is requested (// has an empty host)
    let reply = world.send("GET", "//", &[], b"").await;
    // Then it is 500 with Node's message, before any auth, and http.handler_failed has the method, the path and the error
    assert_eq!((reply.status, reply.body.as_str()), (500, r#"{"error":"Invalid URL"}"#));
    let expected = Event::error("http.handler_failed").with("method", "GET").with("path", "//").with("err", "Invalid URL");
    assert_eq!(world.log.events(), [expected]);
}

#[tokio::test]
async fn given_replies_of_every_kind_when_read_then_none_carries_a_cors_header_and_each_says_exactly_application_json() {
    // Given the service, and a browser's origin on each request
    let world = world(false);
    let origin = ("origin".to_string(), "https://example.com".to_string());
    let preflight = vec![origin.clone(), ("access-control-request-method".to_string(), "GET".to_string())];
    let mut signed_origin = signed_headers("GET", "/nowhere", b"", NOON, &today());
    signed_origin.push(origin.clone());
    // When a preflight, a cross-origin read and a signed unknown path are answered
    let replies = [
        ("a preflight without auth", 401, world.send("OPTIONS", "/health", &preflight, b"").await),
        ("a cross-origin read without auth", 401, world.send("GET", "/health", &[origin], b"").await),
        ("a signed unknown path", 404, world.send("GET", "/nowhere", &signed_origin, b"").await),
    ];
    // Then none carries an Access-Control-* header and each is exactly application/json
    for (what, status, reply) in replies {
        assert_eq!(reply.status, status, "{what}");
        assert!(!reply.headers.iter().any(|(key, _)| key.starts_with("access-control-")), "{what}: {:?}", reply.headers);
        assert_eq!(reply.header("content-type"), Some("application/json"), "{what}");
    }
}

#[tokio::test]
async fn given_signed_reads_when_made_then_the_event_log_stays_empty() {
    // Given the service
    let world = world(false);
    // When two signed reads of /health are made
    world.signed("GET", "/health", b"").await;
    world.signed("GET", "/health", b"").await;
    // Then nothing was logged: a request is not an event
    assert_eq!(world.log.events(), []);
}
