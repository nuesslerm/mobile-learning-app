//! The `agent.lifecycle` sub-group (plan M4): what happens to a thread over its life, beyond its turns. Launch, archive, delete, fork and merge
//! back; settle and snooze (one method, `thread.settle`, whose `state` is `settled`, `active`, `snoozed` or `awake`); search, family and the shelves
//! of archived and settled threads; a forced stop, a workflow's stop, a rollback, a retry and the detach of a provider session; and one item of the
//! transcript. The rules are T3's (`references/t3code`, `orchestration-v2`), stated in the engine's `domain/lifecycle.rs`; this crate only names them.
//! Times are milliseconds since the epoch, fields are snake_case.
use agent_api_proto::{GroupName, Method};

pub mod events;
pub mod params;
pub mod results;

pub use events::LifecycleEvent;
pub use params::{ArchiveParams, Cursor, DeleteParams, DetachParams, FamilyParams, ForceStopParams, ForkParams, ItemGetParams, LaunchParams, MergeBackParams, RetryParams, RollbackParams, SearchParams, SettleParams, SettleState, Shelf, ShelfParams, WorkflowStopParams};
pub use results::{ArchiveResult, DeleteResult, DetachResult, FamilyResult, ForceStopOutcome, ForceStopResult, ForkResult, HitSource, ItemGetResult, LaunchResult, Lineage, MergeBackResult, Resting, RetryResult, RollbackResult, SearchHit, SearchResult, Settle, SettleResult, ShelfResult, ThreadState, WorkflowStopResult};

pub const GROUP: GroupName = GroupName::new("agent.lifecycle");

pub const THREAD_LAUNCH: Method = Method::new(GROUP, "thread.launch");
pub const THREAD_ARCHIVE: Method = Method::new(GROUP, "thread.archive");
pub const THREAD_DELETE: Method = Method::new(GROUP, "thread.delete");
pub const THREAD_FORK: Method = Method::new(GROUP, "thread.fork");
pub const THREAD_SETTLE: Method = Method::new(GROUP, "thread.settle");
pub const THREAD_SEARCH: Method = Method::new(GROUP, "thread.search");
pub const THREAD_FAMILY: Method = Method::new(GROUP, "thread.family");
pub const THREAD_FORCE_STOP: Method = Method::new(GROUP, "thread.forceStop");
pub const ITEM_GET: Method = Method::new(GROUP, "item.get");
pub const WORKFLOW_STOP: Method = Method::new(GROUP, "workflow.stop");
pub const THREAD_MERGE_BACK: Method = Method::new(GROUP, "thread.mergeBack");
pub const THREAD_ROLLBACK: Method = Method::new(GROUP, "thread.rollback");
pub const THREAD_DETACH: Method = Method::new(GROUP, "thread.detach");
pub const TURN_RETRY: Method = Method::new(GROUP, "turn.retry");
pub const THREAD_SHELF: Method = Method::new(GROUP, "thread.shelf");

pub const METHODS: &[Method] = &[
    THREAD_LAUNCH,
    THREAD_ARCHIVE,
    THREAD_DELETE,
    THREAD_FORK,
    THREAD_SETTLE,
    THREAD_SEARCH,
    THREAD_FAMILY,
    THREAD_FORCE_STOP,
    ITEM_GET,
    WORKFLOW_STOP,
    THREAD_MERGE_BACK,
    THREAD_ROLLBACK,
    THREAD_DETACH,
    TURN_RETRY,
    THREAD_SHELF,
];

