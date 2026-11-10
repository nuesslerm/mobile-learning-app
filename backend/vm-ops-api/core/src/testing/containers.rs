//! A double of the container engine: containers, exec answers, logs, follow streams and stats are told to it, and every call is kept.
use crate::application::ports::{Containers, ContainersError, EngineReply, ExecOptions, ExecResult, LogFollow};
use crate::domain::container_stats::DockerStats;
use crate::domain::containers::DockerContainer;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One call the service made, with what it passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainersCall {
    ListByApp(String),
    ListRunning,
    Exec { container_id: String, cmd: Vec<String>, options: ExecOptions },
    PutFile { container_id: String, target_dir: String, tar: Vec<u8> },
    Logs { container_id: String, tail: u32 },
    FollowLogs { container_id: String, tail: u32 },
    Stats { container_id: String },
}

/// What a follow stream does once its scripted texts are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowEnd {
    /// The Engine ends the stream, as it does when the container stops.
    Ends,
    /// No more text ever arrives and the stream never ends.
    Hangs,
}

type ExecAnswer = Result<ExecResult, ContainersError>;

struct ExecRule {
    prefix: Vec<String>,
    answers: Vec<ExecAnswer>,
    asked: usize,
}

#[derive(Clone)]
struct FollowScript {
    texts: Vec<Result<String, ContainersError>>,
    end: FollowEnd,
}

#[derive(Default)]
struct State {
    by_app: HashMap<String, Vec<DockerContainer>>,
    running: Vec<DockerContainer>,
    listing_error: Option<ContainersError>,
    execs: Vec<ExecRule>,
    put_replies: VecDeque<Result<EngineReply, ContainersError>>,
    logs: HashMap<String, Result<String, ContainersError>>,
    follows: HashMap<String, Result<FollowScript, ContainersError>>,
    stats: HashMap<String, Result<DockerStats, ContainersError>>,
    calls: Vec<ContainersCall>,
}

/// Answers from what it is told and nothing else: a call it has no answer for fails with a message that says so. An exec rule
/// matches the commands that start with its words, answers in turn and repeats its last answer; the file put answers 200 unless told.
#[derive(Default)]
pub struct FakeContainers {
    state: Mutex<State>,
    followers_dropped: Arc<AtomicUsize>,
}

impl FakeContainers {
    pub fn new() -> Self {
        Self::default()
    }

    /// `list_by_app` of this app returns these, whatever their state.
    pub fn with_app_containers(self, app: &str, containers: Vec<DockerContainer>) -> Self {
        lock(&self.state).by_app.insert(app.to_string(), containers);
        self
    }

    pub fn with_running(self, containers: Vec<DockerContainer>) -> Self {
        lock(&self.state).running = containers;
        self
    }

    /// Both lists fail with this.
    pub fn with_listing_error(self, error: ContainersError) -> Self {
        lock(&self.state).listing_error = Some(error);
        self
    }

    pub fn with_exec(self, prefix: &[&str], answers: Vec<ExecAnswer>) -> Self {
        let prefix = prefix.iter().map(|word| (*word).to_string()).collect();
        lock(&self.state).execs.push(ExecRule { prefix, answers, asked: 0 });
        self
    }

    /// The replies of the puts, in turn; the last one repeats.
    pub fn with_put_replies(self, replies: Vec<Result<EngineReply, ContainersError>>) -> Self {
        lock(&self.state).put_replies = replies.into();
        self
    }

    pub fn with_logs(self, container_id: &str, logs: Result<String, ContainersError>) -> Self {
        lock(&self.state).logs.insert(container_id.to_string(), logs);
        self
    }

    /// The follow of this container yields `texts` in order, then does what `end` says.
    pub fn with_follow(self, container_id: &str, texts: Vec<Result<String, ContainersError>>, end: FollowEnd) -> Self {
        lock(&self.state).follows.insert(container_id.to_string(), Ok(FollowScript { texts, end }));
        self
    }

    /// The follow of this container is refused when it is opened.
    pub fn with_follow_error(self, container_id: &str, error: ContainersError) -> Self {
        lock(&self.state).follows.insert(container_id.to_string(), Err(error));
        self
    }

