//! The request pipeline every route shares (plan section 2, "Request pipeline"): route matching, 401 before 404, the
//! body cap, a handler's failure, the reply headers; and the coverage of the routes table by the suite's cases.
use crate::world;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use vm_ops_api_integration::coverage::{self, Declared};
use vm_ops_api_integration::{auth, binary_tests, case, now_ms, Reply, Service, SEED};

const UNAUTHORIZED: &str = r#"{"error":"unauthorized"}"#;
const NOT_FOUND: &str = r#"{"error":"not found"}"#;
const TOO_BIG: &str = r#"{"error":"body exceeds 65536 bytes"}"#;
/// One leetcode-backend container, as the Engine lists it.
const LEETCODE_BACKEND: &str =
    r#"[{"Id":"c1","Names":["/leetcode-backend"],"State":"running","Status":"Up 2 hours","Image":"ns/leetcode-backend:sha-1"}]"#;
const ENGINE_DOWN: &str = r#"{"message":"engine down"}"#;

fn sent(svc: &Service, method: &str, target: &str, headers: &[(String, String)], body: Option<&[u8]>) -> Reply {
    svc.raw(method, target, headers, body).unwrap_or_else(|e| panic!("{method} {target}: {e}"))
}

fn now() -> String {
    now_ms().to_string()
}

/// Asserts the reply's status and exact body; a reply to HEAD has no body.
fn expect(r: &Reply, method: &str, target: &str, status: u16, body: &str) {
    let body = if method == "HEAD" { "" } else { body };
    assert_eq!((r.status, r.body.as_str()), (status, body), "{method} {target}");
}

/// Without auth the request is 401; signed over `signed_path` (the pathname and search the service derives from
/// `target`) it is 404, because the service names an unknown path only to a caller it lets in.
fn unknown(svc: &Service, method: &str, target: &str, signed_path: &str) {
    let body: Option<&[u8]> = (method == "POST").then_some(b"");
    expect(&sent(svc, method, target, &[], body), method, target, 401, UNAUTHORIZED);
    expect(&sent(svc, method, target, &auth(method, signed_path, &now(), b""), body), method, target, 404, NOT_FOUND);
}

case!(an_unknown_path_is_401_without_auth_and_404_with_it, ["any <anything else>"], {
    // Given the service
    let svc = world().start();
    // When an unknown path is requested without auth and signed, by GET and by POST with a query
    // Then it is 401 unauthorized without auth and 404 not found when signed
    unknown(&svc, "GET", "/nowhere", "/nowhere");
    unknown(&svc, "POST", "/nowhere?x=1", "/nowhere?x=1");
    // When it is requested with the legacy raw token, Then it is 404 too
    let legacy = [("Authorization".to_string(), format!("Bearer {SEED}"))];
    expect(&sent(&svc, "GET", "/nowhere", &legacy, None), "GET", "/nowhere", 404, NOT_FOUND);
});

case!(the_removed_route_families_are_unknown_paths, ["any <anything else>"], {
    // Given the service, which no longer has the trunk deployer, BetterStack proxy, bootstrap token, desktop resize, the
    // dev bridge of metro-runner, the Expo Go tap-through page or the sticky provisioning refs
    let svc = world().start();
    // When each family's path is requested by GET and by POST, Then each is 401, then 404
    let bridge = ["start", "sync", "stop", "status", "logs"].map(|a| format!("/services/metro-runner/dev-bridge/{a}?branch=dev"));
    let paths = ["/desktop/resize", "/bootstrap-token", "/betterstack/logs", "/trunk/deploy", "/expo-go?url=exp%3A%2F%2Fhost%3A1", "/provision-refs"]
        .map(str::to_string)
        .into_iter()
        .chain(bridge);
    for path in paths {
        for method in ["GET", "POST"] {
            unknown(&svc, method, &path, &path);
        }
    }
    // And the service was up for all of it: a signed read of /health answers
    assert_eq!(svc.signed("GET", "/health", None).status, 200);
});

