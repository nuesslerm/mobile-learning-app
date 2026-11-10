use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use serde::Serialize;
use vm_ops_api_core::domain::js;

const JSON: &str = "application/json";
const HTML: &str = "text/html; charset=utf-8";

/// A reply, as the service writes it: by default a JSON body under exactly `application/json`, with its length. A streamed body
/// has no length and ends when its source does.
pub struct Reply {
    pub status: StatusCode,
    pub body: Vec<u8>,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
    stream: Option<Body>,
    close: bool,
}

impl Reply {
    fn of(status: u16, body: Vec<u8>, content_type: &'static str) -> Reply {
        Reply { status: StatusCode::from_u16(status).expect("a status code"), body, content_type, headers: Vec::new(), stream: None, close: false }
    }

    /// The body as `JSON.stringify` writes it: keys in the order they were given, whole numbers without a fraction.
    pub fn json(status: u16, body: &impl Serialize) -> Reply {
        let value = serde_json::to_value(body).expect("a reply body is JSON");
        Reply::of(status, js::stringify(&value).into_bytes(), JSON)
    }

    pub fn html(status: u16, body: Vec<u8>) -> Reply {
        Reply::of(status, body, HTML)
    }

    /// A JSON body already written as text, for a string that holds a lone surrogate, which no `serde_json::Value` can.
    pub fn json_text(status: u16, body: String) -> Reply {
        Reply::of(status, body.into_bytes(), JSON)
    }

    /// A body that is no JSON, under the content type the route names.
    pub fn text(status: u16, content_type: &'static str, body: String) -> Reply {
        Reply::of(status, body.into_bytes(), content_type)
    }

    /// A reply whose body is written as it is produced.
    pub fn streamed(status: u16, content_type: &'static str, body: Body) -> Reply {
        Reply { stream: Some(body), ..Reply::of(status, Vec::new(), content_type) }
    }

    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Reply {
        self.headers.push((name, value.into()));
        self
    }

    /// 404 for an app that is none of `known`, which it lists in order.
    pub fn unknown_app(known: &[&str]) -> Reply {
        Reply::json(404, &serde_json::json!({ "error": "unknown app", "knownApps": known }))
    }

    /// The connection ends after this reply, for a request whose body was not read to its end.
    pub fn closing(mut self) -> Reply {
        self.close = true;
        self
    }

    pub fn into_response(self) -> Response {
        let mut response = Response::new(self.stream.unwrap_or_else(|| Body::from(self.body)));
        *response.status_mut() = self.status;
        response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(self.content_type));
        for (name, value) in self.headers {
            if let Ok(value) = HeaderValue::try_from(value) {
                response.headers_mut().insert(header::HeaderName::from_static(name), value);
            }
        }
        if self.close {
            response.headers_mut().insert(header::CONNECTION, HeaderValue::from_static("close"));
        }
        response
    }
}
