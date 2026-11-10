use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::opencode_log_writes::Refused;
use vm_ops_api_core::domain::request::Target;
use vm_ops_api_core::domain::validators::is_valid_run_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    Start,
    Line,
    Finish,
}

/// `/opencode-log/<runId>/<action>`: a run id that is none matches no route, so the request is a 404 after auth.
pub fn write_of(target: &Target) -> Option<(Write, &str)> {
    let [log, run_id, action] = target.parts.as_slice() else { return None };
    if log != "opencode-log" || !is_valid_run_id(run_id) {
        return None;
    }
    let write = match action.as_str() {
        "start" => Write::Start,
        "line" => Write::Line,
        "finish" => Write::Finish,
        _ => return None,
    };
    Some((write, run_id))
}

pub fn is_write(target: &Target, write: Write) -> bool {
    write_of(target).is_some_and(|(found, _)| found == write)
}

/// The route has matched, so the second segment is a valid run id.
pub async fn answer(write: Write, call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let run_id = call.target.parts[1].as_str();
    let done = match write {
        Write::Start => state.run_writes.start(run_id, call.body).await,
        Write::Line => state.run_writes.lines(run_id, call.body).await,
        Write::Finish => state.run_writes.finish(run_id, call.body).await,
    };
    match done {
        Ok(()) => Ok(Reply::json(200, &json!({ "ok": true }))),
        Err(Refused::NotJson) => Ok(Reply::json(400, &json!({ "error": "invalid JSON body" }))),
        Err(Refused::Store(error)) => Err(Failure(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(raw: &str) -> Option<(Write, String)> {
        write_of(&Target::parse(raw).expect("a target")).map(|(write, run_id)| (write, run_id.to_string()))
    }

    #[test]
    fn given_opencode_log_paths_when_matched_then_a_valid_run_id_and_a_write_action_reach_a_write_and_nothing_else_does() {
        assert_eq!(write("/opencode-log/run-1/start"), Some((Write::Start, "run-1".to_string())));
        assert_eq!(write("/opencode-log//run-1//line/"), Some((Write::Line, "run-1".to_string())));
        assert_eq!(write("/opencode-log/api/finish"), Some((Write::Finish, "api".to_string())));
        for raw in ["/opencode-log/bad_id!/start", "/opencode-log/run-1/stop", "/opencode-log/run-1", "/opencode-log/run-1/start/x", "/other/run-1/start"] {
            assert_eq!(write(raw), None, "{raw}");
        }
    }
}
