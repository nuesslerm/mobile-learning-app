//! `POST /redeploy`: swaps this service to an image `wf ship` already built, pushed and smoke-tested. The smoke test is its own step
//! (`smoke_start`, `smoke_stop`): the image run as a throwaway container beside the live one, left up until `wf` has asked it over the
//! tailnet; a failure answers with its log and swaps nothing. One swap at a time: a request while one runs joins it.
use crate::application::ports::{Clock, Event, EventLog, SiblingResult, Siblings};
use crate::application::scripts::redeploy::{append_history_args, self_redeploy_sibling_args, self_smoke_sibling_args, smoke_stop_sibling_args, HistoryEntry, RedeployEnv};
use std::sync::{Arc, Mutex, MutexGuard};

const REQUESTED_BY: &str = "self-redeploy";
const KEY: &str = "vm-ops-api";
const REASON_TAIL_CHARS: usize = 4000;

pub struct SelfRedeployDeps {
    pub siblings: Arc<dyn Siblings>,
    pub log: Arc<dyn EventLog>,
    pub clock: Arc<dyn Clock>,
    pub env: RedeployEnv,
}

/// How a request ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The swap was spawned: this process is about to be replaced.
    Swapping { tag: String, commit: String },
    /// Nothing was swapped; `reason` is why the swap could not be spawned.
    Failed { reason: String },
}

/// What a request got: its own run, or the one already running, which it joined.
pub struct Started {
    pub started: bool,
    /// The commit being proved: the request's own, or the one in flight.
    pub reference: String,
    pub run: Option<Run>,
}

struct Inner {
    deps: SelfRedeployDeps,
    flight: Mutex<Option<String>>,
}

pub struct SelfRedeploy {
    inner: Arc<Inner>,
}

/// The run a request started. Ending it, by finishing or by being dropped, lets the next request start another.
pub struct Run {
    inner: Arc<Inner>,
    reference: String,
    tag: String,
}

fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tail(text: &str, chars: usize) -> String {
    let all: Vec<char> = text.chars().collect();
    all[all.len().saturating_sub(chars)..].iter().collect()
}

impl SelfRedeploy {
    pub fn new(deps: SelfRedeployDeps) -> Self {
        Self { inner: Arc::new(Inner { deps, flight: Mutex::new(None) }) }
    }

    /// Starts the smoke test and leaves its throwaway container running for a probe over the tailnet; `Err` is the test's own output.
    pub async fn smoke_start(&self, reference: &str, tag: &str) -> Result<(), String> {
        self.inner.smoke(reference, tag).await
    }

    /// Removes the throwaway container of `smoke_start`, whether or not one is running.
    pub async fn smoke_stop(&self) -> SiblingResult {
        self.inner.deps.siblings.run(&smoke_stop_sibling_args(&self.inner.deps.env, KEY)).await
    }

    /// Takes the single flight when it is free.
    pub fn begin(&self, reference: &str, tag: &str) -> Started {
        {
            let mut flight = held(&self.inner.flight);
            if let Some(running) = flight.as_ref() {
                return Started { started: false, reference: running.clone(), run: None };
            }
            *flight = Some(reference.to_string());
        }
        Started { started: true, reference: reference.to_string(), run: Some(Run { inner: self.inner.clone(), reference: reference.to_string(), tag: tag.to_string() }) }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        *held(&self.inner.flight) = None;
    }
}

impl Run {
    pub async fn finish(self) -> Outcome {
        self.inner.swap(&self.reference, &self.tag).await
    }
}

