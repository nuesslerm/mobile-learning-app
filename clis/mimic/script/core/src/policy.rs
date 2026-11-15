//! Every wait and limit of a run, in one place. Nothing else in the core knows a duration.

#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    /// The quiet window after a send when the case names none: trailing silence measured from the last frame or the send.
    pub settle_ms: u64,
    /// The granularity of every wait.
    pub poll_ms: u64,
    /// How long a Request frame without `await` waits for the first thing the server answers.
    pub first_answer_ms: u64,
    /// How long `await: "exit"` waits for the Exit of its request; on timeout the script goes on with what it has.
    pub await_exit_ms: u64,
    /// How long a close waits for the peer's close.
    pub close_wait_ms: u64,
    /// A replay-only knob: a candidate that answers from memory caps every quiet window here. Never set when recording a reference.
    pub quiet_cap_ms: Option<u64>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { settle_ms: 250, poll_ms: 25, first_answer_ms: 3000, await_exit_ms: 15000, close_wait_ms: 2000, quiet_cap_ms: None }
    }
}

impl Policy {
    /// The quiet window of a case: its own `settle_ms` or the default, under the cap.
    pub fn quiet(&self, case_settle_ms: Option<u64>) -> u64 {
        let window = case_settle_ms.unwrap_or(self.settle_ms);
        self.quiet_cap_ms.map_or(window, |cap| window.min(cap))
    }
}
