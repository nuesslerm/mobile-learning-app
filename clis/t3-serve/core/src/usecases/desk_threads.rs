//! The desktop's in-thread find (`orchestration.searchThread` and its stream), answered from t3's own cache of the thread log. Only the empty
//! result is recorded; non-empty ones follow the contract over raw text (docs/gaps/t3-thread-find-segments.md).
use super::queries::{field, QueryFailure};
use super::service::Service;
use serde_json::{json, Value};
use t3_contracts::wire;

/// The longest query the contract accepts.
const QUERY_MAX: usize = 200;
/// The most navigation entries a result lists.
const NAVIGATION_MAX: usize = 17;

/// How the connection sends what a desktop read answered.
pub enum Desk {
    Answer(Value),
    /// The frames of a stream that ends once the client has taken them.
    Finite(Vec<Value>),
}

pub fn handles(tag: &str) -> bool {
    matches!(tag, "orchestration.searchThread" | "orchestration.searchThreadStream")
}

/// The pin's case fold: lower case, a final sigma as a sigma, a curly apostrophe as a straight one.
fn fold(text: &str) -> String {
    text.to_lowercase().chars().map(|c| if c == 'ς' { 'σ' } else if c == '\u{2018}' || c == '\u{2019}' { '\'' } else { c }).collect()
}

struct Entry {
    id: String,
    run: Value,
    count: u64,
}

impl Service {
    pub fn desk_threads(&self, tag: &str, payload: &Value) -> Result<Desk, QueryFailure> {
        let found = self.search_thread(payload)?;
        Ok(if tag == "orchestration.searchThreadStream" { Desk::Finite(vec![found]) } else { Desk::Answer(found) })
    }

    /// `orchestration.searchThread`: the occurrences of the query in the thread's messages and plans, and the one `index`, `start` and `offset` pick.
    fn search_thread(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        let query = field(payload, "query")?.trim();
        if query.is_empty() || query.chars().count() > QUERY_MAX {
            return Err(QueryFailure::Defect(format!("Expected a non-empty string at most {QUERY_MAX} characters long at [\"query\"]")));
        }
        if !self.thread_is_held(thread) {
            return Err(QueryFailure::Error(wire::thread_missing_error("OrchestrationV2SearchThreadError", thread, json!({}))));
        }
        self.sync_thread(thread);
        let needle = fold(query);
        let (entries, sequence): (Vec<Entry>, u64) = self.engine().read(|state| {
            let items = state.entities.get(thread).map(|e| e.turn_items.as_slice()).unwrap_or_default();
            let entries = items
                .iter()
                .filter_map(|item| {
                    let text = match item["type"].as_str()? {
                        "user_message" | "assistant_message" => item["text"].as_str()?,
                        "proposed_plan" => item["markdown"].as_str()?,
                        _ => return None,
                    };
                    let id = item["messageId"].as_str().or_else(|| item["id"].as_str())?.to_string();
                    Some(Entry { id, run: item["runId"].clone(), count: fold(text).matches(needle.as_str()).count() as u64 })
                })
                .collect();
            (entries, state.last_sequence)
        });
        Ok(select(&entries, payload, sequence))
    }
}

