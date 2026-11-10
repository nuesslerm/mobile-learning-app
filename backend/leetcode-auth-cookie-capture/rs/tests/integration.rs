//! The built capture listener as a black box: its binary on a free port, the backend and the notifier it would call a loopback port
//! nothing listens on, no browser. Nothing real is reachable; what it answers on its one route is its contract.
use service_kit::blackbox::Running;

const SECRET: &str = "integration-trigger-secret";

async fn start() -> Running {
    let env = [
        ("LEETCODE_BACKEND_BASE_URL", "http://127.0.0.1:9".to_string()),
        ("LEETCODE_BACKEND_ADMIN_SECRET", "integration-admin-secret".to_string()),
        ("TELEGRAM_NOTIFY_BASE_URL", "http://127.0.0.1:9".to_string()),
        ("TELEGRAM_NOTIFY_SECRET", "integration-notify-secret".to_string()),
        ("LEETCODE_CAPTURE_TRIGGER_SECRET", SECRET.to_string()),
        // the vault is not reached: a variable the service still lacks fails at once instead of being fetched
        ("WF_BIN", "/bin/false".to_string()),
        ("CAPTURE_MODE", "remote".to_string()),
    ];
    Running::start(env!("CARGO_BIN_EXE_leetcode-auth-cookie-capture-rs"), "CAPTURE_LISTENER_PORT", &env, "/").await
}

async fn trigger(service: &Running, secret: Option<&str>) -> (u16, serde_json::Value) {
    let mut request = service.client.post(format!("{}/trigger", service.base));
    if let Some(secret) = secret {
        request = request.header("x-capture-trigger-secret", secret);
    }
    let response = request.send().await.unwrap();
    (response.status().as_u16(), response.json().await.unwrap_or(serde_json::Value::Null))
}

#[tokio::test]
async fn given_the_listener_when_a_trigger_has_no_secret_or_a_wrong_one_then_it_is_refused_and_nothing_is_accepted() {
    // Given the listener
    let service = start().await;
    // When a trigger arrives with no secret and with a wrong one
    for secret in [None, Some("not-the-secret"), Some("")] {
        let (status, body) = trigger(&service, secret).await;
        // Then it is 401 unauthorized and not accepted
        assert_eq!((status, body["error"].as_str(), body.get("accepted")), (401, Some("unauthorized"), None), "secret={secret:?}: {body}\n{}", service.logs());
    }
}

#[tokio::test]
async fn given_the_listener_when_the_right_secret_triggers_a_capture_then_it_is_accepted_and_the_listener_keeps_answering() {
    let service = start().await;
    // When the trigger comes with the right secret
    let (status, body) = trigger(&service, Some(SECRET)).await;
    // Then it is accepted (a capture is attempted in the background, which fails here: there is no browser or backend), and the
    // listener still answers afterwards
    assert!((status == 202 && body["accepted"] == true) || (status == 200 && body["alreadyRunning"] == true), "{status} {body}");
    let (after, _) = trigger(&service, None).await;
    assert_eq!(after, 401, "{}", service.logs());
}

#[tokio::test]
async fn given_the_listener_when_anything_but_a_post_to_the_trigger_is_sent_then_it_is_not_found() {
    let service = start().await;
    // When the trigger is read, another path is asked and a write goes to another path
    let read = service.client.get(format!("{}/trigger", service.base)).send().await.unwrap().status().as_u16();
    let other = service.client.get(format!("{}/health", service.base)).send().await.unwrap().status().as_u16();
    let write = service.client.post(format!("{}/other", service.base)).send().await.unwrap().status().as_u16();
    // Then each is a 404: the listener serves one route
    assert_eq!((read, other, write), (404, 404, 404));
}

#[tokio::test]
async fn given_the_listener_when_triggers_come_faster_than_its_limit_then_the_excess_is_refused_with_a_429() {
    let service = start().await;
    // When more than the per-minute limit of triggers (30) come in a row with a wrong secret
    let mut statuses = Vec::new();
    for _ in 0..40 {
        statuses.push(trigger(&service, Some("not-the-secret")).await.0);
    }
    // Then the first are 401 and the excess is 429
    assert!(statuses[..30].iter().all(|s| *s == 401), "{statuses:?}");
    assert!(statuses[30..].contains(&429), "{statuses:?}");
}
