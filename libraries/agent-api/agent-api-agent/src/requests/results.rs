//! What the methods of `agent.requests` answer. `seq` is that of the event the call recorded, the `after_seq` to resume from.
use super::params::Decision;
use serde::{Deserialize, Serialize};

/// The result of `approval.answer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalAnswerResult {
    pub thread: String,
    pub approval: String,
    pub decision: Decision,
    pub seq: u64,
}

/// The result of `input.answer` and `input.dismiss`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerResult {
    pub thread: String,
    pub request: String,
    pub seq: u64,
}
