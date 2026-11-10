use async_trait::async_trait;
use leetcode_backend_core::application::ports::{Leetcode, PortResult};
use leetcode_backend_core::application::requests::{SubmitRequest, TestRequest};
use leetcode_backend_core::application::session::Session;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use reqwest::header::{CONTENT_TYPE, COOKIE};
use reqwest::Method;
use serde_json::{json, Value};

/// Everything but the unreserved characters, so a caller's text is one path segment whatever it holds.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// The four relays to leetcode.com under the stored session.
pub struct HttpLeetcode {
    client: reqwest::Client,
    origin: String,
}

/// A path segment from a caller: percent-encoded, and never `.` or `..`.
fn segment(raw: &str) -> PortResult<String> {
    if raw.is_empty() || raw.chars().all(|c| c == '.') {
        return Err(format!("Invalid path segment: {raw}"));
    }
    Ok(utf8_percent_encode(raw, SEGMENT).to_string())
}

impl HttpLeetcode {
    pub fn new(client: reqwest::Client, origin: impl Into<String>) -> Self {
        Self { client, origin: origin.into() }
    }

    async fn relay(&self, method: Method, path: &str, body: Option<Value>, session: &Session) -> PortResult<Value> {
        let csrf = session.csrf_token();
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.origin))
            .header(CONTENT_TYPE, "application/json")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("X-CSRFToken", csrf)
            .header(COOKIE, format!("LEETCODE_SESSION={}; csrftoken={csrf}", session.session_id()));
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        let response = request.send().await.map_err(|error| format!("fetch failed: {error}"))?;
        let status = response.status();
        let text = response.text().await.map_err(|error| format!("fetch failed: {error}"))?;
        if !status.is_success() {
            return Err(format!("LeetCode request failed ({}): {text}", status.as_u16()));
        }
        serde_json::from_str(&text).map_err(|error| format!("LeetCode answered with something that is not JSON: {error}"))
    }
}

#[async_trait]
impl Leetcode for HttpLeetcode {
    async fn user_status(&self, session: &Session) -> PortResult<Value> {
        let query = json!({ "query": "query globalData { userStatus { isSignedIn username } }", "variables": {} });
        let response = self.relay(Method::POST, "/graphql", Some(query), session).await?;
        let status = response.pointer("/data/userStatus");
        let signed_in = status.and_then(|status| status.get("isSignedIn")).and_then(Value::as_bool).unwrap_or(false);
        let username = status.and_then(|status| status.get("username")).filter(|name| !name.is_null()).cloned().unwrap_or(Value::Null);
        Ok(json!({ "isSignedIn": signed_in, "username": username }))
    }

    async fn test(&self, session: &Session, request: &TestRequest) -> PortResult<Value> {
        let path = format!("/problems/{}/interpret_solution/", segment(request.slug.as_str())?);
        let body = json!({
            "lang": request.lang.as_str(),
            "question_id": request.question_id,
            "typed_code": request.code,
            "data_input": request.data_input,
            "test_mode": false,
        });
        self.relay(Method::POST, &path, Some(body), session).await
    }

    async fn submit(&self, session: &Session, request: &SubmitRequest) -> PortResult<Value> {
        let path = format!("/problems/{}/submit/", segment(request.slug.as_str())?);
        let body = json!({
            "lang": request.lang.as_str(),
            "question_id": request.question_id,
            "typed_code": request.code,
            "test_mode": false,
            "judge_type": "large",
        });
        self.relay(Method::POST, &path, Some(body), session).await
    }

    async fn poll(&self, session: &Session, id: &str) -> PortResult<Value> {
        let path = format!("/submissions/detail/{}/check/", segment(id)?);
        self.relay(Method::GET, &path, None, session).await
    }
}
