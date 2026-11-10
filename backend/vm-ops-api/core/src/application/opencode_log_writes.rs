//! The writes of an opencode run's log: `wf offload ask --live` posts a start, lines as they come and a finish, each as JSON. A run id is
//! validated by the route that reached it; the store takes it as it is.
use crate::application::ports::{Clock, RunLog, StateError};
use crate::domain::js::iso_string;
use crate::domain::json_body::parse_record;
use crate::domain::run_meta;
use std::sync::Arc;

/// Why a write has no effect: the body is no JSON, or the store could not write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    NotJson,
    Store(StateError),
}

impl From<StateError> for Refused {
    fn from(error: StateError) -> Self {
        Refused::Store(error)
    }
}

pub struct RunWrites {
    log: Arc<dyn RunLog>,
    clock: Arc<dyn Clock>,
}

impl RunWrites {
    pub fn new(log: Arc<dyn RunLog>, clock: Arc<dyn Clock>) -> Self {
        Self { log, clock }
    }

    fn now(&self) -> String {
        iso_string(self.clock.now_ms())
    }

    pub async fn start(&self, run_id: &str, body: &[u8]) -> Result<(), Refused> {
        let post = parse_record(body).ok_or(Refused::NotJson)?;
        Ok(self.log.write_meta(run_id, run_meta::started(run_id, &post, &self.now())).await?)
    }

    pub async fn lines(&self, run_id: &str, body: &[u8]) -> Result<(), Refused> {
        let post = parse_record(body).ok_or(Refused::NotJson)?;
        Ok(self.log.append_lines(run_id, &run_meta::string_lines(&post)).await?)
    }

    pub async fn finish(&self, run_id: &str, body: &[u8]) -> Result<(), Refused> {
        let post = parse_record(body).ok_or(Refused::NotJson)?;
        let existing = self.log.read_meta(run_id).await;
        Ok(self.log.write_meta(run_id, run_meta::finished(run_id, existing, &post, &self.now())).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, FakeRunLog};

    const T0: i64 = 1_791_115_200_000;

    fn writes(log: FakeRunLog, clock: &Arc<FakeClock>) -> (RunWrites, Arc<FakeRunLog>) {
        let log = Arc::new(log);
        (RunWrites::new(log.clone(), clock.clone()), log)
    }

    #[tokio::test]
    async fn given_a_run_when_started_written_and_finished_then_it_reads_back_running_then_finished_with_its_lines() {
        let clock = Arc::new(FakeClock::at(T0));
        let (runs, log) = writes(FakeRunLog::default(), &clock);

        runs.start("run-1", br#"{"prompt":"Fix it","caller":"implementer","model":"m"}"#).await.expect("started");
        runs.lines("run-1", br#"{"lines":["a",5,"b"]}"#).await.expect("lines");
        let running = log.read_meta("run-1").await.expect("a meta");
        clock.set(T0 + 5_000);
        runs.finish("run-1", br#"{"ok":true,"tokens":{"total":3,"input":2,"output":1},"cost":0.5,"durationMs":5000}"#).await.expect("finished");
        let finished = log.read_meta("run-1").await.expect("a meta");

        assert_eq!((running["status"].as_str(), running["startedAt"].as_str(), running["agent"].as_str()), (Some("running"), Some("2026-10-04T12:00:00.000Z"), Some("implementer")));
        assert_eq!((finished["status"].as_str(), finished["finishedAt"].as_str(), finished["model"].as_str()), (Some("finished"), Some("2026-10-04T12:00:05.000Z"), Some("m")));
        assert_eq!(log.read_lines("run-1").await, ["a", "b"]);
    }

    #[tokio::test]
    async fn given_a_finish_for_a_run_that_never_started_when_written_then_the_stub_is_stored_with_the_finish_fields() {
        let clock = Arc::new(FakeClock::at(T0));
        let (runs, log) = writes(FakeRunLog::default(), &clock);

        runs.finish("run-x", br#"{"model":"opencode/b"}"#).await.expect("finished");

        let meta = log.read_meta("run-x").await.expect("a meta");
        assert_eq!(meta.keys().map(String::as_str).collect::<Vec<_>>(), ["runId", "prompt", "agent", "model", "startedAt", "finishedAt", "status", "ok", "tokens", "cost", "durationMs"]);
        assert_eq!((meta["prompt"].as_str(), meta["model"].as_str(), meta["startedAt"] == meta["finishedAt"]), (Some(""), Some("opencode/b"), true));
    }

    #[tokio::test]
    async fn given_invalid_json_or_a_store_that_cannot_write_when_written_then_each_is_refused_and_nothing_is_stored() {
        let clock = Arc::new(FakeClock::at(T0));
        let (runs, log) = writes(FakeRunLog::default(), &clock);

        for refused in [runs.start("r", b"{oops").await, runs.lines("r", b"{oops").await, runs.finish("r", b"{oops").await] {
            assert_eq!(refused, Err(Refused::NotJson));
        }
        assert_eq!(log.read_meta("r").await, None);
        let (broken, _) = writes(FakeRunLog::default().whose_writes_fail(), &clock);
        let failed = broken.start("r", b"{}").await;
        assert_eq!(failed, Err(Refused::Store(StateError("EACCES: permission denied, open '/state/opencode-log/r/meta.json'".to_string()))));
    }
}
