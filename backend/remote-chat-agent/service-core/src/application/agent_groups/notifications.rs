//! The `service.notifications` group (`agent-api-service`): the journal of what happened that someone who was not looking would want to hear
//! about. The service makes an entry when a turn finishes or fails or asks for an answer; each is numbered from 1 within an epoch (a fresh one
//! for each run of the service) and the last `KEEP` are kept. Nothing here is written to disk: an epoch that ends takes its numbers with it, and
//! a caller that still holds the old epoch is answered with everything kept.
use super::activity::ActivityClock;
use agent_api_agent::{FinishReason, ThreadEvent};
use agent_api_proto::{GroupName, Response};
use agent_api_service::notifications::{self as api, cause, NotificationView, SinceParams, SinceResult, SubscribeParams, SubscribeResult};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_group_agent::ThreadEngine;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch;

const SERVED: [Served; 2] = [Served::service(api::SINCE, &[]), Served::service(api::SUBSCRIBE, &[])];

/// The longest a notification's body is, in characters.
const BODY_MAX: usize = 200;

struct Log {
    seq: u64,
    kept: VecDeque<NotificationView>,
    changed: watch::Sender<u64>,
}

pub struct Notifications {
    clock: Arc<dyn ActivityClock>,
    epoch: String,
    keep: usize,
    log: Mutex<Log>,
}

/// What a caller that wants the entries after a `seq` is given.
struct Resumed {
    entries: Vec<NotificationView>,
}

impl Notifications {
    /// `epoch` names this run of the service: it must differ between runs.
    pub fn new(clock: Arc<dyn ActivityClock>, epoch: String) -> Arc<Notifications> {
        Notifications::keeping(clock, epoch, api::KEEP)
    }

    pub fn keeping(clock: Arc<dyn ActivityClock>, epoch: String, keep: usize) -> Arc<Notifications> {
        Arc::new(Notifications { clock, epoch, keep: keep.max(1), log: Mutex::new(Log { seq: 0, kept: VecDeque::new(), changed: watch::channel(0).0 }) })
    }

    fn log(&self) -> MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn epoch(&self) -> &str {
        &self.epoch
    }

    /// Makes an entry and tells the streams that follow the journal.
    pub fn post(&self, cause: &str, title: &str, body: &str, thread: Option<&str>, workspace: Option<&str>) -> NotificationView {
        let at_ms = self.clock.now_ms();
        let mut log = self.log();
        log.seq += 1;
        let seq = log.seq;
        let id = format!("n-{}-{seq}", self.epoch.chars().take(8).collect::<String>());
        let entry = NotificationView { seq, id, cause: cause.into(), title: title.into(), body: body.chars().take(BODY_MAX).collect(), thread_id: thread.map(str::to_string), workspace: workspace.map(str::to_string), at_ms };
        log.kept.push_back(entry.clone());
        if log.kept.len() > self.keep {
            log.kept.pop_front();
        }
        log.changed.send_replace(seq);
        entry
    }

    /// The entries kept after `after_seq`; all of them when `epoch` is another run's, because its `seq` counts something else.
    fn since(&self, after_seq: u64, epoch: Option<&str>) -> Resumed {
        let log = self.log();
        let entries = if epoch.is_some_and(|e| e != self.epoch) { log.kept.iter().cloned().collect() } else { log.kept.iter().filter(|n| n.seq > after_seq).cloned().collect() };
        Resumed { entries }
    }

    /// Where a stream opened with `after_seq` starts, and whether that could not be honoured.
    fn open(&self, after_seq: Option<u64>, epoch: Option<&str>) -> (u64, u64, bool) {
        let log = self.log();
        let oldest = log.kept.front().map_or(log.seq + 1, |n| n.seq);
        match after_seq {
            Some(after) if epoch.is_none_or(|e| e == self.epoch) && after <= log.seq && after + 1 >= oldest => (log.seq, after, false),
            Some(_) => (log.seq, log.seq, true),
            None => (log.seq, log.seq, false),
        }
    }

