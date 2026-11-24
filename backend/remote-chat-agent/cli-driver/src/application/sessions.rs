//! The live sessions of every CLI that lives, behind the one port the service holds: a turn goes to the session of its driver, and the calls that name only a
//! thread reach whichever session has the thread.
use super::claude_sessions::ClaudeSessions;
use super::codex_sessions::CodexSessions;
use super::opencode_sessions::OpencodeSessions;
use super::ports::{Linker, SessionDriver};
use crate::domain::{Driver, ExecEvent, TurnRequest, Verdict};
use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct Sessions {
    pub claude: ClaudeSessions,
    pub codex: CodexSessions,
    pub opencode: OpencodeSessions,
}

impl Sessions {
    /// How long a live process may stay without a turn before it is closed, for each CLI; zero keeps it for ever.
    pub fn set_idle(&self, claude: Duration, codex: Duration, opencode: Duration) {
        self.claude.set_idle(claude);
        self.codex.set_idle(codex);
        self.opencode.set_idle(opencode);
    }

    fn of(&self, driver: &str) -> Result<&dyn SessionDriver, String> {
        match Driver::parse(driver) {
            Some(Driver::ClaudeAgent) => Ok(&self.claude),
            Some(Driver::Codex) => Ok(&self.codex),
            Some(Driver::Opencode) => Ok(&self.opencode),
            None => Err(format!("{driver} has no live session")),
        }
    }
}

#[async_trait]
impl SessionDriver for Sessions {
    async fn start(&self, workspace: &str, linker: &dyn Linker, cwd: &Path) -> Result<(), String> {
        self.codex.start(workspace, linker, cwd).await
    }

    async fn turn(&self, workspace: &str, linker: &dyn Linker, req: &TurnRequest) -> Result<mpsc::Receiver<ExecEvent>, String> {
        self.of(&req.instance_id)?.turn(workspace, linker, req).await
    }

    async fn steer(&self, thread: &str, prompt: &str) -> Result<(), String> {
        if self.claude.alive(thread) {
            return self.claude.steer(thread, prompt).await;
        }
        if self.opencode.alive(thread) {
            return self.opencode.steer(thread, prompt).await;
        }
        self.codex.steer(thread, prompt).await
    }

    fn interrupt(&self, thread: &str) -> bool {
        self.claude.interrupt(thread) | self.codex.interrupt(thread) | self.opencode.interrupt(thread)
    }

    fn answer(&self, thread: &str, approval: &str, verdict: Verdict) -> bool {
        self.claude.answer(thread, approval, verdict) || self.codex.answer(thread, approval, verdict) || self.opencode.answer(thread, approval, verdict)
    }

    fn answer_input(&self, thread: &str, request: &str, answer: Option<String>) -> bool {
        self.claude.answer_input(thread, request, answer.clone()) || self.opencode.answer_input(thread, request, answer)
    }

    fn alive(&self, thread: &str) -> bool {
        self.claude.alive(thread) || self.codex.alive(thread) || self.opencode.alive(thread)
    }

    fn forget(&self, thread: &str) {
        self.claude.forget(thread);
        self.codex.forget(thread);
        self.opencode.forget(thread);
    }

    fn close(&self, workspace: &str) -> bool {
        self.claude.close(workspace) | self.codex.close(workspace) | self.opencode.close(workspace)
    }
}