    pub fn with_stats(self, container_id: &str, stats: Result<DockerStats, ContainersError>) -> Self {
        lock(&self.state).stats.insert(container_id.to_string(), stats);
        self
    }

    pub fn calls(&self) -> Vec<ContainersCall> {
        lock(&self.state).calls.clone()
    }

    /// How many follow streams have been dropped, which is how a caller ends one.
    pub fn followers_dropped(&self) -> usize {
        self.followers_dropped.load(Ordering::SeqCst)
    }

    fn record(&self, call: ContainersCall) {
        lock(&self.state).calls.push(call);
    }
}

fn unscripted<T>(what: &str) -> Result<T, ContainersError> {
    Err(ContainersError::failed(&format!("the fake has no answer for {what}")))
}

#[async_trait]
impl Containers for FakeContainers {
    async fn list_by_app(&self, app: &str) -> Result<Vec<DockerContainer>, ContainersError> {
        self.record(ContainersCall::ListByApp(app.to_string()));
        let state = lock(&self.state);
        match &state.listing_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.by_app.get(app).cloned().unwrap_or_default()),
        }
    }

    async fn list_running(&self) -> Result<Vec<DockerContainer>, ContainersError> {
        self.record(ContainersCall::ListRunning);
        let state = lock(&self.state);
        match &state.listing_error {
            Some(error) => Err(error.clone()),
            None => Ok(state.running.clone()),
        }
    }

    async fn exec(&self, container_id: &str, cmd: &[String], options: &ExecOptions) -> Result<ExecResult, ContainersError> {
        let mut state = lock(&self.state);
        state.calls.push(ContainersCall::Exec { container_id: container_id.to_string(), cmd: cmd.to_vec(), options: options.clone() });
        match state.execs.iter_mut().find(|rule| cmd.starts_with(&rule.prefix)) {
            Some(rule) => {
                let answer = rule.answers.get(rule.asked.min(rule.answers.len().saturating_sub(1))).cloned();
                rule.asked += 1;
                answer.unwrap_or_else(|| unscripted("an exec rule with no answers"))
            }
            None => unscripted(&format!("exec {cmd:?}")),
        }
    }

    async fn put_file(&self, container_id: &str, target_dir: &str, tar: &[u8]) -> Result<EngineReply, ContainersError> {
        let mut state = lock(&self.state);
        state.calls.push(ContainersCall::PutFile { container_id: container_id.to_string(), target_dir: target_dir.to_string(), tar: tar.to_vec() });
        match state.put_replies.len() {
            0 => Ok(EngineReply { status: 200, body: String::new() }),
            1 => state.put_replies[0].clone(),
            _ => state.put_replies.pop_front().unwrap_or_else(|| unscripted("a put")),
        }
    }

    async fn logs(&self, container_id: &str, tail: u32) -> Result<String, ContainersError> {
        self.record(ContainersCall::Logs { container_id: container_id.to_string(), tail });
        lock(&self.state).logs.get(container_id).cloned().unwrap_or_else(|| unscripted(&format!("the logs of {container_id}")))
    }

    async fn follow_logs(&self, container_id: &str, tail: u32) -> Result<Box<dyn LogFollow>, ContainersError> {
        self.record(ContainersCall::FollowLogs { container_id: container_id.to_string(), tail });
        let script = lock(&self.state).follows.get(container_id).cloned();
        match script {
            Some(Ok(FollowScript { texts, end })) => {
                Ok(Box::new(FakeFollow { texts: texts.into(), end, dropped: self.followers_dropped.clone() }))
            }
            Some(Err(error)) => Err(error),
            None => unscripted(&format!("the follow of {container_id}")),
        }
    }

    async fn stats(&self, container_id: &str) -> Result<DockerStats, ContainersError> {
        self.record(ContainersCall::Stats { container_id: container_id.to_string() });
        lock(&self.state).stats.get(container_id).cloned().unwrap_or_else(|| unscripted(&format!("the stats of {container_id}")))
    }
}

struct FakeFollow {
    texts: VecDeque<Result<String, ContainersError>>,
    end: FollowEnd,
    dropped: Arc<AtomicUsize>,
}

