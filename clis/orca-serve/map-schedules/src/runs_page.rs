//! The pinned host's paging of automation runs (`automation-run-cursor.ts`, `automation-runs-preview-page.ts`), pure: rows are the phone's
//! `AutomationRun` objects, newest first; a cursor is `createdAt:id`, or a plain number for an older client's offset.
use orca_core::wire::{Failure, RUNTIME_ERROR};
use serde_json::{json, Value};
use std::cmp::Ordering;

const MAX_PAGE: f64 = 100.0;
/// `MAX_REMOTE_RPC_RESPONSE_BYTES` less its envelope headroom.
const REPLY_MAX: usize = 4 * 1024 * 1024 - 4 * 1024;

enum Cursor {
    Key { created_at: f64, id: String },
    Offset(f64),
}

fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

/// `Number.parseInt(text, 10)`: leading whitespace, a sign, then the longest run of digits; none is NaN.
fn parse_int(text: &str) -> f64 {
    let text = text.trim_start_matches(js_space);
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.parse::<f64>().unwrap_or(f64::INFINITY)
}

fn decode(cursor: Option<&str>) -> Option<Cursor> {
    let cursor = cursor.filter(|c| !c.is_empty())?;
    match cursor.split_once(':') {
        None => Some(parse_int(cursor)).filter(|n| n.is_finite() && *n > 0.0).map(Cursor::Offset),
        Some((at, id)) => Some(parse_int(at)).filter(|n| n.is_finite() && !id.is_empty()).map(|created_at| Cursor::Key { created_at, id: id.to_string() }),
    }
}

fn created_at(row: &Value) -> f64 {
    row["createdAt"].as_f64().unwrap_or(0.0)
}

fn id(row: &Value) -> &str {
    row["id"].as_str().unwrap_or_default()
}

/// `automationRunCursor`: where a page that ends at `row` resumes.
pub(crate) fn cursor_of(row: &Value) -> String {
    format!("{}:{}", row["createdAt"], id(row))
}

/// `compareAutomationRunsNewestFirst`: later first, then by id (rca's run ids sort the same by bytes as by the host's locale).
pub(crate) fn newest_first(a: &Value, b: &Value) -> Ordering {
    created_at(b).partial_cmp(&created_at(a)).unwrap_or(Ordering::Equal).then_with(|| id(a).cmp(id(b)))
}

fn start(rows: &[Value], cursor: Option<Cursor>) -> usize {
    match cursor {
        None => 0,
        Some(Cursor::Offset(offset)) => offset.min(rows.len() as f64) as usize,
        Some(Cursor::Key { created_at: at, id: key }) => {
            if let Some(i) = rows.iter().position(|r| id(r) == key && created_at(r) == at) {
                return i + 1;
            }
            rows.iter().position(|r| created_at(r) < at || (created_at(r) == at && key.as_str() < id(r))).unwrap_or(rows.len())
        }
    }
}

/// `paginateAutomationRuns`: at most 100 rows from the cursor (a limit under one is one, a fraction is cut), and the cursor after them while
/// rows remain.
pub(crate) fn paginate(rows: Vec<Value>, limit: Option<f64>, cursor: Option<&str>) -> (Vec<Value>, Option<String>) {
    let total = rows.len();
    let from = start(&rows, decode(cursor));
    let size = limit.unwrap_or(MAX_PAGE).clamp(1.0, MAX_PAGE) as usize;
    let page: Vec<Value> = rows.into_iter().skip(from).take(size).collect();
    let next = page.last().filter(|_| from + page.len() < total).map(cursor_of);
    (page, next)
}

pub(crate) fn page_json((runs, next): (Vec<Value>, Option<String>)) -> Value {
    json!({"runs": runs, "nextCursor": next})
}

/// `remoteRpcContentBudget`: the reply's limit less the request id as the reply will quote it.
pub(crate) fn budget(request_id: &str) -> usize {
    REPLY_MAX.saturating_sub(serde_json::to_string(request_id).map_or(0, |s| s.len()))
}

fn bytes(value: &Value) -> usize {
    serde_json::to_string(value).map_or(0, |s| s.len())
}

