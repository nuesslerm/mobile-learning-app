//! The HTTP side: one fallback handler that hands every request to the core's routing and turns its reply into a response.
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::Response;
use axum::Router;
use libs_proxy_core::{respond, route, ObjectRead, Reply, Route};
use std::sync::Arc;

#[derive(Clone)]
pub struct App {
    objects: Arc<dyn ObjectRead>,
    public_base: Arc<str>,
    commit: Option<Arc<str>>,
}

impl App {
    pub fn new(objects: Arc<dyn ObjectRead>, public_base: &str, commit: Option<&str>) -> App {
        App { objects, public_base: public_base.into(), commit: commit.map(Into::into) }
    }

    pub fn router(self) -> Router {
        Router::new().fallback(handle).with_state(self)
    }
}

async fn handle(axum::extract::State(app): axum::extract::State<App>, method: Method, uri: Uri, headers: HeaderMap) -> Response {
    let matched = match route(method.as_str(), uri.path()) {
        Ok(matched) => matched,
        Err(rejection) => {
            tracing::info!(status = rejection.status(), "request.rejected");
            let mut response = into_response(Reply::text(rejection.status(), "rejected"));
            if rejection.status() == 405 {
                response.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET"));
            }
            return response;
        }
    };
    let read = match &matched {
        Route::Index { key } | Route::Crate { key } => {
            let if_none_match = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
            Some(app.objects.get(key, if_none_match).await)
        }
        Route::Health | Route::Config => None,
    };
    let reply = respond(&matched, read, &app.public_base, app.commit.as_deref());
    tracing::info!(status = reply.status, bytes = reply.body.len(), "request.served");
    into_response(reply)
}

fn into_response(reply: Reply) -> Response {
    let mut response = Response::new(Body::from(reply.body));
    *response.status_mut() = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(reply.content_type));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Some(cache) = reply.cache_control {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    }
    if let Some(etag) = reply.etag.and_then(|e| HeaderValue::from_str(&e).ok()) {
        headers.insert(header::ETAG, etag);
    }
    response
}
