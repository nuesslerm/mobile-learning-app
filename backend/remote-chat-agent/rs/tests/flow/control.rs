use crate::harness::{post, service, Env, Options};
use axum::http::StatusCode;
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_sessions_the_service_never_ran_when_they_are_stopped_queued_steered_or_answered_then_opencode_is_told_directly_and_an_unreachable_one_is_an_error() {
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    let wire = || -> Vec<(String, String, Option<String>, Option<Value>)> { oc.calls().into_iter().map(|r| (r.method.clone(), r.path.clone(), r.directory().map(str::to_string), r.body)).collect() };

    // Given bodies that miss a field, when they are posted, then 400 says which and opencode is not called.
    let missing = [
        ("stop", json!({}), "Missing sessionId or directory"),
        ("queue", json!({ "sessionId": "s", "directory": "/x" }), "Missing sessionId, directory, or message"),
        ("queue", json!({ "sessionId": "s", "message": "m" }), "Missing sessionId, directory, or message"),
        ("steer", json!({ "directory": "/x", "message": "m" }), "Missing sessionId, directory, or message"),
        ("steer", json!({ "sessionId": "s", "directory": "/x", "message": "" }), "Missing sessionId, directory, or message"),
    ];
    for (action, body, error) in missing {
        let refused = svc.send(post(&format!("/api/control/{action}")).json(body)).await;
        assert_eq!((refused.status, refused.json()), (StatusCode::BAD_REQUEST, json!({ "error": error })));
    }
    for (body, error) in [(json!({ "answers": [] }), "Missing directory"), (json!({ "directory": "/x" }), "answers must be an array"), (json!({ "directory": "/x", "answers": "yes" }), "answers must be an array")] {
        let refused = svc.send(post("/api/question/q-1/reply").json(body)).await;
        assert_eq!((refused.status, refused.json()), (StatusCode::BAD_REQUEST, json!({ "error": error })));
    }
    assert!(wire().is_empty());

    // When a session the service never ran is stopped, queued or steered, then opencode is told to abort it, or gets the message as
    // a deferred or an immediate prompt, in the directory given.
    let stopped = svc.send(post("/api/control/stop").json(json!({ "sessionId": "oc-known", "directory": "/x" }))).await;
    let queued = svc.send(post("/api/control/queue").json(json!({ "sessionId": "oc-known", "directory": "/x", "message": "later" }))).await;
    let steered = svc.send(post("/api/control/steer").json(json!({ "sessionId": "oc-known", "directory": "/x", "message": "now" }))).await;
    assert_eq!(stopped.json(), json!({ "ok": true, "action": "stop", "sessionId": "oc-known" }));
    assert_eq!(queued.json(), json!({ "ok": true, "action": "queue", "sessionId": "oc-known", "message": "later" }));
    assert_eq!(steered.json(), json!({ "ok": true, "action": "steer", "sessionId": "oc-known", "message": "now" }));
    assert_eq!(
        wire(),
        [
            ("POST".into(), "/session/oc-known/abort".into(), Some("/x".into()), None),
            ("POST".into(), "/api/session/oc-known/prompt".into(), Some("/x".into()), Some(json!({ "prompt": { "text": "later" }, "delivery": "deferred" }))),
            ("POST".into(), "/api/session/oc-known/prompt".into(), Some("/x".into()), Some(json!({ "prompt": { "text": "now" }, "delivery": "immediate" }))),
        ]
    );

    // When a question the service did not see is answered, then opencode gets the answers for that request and the route says 204.
    oc.forget_requests();
    let answered = svc.send(post("/api/question/q-unknown/reply").json(json!({ "directory": "/elsewhere", "answers": [["A"], ["because"]] }))).await;
    assert_eq!((answered.status, answered.text()), (StatusCode::NO_CONTENT, String::new()));
    assert_eq!(wire(), [("POST".into(), "/question/q-unknown/reply".into(), Some("/elsewhere".into()), Some(json!({ "answers": [["A"], ["because"]] })))]);

    // When nothing listens where opencode should be, then stop, queue and steer answer 500 and a reply 502, each a JSON error.
    oc.go_down();
    let down = [
        (svc.send(post("/api/control/stop").json(json!({ "sessionId": "s", "directory": "/x" }))).await, StatusCode::INTERNAL_SERVER_ERROR),
        (svc.send(post("/api/control/queue").json(json!({ "sessionId": "s", "directory": "/x", "message": "m" }))).await, StatusCode::INTERNAL_SERVER_ERROR),
        (svc.send(post("/api/control/steer").json(json!({ "sessionId": "s", "directory": "/x", "message": "m" }))).await, StatusCode::INTERNAL_SERVER_ERROR),
        (svc.send(post("/api/question/q/reply").json(json!({ "directory": "/x", "answers": [["a"]] }))).await, StatusCode::BAD_GATEWAY),
    ];
    for (answer, status) in down {
        assert_eq!(answer.status, status);
        assert!(answer.json()["error"].is_string());
    }
}