#[async_trait]
impl LogFollow for FakeFollow {
    async fn next_text(&mut self) -> Result<Option<String>, ContainersError> {
        match (self.texts.pop_front(), self.end) {
            (Some(text), _) => text.map(Some),
            (None, FollowEnd::Ends) => Ok(None),
            (None, FollowEnd::Hangs) => std::future::pending().await,
        }
    }
}

impl Drop for FakeFollow {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    fn words(cmd: &[&str]) -> Vec<String> {
        cmd.iter().map(|word| (*word).to_string()).collect()
    }

    fn container(id: &str) -> DockerContainer {
        DockerContainer { id: id.to_string(), ..Default::default() }
    }

    fn ran(code: i64, output: &str) -> ExecAnswer {
        Ok(ExecResult::attached(Some(code), output.to_string()))
    }

    #[tokio::test]
    async fn given_told_containers_when_listed_then_each_list_answers_what_it_was_told_and_each_call_is_kept() {
        let fake = FakeContainers::new().with_app_containers("agent-shell", vec![container("a1")]).with_running(vec![container("r1"), container("r2")]);

        let by_app = fake.list_by_app("agent-shell").await;
        let other = fake.list_by_app("reporting-service").await;
        let running = fake.list_running().await;

        assert_eq!(by_app.map(|c| c.len()), Ok(1));
        assert_eq!(other, Ok(Vec::new()));
        assert_eq!(running.map(|c| c.len()), Ok(2));
        let wanted = [ContainersCall::ListByApp("agent-shell".into()), ContainersCall::ListByApp("reporting-service".into()), ContainersCall::ListRunning];
        assert_eq!(fake.calls(), wanted);
    }

    #[tokio::test]
    async fn given_a_listing_error_when_either_list_is_read_then_both_fail_with_it() {
        let error = ContainersError::refused("docker API", 500, "down", 200);
        let fake = FakeContainers::new().with_app_containers("agent-shell", vec![container("a1")]).with_listing_error(error.clone());

        assert_eq!(fake.list_by_app("agent-shell").await, Err(error.clone()));
        assert_eq!(fake.list_running().await, Err(error));
    }

    #[tokio::test]
    async fn given_exec_rules_when_commands_run_then_the_first_rule_that_starts_the_command_answers_in_turn_and_its_last_answer_repeats() {
        let fake = FakeContainers::new()
            .with_exec(&["cat", "/a"], vec![ran(0, "first"), ran(1, "second")])
            .with_exec(&["cat"], vec![ran(2, "any cat")]);
        let options = ExecOptions { detach: false, env: vec!["A=1".into()] };

        let first = fake.exec("c1", &words(&["cat", "/a"]), &options).await;
        let second = fake.exec("c1", &words(&["cat", "/a", "more"]), &options).await;
        let third = fake.exec("c1", &words(&["cat", "/a"]), &options).await;
        let other = fake.exec("c1", &words(&["cat", "/b"]), &options).await;
        let none = fake.exec("c1", &words(&["ls"]), &options).await;

        assert_eq!((first, second, third), (ran(0, "first"), ran(1, "second"), ran(1, "second")));
        assert_eq!(other, ran(2, "any cat"));
        assert!(none.is_err_and(|e| e.to_string().contains("no answer for exec")));
        let kept = fake.calls();
        assert_eq!(kept[0], ContainersCall::Exec { container_id: "c1".into(), cmd: words(&["cat", "/a"]), options });
    }

    #[tokio::test]
    async fn given_puts_when_files_are_put_then_the_default_is_200_and_told_replies_come_in_turn_with_the_last_repeating() {
        let plain = FakeContainers::new();
        let told = FakeContainers::new().with_put_replies(vec![
            Ok(EngineReply { status: 500, body: "full".into() }),
            Ok(EngineReply { status: 200, body: String::new() }),
        ]);

        let default = plain.put_file("c1", "/tmp/x", b"tar").await;
        let replies = [told.put_file("c1", "/d", b"1").await, told.put_file("c1", "/d", b"2").await, told.put_file("c1", "/d", b"3").await];

        assert_eq!(default, Ok(EngineReply { status: 200, body: String::new() }));
        assert_eq!(replies.map(|r| r.map(|r| r.status)), [Ok(500), Ok(200), Ok(200)]);
        let put = ContainersCall::PutFile { container_id: "c1".into(), target_dir: "/tmp/x".into(), tar: b"tar".to_vec() };
        assert_eq!(plain.calls(), [put]);
    }

