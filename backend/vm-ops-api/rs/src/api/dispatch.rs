//! Node's request pipeline, in Node's order: parse the target, find the route, read the body (at most 64 KiB, before auth), check
//! the credentials, run the handler. An unknown path is 401 before 404, so it never reveals which routes exist; a route the
//! listener's port does not serve is an unknown path there.
use crate::api::reply::Reply;
use crate::api::routes::{self, Call, Failure, RouteId};
use crate::api::AppState;
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::response::Response;
use http_body_util::{BodyExt, LengthLimitError, Limited};
use serde_json::json;
use std::time::Duration;
use vm_ops_api_core::application::ports::{Event, EventLog};
use vm_ops_api_core::domain::auth::{Credentials, RequestLine};
use vm_ops_api_core::domain::exposure::Port;
use vm_ops_api_core::domain::request::Target;

const MAX_BODY_BYTES: usize = 64 * 1024;

pub async fn dispatch(State(state): State<AppState>, request: Request) -> Response {
    let (head, body) = request.into_parts();
    let reply = answer(&state, head.method.as_str(), &head.uri.to_string(), &head.headers, body).await;
    reply.into_response()
}

async fn answer(state: &AppState, method: &str, raw_target: &str, headers: &HeaderMap, body: axum::body::Body) -> Reply {
    let target = match Target::parse(raw_target) {
        Ok(target) => target,
        Err(invalid) => {
            record_failure(state.log.as_ref(), method, raw_target, &invalid.to_string());
            return Reply::json(500, &json!({ "error": invalid.to_string() }));
        }
    };
    let (route, off_port) = on_port(routes::resolve(method, &target), state.lane.port);
    if let Some(route) = route.filter(|route| route.is_public()) {
        return match routes::handle(route, &Call { target: &target, body: &[] }, state).await {
            Ok(reply) => reply,
            Err(failure) => {
                record_failure(state.log.as_ref(), method, raw_target, &failure.0);
                Reply::json(500, &json!({ "error": failure.0 }))
            }
        };
    }
    let body = match read_body(body, state.body_timeout).await {
        Ok(bytes) => bytes,
        Err(Refused::TooSlow) => return Reply::json(408, &json!({ "error": "request timeout" })).closing(),
        Err(Refused::Unreadable(message)) => return Reply::json(413, &json!({ "error": message })),
    };
    let (authorization, signature, timestamp) = (first_text(headers, "authorization"), joined_text(headers, "x-signature"), joined_text(headers, "x-signature-timestamp"));
    let credentials = Credentials { authorization: authorization.as_deref(), signature: signature.as_deref(), timestamp: timestamp.as_deref() };
    let path_and_query = target.path_and_query();
    let request = RequestLine { method, path_and_query: &path_and_query };
    let allowed = if route.is_some_and(|route| route.is_signed_only()) { state.lane.gate.allows_signed(&credentials, &request, &body) } else { state.lane.gate.allows(&credentials, &request, &body) };
    if !allowed {
        return Reply::json(401, &json!({ "error": "unauthorized" }));
    }
    let Some(route) = route else {
        if off_port {
            state.log.emit(Event::warn("http.route_not_on_port").with("port", state.lane.port.name()).with("method", method).with("path", target.pathname.as_str()));
        }
        return Reply::json(404, &json!({ "error": "not found" }));
    };
    match routes::handle(route, &Call { target: &target, body: &body }, state).await {
        Ok(reply) => reply,
        Err(failure) => failed(state.log.as_ref(), method, raw_target, failure),
    }
}

/// The route this port serves, and whether the request named one it does not.
fn on_port(route: Option<RouteId>, port: Port) -> (Option<RouteId>, bool) {
    match route {
        Some(route) if !route.exposure().serves(port) => (None, true),
        route => (route, false),
    }
}

/// The 502 of a handler that threw, and the event that says why.
fn failed(log: &dyn EventLog, method: &str, raw_target: &str, failure: Failure) -> Reply {
    record_failure(log, method, raw_target, &failure.0);
    Reply::json(502, &json!({ "error": "request failed", "detail": failure.0 }))
}

fn record_failure(log: &dyn EventLog, method: &str, raw_target: &str, error: &str) {
    log.emit(Event::error("http.handler_failed").with("method", method).with("path", raw_target).with("err", error));
}