    fn after(&self, seq: u64) -> (Vec<NotificationView>, bool) {
        let log = self.log();
        let oldest = log.kept.front().map_or(log.seq + 1, |n| n.seq);
        (log.kept.iter().filter(|n| n.seq > seq).cloned().collect(), seq + 1 < oldest)
    }

    /// Makes the entry an event of a thread is news for, if it is: a turn that completed or failed, or one that waits for an answer.
    pub fn hear(&self, engine: &dyn ThreadEngine, thread: &str, event: &ThreadEvent) {
        let (what, body) = match event {
            ThreadEvent::TurnFinished { reason: FinishReason::Completed, .. } => (cause::TURN_COMPLETED, "Finished".to_string()),
            ThreadEvent::TurnFinished { reason: FinishReason::Failed, .. } => (cause::TURN_FAILED, "The turn failed".to_string()),
            ThreadEvent::ApprovalRequested { tool, .. } => (cause::APPROVAL_REQUESTED, format!("Waiting for approval to run {tool}")),
            ThreadEvent::InputRequested { prompt, .. } => (cause::INPUT_REQUESTED, prompt.clone()),
            _ => return,
        };
        let view = engine.get(thread).ok().flatten();
        let title = view.as_ref().map(|v| v.title.as_str()).filter(|t| !t.is_empty()).unwrap_or("Agent");
        self.post(what, title, &body, Some(thread), view.as_ref().and_then(|v| v.workspace.as_deref()));
    }
}

pub struct NotificationsGroup {
    journal: Arc<Notifications>,
}

impl NotificationsGroup {
    pub fn new(journal: Arc<Notifications>) -> NotificationsGroup {
        NotificationsGroup { journal }
    }
}

