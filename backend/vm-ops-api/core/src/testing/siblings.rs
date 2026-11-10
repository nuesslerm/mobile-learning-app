//! A double of the sibling runner: answers are told to it by what an argument contains, and every call is kept.
use crate::application::ports::{SiblingError, SiblingResult, Siblings};
use async_trait::async_trait;
use std::sync::{Mutex, MutexGuard};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One call the service made, with the arguments it passed to `docker`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiblingCall {
    Run(Vec<String>),
    Detached(Vec<String>),
}

struct RunRule {
    contains: String,
    answers: Vec<SiblingResult>,
    asked: usize,
}

#[derive(Default)]
struct State {
    rules: Vec<RunRule>,
    spawn_error: Option<SiblingError>,
    calls: Vec<SiblingCall>,
}

/// A run is answered by the first rule one of whose arguments contains the rule's text; a rule answers in turn and repeats its
/// last answer. A run no rule matches is a command that could not be started, saying so.
#[derive(Default)]
pub struct FakeSiblings {
    state: Mutex<State>,
}

impl FakeSiblings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs with an argument containing `contains` are answered with `answers`; an empty text matches every run.
    pub fn with_run(self, contains: &str, answers: Vec<SiblingResult>) -> Self {
        lock(&self.state).rules.push(RunRule { contains: contains.to_string(), answers, asked: 0 });
        self
    }

    /// Every detached start fails with this message.
    pub fn with_spawn_error(self, message: &str) -> Self {
        lock(&self.state).spawn_error = Some(SiblingError(message.to_string()));
        self
    }

    pub fn calls(&self) -> Vec<SiblingCall> {
        lock(&self.state).calls.clone()
    }
}

#[async_trait]
impl Siblings for FakeSiblings {
    async fn run(&self, args: &[String]) -> SiblingResult {
        let mut state = lock(&self.state);
        state.calls.push(SiblingCall::Run(args.to_vec()));
        let rule = state.rules.iter_mut().find(|rule| args.iter().any(|arg| arg.contains(&rule.contains)));
        match rule {
            Some(rule) if !rule.answers.is_empty() => {
                let answer = rule.answers[rule.asked.min(rule.answers.len() - 1)].clone();
                rule.asked += 1;
                answer
            }
            _ => SiblingResult::not_started(&format!("the fake has no answer for docker {}", args.join(" "))),
        }
    }

    fn spawn_detached(&self, args: &[String]) -> Result<(), SiblingError> {
        let mut state = lock(&self.state);
        state.calls.push(SiblingCall::Detached(args.to_vec()));
        state.spawn_error.clone().map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|word| (*word).to_string()).collect()
    }

    #[tokio::test]
    async fn given_the_fake_siblings_when_run_then_the_first_matching_rule_answers_in_turn_and_repeats_its_last() {
        let siblings = FakeSiblings::new()
            .with_run("history", vec![SiblingResult::exited(0, "one", ""), SiblingResult::exited(1, "two", "bad")])
            .with_run("", vec![SiblingResult::exited(0, "any", "")]);

        let first = siblings.run(&words(&["run", "sh -c history append"])).await;
        let second = siblings.run(&words(&["run", "history"])).await;
        let third = siblings.run(&words(&["run", "history"])).await;
        let other = siblings.run(&words(&["ps"])).await;

        assert_eq!((first.stdout.as_str(), second.code, third.stderr.as_str(), other.stdout.as_str()), ("one", Some(1), "bad", "any"));
    }

    #[tokio::test]
    async fn given_the_fake_siblings_when_nothing_is_scripted_or_a_start_fails_then_it_says_so_and_every_call_is_kept() {
        let siblings = FakeSiblings::new().with_spawn_error("spawn docker ENOENT");

        let unscripted = siblings.run(&words(&["compose", "up"])).await;
        let started = siblings.spawn_detached(&words(&["run", "-d"]));

        assert_eq!(unscripted, SiblingResult::not_started("the fake has no answer for docker compose up"));
        assert_eq!(started, Err(SiblingError("spawn docker ENOENT".to_string())));
        assert_eq!(siblings.calls(), [SiblingCall::Run(words(&["compose", "up"])), SiblingCall::Detached(words(&["run", "-d"]))]);
        assert_eq!(FakeSiblings::new().spawn_detached(&words(&["x"])), Ok(()));
    }
}
