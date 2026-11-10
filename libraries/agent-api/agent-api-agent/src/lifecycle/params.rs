//! The params of `agent.lifecycle`.
use crate::params::{not_empty, read, required, to_value};
use agent_api_proto::ParamError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most hits a `thread.search` answers when the request names no `limit`, and the most it ever does.
pub const DEFAULT_SEARCH_LIMIT: u32 = 20;
pub const MAX_SEARCH_LIMIT: u32 = 100;
/// The page size of a `thread.shelf` that names no `limit`, and the largest one.
pub const DEFAULT_SHELF_LIMIT: u32 = 20;
pub const MAX_SHELF_LIMIT: u32 = 50;

macro_rules! to_value_impl {
    ($($name:ident),+) => {$(impl $name {
        pub fn to_value(&self) -> Value {
            to_value(self)
        }
    })+};
}

/// `thread.launch`: a thread and its first turn in one call (`thread` is the id the caller chose; the service names it when it is left out). When the
/// turn cannot start, no thread is left behind.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_mode: Option<String>,
}

impl LaunchParams {
    pub fn parse(params: &Value) -> Result<LaunchParams, ParamError> {
        let parsed: LaunchParams = read(params)?;
        for (name, value) in [("workspace", &parsed.workspace), ("driver", &parsed.driver), ("prompt", &parsed.prompt)] {
            required(name, value)?;
        }
        for (name, value) in [("thread", &parsed.thread), ("title", &parsed.title), ("model", &parsed.model), ("runtime_mode", &parsed.runtime_mode), ("interaction_mode", &parsed.interaction_mode)] {
            not_empty(name, value)?;
        }
        Ok(parsed)
    }
}

/// `thread.archive`: archive the thread, or with `archived` false bring it back. Either is refused when the thread is already so.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
}

impl ArchiveParams {
    pub fn parse(params: &Value) -> Result<ArchiveParams, ParamError> {
        let parsed: ArchiveParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }

    /// What the request asks: true to archive.
    pub fn archive(&self) -> bool {
        self.archived.unwrap_or(true)
    }
}

/// `thread.delete`: with `only_if_unused` the thread goes only while it has had no turn and no message, so a message sent since the caller looked keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_if_unused: Option<bool>,
}

impl DeleteParams {
    pub fn parse(params: &Value) -> Result<DeleteParams, ParamError> {
        let parsed: DeleteParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }
}

/// `thread.fork`: a new thread, `fork` (named by the caller, or by the service), holding the finished turns of `thread` up to and including `turn`
/// (its latest completed turn when left out), titled `<title> fork` unless `title` says otherwise.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl ForkParams {
    pub fn parse(params: &Value) -> Result<ForkParams, ParamError> {
        let parsed: ForkParams = read(params)?;
        required("thread", &parsed.thread)?;
        for (name, value) in [("fork", &parsed.fork), ("turn", &parsed.turn), ("title", &parsed.title)] {
            not_empty(name, value)?;
        }
        Ok(parsed)
    }
}

/// What `thread.settle` puts a thread in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettleState {
    /// Out of the way: the first settle time is kept, a snooze ends and a pin goes.
    Settled,
    /// Settled no more, and kept from settling by itself.
    Active,
    /// Out of the way until `until_ms`, which must be in the future, with nothing open, nothing queued, and the thread not archived.
    Snoozed,
    /// A snooze ended early.
    Awake,
}

/// `thread.settle`. `until_ms` goes with `snoozed` alone, `at_ms` (when the settle is counted from) with `settled` alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettleParams {
    #[serde(default)]
    pub thread: String,
    pub state: SettleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

impl SettleParams {
    pub fn parse(params: &Value) -> Result<SettleParams, ParamError> {
        let parsed: SettleParams = read(params)?;
        required("thread", &parsed.thread)?;
        let snoozed = parsed.state == SettleState::Snoozed;
        if snoozed && parsed.until_ms.is_none() {
            return Err(ParamError("until_ms is required when state is snoozed".into()));
        }
        if !snoozed && parsed.until_ms.is_some() {
            return Err(ParamError("until_ms goes with state snoozed".into()));
        }
        if parsed.state != SettleState::Settled && parsed.at_ms.is_some() {
            return Err(ParamError("at_ms goes with state settled".into()));
        }
        Ok(parsed)
    }
}

/// `thread.search`: the threads whose title, or a message, holds every word of `query`, title matches first. Archived and deleted threads are left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    #[serde(default)]
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl SearchParams {
    pub fn parse(params: &Value) -> Result<SearchParams, ParamError> {
        let parsed: SearchParams = read(params)?;
        required("query", &parsed.query)?;
        if !matches!(parsed.limit, None | Some(1..=MAX_SEARCH_LIMIT)) {
            return Err(ParamError(format!("limit must be between 1 and {MAX_SEARCH_LIMIT}")));
        }
        Ok(parsed)
    }

    pub fn page(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_SEARCH_LIMIT)
    }
}