impl Inner {
    /// Runs the smoke test: the throwaway container passes its own probe and is left running for a probe over the tailnet, or the
    /// history and the log record why it did not.
    async fn smoke(&self, reference: &str, tag: &str) -> Result<(), String> {
        let deps = &self.deps;
        let event = |event: Event| event.with("ref", reference).with("tag", tag);
        deps.log.emit(event(Event::info("self_redeploy.smoke_started")));
        let smoke = deps.siblings.run(&self_smoke_sibling_args(&deps.env, tag, reference)).await;
        if smoke.code == Some(0) {
            return Ok(());
        }
        let output = if smoke.stderr.trim().is_empty() { &smoke.stdout } else { &smoke.stderr };
        let reason = tail(output.trim(), REASON_TAIL_CHARS);
        let reason = if reason.is_empty() { format!("the smoke test ended {}", smoke.code.map_or("by a signal".to_string(), |code| format!("with exit code {code}"))) } else { reason };
        let entry = HistoryEntry { key: KEY, requested_by: REQUESTED_BY, tag, reference, outcome: "redeploy.failed", exit_code: smoke.code };
        deps.siblings.run(&append_history_args(&deps.env.vm_dir, &deps.env.sibling_image(), &entry, deps.clock.now_ms())).await;
        deps.log.emit(event(Event::error("self_redeploy.smoke_failed")).with("reason", reason.as_str()).with("exitCode", smoke.code.map_or_else(|| "null".to_string(), |code| code.to_string())));
        Err(reason)
    }

