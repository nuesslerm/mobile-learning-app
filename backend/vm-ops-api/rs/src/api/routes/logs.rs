use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use axum::body::{Body, Bytes};
use futures_util::stream::unfold;
use serde_json::json;
use std::convert::Infallible;
use vm_ops_api_core::application::logs::{stream_ended, Follow, Outcome, Query};
use vm_ops_api_core::application::ports::LogFollow;
use vm_ops_api_core::domain::apps::LOG_APPS;

const AMBIGUOUS: &str = "multiple containers for this app -- pass &container=<name> to pick one";

pub async fn answer(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let (tail, follow, container) = (call.target.query_param("tail"), call.target.query_param("follow"), call.target.query_param("container"));
    let query = Query { tail: tail.as_deref(), follow: follow.as_deref(), container: container.as_deref() };
    Ok(match state.logs.read(&call.target.parts[1], &query).await? {
        Outcome::UnknownApp => Reply::unknown_app(&LOG_APPS),
        Outcome::NoContainers => Reply::json(404, &json!({ "error": "no containers found for this app (not running?)" })),
        Outcome::Snapshot(snapshot) => Reply::json(200, &snapshot),
        Outcome::Ambiguous(containers) => Reply::json(400, &json!({ "error": AMBIGUOUS, "containers": containers })),
        Outcome::NoSuchContainer(containers) => Reply::json(404, &json!({ "error": "no container with that name for this app", "containers": containers })),
        Outcome::Follow(follow) => followed(follow),
    })
}

/// Where the next read of a followed log comes from.
enum Phase {
    Open(Follow),
    Read(Box<dyn LogFollow>),
    Done,
}

/// The log as plain text for as long as the Engine's stream lasts. The reply is sent before the Engine is asked, so a failure of
/// either ends the text with the line that says why. The Engine's stream is dropped, and its connection with it, when the caller
/// goes away.
fn followed(follow: Follow) -> Reply {
    let name = follow.container_name.clone();
    let texts = unfold(Phase::Open(follow), |phase| async move {
        let mut stream = match phase {
            Phase::Open(follow) => match follow.open().await {
                Ok(stream) => stream,
                Err(error) => return Some((Ok::<_, Infallible>(Bytes::from(stream_ended(&error))), Phase::Done)),
            },
            Phase::Read(stream) => stream,
            Phase::Done => return None,
        };
        loop {
            match stream.next_text().await {
                Ok(Some(text)) if text.is_empty() => continue,
                Ok(Some(text)) => return Some((Ok(Bytes::from(text)), Phase::Read(stream))),
                Ok(None) => return None,
                Err(error) => return Some((Ok(Bytes::from(stream_ended(&error))), Phase::Done)),
            }
        }
    });
    Reply::streamed(200, "text/plain; charset=utf-8", Body::from_stream(texts)).with_header("cache-control", "no-cache").with_header("x-container-name", name)
}
