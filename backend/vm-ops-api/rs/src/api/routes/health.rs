use crate::api::reply::Reply;
use crate::api::AppState;

pub fn answer(state: &AppState) -> Reply {
    Reply::json(200, &state.health.report())
}
