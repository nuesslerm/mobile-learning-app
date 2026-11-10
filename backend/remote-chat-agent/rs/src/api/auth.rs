//! Who the front door lets in. Two kinds of credential, never interchangeable: the one service secret (`CHAT_AGENT_API_SECRET`), which the
//! machine routes, the executor channel and the device-bearer admin endpoints take, and a device bearer (`rca_agent_core::DeviceBearers`), which
//! opens the agent API socket and nothing else. The checks differ in what a missing secret means and in how the caller presents it. The decisions
//! are plain functions of the credential and the request, so a test feeds both.

use super::app::App;
use super::routes::{respond, Req};
use axum::response::Response;
use rca_agent_core::{bearer_token, Bearer, DeviceBearers};
use serde_json::json;

/// `X-Bot-Secret` against the secret: the check the worktree and source-control routes share. No secret configured and none sent lets the caller in.
pub fn authorised(app: &App, req: &Req) -> bool {
    bot_secret_or_open(app.config.api_secret.as_deref(), req)
}

/// The check `/runs` makes: no secret configured means nobody is let in, where `authorised` lets everybody in.
pub fn authorised_strict(app: &App, req: &Req) -> bool {
    bot_secret(app.config.api_secret.as_deref(), req)
}

/// The executor channel, the MCP-free machine routes and the agent API: the service secret as `X-Bot-Secret` or as `Authorization: Bearer`.
pub fn channel_authorised(app: &App, req: &Req) -> bool {
    channel(app.config.api_secret.as_deref(), req)
}

/// The device bearer admin endpoints (mint, list, revoke): the service secret, as the executor channel takes it, and no device bearer. A bearer
/// that could mint another would make one stolen device the key to all of them.
pub fn admin_authorised(app: &App, req: &Req) -> bool {
    channel(app.config.api_secret.as_deref(), req)
}

/// The agent API socket: the device bearer the `Authorization: Bearer` token belongs to. The service secret is no device bearer, so it opens
/// nothing here: a mapper holds its own bearer, never the secret.
pub fn agent_socket_bearer(bearers: &dyn DeviceBearers, req: &Req) -> Option<Bearer> {
    bearers.verify(bearer_token(req.header("authorization"))?)
}

/// The thread view's WebSocket: the secret as the `token` query parameter, when one is configured.
pub fn socket_authorised(secret: Option<&str>, req: &Req) -> bool {
    secret.is_none_or(|secret| req.query_param("token").as_deref() == Some(secret))
}

pub fn unauthorized() -> Response {
    respond(401, &json!({ "error": "Unauthorized", "code": "UNAUTHORIZED" }))
}

fn bot_secret_or_open(secret: Option<&str>, req: &Req) -> bool {
    match (secret, req.header("x-bot-secret")) {
        (Some(secret), Some(given)) => secret == given,
        (None, None) => true,
        _ => false,
    }
}

fn bot_secret(secret: Option<&str>, req: &Req) -> bool {
    secret.is_some_and(|secret| req.header("x-bot-secret") == Some(secret))
}

