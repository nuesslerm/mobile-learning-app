use crate::api::reply::Reply;
use crate::api::routes::Call;
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::self_redeploy::Outcome;
use vm_ops_api_core::domain::validators::is_valid_image_tag;

fn is_commit(reference: &str) -> bool {
    reference.len() == 40 && reference.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn query(call: &Call<'_>) -> Result<(String, String), Reply> {
    let Some(reference) = call.target.query_param("ref").filter(|reference| is_commit(reference)) else {
        return Err(Reply::json(400, &json!({ "error": "ref must be a full 40-character commit sha" })));
    };
    let Some(tag) = call.target.query_param("tag").filter(|tag| is_valid_image_tag(tag)) else {
        return Err(Reply::json(400, &json!({ "error": "tag is missing or not an image tag" })));
    };
    Ok((reference, tag))
}

/// `POST /redeploy/smoke?ref=<commit>&tag=<tag>`: the pulled image runs as a throwaway container on the spare port and is left running
/// when it passes its own probe, so `wf ship` can reach it over the tailnet. 200 when up, 502 with the test's output when not.
pub async fn smoke(call: &Call<'_>, state: &AppState) -> Reply {
    let (reference, tag) = match query(call) {
        Ok(query) => query,
        Err(reply) => return reply,
    };
    match state.self_redeploy.smoke_start(&reference, &tag).await {
        Ok(()) => Reply::json(200, &json!({ "smoke": "up", "port": 18999, "ref": reference, "tag": tag })),
        Err(reason) => Reply::json(502, &json!({ "error": "smoke test failed, nothing was swapped", "reason": reason })),
    }
}

/// `POST /redeploy/smoke/stop`: removes the throwaway container.
pub async fn smoke_stop(_call: &Call<'_>, state: &AppState) -> Reply {
    let result = state.self_redeploy.smoke_stop().await;
    Reply::json(if result.code == Some(0) { 200 } else { 502 }, &json!({ "exitCode": result.code, "stdout": result.stdout, "stderr": result.stderr }))
}

/// `POST /redeploy?ref=<commit>&tag=<tag>`: `wf ship` has built and pushed the images at the tag and smoke-tested them (`/redeploy/smoke`).
/// 202 once the swap is spawned; the swap ends this process, so its end is `/redeploy/status` and `/health`.
pub async fn answer(call: &Call<'_>, state: &AppState) -> Reply {
    let (reference, tag) = match query(call) {
        Ok(query) => query,
        Err(reply) => return reply,
    };
    let started = state.self_redeploy.begin(&reference, &tag);
    let Some(run) = started.run else {
        return Reply::json(202, &json!({ "started": false, "swapping": started.reference }));
    };
    match run.finish().await {
        Outcome::Swapping { tag, commit } => Reply::json(202, &json!({ "started": true, "swapping": commit, "tag": tag })),
        Outcome::Failed { reason } => Reply::json(502, &json!({ "error": "the swap could not be started", "reason": reason })),
    }
}
