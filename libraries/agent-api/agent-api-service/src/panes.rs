//! The `service.panes` sub-group: the agents of the service as a person's list shows them. A pane is a thread (`thread:<id>`); the service keeps
//! for each what it is doing (from the events of its turns), when that began, whether its last finish was looked at, and whether it was cleared
//! from the list. `pane.list` reads the list, `pane.mark` marks panes read or unread, or clears them: a cleared pane is unlisted until it changes
//! again. Marks and states are kept by the service across restarts. Neutral of any app: snake_case fields, times in milliseconds since the epoch.
use agent_api_proto::{GroupName, Method, ParamError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GROUP: GroupName = GroupName::new("service.panes");

pub const LIST: Method = Method::new(GROUP, "pane.list");
pub const MARK: Method = Method::new(GROUP, "pane.mark");

/// Every method of the group.
pub const METHODS: &[Method] = &[LIST, MARK];

/// The most panes one `pane.mark` names, and the longest pane key.
pub const MAX_MARKED: usize = 500;
pub const MAX_KEY: usize = 512;

/// The key of the pane of a thread.
pub fn pane_key(thread: &str) -> String {
    format!("thread:{thread}")
}

/// The thread of a pane key.
pub fn thread_of(pane: &str) -> Option<&str> {
    pane.strip_prefix("thread:").filter(|t| !t.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneState {
    /// A turn runs.
    Working,
    /// The turn asks for an answer to a question.
    Waiting,
    /// The turn asks to be allowed a tool.
    Blocked,
    Done,
    Failed,
    Interrupted,
    /// No turn has run.
    Idle,
}

impl PaneState {
    /// A pane in one of these states is over: it can be cleared from the list.
    pub fn finished(self) -> bool {
        matches!(self, PaneState::Done | PaneState::Failed | PaneState::Interrupted | PaneState::Idle)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneView {
    pub pane: String,
    pub thread: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub state: PaneState,
    /// False for a pane cleared and not changed since.
    pub listed: bool,
    /// The pane finished (done or failed) and its finish was not marked read since.
    pub unread: bool,
    pub clearable: bool,
    /// When the pane last changed: the list's order, newest first.
    pub sort_at_ms: u64,
    pub state_started_at_ms: u64,
}

/// The params of `pane.list`: the panes of one workspace, or of every one.
#[derive(Debug, Clone, PartialEq)]
pub struct ListParams {
    pub workspace: Option<String>,
}

impl ListParams {
    pub fn parse(params: &Value) -> Result<ListParams, ParamError> {
        if let Some(extra) = params.as_object().and_then(|o| o.keys().find(|k| *k != "workspace")) {
            return Err(ParamError(format!("unknown field `{extra}`")));
        }
        match params.get("workspace") {
            None | Some(Value::Null) => Ok(ListParams { workspace: None }),
            Some(Value::String(w)) if !w.is_empty() => Ok(ListParams { workspace: Some(w.clone()) }),
            Some(_) => Err(ParamError("workspace must be a non-empty string".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub panes: Vec<PaneView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Read,
    Unread,
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarkParams {
    pub panes: Vec<String>,
    pub mark: Mark,
}

impl MarkParams {
    pub fn parse(params: &Value) -> Result<MarkParams, ParamError> {
        if let Some(extra) = params.as_object().and_then(|o| o.keys().find(|k| !["panes", "mark"].contains(&k.as_str()))) {
            return Err(ParamError(format!("unknown field `{extra}`")));
        }
        let panes: Vec<String> = match params.get("panes") {
            Some(Value::Array(items)) => items.iter().map(|p| p.as_str().filter(|p| !p.is_empty() && p.len() <= MAX_KEY).map(str::to_string).ok_or_else(|| ParamError(format!("each pane is a string of 1 to {MAX_KEY} characters")))).collect::<Result<_, _>>()?,
            _ => return Err(ParamError("panes is required".into())),
        };
        if panes.is_empty() || panes.len() > MAX_MARKED {
            return Err(ParamError(format!("panes names 1 to {MAX_MARKED} panes")));
        }
        let mark = match params.get("mark").and_then(Value::as_str) {
            Some("read") => Mark::Read,
            Some("unread") => Mark::Unread,
            Some("clear") => Mark::Clear,
            _ => return Err(ParamError("mark is read, unread or clear".into())),
        };
        Ok(MarkParams { panes, mark })
    }
}

/// The answer to `pane.mark`: how many of the panes named are panes of the service (the others are ignored).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkResult {
    pub marked: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_pane_keys_when_read_then_the_key_names_the_thread_and_nothing_else_is_a_pane() {
        assert_eq!(pane_key("t1"), "thread:t1");
        assert_eq!((thread_of("thread:t1"), thread_of("thread:"), thread_of("tab:1")), (Some("t1"), None, None));
        assert_eq!(METHODS, [LIST, MARK]);
        assert!([PaneState::Done, PaneState::Failed, PaneState::Interrupted, PaneState::Idle].iter().all(|s| s.finished()) && ![PaneState::Working, PaneState::Waiting, PaneState::Blocked].iter().any(|s| s.finished()));
    }

    #[test]
    fn given_the_params_of_list_and_mark_when_read_then_each_bad_one_is_refused_by_name() {
        assert_eq!(ListParams::parse(&json!({})).unwrap().workspace, None);
        assert_eq!(ListParams::parse(&json!({"workspace": "w"})).unwrap().workspace.as_deref(), Some("w"));
        assert_eq!(ListParams::parse(&json!({"workspace": ""})).unwrap_err().0, "workspace must be a non-empty string");
        assert_eq!(MarkParams::parse(&json!({"panes": ["thread:a"], "mark": "clear"})).unwrap(), MarkParams { panes: vec!["thread:a".into()], mark: Mark::Clear });
        assert_eq!(MarkParams::parse(&json!({"mark": "read"})).unwrap_err().0, "panes is required");
        assert_eq!(MarkParams::parse(&json!({"panes": [], "mark": "read"})).unwrap_err().0, "panes names 1 to 500 panes");
        assert_eq!(MarkParams::parse(&json!({"panes": [3], "mark": "read"})).unwrap_err().0, "each pane is a string of 1 to 512 characters");
        assert_eq!(MarkParams::parse(&json!({"panes": ["a"], "mark": "hide"})).unwrap_err().0, "mark is read, unread or clear");
        assert_eq!(MarkParams::parse(&json!({"panes": ["a"], "mark": "read", "x": 1})).unwrap_err().0, "unknown field `x`");
        assert!(MarkParams::parse(&json!({"panes": vec!["a"; 501], "mark": "read"})).is_err());
    }
}