    async fn swap(&self, reference: &str, tag: &str) -> Outcome {
        let deps = &self.deps;
        let event = |event: Event| event.with("ref", reference).with("tag", tag);
        deps.log.emit(event(Event::info("self_redeploy.swapping")));
        if let Err(error) = deps.siblings.spawn_detached(&self_redeploy_sibling_args(&deps.env, tag, reference, REQUESTED_BY)) {
            let reason = error.to_string();
            deps.log.emit(event(Event::error("self_redeploy.failed")).with("reason", reason.as_str()));
            return Outcome::Failed { reason };
        }
        Outcome::Swapping { tag: tag.to_string(), commit: reference.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::SiblingResult;
    use crate::testing::{FakeClock, FakeEventLog, FakeSiblings, SiblingCall};

    const COMMIT: &str = "123456789abcdef0123456789abcdef012345678";
    const NOON: i64 = 1_791_115_200_000;

    struct Rig {
        redeploy: SelfRedeploy,
        siblings: Arc<FakeSiblings>,
        log: Arc<FakeEventLog>,
    }

    fn rig(smoke: SiblingResult) -> Rig {
        let (siblings, log) = (Arc::new(FakeSiblings::new().with_run("--healthcheck", vec![smoke]).with_run("stopped'", vec![SiblingResult { code: Some(0), stdout: "[smoke] vm-ops-api stopped\n".into(), stderr: String::new() }])), Arc::new(FakeEventLog::default()));
        let env = RedeployEnv { image_namespace: "ns".into(), image_tag: Some("sha-7654321".into()), vm_dir: "/home/ubuntu/vm-ops-api".into(), provision_token: "tok".into(), ..RedeployEnv::default() };
        let deps = SelfRedeployDeps { siblings: siblings.clone(), log: log.clone(), clock: Arc::new(FakeClock::at(NOON)), env };
        Rig { redeploy: SelfRedeploy::new(deps), siblings, log }
    }

    fn ran(code: i32, stdout: &str, stderr: &str) -> SiblingResult {
        SiblingResult { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn messages(rig: &Rig) -> Vec<&'static str> {
        rig.log.events().iter().map(|event| event.message).collect()
    }

    fn detached(rig: &Rig) -> Vec<Vec<String>> {
        rig.siblings.calls().into_iter().filter_map(|call| if let SiblingCall::Detached(args) = call { Some(args) } else { None }).collect()
    }

    fn foreground_scripts(rig: &Rig) -> Vec<String> {
        rig.siblings.calls().into_iter().filter_map(|call| if let SiblingCall::Run(args) = call { args.last().cloned() } else { None }).collect()
    }

    #[tokio::test]
    async fn given_a_commit_when_redeployed_then_only_the_detached_swap_is_spawned_at_that_commit_and_tag() {
        // Given a service whose smoke test the caller already ran
        let rig = rig(ran(0, "", ""));

        // When the commit is redeployed at its tag
        let started = rig.redeploy.begin(COMMIT, "sha-1234567");
        let outcome = started.run.expect("a run").finish().await;

        // Then one detached swap carries the tag, and no foreground script (no smoke test) ran
        assert_eq!((started.started, started.reference.as_str()), (true, COMMIT));
        assert_eq!(outcome, Outcome::Swapping { tag: "sha-1234567".into(), commit: COMMIT.into() });
        assert_eq!(foreground_scripts(&rig), Vec::<String>::new());
        let swaps = detached(&rig);
        assert_eq!(swaps.len(), 1);
        assert!(swaps[0].windows(2).any(|pair| pair == ["-e", "IMAGE_TAG=sha-1234567"]), "{:?}", swaps[0]);
        assert_eq!(messages(&rig), ["self_redeploy.swapping"]);
    }

    #[tokio::test]
    async fn given_an_image_that_passes_its_smoke_test_when_it_is_started_then_the_container_is_kept_and_stop_removes_it() {
        // Given a smoke test that passes
        let rig = rig(ran(0, "[smoke] vm-ops-api healthy on port 18999\n", ""));

        // When it is started and then stopped
        let started = rig.redeploy.smoke_start(COMMIT, "sha-1234567").await;
        let stopped = rig.redeploy.smoke_stop().await;

        // Then it passed against that commit, its script never removes the container, nothing was swapped, and the stop removes it
        assert_eq!(started, Ok(()));
        let scripts = foreground_scripts(&rig);
        assert!(scripts[0].contains("--name vm-ops-api-smoke -e PORT=18999 -e CI_PORT=18998 ops-api") && scripts[0].contains(COMMIT) && !scripts[0].contains("docker rm -f vm-ops-api-smoke >/dev/null 2>&1\necho"), "{}", scripts[0]);
        assert!(scripts[1].contains("docker rm -f vm-ops-api-smoke"), "{}", scripts[1]);
        assert_eq!((stopped.code, detached(&rig)), (Some(0), Vec::<Vec<String>>::new()));
    }

    #[tokio::test]
    async fn given_an_image_that_fails_its_smoke_test_when_it_is_started_then_it_fails_with_the_log_and_records_it() {
        // Given a smoke test that fails with the container's log
        let rig = rig(ran(1, "", "[smoke] vm-ops-api did not answer on port 18999; nothing was swapped. Its log:\nthread 'main' panicked: no such file\n"));

        // When it is started
        let started = rig.redeploy.smoke_start(COMMIT, "sha-1234567").await;

        // Then the failure is the smoke test's own output, in the redeploy history where /redeploy/status reads it, and in the event log
        let reason = started.expect_err("a refused image");
        assert!(reason.contains("panicked: no such file"), "{reason}");
        let history = foreground_scripts(&rig).into_iter().filter(|script| script.contains(">> redeploy-history.jsonl")).collect::<Vec<_>>();
        assert_eq!(history.len(), 1);
        assert!(history[0].contains("\"outcome\":\"redeploy.failed\"") && history[0].contains("\"exitCode\":1") && history[0].contains(COMMIT), "{}", history[0]);
        assert_eq!(rig.log.events().last().map(|event| event.message), Some("self_redeploy.smoke_failed"));
    }

    #[tokio::test]
    async fn given_a_smoke_test_that_prints_nothing_when_it_fails_then_the_reason_names_its_exit() {
        let rig = rig(ran(137, "", ""));

        let started = rig.redeploy.smoke_start(COMMIT, "sha-1234567").await;

        assert_eq!(started, Err("the smoke test ended with exit code 137".to_string()));
    }

    #[tokio::test]
    async fn given_a_run_in_flight_when_another_is_requested_then_it_joins_and_after_the_end_a_new_one_starts() {
        // Given a run that has begun but not finished
        let rig = rig(ran(0, "", ""));
        let first = rig.redeploy.begin(COMMIT, "sha-1234567");

        // When two more are requested, one for another commit
        let (same, other) = (rig.redeploy.begin(COMMIT, "sha-1234567"), rig.redeploy.begin("feedface", "sha-feedfac"));

        // Then each joins the one in flight and runs nothing
        assert_eq!([(same.started, same.reference.as_str()), (other.started, other.reference.as_str())], [(false, COMMIT), (false, COMMIT)]);
        assert!(same.run.is_none() && other.run.is_none());
        // And when it has ended, the next request starts its own
        first.run.expect("a run").finish().await;
        let next = rig.redeploy.begin("feedface", "sha-feedfac");
        assert_eq!((next.started, next.reference.as_str()), (true, "feedface"));
    }
}