/// The codes of the group. `THREAD_NOT_FOUND`, `TURN_RUNNING`, `NO_TURN`, `UNKNOWN_DRIVER`, `NO_EXECUTOR` and `INVALID_PARAMS` are the shared ones.
pub mod code {
    pub const ALREADY_ARCHIVED: &str = "ALREADY_ARCHIVED";
    pub const NOT_ARCHIVED: &str = "NOT_ARCHIVED";
    /// A fork or launch named a thread id that exists.
    pub const THREAD_EXISTS: &str = "THREAD_EXISTS";
    /// A fork named a turn that still runs.
    pub const FORK_SOURCE_RUNNING: &str = "FORK_SOURCE_RUNNING";
    /// No turn of the source has finished, or the one named is none of its turns.
    pub const NO_STABLE_TURN: &str = "NO_STABLE_TURN";
    pub const SNOOZE_NOT_FUTURE: &str = "SNOOZE_NOT_FUTURE";
    pub const SNOOZE_REQUEST_OPEN: &str = "SNOOZE_REQUEST_OPEN";
    pub const SNOOZE_QUEUED: &str = "SNOOZE_QUEUED";
    pub const SNOOZE_ARCHIVED: &str = "SNOOZE_ARCHIVED";
    /// A forced stop named a turn that is not the thread's newest.
    pub const SUPERSEDED: &str = "SUPERSEDED";
    pub const ITEM_NOT_FOUND: &str = "ITEM_NOT_FOUND";
    pub const NOT_A_FORK: &str = "NOT_A_FORK";
    /// A merge back while the target runs or has a queue.
    pub const TARGET_BUSY: &str = "TARGET_BUSY";
    pub const MERGE_PENDING: &str = "MERGE_PENDING";
    /// A merge back from a turn that has not completed.
    pub const SOURCE_NOT_FINISHED: &str = "SOURCE_NOT_FINISHED";
    pub const TURN_NOT_FOUND: &str = "TURN_NOT_FOUND";
    /// A rollback that restores files to before a turn that has no checkpoint (the workspace is no repository, or the turn ran before checkpoints
    /// were taken): roll back with `restore_files` false.
    pub const NO_CHECKPOINT: &str = "NO_CHECKPOINT";
    pub const SESSION_NOT_OWNED: &str = "SESSION_NOT_OWNED";
    /// A retry of a turn that is not the thread's newest, or did not fail.
    pub const NOT_RETRYABLE: &str = "NOT_RETRYABLE";

    pub const ALL: [&str; 19] = [
        ALREADY_ARCHIVED,
        NOT_ARCHIVED,
        THREAD_EXISTS,
        FORK_SOURCE_RUNNING,
        NO_STABLE_TURN,
        SNOOZE_NOT_FUTURE,
        SNOOZE_REQUEST_OPEN,
        SNOOZE_QUEUED,
        SNOOZE_ARCHIVED,
        SUPERSEDED,
        ITEM_NOT_FOUND,
        NOT_A_FORK,
        TARGET_BUSY,
        MERGE_PENDING,
        SOURCE_NOT_FINISHED,
        TURN_NOT_FOUND,
        NO_CHECKPOINT,
        SESSION_NOT_OWNED,
        NOT_RETRYABLE,
    ];
}

/// The message each code is answered with, so the engine, the service and the recorded exchanges say the same.
pub mod message {
    pub fn already_archived(thread: &str) -> String {
        format!("thread {thread} is already archived")
    }

    pub fn not_archived(thread: &str) -> String {
        format!("thread {thread} is not archived")
    }

    pub fn thread_exists(thread: &str) -> String {
        format!("thread {thread} already exists")
    }

    pub fn fork_source_running(turn: &str) -> String {
        format!("Fork source run {turn} is running; in-progress and rolled-back runs cannot be forked.")
    }

    /// `point` is `latest_stable` when the request named no turn, else `run`.
    pub fn no_stable_fork(point: &str) -> String {
        format!("No stable source run was found for fork source {point}.")
    }

    pub fn no_stable_merge(point: &str) -> String {
        format!("No stable source run was found for merge-back source {point}.")
    }

    pub fn snooze_not_future(thread: &str, until_ms: u64) -> String {
        format!("Thread {thread} snooze wake time {until_ms} is not in the future.")
    }