#[async_trait]
impl Group for NotificationsGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, connection, .. } = call;
        let journal = &self.journal;
        match method {
            api::SINCE => match SinceParams::parse(params) {
                Ok(p) => {
                    let notifications = journal.since(p.after_seq, p.epoch.as_deref()).entries;
                    Response::result(id, serde_json::to_value(SinceResult { epoch: journal.epoch.clone(), notifications }).unwrap_or_default())
                }
                Err(e) => e.response(id),
            },
            api::SUBSCRIBE => match SubscribeParams::parse(params) {
                Ok(p) => {
                    let (seq, from, reset) = journal.open(p.after_seq, p.epoch.as_deref());
                    let (journal_for_stream, sink) = (journal.clone(), connection.sink());
                    let mut changes = journal.log().changed.subscribe();
                    connection.follow(api::TOPIC, async move {
                        let mut after = from;
                        loop {
                            let (entries, lost) = journal_for_stream.after(after);
                            if lost {
                                return;
                            }
                            for entry in &entries {
                                after = entry.seq;
                                if !sink.send_replay(&api::posted_event(entry)).await {
                                    return;
                                }
                            }
                            if entries.is_empty() && changes.changed().await.is_err() {
                                return;
                            }
                        }
                    });
                    Response::result(id, serde_json::to_value(SubscribeResult { topic: api::TOPIC.into(), epoch: journal.epoch.clone(), seq, reset }).unwrap_or_default())
                }
                Err(e) => e.response(id),
            },
            other => unreachable!("the notifications group does not serve {}", other.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActivityClock, Notifications, Resumed, BODY_MAX};
    use agent_api_agent::{FinishReason, ThreadEvent};
    use agent_api_service::notifications::cause;
    use rca_group_agent::testing::MemoryThreads;
    use rca_group_agent::{NewThread, ThreadEngine};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    struct Tick(AtomicU64);

    impl ActivityClock for Tick {
        fn now_ms(&self) -> u64 {
            self.0.fetch_add(1, Ordering::SeqCst)
        }
    }

    fn journal(epoch: &str, keep: usize) -> Arc<Notifications> {
        Notifications::keeping(Arc::new(Tick(AtomicU64::new(100))), epoch.into(), keep)
    }

    #[test]
    fn given_three_entries_when_read_since_a_seq_then_only_the_later_ones_come_and_another_epoch_gets_everything_kept() {
        // Given a journal that kept the last two of three entries
        let j = journal("epoch-aaaa-1", 2);
        for n in 1..=3 {
            j.post(cause::TURN_COMPLETED, "T", &format!("body {n}"), Some("t1"), Some("w1"));
        }
        // When read since 2, since 3, since 0 and since 3 from another epoch
        let seqs = |r: Resumed| r.entries.iter().map(|n| n.seq).collect::<Vec<_>>();
        // Then the later ones come, none past the end, what is kept for 0, and everything kept for another epoch (the seq counts something else)
        assert_eq!(seqs(j.since(2, Some("epoch-aaaa-1"))), [3]);
        assert_eq!(seqs(j.since(3, None)), Vec::<u64>::new());
        assert_eq!(seqs(j.since(0, None)), [2, 3]);
        assert_eq!(seqs(j.since(3, Some("epoch-bbbb-2"))), [2, 3]);
        let last = j.since(0, None).entries.pop().unwrap();
        assert_eq!((last.id.as_str(), last.at_ms, last.thread_id.as_deref(), last.workspace.as_deref()), ("n-epoch-aa-3", 102, Some("t1"), Some("w1")));
    }

    #[test]
    fn given_a_stream_asked_to_resume_when_it_opens_then_it_starts_where_asked_when_it_can_and_from_now_with_reset_when_it_cannot() {
        // Given a journal of three entries with the first gone
        let j = journal("e", 2);
        for _ in 0..3 {
            j.post(cause::TURN_FAILED, "T", "b", None, None);
        }
        // Then no seq starts from now, a seq inside what is kept starts there, and an older one, a later one or another epoch resets
        assert_eq!(j.open(None, None), (3, 3, false));
        assert_eq!(j.open(Some(1), Some("e")), (3, 1, false));
        assert_eq!(j.open(Some(0), None), (3, 3, true));
        assert_eq!(j.open(Some(9), None), (3, 3, true));
        assert_eq!(j.open(Some(2), Some("other")), (3, 3, true));
    }

    #[test]
    fn given_the_events_of_a_turn_when_the_journal_hears_them_then_only_a_finish_a_failure_and_a_wait_are_news_and_each_names_the_thread_and_its_workspace() {
        // Given a thread in a workspace and a journal
        let engine = MemoryThreads::default();
        engine.create(NewThread { id: "t1".into(), title: "Fix the build".into(), workspace: Some("w1".into()), driver: None, model: None }).unwrap();
        let j = Notifications::new(Arc::new(Tick(AtomicU64::new(1))), "epoch-1234".into());
        // When it hears a start, a delta, a finish, a failure, an interrupt, a question and a permission request
        for event in [
            ThreadEvent::TurnStarted {},
            ThreadEvent::message_delta(agent_api_agent::Role::Assistant, "hi".into()),
            ThreadEvent::TurnFinished { reason: FinishReason::Completed, cause: None },
            ThreadEvent::TurnFinished { reason: FinishReason::Failed, cause: None },
            ThreadEvent::TurnFinished { reason: FinishReason::Interrupted, cause: None },
            ThreadEvent::InputRequested { request: "r".into(), prompt: "x".repeat(300), options: vec![] },
            ThreadEvent::ApprovalRequested { approval: "p".into(), tool: "bash".into(), args: serde_json::json!({}) },
        ] {
            j.hear(&engine, "t1", &event);
        }
        // Then four entries were made, in order, each with the thread's title and workspace, and the long prompt is cut
        let kept = j.since(0, None).entries;
        assert_eq!(kept.iter().map(|n| (n.seq, n.cause.as_str())).collect::<Vec<_>>(), [(1, "turn.completed"), (2, "turn.failed"), (3, "input.requested"), (4, "approval.requested")]);
        assert!(kept.iter().all(|n| n.title == "Fix the build" && n.thread_id.as_deref() == Some("t1") && n.workspace.as_deref() == Some("w1")));
        assert_eq!((kept[2].body.chars().count(), kept[3].body.as_str()), (BODY_MAX, "Waiting for approval to run bash"));
        // And a thread the engine does not have is still news, with a title of the journal's own
        j.hear(&engine, "ghost", &ThreadEvent::TurnFinished { reason: FinishReason::Completed, cause: None });
        assert_eq!(j.since(4, None).entries[0].title, "Agent");
    }
}
