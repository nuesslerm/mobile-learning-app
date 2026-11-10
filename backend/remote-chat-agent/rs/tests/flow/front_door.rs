use crate::harness::{call, get, post, service, Env, Options, ALLOWED_ORIGIN};
use axum::http::{Method, StatusCode};
use rca_service_core::application::startup;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_any_caller_when_it_reaches_the_front_door_then_health_origins_bodies_the_code_assist_mock_and_the_rate_limit_are_enforced() {
    // Given the service as main starts it: an AGENT_CLI it does not serve never gets that far.
    assert_eq!(startup::check(None), Ok("opencode"));
    assert_eq!(startup::check(Some(" opencode ")), Ok("opencode"));
    let typo = startup::check(Some("cluade")).unwrap_err();
    assert!(typo.starts_with("AGENT_CLI=\"cluade\" is not a supported agent CLI. Supported: opencode, claude, codex. Available in this image: opencode."), "{typo}");
    for cli in ["claude", "codex"] {
        let refused = startup::check(Some(cli)).unwrap_err();
        assert!(refused.starts_with(&format!("AGENT_CLI={cli} was selected, but this build does not serve that backend")) && refused.contains("Available: opencode."), "{refused}");
    }
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());

    // When health is asked without credentials, then it is ok with the protocol version and the commit.
    let health = svc.send(get("/health")).await;
    assert_eq!(health.status, StatusCode::OK);
    assert_eq!(health.json(), json!({ "status": "ok", "wsProtocolVersion": 1, "commit": "flow-sha" }));
    assert_eq!(health.header("access-control-allow-origin"), None);

    // When requests carry an origin, then a listed one (written with a trailing slash in the environment), localhost on any port,
    // and 127.0.0.1 are echoed and varied on, and lookalikes and the retired Metro host get no allow headers.
    for origin in [ALLOWED_ORIGIN, "http://localhost:8081", "http://127.0.0.1"] {
        let answer = svc.send(get("/health").header("origin", origin)).await;
        assert_eq!(answer.header("access-control-allow-origin"), Some(origin));
        assert!(answer.header("vary").unwrap_or_default().to_lowercase().contains("origin"), "{origin}");
    }
    for origin in ["https://evil.example", "https://allowed.example.evil.example", "http://metro-runner-main:20000"] {
        let answer = svc.send(get("/health").header("origin", origin)).await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!((answer.header("access-control-allow-origin"), answer.header("access-control-allow-methods")), (None, None), "{origin}");
    }

    // When a preflight arrives for any path, then the layer answers it with the methods, headers and max age, or without an origin.
    let preflight = svc.send(call(Method::OPTIONS, "/api/convert/anything").header("origin", "http://localhost:19006").header("access-control-request-method", "POST")).await;
    assert_eq!((preflight.status, preflight.text()), (StatusCode::OK, String::new()));
    assert_eq!(preflight.header("access-control-allow-origin"), Some("http://localhost:19006"));
    assert_eq!(preflight.header("access-control-allow-methods"), Some("GET,POST,PUT,PATCH,DELETE,OPTIONS"));
    assert_eq!(preflight.header("access-control-allow-headers"), Some("content-type,authorization,x-bot-secret,accept"));
    assert_eq!(preflight.header("access-control-max-age"), Some("86400"));
    let foreign = svc.send(call(Method::OPTIONS, "/health").header("origin", "https://evil.example").header("access-control-request-method", "GET")).await;
    assert_eq!((foreign.status, foreign.header("access-control-allow-origin")), (StatusCode::OK, None));

    // When a path does not exist or a body does not parse, then it is a JSON 404 or a 400 before any route looks at it.
    let missing = svc.send(get("/api/nope")).await;
    assert_eq!((missing.status, missing.json()), (StatusCode::NOT_FOUND, json!({ "error": "not_found" })));
    // Given the retired chat shim, when it is posted to with the secret, then it is that same 404 and no opencode session was started.
    let shim = svc.send(post("/api/chat").secret().json(json!({ "prompt": "hi" }))).await;
    assert_eq!((shim.status, shim.json()), (StatusCode::NOT_FOUND, json!({ "error": "not_found" })));
    assert!(svc.opencode.requests().is_empty());
    let malformed = svc.send(post("/runs").secret().raw_json("{\"repo\": ")).await;
    assert_eq!((malformed.status, malformed.json()), (StatusCode::BAD_REQUEST, json!({ "error": "invalid_json" })));

    // When a body is over 30 megabytes, then 413; one well under it is read (and refused as a run request, not as too large).
    let huge = svc.send(post("/runs").secret().json(json!({ "repo": "x".repeat(31 * 1024 * 1024) }))).await;
    assert_eq!(huge.status, StatusCode::PAYLOAD_TOO_LARGE);
    let large = svc.send(post("/runs").secret().json(json!({ "repo": "x".repeat(2 * 1024 * 1024) }))).await;
    assert_eq!(large.status, StatusCode::BAD_REQUEST);

    // When the retired code-assist mock is asked for, Then the route is gone
    assert_eq!(svc.send(post("/api/code-assist").json(json!({ "transcript": "solve" }))).await.status, StatusCode::NOT_FOUND);

    // When one caller sends more requests in a minute than the limit (3 here: it refills one every 20 s, so the refusal does not
    // depend on how fast this host sends), then the next is a 429 with a Retry-After; a preflight is never counted, and the
    // caller is the first address a loopback proxy forwards that is not loopback itself; a peer that is not loopback is the
    // caller whatever it forwards.
    let svc = service(&env, Options { requests_per_minute: 3, ..Options::default() });
    let from = |ip: &str| get("/health").header("x-forwarded-for", ip);
    for _ in 0..130 {
        svc.send(call(Method::OPTIONS, "/health").header("x-forwarded-for", "203.0.113.9").header("origin", "http://localhost").header("access-control-request-method", "GET")).await;
    }
    for i in 0..3 {
        assert_eq!(svc.send(from("203.0.113.9")).await.status, StatusCode::OK, "request {i}");
    }
    let blocked = svc.send(from("203.0.113.9")).await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    let retry: u64 = blocked.header("retry-after").expect("a Retry-After").parse().expect("seconds");
    assert!(retry <= 60);
    assert_eq!(svc.send(from("203.0.113.8")).await.status, StatusCode::OK);
    assert_eq!(svc.send(get("/health").header("x-forwarded-for", "198.51.100.1, 203.0.113.9")).await.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(svc.send(get("/health").header("x-forwarded-for", "203.0.113.9, 127.0.0.1")).await.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(svc.send(get("/health").from([192, 0, 2, 9]).header("x-forwarded-for", "203.0.113.9")).await.status, StatusCode::OK);
}