/// `previewAutomationRunsPage`: the rows whose whole reply fits `max_bytes`, the cursor resuming at the cut (rca keeps no output to shorten).
pub(crate) fn preview((runs, next): (Vec<Value>, Option<String>), max_bytes: usize) -> Result<Value, Failure> {
    let mut row_bytes = 0;
    for count in 0..runs.len() {
        row_bytes += bytes(&runs[count]) + usize::from(count > 0);
        let candidate = if count + 1 < runs.len() { Some(cursor_of(&runs[count])) } else { next.clone() };
        if row_bytes + bytes(&json!({"runs": [], "nextCursor": candidate})) <= max_bytes {
            continue;
        }
        if count == 0 {
            return Err(Failure::new(RUNTIME_ERROR, "This automation run is too large to list over this connection."));
        }
        let kept: Vec<Value> = runs[..count].to_vec();
        let resume = cursor_of(&kept[count - 1]);
        return Ok(json!({"runs": kept, "nextCursor": resume}));
    }
    Ok(json!({"runs": runs, "nextCursor": next}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(specs: &[(u64, &str)]) -> Vec<Value> {
        specs.iter().map(|(at, id)| json!({"id": id, "createdAt": at})).collect()
    }

    fn ids(page: &(Vec<Value>, Option<String>)) -> (Vec<String>, Option<String>) {
        (page.0.iter().map(|r| id(r).to_string()).collect(), page.1.clone())
    }

    #[test]
    fn given_runs_newest_first_when_paged_by_limit_key_cursor_or_offset_then_each_page_is_the_hosts() {
        // Given four runs, two of them at the same instant
        let all = rows(&[(40, "run-4"), (30, "run-2"), (30, "run-3"), (10, "run-1")]);
        // When paged two at a time, Then the second page resumes after the first's last run
        let first = paginate(all.clone(), Some(2.0), None);
        assert_eq!(ids(&first), (vec!["run-4".into(), "run-2".into()], Some("30:run-2".into())));
        assert_eq!(ids(&paginate(all.clone(), Some(2.0), first.1.as_deref())), (vec!["run-3".into(), "run-1".into()], None));
        // And a limit of zero or a fraction is one row, a legacy offset skips that many, and a cursor that reads as nothing starts at the top
        assert_eq!(ids(&paginate(all.clone(), Some(0.0), None)).0, ["run-4"]);
        assert_eq!(ids(&paginate(all.clone(), Some(1.5), None)).0, ["run-4"]);
        assert_eq!(ids(&paginate(all.clone(), None, Some(" 3x"))).0, ["run-1"]);
        assert_eq!(ids(&paginate(all.clone(), None, Some("a"))).0.len(), 4);
        assert_eq!(ids(&paginate(all.clone(), None, Some("0"))).0.len(), 4);
        // And a cursor whose run is gone resumes at the first run after it, a tie by its id
        assert_eq!(ids(&paginate(all.clone(), None, Some("30:run-25"))).0, ["run-3", "run-1"]);
        assert_eq!(ids(&paginate(all.clone(), None, Some("35:gone"))).0, ["run-2", "run-3", "run-1"]);
        assert_eq!(ids(&paginate(all, None, Some("5:gone"))), (vec![], None));
    }

    #[test]
    fn given_a_page_and_a_byte_budget_when_previewed_then_rows_past_the_budget_are_cut_with_a_cursor_and_a_first_row_over_it_is_refused() {
        // Given a page of three runs and a budget that holds two of them
        let page = (rows(&[(3, "c"), (2, "b"), (1, "a")]), None);
        let two = bytes(&json!({"runs": [page.0[0], page.0[1]], "nextCursor": "2:b"}));
        // When previewed, Then the third is cut and the cursor resumes after the second
        assert_eq!(preview(page.clone(), two).unwrap(), json!({"runs": [page.0[0], page.0[1]], "nextCursor": "2:b"}));
        // And a budget that holds everything answers the page as it is, and one too small for the first row is the host's refusal
        assert_eq!(preview(page.clone(), usize::MAX).unwrap(), json!({"runs": page.0, "nextCursor": null}));
        assert_eq!(preview(page, 10).unwrap_err().message, "This automation run is too large to list over this connection.");
        assert_eq!(budget("r1"), 4 * 1024 * 1024 - 4096 - 4);
    }
}