case!(whole_path_routes_match_the_exact_pathname_and_methods_are_not_mapped, ["any <anything else>"], {
    // Given the service, whose GET /health is a whole-path route
    let svc = world().start();
    // When /health is requested with a trailing slash, Then it is an unknown path
    unknown(&svc, "GET", "/health/", "/health/");
    // When it is requested as //health, Then health is the host and the pathname is /: unknown, and only a signature
    // over / is one the service accepts
    unknown(&svc, "GET", "//health", "/");
    expect(&sent(&svc, "GET", "//health", &auth("GET", "//health", &now(), b""), None), "GET", "//health", 401, UNAUTHORIZED);
    // When /health is requested by HEAD, OPTIONS or POST, Then no route matches: 401, then 404
    unknown(&svc, "HEAD", "/health", "/health");
    unknown(&svc, "OPTIONS", "/health", "/health");
    unknown(&svc, "POST", "/health", "/health");
});

case!(segment_routes_take_doubled_slashes_and_never_decode_a_segment, ["any <anything else>"], {
    // Given an Engine that lists one leetcode-backend container
    let mut w = world();
    w.engine(200, LEETCODE_BACKEND);
    let svc = w.start();
    // When ps is read with doubled slashes between its segments, Then the segment route matches: 200 for the app
    let doubled = "/services//leetcode-backend//ps";
    let r = sent(&svc, "GET", doubled, &auth("GET", doubled, &now(), b""), None);
    assert_eq!((r.status, &r.json()["app"]), (200, &json!("leetcode-backend")), "{}", r.body);
    // When the target starts with //, Then services is the host and the parts are leetcode-backend and ps: no route
    unknown(&svc, "GET", "//services/leetcode-backend/ps", "/leetcode-backend/ps");
    // When the app segment is percent-encoded, Then it is not decoded: 404 unknown app
    let encoded = "/services/leetcode%2Dbackend/ps";
    let r = sent(&svc, "GET", encoded, &auth("GET", encoded, &now(), b""), None);
    assert_eq!((r.status, &r.json()["error"]), (404, &json!("unknown app")), "{}", r.body);
    // And only the doubled-slash read reached the Engine, with one list request
    let list = json!({ "method": "GET", "target": "/containers/json?all=true", "headers": {}, "body": "" });
    assert_eq!(svc.world().engine_requests(), [list]);
});

case!(a_body_over_65536_bytes_is_refused_before_auth, {
    // Given the service
    let svc = world().start();
    // When a signed POST carries exactly 65,536 bytes, Then the body is read and the request let in: 404 for its path
    let at_cap = vec![b'a'; 65_536];
    let r = sent(&svc, "POST", "/nowhere", &auth("POST", "/nowhere", &now(), &at_cap), Some(&at_cap));
    expect(&r, "POST", "/nowhere", 404, NOT_FOUND);
    // When a POST carries 65,537 bytes, signed or without auth
    // Then it is answered 413; never 401, and never a hang
    let over = vec![b'a'; 65_537];
    for (who, headers) in [("signed", auth("POST", "/nowhere", &now(), &over)), ("without auth", Vec::new())] {
        let outcome = svc.raw("POST", "/nowhere", &headers, Some(&over));
        let is_413 = matches!(&outcome, Ok(r) if (r.status, r.body.as_str()) == (413, TOO_BIG));
        assert!(is_413, "{who}, 65,537 bytes: {outcome:?}");
    }
    // And the service still answers
    assert_eq!(svc.signed("GET", "/health", None).status, 200);
});

case!(a_handler_that_fails_answers_502_and_logs_http_handler_failed, {
    // Given an Engine that answers the container list with 500
    let mut w = world();
    w.engine(500, ENGINE_DOWN);
    let svc = w.start();
    // When ps is read, signed
    let r = svc.signed("GET", "/services/leetcode-backend/ps", None);
    // Then it is 502 request failed with the Engine's answer as the detail, and stdout has the http.handler_failed event
    let detail = format!("docker API returned 500: {ENGINE_DOWN}");
    assert_eq!((r.status, r.body.as_str()), (502, json!({ "error": "request failed", "detail": detail }).to_string().as_str()));
    let event = json!({
        "level": "error",
        "message": "http.handler_failed",
        "method": "GET",
        "path": "/services/leetcode-backend/ps",
        "err": detail,
    });
    svc.wait_line(&event.to_string(), Duration::from_secs(5));
});