    #[tokio::test]
    async fn given_logs_and_stats_when_read_then_they_answer_what_was_told_and_an_unknown_container_fails_saying_so() {
        let fake = FakeContainers::new().with_logs("c1", Ok("text".into())).with_stats("c1", Ok(DockerStats::default()));

        let logs = fake.logs("c1", 200).await;
        let stats = fake.stats("c1").await;
        let missing = fake.logs("c2", 5).await;

        assert_eq!(logs, Ok("text".to_string()));
        assert!(stats.is_ok());
        assert!(missing.is_err_and(|e| e.to_string().contains("the logs of c2")));
        assert_eq!(fake.calls()[0], ContainersCall::Logs { container_id: "c1".into(), tail: 200 });
    }

    #[tokio::test]
    async fn given_a_scripted_follow_when_read_then_the_texts_come_in_order_and_the_stream_ends_and_dropping_it_is_counted() {
        let texts = vec![Ok("one".to_string()), Err(ContainersError::failed("aborted")), Ok("three".to_string())];
        let fake = FakeContainers::new().with_follow("c1", texts, FollowEnd::Ends);

        let mut follow = fake.follow_logs("c1", 5).await.expect("a scripted follow");
        let read = [follow.next_text().await, follow.next_text().await, follow.next_text().await, follow.next_text().await];
        assert_eq!(fake.followers_dropped(), 0);
        drop(follow);

        let aborted = Err(ContainersError::failed("aborted"));
        assert_eq!(read, [Ok(Some("one".to_string())), aborted, Ok(Some("three".to_string())), Ok(None)]);
        assert_eq!(fake.followers_dropped(), 1);
        assert_eq!(fake.calls(), [ContainersCall::FollowLogs { container_id: "c1".into(), tail: 5 }]);
    }

    #[tokio::test]
    async fn given_a_follow_that_hangs_when_its_texts_are_read_then_the_next_read_never_completes() {
        let fake = FakeContainers::new().with_follow("c1", vec![Ok("only".to_string())], FollowEnd::Hangs);
        let mut follow = fake.follow_logs("c1", 1).await.expect("a scripted follow");
        assert_eq!(follow.next_text().await, Ok(Some("only".to_string())));

        let mut next = pin!(follow.next_text());
        let polled = next.as_mut().poll(&mut Context::from_waker(Waker::noop()));

        assert!(matches!(polled, Poll::Pending));
    }

    #[test]
    fn given_a_refusal_with_a_long_body_when_its_message_is_made_then_it_is_the_lead_and_the_first_units_of_the_body_cut_through_a_pair() {
        let body = format!("{}\u{1f600}tail", "x".repeat(4));

        let error = ContainersError::refused("docker API", 502, &body, 5);

        assert_eq!(error.message().to_json(), r#""docker API returned 502: xxxx\ud83d""#);
        assert_eq!(error.to_string(), "docker API returned 502: xxxx\u{fffd}");
        assert!(matches!(error, ContainersError::Refused { status: 502, .. }));
        assert_eq!(ContainersError::failed("socket hang up").to_string(), "socket hang up");
    }

    #[tokio::test]
    async fn given_a_refused_follow_when_opened_then_it_is_the_error_and_no_stream_exists() {
        let refused = ContainersError::refused("docker API", 500, "logs failed", 300);
        let fake = FakeContainers::new().with_follow_error("c1", refused.clone());

        let opened = fake.follow_logs("c1", 5).await;
        let unknown = fake.follow_logs("c9", 5).await;

        assert_eq!(opened.err(), Some(refused));
        assert!(unknown.is_err());
        assert_eq!(fake.followers_dropped(), 0);
    }
}
