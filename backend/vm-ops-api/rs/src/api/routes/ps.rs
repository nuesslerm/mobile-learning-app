use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use vm_ops_api_core::domain::apps::KNOWN_APPS;

pub async fn answer(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    Ok(match state.ps.list(&call.target.parts[1]).await? {
        Some(report) => Reply::json(200, &report),
        None => Reply::unknown_app(&KNOWN_APPS),
    })
}
