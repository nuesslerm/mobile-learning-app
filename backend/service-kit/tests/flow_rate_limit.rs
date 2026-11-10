//! Flow test of the rate limit layer: the real router and layer driven in-process, the caller's address supplied the way the
//! server's connect info would. Given/When/Then.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use service_kit::rate_limit;
use std::net::SocketAddr;
use tower::ServiceExt;

fn request(from: &str) -> Request<Body> {
    let mut request = Request::builder().uri("/health").body(Body::empty()).unwrap();
    request.extensions_mut().insert(ConnectInfo(from.parse::<SocketAddr>().unwrap()));
    request
}

#[tokio::test]
async fn given_a_limit_of_three_when_a_caller_makes_four_requests_then_the_fourth_is_refused_with_retry_after() {
    let app = Router::new().route("/health", get(|| async { "ok" })).layer(rate_limit::per_minute(3));

    let mut statuses = Vec::new();
    let mut last = None;
    for _ in 0..4 {
        let response = app.clone().oneshot(request("10.0.0.1:5000")).await.unwrap();
        statuses.push(response.status());
        last = Some(response);
    }

    assert_eq!(statuses, [StatusCode::OK, StatusCode::OK, StatusCode::OK, StatusCode::TOO_MANY_REQUESTS]);
    assert!(last.unwrap().headers().contains_key("retry-after"));
}

#[tokio::test]
async fn given_one_caller_over_its_limit_when_another_calls_then_the_other_is_served() {
    let app = Router::new().route("/health", get(|| async { "ok" })).layer(rate_limit::per_minute(1));

    app.clone().oneshot(request("10.0.0.1:5000")).await.unwrap();
    let blocked = app.clone().oneshot(request("10.0.0.1:5000")).await.unwrap();
    let other = app.clone().oneshot(request("10.0.0.2:5000")).await.unwrap();

    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(other.status(), StatusCode::OK);
}
