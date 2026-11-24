//! The live conversation processes the executor keeps for `session_open`: the input of each, by the id the caller named it with, so
//! `session_send` can write to it while the open action streams its output.
use crate::adapters::opencode_bridge::Bridge;
use exec_proc::{SESSIONS_MAX, SESSION_LINE_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use std::collections::HashMap;
use std::io::Write;
use std::process::ChildStdin;
use std::sync::{Arc, Mutex, MutexGuard};

enum Slot {
    /// The id is taken and the process is starting.
    Reserved,
    Open(Arc<Mutex<ChildStdin>>),
    /// An opencode server: a line is an HTTP request for it.
    Http(Arc<Bridge>),
}

#[derive(Default)]
pub struct Sessions {
    open: Mutex<HashMap<String, Slot>>,
}

impl Sessions {
    fn locked(&self) -> MutexGuard<'_, HashMap<String, Slot>> {
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes the id for a session about to start; refused when it is in use or the executor keeps too many.
    pub fn reserve(&self, session: &str) -> ExecResult<()> {
        let mut open = self.locked();
        if open.contains_key(session) {
            return Err(ExecError::invalid_args(format!("session {session} is open already")));
        }
        if open.len() >= SESSIONS_MAX {
            return Err(ExecError::invalid_args(format!("{SESSIONS_MAX} sessions are open here already")));
        }
        open.insert(session.to_string(), Slot::Reserved);
        Ok(())
    }

    pub fn attach(&self, session: &str, input: ChildStdin) {
        self.locked().insert(session.to_string(), Slot::Open(Arc::new(Mutex::new(input))));
    }

    pub fn attach_http(&self, session: &str, bridge: Arc<Bridge>) {
        self.locked().insert(session.to_string(), Slot::Http(bridge));
    }

    pub fn release(&self, session: &str) {
        self.locked().remove(session);
    }

    /// Writes `line` and its newline to the session's input; the bytes written.
    pub fn send(&self, session: &str, line: &str) -> ExecResult<usize> {
        let opened = match self.locked().get(session) {
            Some(Slot::Open(input)) => Ok(Arc::clone(input)),
            Some(Slot::Http(bridge)) => Err(Arc::clone(bridge)),
            _ => return Err(ExecError::new(Code::NotFound, format!("no session {session} is open here"))),
        };
        if line.len() > SESSION_LINE_MAX {
            return Err(ExecError::invalid_args("the line is too long"));
        }
        let input = match opened {
            Ok(input) => input,
            Err(bridge) => return bridge.request(line).map(|()| line.len() + 1),
        };
        let mut pipe = input.lock().unwrap_or_else(|e| e.into_inner());
        pipe.write_all(line.as_bytes()).and_then(|()| pipe.write_all(b"\n")).and_then(|()| pipe.flush()).map_err(|e| ExecError::new(Code::Io, format!("session {session}: {e}")))?;
        Ok(line.len() + 1)
    }
}
