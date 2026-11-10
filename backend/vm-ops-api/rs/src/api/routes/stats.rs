use crate::api::reply::Reply;
use crate::api::routes::Failure;
use crate::api::AppState;

pub async fn answer(state: &AppState) -> Result<Reply, Failure> {
    Ok(Reply::json(200, &state.stats.report().await?))
}
