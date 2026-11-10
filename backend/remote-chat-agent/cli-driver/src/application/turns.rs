//! The turns in flight: a turn is a provider process whose output lines are translated into neutral events. A thread has one running turn
//! at a time, and an interrupt kills its process.
use super::ports::Spawner;
use crate::domain::{session_of, translate, Command, Driver, ExecEvent};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

type Kill = Arc<dyn Fn() + Send + Sync>;

pub struct Turns {
    spawner: Arc<dyn Spawner>,
    running: Arc<Mutex<HashMap<String, Kill>>>,
}

impl Turns {
    pub fn new(spawner: Arc<dyn Spawner>) -> Turns {
        Turns { spawner, running: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Starts `command` for `thread` in `cwd`. `Err` is a refusal before anything ran (busy thread, program not startable); after that the
    /// events arrive on the channel and the last one is `Done` or `Error`.
    pub async fn launch(&self, driver: Driver, thread: &str, command: Command, cwd: &Path) -> Result<mpsc::Receiver<ExecEvent>, String> {
        if self.running.lock().unwrap_or_else(|p| p.into_inner()).contains_key(thread) {
            return Err(format!("thread {thread} already has a running turn"));
        }
        let mut process = self.spawner.spawn(&command.program, &command.args, cwd).await.map_err(|e| format!("{} could not start: {e}", command.program))?;
        self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(thread.to_string(), process.kill.clone());
        let (tx, rx) = mpsc::channel(64);
        let (running, thread, stopped) = (self.running.clone(), thread.to_string(), process.kill.clone());
        tokio::spawn(async move {
            let mut failed: Option<String> = None;
            while let Some(line) = process.lines.next().await {
                let session = session_of(driver, &line).map(ExecEvent::Session);
                for event in session.into_iter().chain(translate(driver, &line)) {
                    if let ExecEvent::Error(m) = &event {
                        failed.get_or_insert_with(|| m.clone());
                    }
                    if tx.send(event).await.is_err() {
                        stopped();
                        break;
                    }
                }
            }
            let ended = process.finish.await;
            let interrupted = running.lock().unwrap_or_else(|p| p.into_inner()).remove(&thread).is_none();
            let last = match (failed, ended, interrupted) {
                (_, _, true) | (None, Ok(()), _) => ExecEvent::Done,
                (Some(_), _, _) => ExecEvent::Done,
                (None, Err(why), _) => ExecEvent::Error(why),
            };
            let _ = tx.send(last).await;
        });
        Ok(rx)
    }

    /// Kills the thread's running turn; whether there was one.
    pub fn interrupt(&self, thread: &str) -> bool {
        let kill = self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(thread);
        kill.map(|k| k()).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{command_for, TurnRequest};
    use crate::testing::{CliScript, ScriptedSpawner};

    fn request(driver: Driver, thread: &str) -> (Command, TurnRequest) {
        let req = TurnRequest { thread_id: thread.into(), run_id: "r".into(), instance_id: driver.id().into(), model: None, cwd: "/w".into(), prompt: "go".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None };
        (command_for(driver, &req), req)
    }

    async fn drain(mut events: mpsc::Receiver<ExecEvent>) -> Vec<ExecEvent> {
        let mut all = Vec::new();
        while let Some(event) = events.recv().await {
            all.push(event);
        }
        all
    }

    fn turns(spawner: &Arc<ScriptedSpawner>) -> Turns {
        Turns::new(spawner.clone())
    }

    #[tokio::test]
    async fn given_a_cli_that_prints_lines_when_a_turn_runs_then_each_driver_s_lines_arrive_as_neutral_events_ending_with_done() {
        // Given each driver's CLI printing one line of its own dialect
        let lines = [
            (Driver::ClaudeAgent, r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#),
            (Driver::Codex, r#"{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}"#),
            (Driver::Opencode, r#"{"type":"text","part":{"text":"hello"}}"#),
        ];
        for (driver, line) in lines {
            let spawner = ScriptedSpawner::new();
            spawner.script(driver.program(), CliScript { lines: vec![line.into(), "not json".into()], ..CliScript::default() });
            let (command, req) = request(driver, "t1");
            // When the turn is launched in a directory
            let events = turns(&spawner).launch(driver, &req.thread_id, command, Path::new("/w")).await.unwrap();
            // Then the CLI was started there, and the client got the text and then done (the line that is no JSON says nothing)
            assert_eq!(drain(events).await, vec![ExecEvent::Text("hello".into()), ExecEvent::Done], "{}", driver.id());
            assert_eq!(spawner.spawned()[0].cwd, Path::new("/w"), "{}", driver.id());
        }
    }

    #[tokio::test]
    async fn given_a_claude_cli_that_announces_its_conversation_when_a_turn_runs_then_the_session_is_the_first_event_and_the_other_drivers_have_none() {
        // Given Claude Code printing its init line and a text, and opencode printing a line that carries a session_id
        let spawner = ScriptedSpawner::new();
        spawner.script("claude", CliScript { lines: vec![r#"{"type":"system","subtype":"init","session_id":"s-9"}"#.into(), r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#.into()], ..CliScript::default() });
        spawner.script("opencode", CliScript { lines: vec![r#"{"type":"system","subtype":"init","session_id":"s-9"}"#.into()], ..CliScript::default() });
        let turns = turns(&spawner);
        // When each turn runs
        let (command, _) = request(Driver::ClaudeAgent, "a");
        let claude = drain(turns.launch(Driver::ClaudeAgent, "a", command, Path::new("/w")).await.unwrap()).await;
        let (command, _) = request(Driver::Opencode, "b");
        let opencode = drain(turns.launch(Driver::Opencode, "b", command, Path::new("/w")).await.unwrap()).await;
        // Then Claude Code's events open with its session and opencode's have none
        assert_eq!(claude, vec![ExecEvent::Session("s-9".into()), ExecEvent::Text("hi".into()), ExecEvent::Done]);
        assert_eq!(opencode, vec![ExecEvent::Done]);
    }

    #[tokio::test]
    async fn given_a_cli_that_cannot_start_or_fails_when_a_turn_runs_then_the_refusal_or_the_error_is_what_the_client_hears() {
        // Given a CLI that is not installed, one that exits with a failure, and one that reports its own error line before exiting badly
        let spawner = ScriptedSpawner::new();
        spawner.script("opencode", CliScript { not_started: Some("not found".into()), ..CliScript::default() });
        spawner.script("codex", CliScript { exit: Some("exited with 1: not logged in".into()), ..CliScript::default() });
        spawner.script("claude", CliScript { lines: vec![r#"{"type":"result","is_error":true,"result":"rate limited"}"#.into()], exit: Some("exited with 1".into()), ..CliScript::default() });
        let turns = turns(&spawner);
        // When each turn is launched
        let (command, _) = request(Driver::Opencode, "a");
        let refused = turns.launch(Driver::Opencode, "a", command, Path::new("/w")).await.err();
        let (command, _) = request(Driver::Codex, "b");
        let failed = drain(turns.launch(Driver::Codex, "b", command, Path::new("/w")).await.unwrap()).await;
        let (command, _) = request(Driver::ClaudeAgent, "c");
        let reported = drain(turns.launch(Driver::ClaudeAgent, "c", command, Path::new("/w")).await.unwrap()).await;
        // Then the first is refused before anything ran, the second ends in the stderr tail, and the third ends with the error it reported
        assert_eq!(refused.as_deref(), Some("opencode could not start: not found"));
        assert_eq!(failed, vec![ExecEvent::Error("exited with 1: not logged in".into())]);
        assert_eq!(reported, vec![ExecEvent::Error("rate limited".into()), ExecEvent::Done]);
    }

    #[tokio::test]
    async fn given_a_running_turn_when_the_thread_starts_another_then_it_is_refused_and_an_interrupt_kills_the_first_and_frees_the_thread() {
        // Given a CLI that stays up after its first line
        let spawner = ScriptedSpawner::new();
        spawner.script("claude", CliScript { lines: vec![r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#.into()], hold: true, ..CliScript::default() });
        let turns = turns(&spawner);
        let (command, _) = request(Driver::ClaudeAgent, "t");
        let mut first = turns.launch(Driver::ClaudeAgent, "t", command.clone(), Path::new("/w")).await.unwrap();
        assert_eq!(first.recv().await, Some(ExecEvent::Text("working".into())));
        // When the same thread starts a second turn, and another thread starts one
        let again = turns.launch(Driver::ClaudeAgent, "t", command.clone(), Path::new("/w")).await.err();
        let mut elsewhere = turns.launch(Driver::ClaudeAgent, "other", command.clone(), Path::new("/w")).await.unwrap();
        // Then the second is refused, the other thread runs, and a thread that has no turn has nothing to interrupt
        assert_eq!(again.as_deref(), Some("thread t already has a running turn"));
        assert_eq!(elsewhere.recv().await, Some(ExecEvent::Text("working".into())));
        assert!(!turns.interrupt("nobody"));
        // And interrupting the first kills its process, ends its events with done, and lets the thread run again
        assert!(turns.interrupt("t"));
        assert_eq!(drain(first).await, vec![ExecEvent::Done]);
        assert_eq!(spawner.kills(), 1);
        assert!(!turns.interrupt("t"));
        assert!(turns.launch(Driver::ClaudeAgent, "t", command, Path::new("/w")).await.is_ok());
    }

    #[tokio::test]
    async fn given_a_client_that_stops_listening_when_the_turn_goes_on_then_the_process_is_killed() {
        // Given a CLI printing many lines and held up, and a client that drops its end after the first event
        let spawner = ScriptedSpawner::new();
        let line = r#"{"type":"text","part":{"text":"x"}}"#;
        spawner.script("opencode", CliScript { lines: vec![line.into(); 200], hold: true, ..CliScript::default() });
        let (command, _) = request(Driver::Opencode, "t");
        let mut events = turns(&spawner).launch(Driver::Opencode, "t", command, Path::new("/w")).await.unwrap();
        assert!(events.recv().await.is_some());
        // When the client goes away
        drop(events);
        // Then the process is killed (once per line it still prints, which the adapter's kill takes as one)
        for _ in 0..200 {
            if spawner.kills() >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(spawner.kills() >= 1);
    }
}