    pub fn snooze_request_open(thread: &str) -> String {
        format!("Thread {thread} has a pending approval or user-input request and cannot be snoozed.")
    }

    pub fn snooze_queued(thread: &str) -> String {
        format!("Thread {thread} has a queued run and cannot be snoozed.")
    }

    pub fn snooze_archived(thread: &str) -> String {
        format!("Thread {thread} is archived and cannot be snoozed.")
    }

    pub fn superseded(thread: &str, turn: &str) -> String {
        format!("Thread {thread} has started a newer turn than {turn}; nothing was stopped.")
    }

    pub fn item_not_found(thread: &str, seq: u64) -> String {
        format!("thread {thread} has no item opened at seq {seq}")
    }

    pub fn not_a_fork(source: &str, target: &str) -> String {
        format!("Thread {source} is not a fork of {target}.")
    }

    pub const TARGET_BUSY: &str = "Finish or stop the target thread's active work and clear its queue before merging back.";

    pub fn merge_pending(target: &str) -> String {
        format!("Thread {target} already has pending context from another side chat or fork. Send a message there first.")
    }

    pub fn source_not_finished(turn: &str, state: &str) -> String {
        format!("Merge-back source run {turn} is {state}; only provider-finished runs are supported.")
    }

    pub fn turn_not_found(thread: &str, turn: &str) -> String {
        format!("thread {thread} has no turn {turn}")
    }

    /// `turn` is the first turn the rollback drops: the checkpoint wanted is the one taken as it started.
    pub fn no_checkpoint(thread: &str, turn: &str) -> String {
        format!("Thread {thread} has no file checkpoint from the start of turn {turn}; roll back with restore_files false.")
    }

    pub fn session_not_owned(thread: &str, session: &str) -> String {
        format!("Provider session {session} does not belong to thread {thread}.")
    }