/// `thread.family`: the threads that share the thread's root.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyParams {
    #[serde(default)]
    pub thread: String,
}

impl FamilyParams {
    pub fn parse(params: &Value) -> Result<FamilyParams, ParamError> {
        let parsed: FamilyParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }
}

/// `thread.forceStop`: end the thread's turn and its CLI process now, and hold its queue. `turn` is the turn the person saw ignore a stop; a
/// newer turn is not stopped.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForceStopParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
}

impl ForceStopParams {
    pub fn parse(params: &Value) -> Result<ForceStopParams, ParamError> {
        let parsed: ForceStopParams = read(params)?;
        required("thread", &parsed.thread)?;
        not_empty("turn", &parsed.turn)?;
        Ok(parsed)
    }
}

/// `item.get`: the transcript row the event at `seq` opened (the `seq` of a `TranscriptItem`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemGetParams {
    #[serde(default)]
    pub thread: String,
    pub seq: u64,
}

impl ItemGetParams {
    pub fn parse(params: &Value) -> Result<ItemGetParams, ParamError> {
        let parsed: ItemGetParams = read(params)?;
        required("thread", &parsed.thread)?;
        Ok(parsed)
    }
}

/// `workflow.stop`: stop what the running `turn` runs in the background. The CLIs are one process per turn, so the service ends the turn's process.
/// `workflow` names the subagent the caller saw, when it saw one. `turn` must be the turn that runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStopParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub turn: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
}

impl WorkflowStopParams {
    pub fn parse(params: &Value) -> Result<WorkflowStopParams, ParamError> {
        let parsed: WorkflowStopParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("turn", &parsed.turn)?;
        not_empty("workflow", &parsed.workflow)?;
        Ok(parsed)
    }
}

/// `thread.mergeBack`: carry what the fork `thread` did, up to its `turn` (its latest completed one when left out), into its parent `into`, whose
/// next prompt takes it as context.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeBackParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub into: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<String>,
}

impl MergeBackParams {
    pub fn parse(params: &Value) -> Result<MergeBackParams, ParamError> {
        let parsed: MergeBackParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("into", &parsed.into)?;
        not_empty("turn", &parsed.turn)?;
        Ok(parsed)
    }
}

/// `thread.rollback`: the thread as it was after `turn`: later turns' messages, activity and diffs go. `restore_files` (true when left out) also
/// puts the workspace's files back to what they were as the first dropped turn started (the checkpoint the executor took then, restored with
/// `git_discard`); refused with `NO_CHECKPOINT` when that turn has none, and a rollback that drops no turn restores nothing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RollbackParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub turn: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_files: Option<bool>,
}

impl RollbackParams {
    pub fn parse(params: &Value) -> Result<RollbackParams, ParamError> {
        let parsed: RollbackParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("turn", &parsed.turn)?;
        Ok(parsed)
    }

    pub fn restores_files(&self) -> bool {
        self.restore_files.unwrap_or(true)
    }
}

/// `thread.detach`: forget the CLI session `session` of the thread, so its next turn starts a fresh one. It must be the thread's own session.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetachParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl DetachParams {
    pub fn parse(params: &Value) -> Result<DetachParams, ParamError> {
        let parsed: DetachParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("session", &parsed.session)?;
        not_empty("reason", &parsed.reason)?;
        Ok(parsed)
    }
}

/// `turn.retry`: run the prompt of a failed `turn` again, as a new turn. Only the thread's newest turn, and only when it failed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub turn: String,
}

impl RetryParams {
    pub fn parse(params: &Value) -> Result<RetryParams, ParamError> {
        let parsed: RetryParams = read(params)?;
        required("thread", &parsed.thread)?;
        required("turn", &parsed.turn)?;
        Ok(parsed)
    }
}

/// Which threads `thread.shelf` pages: those archived, newest archive first, or those settled, newest settle first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shelf {
    Archived,
    Settled,
}

/// Where a page starts: below `at_ms` (the archive or settle time), and after `thread` among those at the same time (ids ascending). A cursor with no
/// thread is strict: it leaves out every thread at `at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
}

/// `thread.shelf`: a page of at most `limit` threads of the shelf (0 reads only the count), after `before`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShelfParams {
    pub shelf: Shelf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ShelfParams {
    pub fn parse(params: &Value) -> Result<ShelfParams, ParamError> {
        let parsed: ShelfParams = read(params)?;
        if parsed.limit.is_some_and(|limit| limit > MAX_SHELF_LIMIT) {
            return Err(ParamError(format!("limit must be at most {MAX_SHELF_LIMIT}")));
        }
        if parsed.before.as_ref().and_then(|c| c.thread.as_deref()) == Some("") {
            return Err(ParamError("before.thread must not be empty".into()));
        }
        Ok(parsed)
    }

    pub fn page(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_SHELF_LIMIT)
    }
}