case!(no_reply_carries_an_access_control_header_and_json_is_exactly_application_json, {
    // Given the service, with an Engine that answers the container list with 500
    let mut w = world();
    w.engine(500, ENGINE_DOWN);
    let svc = w.start();
    let origin = ("Origin".to_string(), "https://example.com".to_string());
    let preflight =
        [origin.clone(), ("Access-Control-Request-Method".into(), "GET".into()), ("Access-Control-Request-Headers".into(), "authorization".into())];
    let with = |method: &str, path: &str, extra: &[(String, String)]| {
        let mut h = auth(method, path, &now(), b"");
        h.extend(extra.iter().cloned());
        h
    };
    // When a browser's preflight, a cross-origin read and a signed read reach each kind of reply
    let replies = [
        ("a preflight without auth", 401, sent(&svc, "OPTIONS", "/health", &preflight, None)),
        ("a signed preflight", 404, sent(&svc, "OPTIONS", "/health", &with("OPTIONS", "/health", &preflight), None)),
        ("a cross-origin read without auth", 401, sent(&svc, "GET", "/health", &[origin.clone()], None)),
        ("a signed cross-origin read", 200, sent(&svc, "GET", "/health", &with("GET", "/health", &[origin.clone()]), None)),
        ("an unknown path", 404, sent(&svc, "GET", "/nowhere", &with("GET", "/nowhere", &[origin.clone()]), None)),
        ("an unknown app", 404, sent(&svc, "GET", "/services/x/ps", &with("GET", "/services/x/ps", &[origin.clone()]), None)),
        ("a failing handler", 502, sent(&svc, "GET", "/services/agent-shell/ps", &with("GET", "/services/agent-shell/ps", &[origin]), None)),
    ];
    // Then none carries an Access-Control-* header, each says exactly application/json, and each has a content-length
    for (what, status, r) in &replies {
        assert_eq!(r.status, *status, "{what}: {}", r.body);
        let cors: Vec<&String> = r.headers.iter().map(|(k, _)| k).filter(|k| k.starts_with("access-control-")).collect();
        assert!(cors.is_empty(), "{what} carries {cors:?}");
        assert_eq!(r.header("content-type"), Some("application/json"), "{what}");
        assert!(r.header("content-length").is_some(), "{what} has no content-length");
    }
    // And the reply wf's own client reads says the same
    let r = svc.signed("GET", "/health", None);
    assert_eq!((r.status, r.header("content-type")), (200, Some("application/json")));
    assert!(!r.headers.iter().any(|(k, _)| k.starts_with("access-control-")), "{:?}", r.headers);
});

case!(every_route_id_has_a_case, {
    // Given the route ids of plan section 2, the route lists of this suite's cases and its tests
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let declared = coverage::declared_in(&dir.join("tests/integration"));
    // When they are compared
    let problems = coverage::problems(&coverage::ROUTE_IDS, &declared, &binary_tests());
    // Then every id has a case, and the list of ids no case checked yet is gone for good
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(!dir.join("uncovered.txt").exists(), "uncovered.txt is back, but every route id must have a case");
});

case!(the_coverage_check_names_each_gap_and_each_stale_name, {
    // Given three route ids
    let ids = ["GET /a", "GET /b", "any <c>"];
    let declared = |case: &str, routes: &[&str]| Declared { case: case.into(), routes: routes.iter().map(|r| r.to_string()).collect() };
    // When a case checks the first and another the other two, Then there is nothing to report
    let all = [declared("m::checks_a", &["GET /a"]), declared("m::checks_rest", &["GET /b", "any <c>"])];
    let tests = ["m::checks_a".to_string(), "m::checks_rest".to_string()];
    assert_eq!(coverage::problems(&ids, &all, &tests), Vec::<String>::new());
    // When the only case for the second is no test of the binary, a case names an unknown id, and no case checks the third
    let wrong = [declared("m::checks_a", &["GET /a"]), declared("m::gone", &["GET /b"]), declared("m::checks_a", &["GET /typo"])];
    let problems = coverage::problems(&ids, &wrong, &tests[..1]);
    // Then each is named
    assert_eq!(
        problems,
        [
            "m::gone names routes but is not a test of this binary",
            r#"m::checks_a names "GET /typo", which is no route id"#,
            r#""GET /b" has no case"#,
            r#""any <c>" has no case"#,
        ]
    );
});