fn channel(secret: Option<&str>, req: &Req) -> bool {
    let bearer = req.header("authorization").and_then(|v| v.strip_prefix("Bearer "));
    bot_secret(secret, req) || secret.is_some_and(|secret| bearer == Some(secret))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, Method};

    const SECRET: &str = "s3cret-value";

    fn request(headers: &[(&'static str, &str)], query: &str) -> Req {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(*name, HeaderValue::from_str(value).expect("a header value"));
        }
        Req { method: Method::GET, path: "/".into(), query: query.into(), headers: map }
    }

    #[test]
    fn given_a_secret_when_a_route_is_strict_then_only_the_bot_secret_header_gets_in() {
        let right = request(&[("x-bot-secret", SECRET)], "");
        let wrong = request(&[("x-bot-secret", "other")], "");
        let none = request(&[], "");
        let bearer = request(&[("authorization", &format!("Bearer {SECRET}"))], "");
        assert_eq!([&right, &wrong, &none, &bearer].map(|req| bot_secret(Some(SECRET), req)), [true, false, false, false]);
    }

    #[test]
    fn given_no_secret_when_a_route_is_strict_then_nobody_gets_in_but_a_lenient_route_lets_a_caller_without_the_header_in() {
        let anonymous = request(&[], "");
        let sending = request(&[("x-bot-secret", SECRET)], "");
        assert_eq!([&anonymous, &sending].map(|req| bot_secret(None, req)), [false, false]);
        assert_eq!([&anonymous, &sending].map(|req| bot_secret_or_open(None, req)), [true, false]);
    }

    #[test]
    fn given_a_secret_when_a_lenient_route_is_called_then_a_missing_or_wrong_header_is_refused() {
        let right = request(&[("x-bot-secret", SECRET)], "");
        let wrong = request(&[("x-bot-secret", "other")], "");
        let none = request(&[], "");
        assert_eq!([&right, &wrong, &none].map(|req| bot_secret_or_open(Some(SECRET), req)), [true, false, false]);
    }

    #[test]
    fn given_a_secret_when_a_machine_route_is_called_then_the_bearer_gets_in_like_the_header_and_other_schemes_do_not() {
        let bearer = request(&[("authorization", &format!("Bearer {SECRET}"))], "");
        let header = request(&[("x-bot-secret", SECRET)], "");
        let basic = request(&[("authorization", &format!("Basic {SECRET}"))], "");
        let bare = request(&[("authorization", SECRET)], "");
        let wrong = request(&[("authorization", "Bearer other")], "");
        assert_eq!([&bearer, &header, &basic, &bare, &wrong].map(|req| channel(Some(SECRET), req)), [true, true, false, false, false]);
    }

    #[test]
    fn given_no_secret_when_a_machine_route_is_called_then_a_bearer_is_not_enough() {
        let bearer = request(&[("authorization", "Bearer ")], "");
        let empty = request(&[("authorization", "Bearer")], "");
        assert_eq!([&bearer, &empty].map(|req| channel(None, req)), [false, false]);
    }

    #[test]
    fn given_a_minted_bearer_when_presented_to_each_check_then_only_the_agent_socket_takes_it_and_it_takes_no_other_credential() {
        // Given a device bearer, and the service secret, each presented as a bearer token and as X-Bot-Secret
        let bearers = rca_agent_core::bearers::MemoryBearers::default();
        let token = bearers.mint("phone").unwrap().token;
        let as_bearer = |value: &str| request(&[("authorization", &format!("Bearer {value}"))], "");
        let as_header = |value: &str| request(&[("x-bot-secret", value)], "");
        // When each check reads each
        let socket = [agent_socket_bearer(&bearers, &as_bearer(&token)).is_some(), agent_socket_bearer(&bearers, &as_header(&token)).is_some(), agent_socket_bearer(&bearers, &as_bearer(SECRET)).is_some(), agent_socket_bearer(&bearers, &as_header(SECRET)).is_some()];
        let admin_or_executor = [channel(Some(SECRET), &as_bearer(&token)), channel(Some(SECRET), &as_header(&token)), channel(Some(SECRET), &as_bearer(SECRET)), channel(Some(SECRET), &as_header(SECRET))];
        // Then the socket opens for the bearer as a bearer token only, and never for the secret, while the admin and executor check opens for the secret only
        assert_eq!(socket, [true, false, false, false]);
        assert_eq!(admin_or_executor, [false, false, true, true]);
        // And a revoked bearer, a made-up token and a request with no credential open nothing
        let id = bearers.list().unwrap()[0].id.clone();
        assert!(bearers.revoke(&id).unwrap());
        assert!([agent_socket_bearer(&bearers, &as_bearer(&token)), agent_socket_bearer(&bearers, &as_bearer("rcab_made_up")), agent_socket_bearer(&bearers, &request(&[], ""))].iter().all(Option::is_none));
    }

    #[test]
    fn given_a_secret_when_the_socket_upgrades_then_the_token_parameter_must_match_and_without_a_secret_anyone_connects() {
        let right = request(&[], &format!("token={SECRET}"));
        let wrong = request(&[], "token=other");
        let missing = request(&[], "");
        assert_eq!([&right, &wrong, &missing].map(|req| socket_authorised(Some(SECRET), req)), [true, false, false]);
        assert_eq!([&right, &wrong, &missing].map(|req| socket_authorised(None, req)), [true, true, true]);
    }
}