fn select(entries: &[Entry], payload: &Value, sequence: u64) -> Value {
    let total: u64 = entries.iter().map(|e| e.count).sum();
    let index = payload.get("index").and_then(Value::as_u64);
    let mut requested = index.unwrap_or(0);
    if let (None, Some(start)) = (index, payload.get("start").filter(|s| s.is_object())) {
        if let Some(at) = entries.iter().position(|e| Some(e.id.as_str()) == start["entryId"].as_str()) {
            requested = entries[..at].iter().map(|e| e.count).sum::<u64>() + start["occurrence"].as_u64().unwrap_or(0).min(entries[at].count.saturating_sub(1));
            if requested >= total {
                requested = 0;
            }
        }
    }
    let active = match (index, total) {
        (None, 1..) => (requested as i64 + payload["offset"].as_i64().unwrap_or(0)).rem_euclid(total as i64) as u64,
        _ => requested.min(total.saturating_sub(1)),
    };
    let mut left = active;
    let chosen = entries.iter().find_map(|e| {
        if left < e.count {
            return Some((e, left));
        }
        left -= e.count;
        None
    });
    let mut start_index = 0;
    let listed: Vec<(&Entry, u64)> = entries
        .iter()
        .filter_map(|e| {
            let at = start_index;
            start_index += e.count;
            (e.count > 0).then_some((e, at))
        })
        .collect();
    let selected = chosen.and_then(|(c, _)| listed.iter().position(|(e, _)| e.id == c.id));
    let window: Vec<&(&Entry, u64)> = match selected {
        Some(at) if listed.len() > NAVIGATION_MAX => (0..NAVIGATION_MAX).map(|i| &listed[(at + i + listed.len() - 8) % listed.len()]).collect(),
        _ => listed.iter().collect(),
    };
    let navigation: Vec<Value> = window.into_iter().map(|(e, at)| json!({"entryId": e.id, "runId": e.run, "startIndex": at, "count": e.count})).collect();
    let found = chosen.map_or(Value::Null, |(e, occurrence)| json!({"entryId": e.id, "runId": e.run, "occurrence": occurrence}));
    json!({"snapshotSequence": sequence, "totalMatches": total, "activeIndex": active, "navigation": navigation, "match": found})
}

/// What the agent API calls a CLI, as the contract's session source names it.
fn source_of(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" => Some("claudeAgent"),
        "codex" => Some("codex"),
        _ => None,
    }
}

impl Service {
    /// The sessions of the executor's CLIs grouped by the directory they ran in, each as a project to import; one t3 holds a project of is already imported.
    pub(super) fn session_candidates(&self, sessions: &[Value], scanned_at: String) -> Value {
        struct Group {
            path: String,
            sources: Vec<&'static str>,
            count: u64,
            last: u64,
        }
        let mut groups: Vec<Group> = Vec::new();
        for session in sessions {
            let (Some(path), Some(source)) = (session["cwd"].as_str().map(|p| if p.len() > 1 { p.trim_end_matches('/') } else { p }).filter(|p| !p.is_empty()), session["agent"].as_str().and_then(source_of)) else { continue };
            let at = session["modified_ms"].as_u64().unwrap_or(0);
            let group = match groups.iter().position(|g| g.path == path) {
                Some(i) => &mut groups[i],
                None => {
                    groups.push(Group { path: path.to_string(), sources: vec![], count: 0, last: 0 });
                    groups.last_mut().expect("just pushed")
                }
            };
            if !group.sources.contains(&source) {
                group.sources.push(source);
            }
            group.count += 1;
            group.last = group.last.max(at);
        }
        groups.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.path.cmp(&b.path)));
        let held: Vec<(String, String)> = self.engine().read(|state| state.projects.iter().filter(|(_, p)| !crate::domain::state::is_deleted(p)).filter_map(|(id, p)| Some((id.clone(), p["workspaceRoot"].as_str()?.trim_end_matches('/').to_string()))).collect());
        let candidates: Vec<Value> = groups
            .iter()
            .map(|g| {
                let project = held.iter().find(|(_, root)| *root == g.path);
                let title = g.path.rsplit('/').next().filter(|t| !t.is_empty()).unwrap_or(&g.path).to_string();
                let mut candidate = json!({"path": g.path, "title": title, "sources": g.sources, "threadCount": g.count, "lastActiveAt": if g.last == 0 { Value::Null } else { json!(t3_contracts::time::iso(g.last as i64)) }, "alreadyImported": project.is_some()});
                if let Some((id, _)) = project {
                    candidate["projectId"] = json!(id);
                }
                candidate
            })
            .collect();
        let mut result = json!({"candidates": candidates, "scannedAt": scanned_at});
        if sessions.len() >= 500 {
            result["truncated"] = json!(true);
        }
        result
    }
}
