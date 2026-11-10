//! The opencode run log as people read it: the runs list with its filters, and one run with its last lines.
use crate::application::ports::{RunLog, RunMeta};
use crate::domain::js;
use crate::domain::validators::is_valid_run_id;
use serde_json::Value;
use std::sync::Arc;

pub struct OpencodeLog {
    runs: Arc<dyn RunLog>,
}

/// `String(value)`, as the prompt of a run is searched.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => js::number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `String(meta.prompt || '')`: a prompt that is falsy searches as empty.
fn searched_prompt(meta: &RunMeta) -> String {
    match meta.get("prompt") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::Number(number)) if number.as_f64() == Some(0.0) => String::new(),
        Some(prompt) => js_string(prompt),
    }
}

impl OpencodeLog {
    pub fn new(runs: Arc<dyn RunLog>) -> Self {
        Self { runs }
    }

    /// Every run, newest first. An empty `agent` or `q` filters nothing; `agent` is an exact match and `q` a substring of the
    /// prompt in any case.
    pub async fn list(&self, agent: &str, q: &str) -> Vec<RunMeta> {
        let q = q.to_lowercase();
        let matches = |meta: &RunMeta| {
            (agent.is_empty() || meta.get("agent").and_then(Value::as_str) == Some(agent)) && (q.is_empty() || searched_prompt(meta).to_lowercase().contains(&q))
        };
        self.runs.list_runs().await.into_iter().filter(matches).collect()
    }

    /// The run's meta with its last 5000 non-empty lines under `lines`; none for an id that is none or a run with no meta.
    pub async fn detail(&self, run_id: &str) -> Option<RunMeta> {
        if !is_valid_run_id(run_id) {
            return None;
        }
        let mut meta = self.runs.read_meta(run_id).await?;
        let lines = self.runs.read_lines(run_id).await;
        meta.insert("lines".to_string(), Value::Array(lines.into_iter().map(Value::String).collect()));
        Some(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeRunLog;
    use serde_json::json;

    fn meta(run_id: &str, prompt: Value, agent: &str, started_at: &str) -> RunMeta {
        json!({ "runId": run_id, "prompt": prompt, "agent": agent, "startedAt": started_at }).as_object().cloned().expect("an object")
    }

    async fn log_of(metas: Vec<RunMeta>) -> OpencodeLog {
        let runs = FakeRunLog::default();
        for meta in metas {
            let id = meta["runId"].as_str().expect("a run id").to_string();
            runs.write_meta(&id, meta).await.expect("stored");
        }
        OpencodeLog::new(Arc::new(runs))
    }

    async fn ids(log: &OpencodeLog, agent: &str, q: &str) -> Vec<String> {
        log.list(agent, q).await.iter().map(|run| run["runId"].as_str().unwrap_or_default().to_string()).collect()
    }

    #[tokio::test]
    async fn given_three_runs_when_listed_with_filters_then_they_come_newest_first_with_agent_exact_and_the_prompt_search_in_any_case() {
        // Given runs by two agents started at different times
        let log = log_of(vec![
            meta("run-a", json!("Fix the Login bug"), "implementer", "2026-10-20T10:00:00.000Z"),
            meta("run-b", json!("Find the docs"), "researcher", "2026-10-21T09:00:00.000Z"),
            meta("run-c", json!("add a LOGIN test"), "implementer", "2026-10-21T11:00:00.000Z"),
        ])
        .await;
        // When they are listed with no filter, by agent, by prompt text in two cases and by both
        // Then each list is newest first; the agent is exact and the search ignores case
        assert_eq!(ids(&log, "", "").await, ["run-c", "run-b", "run-a"]);
        assert_eq!(ids(&log, "implementer", "").await, ["run-c", "run-a"]);
        assert_eq!(ids(&log, "Implementer", "").await, Vec::<String>::new());
        assert_eq!(ids(&log, "", "login").await, ["run-c", "run-a"]);
        assert_eq!(ids(&log, "", "LoGiN").await, ["run-c", "run-a"]);
        assert_eq!(ids(&log, "researcher", "login").await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn given_a_prompt_that_is_no_string_when_searched_then_it_reads_as_javascript_stringifies_it() {
        let log = log_of(vec![
            meta("falsy", json!(0), "a", "2026-10-20T10:00:00.000Z"),
            meta("number", json!(42.5), "a", "2026-10-20T11:00:00.000Z"),
            meta("list", json!(["x", null, 7]), "a", "2026-10-20T12:00:00.000Z"),
            meta("object", json!({ "k": 1 }), "a", "2026-10-20T13:00:00.000Z"),
        ])
        .await;

        assert_eq!(ids(&log, "", "42.5").await, ["number"]);
        assert_eq!(ids(&log, "", "x,,7").await, ["list"]);
        assert_eq!(ids(&log, "", "[object object]").await, ["object"]);
        assert_eq!(ids(&log, "", "0").await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn given_a_run_with_lines_when_its_detail_is_read_then_the_lines_follow_its_meta_and_a_bad_or_unknown_id_has_none() {
        // Given a run with a meta and two lines, and one with no meta
        let runs = FakeRunLog::default();
        runs.write_meta("run-a", meta("run-a", json!("p"), "implementer", "2026-10-20T10:00:00.000Z")).await.expect("stored");
        runs.append_lines("run-a", &["{\"n\":1}".to_string(), "{\"n\":2}".to_string()]).await.expect("stored");
        runs.append_lines("run-b", &["orphan".to_string()]).await.expect("stored");
        let log = OpencodeLog::new(Arc::new(runs));
        // When the detail is read, Then `lines` is the last key
        let detail = log.detail("run-a").await.expect("a run");
        assert_eq!(
            js::stringify(&Value::Object(detail)),
            r#"{"runId":"run-a","prompt":"p","agent":"implementer","startedAt":"2026-10-20T10:00:00.000Z","lines":["{\"n\":1}","{\"n\":2}"]}"#
        );
        // When a run with no meta, an unknown run and an id that is no run id are read, Then none has a detail
        for id in ["run-b", "run-z", "bad_id!", ""] {
            assert_eq!(log.detail(id).await, None, "{id:?}");
        }
    }
}
