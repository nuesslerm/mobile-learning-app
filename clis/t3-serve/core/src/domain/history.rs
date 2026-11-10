//! A thread's timeline read from the read model: a page of it older than a cursor, and the whole of it as a transcript. Pure; the
//! paging follows the pin's `threadHistoryPaging.ts` for the rows this server writes (no user-turn rows, so the row cap is 75).
use super::state::State;
use serde_json::Value;
use t3_contracts::history::{self, Cursor, PAGE_MAX_ENCODED_BYTES, PAGE_MAX_ITEMS, TRANSCRIPT_MAX_BYTES};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HistoryError {
    ThreadNotFound,
    InvalidCursor,
    TranscriptTooLarge,
}

fn timeline(state: &State, thread: &str) -> Result<Vec<Value>, HistoryError> {
    state.threads.get(thread).ok_or(HistoryError::ThreadNotFound)?;
    Ok(history::projected_rows(thread, state.entities.get(thread).map_or(&[][..], |e| &e.turn_items)))
}

/// The index the page ends before: the cursor's row by identity (after it when inclusive), else the first row at its position.
fn end_index(rows: &[Value], cursor: &Cursor) -> usize {
    let at = |row: &Value, key: &str| row[key].as_str().unwrap_or("").to_string();
    if let Some(i) = rows.iter().position(|r| at(r, "sourceThreadId") == cursor.source_thread_id && at(r, "sourceItemId") == cursor.source_item_id) {
        return i + usize::from(cursor.inclusive);
    }
    let position = |r: &Value| r["position"].as_u64().unwrap_or(0);
    rows.iter()
        .position(|r| if cursor.inclusive { at(r, "sourceThreadId") == cursor.source_thread_id && r["item"]["ordinal"].as_u64().unwrap_or_else(|| position(r)) > cursor.position } else { position(r) >= cursor.position })
        .unwrap_or(rows.len())
}

/// `GET .../history?cursor=`: the rows older than the cursor, newest last, at most a page of them, and the cursor of the next page.
pub fn page(state: &State, thread: &str, cursor: &str, conversation_only: bool) -> Result<Value, HistoryError> {
    let cursor = Cursor::decode(cursor).ok_or(HistoryError::InvalidCursor)?;
    let rows = timeline(state, thread)?;
    let end = end_index(&rows, &cursor);
    let (mut taken, mut bytes) = (0, 0);
    for row in rows[..end].iter().rev() {
        let cost = row.to_string().len();
        if taken > 0 && (taken >= PAGE_MAX_ITEMS || bytes + cost > PAGE_MAX_ENCODED_BYTES) {
            break;
        }
        taken += 1;
        bytes += cost;
    }
    let start = end - taken;
    let next = (start > 0).then(|| {
        let oldest = &rows[start];
        let text = |key: &str| oldest[key].as_str().unwrap_or("").to_string();
        Cursor { snapshot_sequence: state.last_sequence, source_thread_id: text("sourceThreadId"), source_item_id: text("sourceItemId"), position: oldest["position"].as_u64().unwrap_or(0), inclusive: false }.encode()
    });
    let items = rows[start..end]
        .iter()
        .enumerate()
        .map(|(position, row)| {
            let mut row = row.clone();
            row["position"] = position.into();
            row
        })
        .filter(|row| !conversation_only || history::is_conversation_row(row))
        .collect();
    Ok(history::history_page(state.last_sequence, items, next))
}

/// `GET .../transcript`: every row of the thread with its title, refused when the saved form would exceed the size limit.
pub fn transcript(state: &State, thread: &str, environment_id: &str) -> Result<Value, HistoryError> {
    let rows = timeline(state, thread)?;
    let shell = &state.threads[thread];
    let (title, updated_at) = (&shell["title"], &shell["updatedAt"]);
    let size = history::transcript_header(environment_id, thread, title, updated_at).len() + rows.iter().map(|r| r.to_string().len() + 1).sum::<usize>();
    if size > TRANSCRIPT_MAX_BYTES {
        return Err(HistoryError::TranscriptTooLarge);
    }
    Ok(history::transcript(thread, title, updated_at, rows))
}
