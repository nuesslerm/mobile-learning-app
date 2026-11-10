//! `health.heartbeat` at start and every 10 minutes: without it an idle service writes no rows, which reads at the sink exactly
//! like broken forwarding. The start-up beat is the caller's (`beat`), so it can come before the service says it listens, as it
//! does in Node; `run` is the beats after.
use crate::application::ports::{Clock, Event, EventLog, ProcessMemory};
use std::sync::Arc;
use std::time::Duration;

pub const INTERVAL: Duration = Duration::from_secs(10 * 60);

pub struct Heartbeat {
    host: String,
    started_ms: i64,
    clock: Arc<dyn Clock>,
    memory: Arc<dyn ProcessMemory>,
    log: Arc<dyn EventLog>,
}

impl Heartbeat {
    /// The moment it is made is the moment the process started, as far as `uptimeSec` goes.
    pub fn new(host: String, clock: Arc<dyn Clock>, memory: Arc<dyn ProcessMemory>, log: Arc<dyn EventLog>) -> Self {
        Self { host, started_ms: clock.now_ms(), clock, memory, log }
    }

    pub fn beat(&self) {
        let uptime_sec = ((self.clock.now_ms() - self.started_ms) as f64 / 1000.0).round() as u64;
        self.log.emit(
            Event::info("health.heartbeat").with("host", self.host.as_str()).with("uptimeSec", uptime_sec).with("rssMb", self.memory.rss_mb()).with("ok", true),
        );
    }

    /// Beats every [`INTERVAL`] from now on, for as long as it is polled.
    pub async fn run(&self) {
        loop {
            self.clock.sleep(INTERVAL).await;
            self.beat();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, FakeEventLog, FakeMemory};
    use serde_json::json;

    fn heartbeat(clock: &Arc<FakeClock>, log: &Arc<FakeEventLog>) -> Heartbeat {
        Heartbeat::new("main".into(), clock.clone(), Arc::new(FakeMemory(87)), log.clone())
    }

    #[test]
    fn given_a_process_up_for_90_seconds_when_it_beats_then_the_event_has_the_host_uptime_memory_and_ok_in_order() {
        // Given a heartbeat made at the start of the clock, and a clock 89.6 s on
        let (clock, log) = (Arc::new(FakeClock::at(1_000)), Arc::new(FakeEventLog::default()));
        let heartbeat = heartbeat(&clock, &log);
        clock.set(1_000 + 89_600);
        // When it beats
        heartbeat.beat();
        // Then the event rounds the uptime to whole seconds and carries the memory in megabytes
        let expected = Event::info("health.heartbeat").with("host", "main").with("uptimeSec", 90u64).with("rssMb", 87u64).with("ok", true);
        assert_eq!(log.events(), [expected]);
        assert_eq!(json!(log.events()[0].attributes.iter().map(|(k, _)| *k).collect::<Vec<_>>()), json!(["host", "uptimeSec", "rssMb", "ok"]));
    }

    #[tokio::test]
    async fn given_a_clock_that_lets_two_sleeps_pass_when_it_runs_then_it_beats_after_each_ten_minutes_and_not_at_once() {
        // Given a clock that answers two sleeps at once and then never
        let (clock, log) = (Arc::new(FakeClock::at(0).with_sleeps_allowed(2)), Arc::new(FakeEventLog::default()));
        let heartbeat = heartbeat(&clock, &log);
        // When it runs until the third sleep, which does not return
        tokio::select! {
            () = heartbeat.run() => unreachable!("the heartbeat never ends"),
            () = clock.exhausted() => {}
        }
        // Then it beat twice, each after a ten-minute sleep, and is in its third sleep
        assert_eq!(log.events().len(), 2);
        assert_eq!(clock.sleeps(), [INTERVAL; 3]);
    }
}