    pub fn not_retryable(turn: &str, why: &str) -> String {
        format!("Turn {turn} cannot be retried: {why}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::group_fixtures;
    use serde_json::Value;
    use std::collections::BTreeSet;

    fn parse_params(method: &str, params: &Value) -> Result<(), agent_api_proto::ParamError> {
        match method {
            "thread.launch" => LaunchParams::parse(params).map(drop),
            "thread.archive" => ArchiveParams::parse(params).map(drop),
            "thread.delete" => DeleteParams::parse(params).map(drop),
            "thread.fork" => ForkParams::parse(params).map(drop),
            "thread.settle" => SettleParams::parse(params).map(drop),
            "thread.search" => SearchParams::parse(params).map(drop),
            "thread.family" => FamilyParams::parse(params).map(drop),
            "thread.forceStop" => ForceStopParams::parse(params).map(drop),
            "item.get" => ItemGetParams::parse(params).map(drop),
            "workflow.stop" => WorkflowStopParams::parse(params).map(drop),
            "thread.mergeBack" => MergeBackParams::parse(params).map(drop),
            "thread.rollback" => RollbackParams::parse(params).map(drop),
            "thread.detach" => DetachParams::parse(params).map(drop),
            "turn.retry" => RetryParams::parse(params).map(drop),
            "thread.shelf" => ShelfParams::parse(params).map(drop),
            other => panic!("{other} is no method of the group"),
        }
    }

    fn parse_result(method: &str, result: &Value) -> Result<(), String> {
        fn read<T: serde::de::DeserializeOwned>(result: &Value) -> Result<(), String> {
            serde_json::from_value::<T>(result.clone()).map(drop).map_err(|e| e.to_string())
        }
        match method {
            "thread.launch" => read::<LaunchResult>(result),
            "thread.archive" => read::<ArchiveResult>(result),
            "thread.delete" => read::<DeleteResult>(result),
            "thread.fork" => read::<ForkResult>(result),
            "thread.settle" => read::<SettleResult>(result),
            "thread.search" => read::<SearchResult>(result),
            "thread.family" => read::<FamilyResult>(result),
            "thread.forceStop" => read::<ForceStopResult>(result),
            "item.get" => read::<ItemGetResult>(result),
            "workflow.stop" => read::<WorkflowStopResult>(result),
            "thread.mergeBack" => read::<MergeBackResult>(result),
            "thread.rollback" => read::<RollbackResult>(result),
            "thread.detach" => read::<DetachResult>(result),
            "turn.retry" => read::<RetryResult>(result),
            "thread.shelf" => read::<ShelfResult>(result),
            other => panic!("{other} is no method of the group"),
        }
    }

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_each_method_has_one_and_every_success_and_refusal_says_what_the_contract_says() {
        // Given the recorded exchanges of the group
        let recorded = group_fixtures("lifecycle");
        // Then there is one file per method, no more, and every method has at least a refusal or a success
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        let codes: BTreeSet<&str> = code::ALL.iter().copied().chain(["INVALID_PARAMS", "THREAD_NOT_FOUND", "TURN_RUNNING", "NO_TURN", "UNKNOWN_DRIVER", "NO_EXECUTOR"]).collect();
        for exchange in recorded.iter().flat_map(|f| &f.exchanges) {
            let method = exchange.request["method"].as_str().unwrap();
            let what = format!("{method}: {}", exchange.name);
            let params = &exchange.request["params"];
            if exchange.succeeds() {
                // And a success is a request the params accept and a result the type reads
                assert_eq!(parse_params(method, params), Ok(()), "{what}");
                assert_eq!(parse_result(method, &exchange.response["result"]), Ok(()), "{what}");
            } else {
                // And a refusal is one of the group's codes (or a shared one), with the message the contract gives it where it has one
                let (code, text) = (exchange.response["error"]["code"].as_str().unwrap(), exchange.response["error"]["message"].as_str().unwrap());
                assert!(codes.contains(code), "{what}: {code}");
                let thread = params["thread"].as_str().unwrap_or_default();
                match code {
                    "INVALID_PARAMS" => assert_eq!(parse_params(method, params).unwrap_err().0, text, "{what}"),
                    "THREAD_NOT_FOUND" => assert_eq!(text, crate::message::thread_not_found(thread), "{what}"),
                    "ALREADY_ARCHIVED" => assert_eq!(text, message::already_archived(thread), "{what}"),
                    "NOT_ARCHIVED" => assert_eq!(text, message::not_archived(thread), "{what}"),
                    "THREAD_EXISTS" => assert_eq!(text, message::thread_exists(params["fork"].as_str().or(params["thread"].as_str()).unwrap()), "{what}"),
                    "NO_STABLE_TURN" => assert!(text == message::no_stable_fork("latest_stable") || text == message::no_stable_fork("run") || text == message::no_stable_merge("latest_stable"), "{what}: {text}"),
                    "SNOOZE_NOT_FUTURE" => assert_eq!(text, message::snooze_not_future(thread, params["until_ms"].as_u64().unwrap()), "{what}"),
                    "SNOOZE_ARCHIVED" => assert_eq!(text, message::snooze_archived(thread), "{what}"),
                    "ITEM_NOT_FOUND" => assert_eq!(text, message::item_not_found(thread, params["seq"].as_u64().unwrap()), "{what}"),
                    "NOT_A_FORK" => assert_eq!(text, message::not_a_fork(thread, params["into"].as_str().unwrap()), "{what}"),
                    "TURN_NOT_FOUND" => assert_eq!(text, message::turn_not_found(thread, params["turn"].as_str().unwrap()), "{what}"),
                    "SESSION_NOT_OWNED" => assert_eq!(text, message::session_not_owned(thread, params["session"].as_str().unwrap()), "{what}"),
                    _ => assert!(!text.is_empty(), "{what}"),
                }
            }
        }
    }
}
