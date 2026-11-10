use crate::application::ports::ReadResult;
use crate::domain::routing::Route;
use serde_json::json;

pub const CRATE_CACHE: &str = "public, max-age=31536000, immutable";
pub const INDEX_CACHE: &str = "public, max-age=60, must-revalidate";

/// A response as the proxy sends it: nothing of the bucket reaches it beyond the object's bytes and entity tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub cache_control: Option<&'static str>,
    pub etag: Option<String>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn text(status: u16, body: &str) -> Reply {
        Reply { status, content_type: "text/plain; charset=utf-8", cache_control: None, etag: None, body: format!("{body}\n").into_bytes() }
    }
}

/// `config.json`, generated: Cargo downloads crates from the proxy, never from the bucket.
pub fn config_json(public_base: &str) -> String {
    format!("{}\n", json!({ "dl": format!("{}/dl/{{crate}}/{{version}}.crate", public_base.trim_end_matches('/')) }))
}

pub fn health(commit: Option<&str>) -> Reply {
    let body = json!({ "status": "ok", "commit": commit.unwrap_or("unknown") }).to_string();
    Reply { status: 200, content_type: "application/json", cache_control: Some("no-store"), etag: None, body: body.into_bytes() }
}

/// The reply for a served route given what the bucket answered (`None` for the routes that read nothing).
pub fn respond(route: &Route, read: Option<ReadResult>, public_base: &str, commit: Option<&str>) -> Reply {
    match route {
        Route::Health => health(commit),
        Route::Config => Reply { status: 200, content_type: "application/json", cache_control: Some(INDEX_CACHE), etag: None, body: config_json(public_base).into_bytes() },
        Route::Index { .. } | Route::Crate { .. } => {
            let (content_type, cache) = match route {
                Route::Crate { .. } => ("application/octet-stream", CRATE_CACHE),
                _ => ("text/plain; charset=utf-8", INDEX_CACHE),
            };
            match read {
                Some(ReadResult::Found(object)) => Reply { status: 200, content_type, cache_control: Some(cache), etag: object.etag, body: object.bytes },
                Some(ReadResult::NotModified) => Reply { status: 304, content_type, cache_control: Some(cache), etag: None, body: Vec::new() },
                Some(ReadResult::Missing) => Reply::text(404, "not found"),
                Some(ReadResult::Unavailable) | None => Reply::text(502, "bad gateway"),
            }
        }
    }
}
