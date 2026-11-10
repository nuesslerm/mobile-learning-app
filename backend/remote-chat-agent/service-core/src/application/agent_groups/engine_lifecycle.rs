//! The lifecycle of the agent API's threads in the contract's words: the groups' commands become the engine's, and the engine's state and queries become results.
//! The engine decides and keeps; nothing here counts or judges.
use super::engine_threads::view;
use agent_api_agent::lifecycle::{self, HitSource, Lineage, Resting, SearchHit, Settle, Shelf, ShelfResult, ThreadState};
use agent_api_agent::queue::QueueItem;
use remote_chat_agent_core::application::engine::NeutralView;
use remote_chat_agent_core::domain::ids::ms_from_iso;
use remote_chat_agent_core::domain::lifecycle::{self as rules, QueuedPrompt, SettleTo};
use remote_chat_agent_core::domain::queries::{self, ShelfPage};
use remote_chat_agent_core::domain::types::Body;
use rca_group_agent::Command;

fn named<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The engine's command for the contract's.
pub(super) fn body(command: Command) -> Body {
    match command {
        Command::ApprovalAnswer { approval, decision } => Body::ApprovalAnswer { approval, decision: named(&decision) },
        Command::InputAnswer { request, answer } => Body::InputAnswer { request, answer },
        Command::InputDismiss { request } => Body::InputDismiss { request },
        Command::QueueAdd(add) => Body::QueueAdd {
            item: add.item,
            prompt: add.prompt,
            attachments: add.attachments.unwrap_or_default(),
            workspace: add.workspace,
            driver: add.driver,
            model: add.model,
            runtime_mode: add.runtime_mode,
            interaction_mode: add.interaction_mode,
        },
        Command::QueueEdit { item, prompt } => Body::QueueEdit { item, prompt },
        Command::QueueMove { item, before } => Body::QueueMove { item, before },
        Command::QueueDelete { item } => Body::QueueDelete { item },
        Command::QueueResume { item } => Body::QueueResume { item },
        Command::QueueStart { item } => Body::QueueStart { item },
        Command::Steer { turn, prompt, item } => Body::Steer { turn, prompt, item },
        Command::Archive { archived } => Body::LifecycleArchive { archive: archived },
        Command::Delete { only_if_unused } => Body::LifecycleDelete { only_if_unused },
        Command::Settle { state, until_ms, at_ms } => {
            let to = match state {
                lifecycle::SettleState::Settled => SettleTo::Settled,
                lifecycle::SettleState::Active => SettleTo::Active,
                lifecycle::SettleState::Snoozed => SettleTo::Snoozed,
                lifecycle::SettleState::Awake => SettleTo::Awake,
            };
            Body::SettleSet { to, until_ms, at_ms }
        }
        Command::Fork { source, turn, title } => Body::Fork { source, at_turn: turn, title },
        Command::ForceStop { turn } => Body::ForceStop { turn },
        Command::WorkflowStop { turn, workflow } => Body::WorkflowStop { turn, workflow },
        Command::MergeBack { source, turn } => Body::MergeBack { source, turn },
        Command::Rollback { turn, restore_files } => Body::Rollback { turn, restore_files },
        Command::Detach { session, reason } => Body::Detach { session, reason },
        Command::Retry { turn, new_turn } => Body::Retry { turn, new_turn },
    }
}

fn settle_of(settle: &rules::Settle) -> Settle {
    let state = match settle.state.as_deref() {
        Some("settled") => Some(Resting::Settled),
        Some("active") => Some(Resting::Active),
        _ => None,
    };
    Settle { state, settled_at_ms: settle.settled_at_ms, unsettled_at_ms: settle.unsettled_at_ms, snoozed_until_ms: settle.snoozed_until_ms, snoozed_at_ms: settle.snoozed_at_ms, woke_at_ms: settle.woke_at_ms }
}

/// A thread with what the lifecycle adds to its view.
pub(super) fn state_of(thread: &NeutralView) -> ThreadState {
    let lifecycle = &thread.lifecycle;
    ThreadState {
        thread: view(thread),
        archived_at_ms: thread.thread.archived_at.as_deref().and_then(ms_from_iso),
        deleted_at_ms: thread.thread.deleted_at.as_deref().and_then(ms_from_iso),
        settle: lifecycle.settle.as_ref().map(settle_of),
        lineage: lifecycle.lineage.as_ref().map(|l| Lineage { parent: l.parent.clone(), relationship: l.relationship.clone(), root: l.root.clone() }),
        queued: u32::try_from(lifecycle.queue.len()).unwrap_or(u32::MAX),
        queue_held: lifecycle.queue_held,
        pending_merge_back: lifecycle.pending_merge_back.as_ref().map(|p| p.source.clone()),
    }
}

pub(super) fn queue_item(queued: &QueuedPrompt) -> QueueItem {
    QueueItem {
        item: queued.item.clone(),
        prompt: queued.prompt.clone(),
        attachments: queued.attachments.clone(),
        workspace: queued.workspace.clone(),
        driver: queued.driver.clone(),
        model: queued.model.clone(),
        runtime_mode: queued.runtime_mode.clone(),
        interaction_mode: queued.interaction_mode.clone(),
        created_at_ms: queued.created_at_ms,
    }
}

pub(super) fn hit_of(hit: queries::SearchHit) -> SearchHit {
    let source = match hit.source {
        queries::HitSource::Title => HitSource::Title,
        queries::HitSource::User => HitSource::User,
        queries::HitSource::Assistant => HitSource::Assistant,
    };
    SearchHit { thread: hit.thread, title: hit.title, source, snippet: hit.snippet, at_ms: hit.at_ms }
}

pub(super) fn shelf_of(which: queries::Shelf) -> Shelf {
    match which {
        queries::Shelf::Archived => Shelf::Archived,
        queries::Shelf::Settled => Shelf::Settled,
    }
}

pub(super) fn engine_shelf(which: Shelf) -> queries::Shelf {
    match which {
        Shelf::Archived => queries::Shelf::Archived,
        Shelf::Settled => queries::Shelf::Settled,
    }
}

pub(super) fn cursor(cursor: lifecycle::Cursor) -> queries::Cursor {
    queries::Cursor { at_ms: cursor.at_ms, thread: cursor.thread }
}

/// A page of a shelf with the threads on it, in the page's order.
pub(super) fn shelf_result(which: queries::Shelf, page: ShelfPage, threads: &[NeutralView]) -> ShelfResult {
    ShelfResult {
        shelf: shelf_of(which),
        threads: threads.iter().map(state_of).collect(),
        next: page.next.map(|c| lifecycle::Cursor { at_ms: c.at_ms, thread: c.thread }),
        total: u32::try_from(page.total).unwrap_or(u32::MAX),
    }
}