to_value_impl!(LaunchParams, ArchiveParams, DeleteParams, ForkParams, SettleParams, SearchParams, FamilyParams, ForceStopParams, ItemGetParams, WorkflowStopParams, MergeBackParams, RollbackParams, DetachParams, RetryParams, ShelfParams);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_a_settle_when_its_state_and_times_do_not_fit_together_then_it_is_refused_saying_which_goes_with_which() {
        // Given settles of each state with the times that do and do not belong to it
        let cases = [
            (json!({"thread": "t", "state": "snoozed"}), Err("until_ms is required when state is snoozed")),
            (json!({"thread": "t", "state": "snoozed", "until_ms": 5}), Ok(())),
            (json!({"thread": "t", "state": "snoozed", "until_ms": 5, "at_ms": 1}), Err("at_ms goes with state settled")),
            (json!({"thread": "t", "state": "settled", "until_ms": 5}), Err("until_ms goes with state snoozed")),
            (json!({"thread": "t", "state": "settled", "at_ms": 5}), Ok(())),
            (json!({"thread": "t", "state": "active", "at_ms": 5}), Err("at_ms goes with state settled")),
            (json!({"thread": "t", "state": "awake"}), Ok(())),
            (json!({"thread": "t", "state": "asleep"}), Err("unknown variant `asleep`")),
        ];
        for (params, expected) in cases {
            // When read, Then each is accepted or refused with the message that names the field
            let read = SettleParams::parse(&params).map(drop);
            match expected {
                Ok(()) => assert_eq!(read, Ok(()), "{params}"),
                Err(says) => assert!(read.unwrap_err().0.contains(says), "{params}"),
            }
        }
    }

    #[test]
    fn given_params_with_a_bound_or_a_required_field_wrong_when_read_then_each_is_refused_and_a_name_the_method_lacks_is_too() {
        // Given a search with no query and one past the limit, a shelf past its page size, a launch with no prompt, a rollback with no turn, and an extra field
        let cases = [
            (SearchParams::parse(&json!({})).map(drop), "query is required"),
            (SearchParams::parse(&json!({"query": "x", "limit": 101})).map(drop), "limit must be between 1 and 100"),
            (SearchParams::parse(&json!({"query": "x", "limit": 0})).map(drop), "limit must be between 1 and 100"),
            (ShelfParams::parse(&json!({"shelf": "settled", "limit": 51})).map(drop), "limit must be at most 50"),
            (ShelfParams::parse(&json!({"shelf": "trash"})).map(drop), "unknown variant `trash`"),
            (LaunchParams::parse(&json!({"workspace": "w", "driver": "d"})).map(drop), "prompt is required"),
            (RollbackParams::parse(&json!({"thread": "t"})).map(drop), "turn is required"),
            (ForkParams::parse(&json!({"thread": "t", "colour": "red"})).map(drop), "unknown field `colour`"),
            (ItemGetParams::parse(&json!({"thread": "t"})).map(drop), "missing field `seq`"),
        ];
        // When read, Then each is refused and the message names what is wrong
        for (parsed, says) in cases {
            let refused = parsed.unwrap_err();
            assert!(refused.0.contains(says), "{says}: {}", refused.0);
        }
    }

    #[test]
    fn given_params_a_mapper_builds_when_encoded_and_read_back_then_they_are_the_same_and_the_defaults_are_the_contracts() {
        // Given a shelf page with a cursor, an archive that names nothing, a rollback that names nothing, and a search with no limit
        let shelf = ShelfParams { shelf: Shelf::Settled, before: Some(Cursor { at_ms: 1_700_000_000_000, thread: Some("t9".into()) }), limit: Some(10) };
        // When each is encoded and read back, Then the shelf is the same and the defaults read as the contract says
        assert_eq!(ShelfParams::parse(&shelf.to_value()), Ok(shelf));
        assert!(ArchiveParams::parse(&json!({"thread": "t"})).unwrap().archive());
        assert!(!ArchiveParams::parse(&json!({"thread": "t", "archived": false})).unwrap().archive());
        assert!(RollbackParams::parse(&json!({"thread": "t", "turn": "u"})).unwrap().restores_files());
        assert_eq!((SearchParams::parse(&json!({"query": "x"})).unwrap().page(), ShelfParams::parse(&json!({"shelf": "archived"})).unwrap().page()), (DEFAULT_SEARCH_LIMIT, DEFAULT_SHELF_LIMIT));
        assert_eq!(ShelfParams::parse(&json!({"shelf": "archived", "limit": 0})).unwrap().page(), 0);
    }
}
