//! A thread read as the phone's journal: the cursor is `{epoch, sequence}` with rca's per-thread `seq` as the sequence, and the epoch is
//! fixed (rca's log is append-only), so a cursor of another epoch is a reset.
use crate::agent::ask;
use crate::fold::{fold, Folded, Item};
use agent_api_agent::{THREAD_GET, THREAD_HISTORY};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

pub(crate) const EPOCH: &str = "rca";
pub(crate) const FENCE: u64 = 1;
pub const DEFAULT_LIMIT: usize = 40;
/// rca's page size for `thread.history`.
const PAGE: u64 = 200;

pub(crate) fn cursor(seq: u64) -> Value {
    json!({"epoch": EPOCH, "sequence": seq})
}

/// Every event of the thread after `after`, oldest first, and the thread's head.
pub(crate) fn events_after(agent: &dyn AgentApi, thread: &str, after: u64) -> Result<(Vec<Value>, u64), Failure> {
    let (mut events, mut from, mut head) = (Vec::new(), after, after);
    loop {
        let page = ask(agent, THREAD_HISTORY.name(), json!({"thread": thread, "after_seq": from, "limit": PAGE}))?;
        let got = page["events"].as_array().cloned().unwrap_or_default();
        head = head.max(page["last_seq"].as_u64().unwrap_or(0));
        from = got.last().and_then(|e| e["seq"].as_u64()).unwrap_or(from);
        let more = page["more"] == true && !got.is_empty();
        events.extend(got);
        if !more {
            return Ok((events, head));
        }
    }
}

pub(crate) struct Loaded {
    pub folded: Folded,
}

pub(crate) fn view(agent: &dyn AgentApi, thread: &str) -> Result<Value, Failure> {
    Ok(ask(agent, THREAD_GET.name(), json!({"thread": thread}))?["thread"].clone())
}

pub(crate) fn load(agent: &dyn AgentApi, thread: &str) -> Result<Loaded, Failure> {
    view(agent, thread)?;
    let (events, _) = events_after(agent, thread, 0)?;
    Ok(Loaded { folded: fold(&events) })
}

/// The fields every history page and batch carries about what the session is doing now.
pub(crate) fn current_work(folded: &Folded) -> Value {
    json!({"actionablePromptIds": folded.pending_prompt_ids(), "working": folded.running_turn.is_some(), "latestTurn": folded.latest_turn()})
}

fn direction_items<'a>(items: &'a [Item], direction: &str, cursor: Option<u64>, limit: usize) -> (Vec<&'a Item>, bool, bool) {
    match direction {
        "before" => {
            let older: Vec<&Item> = items.iter().filter(|i| cursor.is_some_and(|c| i.sequence < c)).collect();
            let start = older.len().saturating_sub(limit);
            (older[start..].to_vec(), start > 0, true)
        }
        "after" => {
            let newer: Vec<&Item> = items.iter().filter(|i| cursor.is_some_and(|c| i.revision > c)).collect();
            let more = newer.len() > limit;
            (newer.into_iter().take(limit).collect(), true, more)
        }
        _ => {
            let start = items.len().saturating_sub(limit);
            (items[start..].iter().collect(), start > 0, false)
        }
    }
}

/// `AgentSessionHistoryPage` for `direction` from `cursor`.
pub(crate) fn page(session: &str, folded: &Folded, direction: &str, cursor_seq: Option<u64>, limit: usize) -> Value {
    let (items, has_older, has_newer) = direction_items(&folded.items, direction, cursor_seq, limit);
    let oldest = items.iter().map(|i| i.sequence).min();
    let newest = items.iter().map(|i| i.revision).max();
    let next = match direction {
        "before" => oldest.or(cursor_seq).unwrap_or(0),
        "after" => newest.or(cursor_seq).unwrap_or(folded.last_seq),
        _ => oldest.unwrap_or(0),
    };
    let mut page = json!({
        "sessionId": session, "epoch": EPOCH, "fence": FENCE, "direction": direction,
        "items": items.iter().map(|i| i.render()).collect::<Vec<_>>(),
        "removedItemIds": [], "submissions": [],
        "window": {"oldest": oldest.map(cursor), "newest": newest.map(cursor), "nextCursor": cursor(next)},
        "liveCursor": cursor(folded.last_seq), "hasOlder": has_older, "hasNewer": has_newer,
    });
    if let (Some(p), Some(w)) = (page.as_object_mut(), current_work(folded).as_object()) {
        p.extend(w.clone());
    }
    page
}