/// Why a body was not taken.
#[derive(Debug, PartialEq, Eq)]
enum Refused {
    /// It did not arrive within the limit: Node answers 408 and drops the connection.
    TooSlow,
    /// The message of the 413: the cap, or whatever ended the read.
    Unreadable(String),
}

async fn read_body(body: axum::body::Body, within: Duration) -> Result<Vec<u8>, Refused> {
    match tokio::time::timeout(within, Limited::new(body, MAX_BODY_BYTES).collect()).await {
        Err(_elapsed) => Err(Refused::TooSlow),
        Ok(Ok(collected)) => Ok(collected.to_bytes().to_vec()),
        Ok(Err(error)) if error.downcast_ref::<LengthLimitError>().is_some() => Err(Refused::Unreadable(format!("body exceeds {MAX_BODY_BYTES} bytes"))),
        Ok(Err(error)) => Err(Refused::Unreadable(error.to_string())),
    }
}

/// A header's bytes read as Latin-1, as Node's HTTP layer reads them.
fn latin1(value: &axum::http::HeaderValue) -> String {
    value.as_bytes().iter().map(|byte| char::from(*byte)).collect()
}

/// The first value, as Node keeps it for a header it does not join (`authorization`).
fn first_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).map(latin1)
}

/// Every value joined with `, `, as Node joins a repeated header.
fn joined_text(headers: &HeaderMap, name: &str) -> Option<String> {
    let values: Vec<String> = headers.get_all(name).iter().map(latin1).collect();
    (!values.is_empty()).then(|| values.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use vm_ops_api_core::application::ports::Level;
    use vm_ops_api_core::testing::FakeEventLog;

    #[test]
    fn given_a_handler_that_failed_when_answered_then_it_is_a_502_with_the_detail_and_the_event_names_the_request() {
        // Given a failure with a message and an event log
        let log = FakeEventLog::default();
        // When the dispatcher answers it for GET /services/x/ps
        let reply = failed(&log, "GET", "/services/x/ps", Failure("docker API returned 500: boom".to_string()));
        // Then the reply is 502 {error, detail} and the event is http.handler_failed with method, path and err in that order
        assert_eq!((reply.status.as_u16(), String::from_utf8_lossy(&reply.body).to_string()), (502, r#"{"error":"request failed","detail":"docker API returned 500: boom"}"#.to_string()));
        let expected = Event::error("http.handler_failed").with("method", "GET").with("path", "/services/x/ps").with("err", "docker API returned 500: boom");
        assert_eq!(log.events(), [expected]);
        assert_eq!(log.events()[0].level, Level::Error);
    }

    #[test]
    fn given_a_route_when_it_is_checked_against_a_port_then_the_ci_port_drops_an_admin_route_and_says_so() {
        assert_eq!(on_port(Some(RouteId::ExecScriptStart), Port::Ci), (None, true));
        assert_eq!(on_port(Some(RouteId::Health), Port::Ci), (Some(RouteId::Health), false));
        assert_eq!(on_port(Some(RouteId::ExecScriptStart), Port::Admin), (Some(RouteId::ExecScriptStart), false));
        assert_eq!(on_port(None, Port::Ci), (None, false));
    }

    #[test]
    fn given_repeated_headers_when_read_then_authorization_keeps_the_first_and_the_signature_headers_are_joined() {
        let mut headers = HeaderMap::new();
        for value in ["Bearer a", "Bearer b"] {
            headers.append("authorization", HeaderValue::from_static(value));
        }
        for value in ["one", "two"] {
            headers.append("x-signature", HeaderValue::from_static(value));
        }
        assert_eq!(first_text(&headers, "authorization"), Some("Bearer a".to_string()));
        assert_eq!(joined_text(&headers, "x-signature"), Some("one, two".to_string()));
        assert_eq!(joined_text(&headers, "x-signature-timestamp"), None);
    }

    #[test]
    fn given_a_header_with_bytes_above_ascii_when_read_then_each_byte_is_one_latin1_character() {
        let mut headers = HeaderMap::new();
        headers.insert("x-signature", HeaderValue::from_bytes(b"ab\xe9").expect("obs-text is a header value"));
        assert_eq!(joined_text(&headers, "x-signature"), Some("ab\u{e9}".to_string()));
    }
}
