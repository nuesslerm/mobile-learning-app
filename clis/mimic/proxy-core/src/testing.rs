//! Doubles of the core's ports.
use crate::application::ports::{Candidate, HttpAnswer};
use crate::application::replay::{Answer, Replayer, Upgrade};
use crate::domain::ordering::{Out, Payload};
use std::collections::VecDeque;

/// A candidate that answers from a cassette: the check of a set against a replay of itself, or of another set, with no socket.
pub struct ReplayCandidate {
    replayer: Replayer,
    session: Option<u64>,
    inbox: VecDeque<Payload>,
    now_ms: u64,
}

impl ReplayCandidate {
    pub fn new(replayer: Replayer) -> ReplayCandidate {
        ReplayCandidate { replayer, session: None, inbox: VecDeque::new(), now_ms: 0 }
    }

    fn take(&mut self, out: Vec<Out>) {
        self.inbox.extend(out.into_iter().filter_map(|o| if let Out::Send(message) = o { Some(message) } else { None }));
    }
}

impl Candidate for ReplayCandidate {
    fn http(&mut self, method: &str, target: &str, headers: &[(String, String)], body: &[u8]) -> Result<HttpAnswer, String> {
        match self.replayer.http(method, target, headers, body) {
            Answer::Reply(reply) => Ok(HttpAnswer { status: reply.status, headers: reply.headers, body: reply.body }),
            Answer::Miss(mismatch) => Err(format!("{}: {}", mismatch.what, mismatch.reason)),
        }
    }

    fn open(&mut self, target: &str) -> Result<u16, String> {
        match self.replayer.upgrade(target, self.now_ms) {
            Upgrade::Accept(session) => {
                self.session = Some(session);
                let due = self.replayer.tick(session, self.now_ms);
                self.take(due);
                Ok(101)
            }
            Upgrade::Refuse(reply) => Ok(reply.status),
            Upgrade::Miss(mismatch) => Err(format!("{}: {}", mismatch.what, mismatch.reason)),
        }
    }

    fn send(&mut self, message: &Payload) -> Result<(), String> {
        let session = self.session.ok_or("no socket is open")?;
        let out = self.replayer.frame(session, message, self.now_ms);
        self.take(out);
        Ok(())
    }

    fn receive(&mut self, wait_ms: u64) -> Result<Option<Payload>, String> {
        if self.inbox.is_empty() {
            self.now_ms += wait_ms;
            if let Some(session) = self.session {
                let due = self.replayer.tick(session, self.now_ms);
                self.take(due);
            }
        }
        Ok(self.inbox.pop_front())
    }

    fn close(&mut self) {
        if let Some(session) = self.session.take() {
            self.replayer.closed(session);
        }
    }
}
