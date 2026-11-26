//! The phone's workspace view preferences: what the phone's Workspace-options UI keeps (group by, sort by, project order, card layout, card
//! properties, the agent-activity layout and the hide-* filters), the server's defaults, the values a patch may carry and the merge. The pinned
//! T3 API has no method for them, so this shape is our extension; the blob is versioned, so a newer phone's state is never read as an older one.
use serde_json::{json, Value};

/// The version of the blob this server writes and reads.
pub const VERSION: u64 = 1;

/// The groups the workspace list may be grouped by.
pub const GROUP_BY: [&str; 4] = ["none", "workspace-status", "pr-status", "repo"];
/// The orders the workspace list may be sorted by.
pub const SORT_BY: [&str; 5] = ["name", "smart", "recent", "repo", "manual"];
/// The orders the projects may be listed in.
pub const PROJECT_ORDER_BY: [&str; 2] = ["manual", "recent"];
/// The layouts a worktree card may have.
pub const CARD_LAYOUT: [&str; 2] = ["detailed", "compact"];
/// The layouts the agent activity of a card may have.
pub const AGENT_ACTIVITY_DISPLAY_MODE: [&str; 2] = ["compact", "full"];

/// The properties a worktree card may show.
pub const CARD_PROPERTY_IDS: [&str; 12] = ["issue", "linear-issue", "jira-issue", "pr", "automation", "comment", "ports", "inline-agents", "host", "cli", "status", "unread"];

/// The filters a workspace list may be narrowed by.
pub const FILTER_KEYS: [&str; 9] = [
    "hideSleepingWorkspaces",
    "alwaysShowDefaultBranchWorkspace",
    "hideDefaultBranchWorkspace",
    "hideAutomationGeneratedWorkspaces",
    "hideCliCreatedWorkspaces",
    "hideDetachedHeadWorkspaces",
    "hideWorkspacesFromOtherDevices",
    "filterRepoIds",
    "visibleHostIds",
];

/// A refusal of a patch or of a stored blob: the path names where in the request it is, the message says what is expected there.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub path: String,
    pub message: String,
}

impl Refusal {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Refusal {
        Refusal { path: path.into(), message: message.into() }
    }
}

/// The preferences as this server answers them when the phone has changed none.
pub fn defaults() -> Value {
    json!({
        "groupBy": "repo",
        "sortBy": "recent",
        "projectOrderBy": "manual",
        "cardLayout": "detailed",
        "worktreeCardProperties": ["status", "unread", "issue", "linear-issue", "jira-issue", "pr", "automation", "cli", "comment", "ports", "inline-agents", "host"],
        "agentActivityDisplayMode": "compact",
        "filters": {
            "hideSleepingWorkspaces": false,
            "alwaysShowDefaultBranchWorkspace": true,
            "hideDefaultBranchWorkspace": false,
            "hideAutomationGeneratedWorkspaces": false,
            "hideCliCreatedWorkspaces": false,
            "hideDetachedHeadWorkspaces": false,
            "hideWorkspacesFromOtherDevices": false,
            "filterRepoIds": [],
            "visibleHostIds": null,
        },
    })
}

/// The blob the agent API keeps: the version and the whole preferences.
pub fn blob(preferences: &Value) -> Value {
    json!({"version": VERSION, "preferences": preferences})
}

/// The refusal of a version this server does not read or write: the version is named, so the phone knows which one it is.
pub fn version_refusal(version: f64) -> Refusal {
    Refusal::new("version", format!("View preferences version {version} is not supported: this server reads and writes version {VERSION}."))
}

/// The values of a field, as the phone may spell them.
fn literals(values: &[&str]) -> String {
    values.iter().map(|value| format!("\"{value}\"")).collect::<Vec<_>>().join(", ")
}

/// A field of the literal set the phone may send.
fn literal(value: &Value, path: &str, allowed: &[&str]) -> Result<(), Refusal> {
    let Some(text) = value.as_str() else {
        return Err(Refusal::new(path, format!("{path} must be a string.")));
    };
    if allowed.contains(&text) {
        return Ok(());
    }
    Err(Refusal::new(path, format!("{path} must be one of {}.", literals(allowed))))
}

/// A field that is a flag.
fn boolean(value: &Value, path: &str) -> Result<(), Refusal> {
    if value.is_boolean() {
        return Ok(());
    }
    Err(Refusal::new(path, format!("{path} must be a boolean.")))
}

/// A field that is a list of names.
fn strings(value: &Value, path: &str) -> Result<(), Refusal> {
    let Some(items) = value.as_array() else {
        return Err(Refusal::new(path, format!("{path} must be an array of strings.")));
    };
    items.iter().enumerate().try_for_each(|(at, item)| {
        item.is_string().then_some(()).ok_or_else(|| Refusal::new(format!("{path}[{at}]"), format!("{path}[{at}] must be a string.")))
    })
}

/// A field that is the list of properties a card shows.
fn card_properties(value: &Value, path: &str) -> Result<(), Refusal> {
    let Some(items) = value.as_array() else {
        return Err(Refusal::new(path, format!("{path} must be an array of card properties.")));
    };
    items.iter().enumerate().try_for_each(|(at, item)| {
        let Some(id) = item.as_str() else {
            return Err(Refusal::new(format!("{path}[{at}]"), format!("{path}[{at}] must be a string.")));
        };
        if CARD_PROPERTY_IDS.contains(&id) {
            return Ok(());
        }
        Err(Refusal::new(format!("{path}[{at}]"), format!("{path}[{at}] must be one of {}.", literals(&CARD_PROPERTY_IDS))))
    })
}

/// The patch as the phone sent it, checked against what this server writes: every refusal names the path it is at.
pub fn validate_patch(patch: &Value) -> Result<(), Refusal> {
    let Some(fields) = patch.as_object() else {
        return Err(Refusal::new("preferences", "preferences must be an object."));
    };
    for (key, value) in fields {
        let path = format!("preferences.{key}");
        match key.as_str() {
            "groupBy" => literal(value, &path, &GROUP_BY)?,
            "sortBy" => literal(value, &path, &SORT_BY)?,
            "projectOrderBy" => literal(value, &path, &PROJECT_ORDER_BY)?,
            "cardLayout" => literal(value, &path, &CARD_LAYOUT)?,
            "agentActivityDisplayMode" => literal(value, &path, &AGENT_ACTIVITY_DISPLAY_MODE)?,
            "worktreeCardProperties" => card_properties(value, &path)?,
            "filters" => validate_filters(value)?,
            _other => return Err(Refusal::new(path.clone(), format!("{path} is not a view preference this server stores."))),
        }
    }
    Ok(())
}

/// The filters of a patch, checked the same way.
fn validate_filters(value: &Value) -> Result<(), Refusal> {
    let Some(fields) = value.as_object() else {
        return Err(Refusal::new("preferences.filters", "preferences.filters must be an object."));
    };
    for (key, item) in fields {
        let path = format!("preferences.filters.{key}");
        match key.as_str() {
            "hideSleepingWorkspaces" | "alwaysShowDefaultBranchWorkspace" | "hideDefaultBranchWorkspace" | "hideAutomationGeneratedWorkspaces" | "hideCliCreatedWorkspaces" | "hideDetachedHeadWorkspaces" | "hideWorkspacesFromOtherDevices" => boolean(item, &path)?,
            "filterRepoIds" => strings(item, &path)?,
            "visibleHostIds" => match item {
                Value::Null => {}
                other => strings(other, &path)?,
            },
            _other => return Err(Refusal::new(path.clone(), format!("{path} is not a filter this server stores."))),
        }
    }
    Ok(())
}

/// The stored preferences with the patch over them: the filters merge by key, every other field is the patch's.
pub fn merge(stored: &Value, patch: &Value) -> Value {
    let mut merged = stored.as_object().cloned().unwrap_or_default();
    let Some(fields) = patch.as_object() else { return Value::Object(merged) };
    for (key, value) in fields {
        if key == "filters" {
            let mut filters = merged.get("filters").and_then(Value::as_object).cloned().unwrap_or_default();
            for (name, item) in value.as_object().into_iter().flatten() {
                filters.insert(name.clone(), item.clone());
            }
            merged.insert("filters".into(), Value::Object(filters));
            continue;
        }
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

/// The defaults with the stored preferences over them: a stored blob that has no value for a field keeps the default of it.
pub fn with_defaults(stored: &Value) -> Value {
    merge(&defaults(), stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_that_names_a_key_the_server_does_not_store_or_a_value_outside_its_set_is_refused_with_the_path_it_is_at() {
        // Given a patch with an unknown key, an unknown filter, a value outside the literal set and a card property that is none
        let patch = |value: Value| validate_patch(&value).unwrap_err();
        // Then each refusal names the path and what is expected there
        let unknown = patch(json!({"noSuchPreference": 1}));
        assert_eq!((unknown.path.as_str(), unknown.message.as_str()), ("preferences.noSuchPreference", "preferences.noSuchPreference is not a view preference this server stores."));
        let unknown_filter = patch(json!({"filters": {"noSuchFilter": true}}));
        assert_eq!(unknown_filter.path.as_str(), "preferences.filters.noSuchFilter");
        let literal = patch(json!({"groupBy": "by-mood"}));
        assert_eq!((literal.path.as_str(), literal.message.as_str()), ("preferences.groupBy", "preferences.groupBy must be one of \"none\", \"workspace-status\", \"pr-status\", \"repo\"."));
        let property = patch(json!({"worktreeCardProperties": ["issue", "mood"]}));
        assert_eq!(property.path.as_str(), "preferences.worktreeCardProperties[1]");
        // And a wrong type is refused as well
        assert_eq!(patch(json!({"groupBy": 7})).path.as_str(), "preferences.groupBy");
        assert_eq!(patch(json!({"filters": {"hideSleepingWorkspaces": "yes"}})).path.as_str(), "preferences.filters.hideSleepingWorkspaces");
        assert_eq!(patch(json!({"filters": {"filterRepoIds": [1]}})).path.as_str(), "preferences.filters.filterRepoIds[0]");
        assert_eq!(patch(json!({"filters": {"visibleHostIds": 7}})).path.as_str(), "preferences.filters.visibleHostIds");
    }

    #[test]
    fn a_patch_that_names_what_the_server_stores_is_accepted_and_merged_over_what_is_stored() {
        // Given a patch that changes the group, one filter and the card properties
        let patch = json!({"groupBy": "none", "filters": {"hideSleepingWorkspaces": true}, "worktreeCardProperties": ["status"]});
        // When it is checked and merged over the stored preferences
        validate_patch(&patch).unwrap();
        let merged = merge(&defaults(), &patch);
        // Then the fields of the patch are the patch's, the other filters stay, and the defaults are under them
        assert_eq!(merged["groupBy"], json!("none"));
        assert_eq!(merged["worktreeCardProperties"], json!(["status"]));
        assert_eq!(merged["filters"]["hideSleepingWorkspaces"], json!(true));
        assert_eq!(merged["filters"]["alwaysShowDefaultBranchWorkspace"], json!(true));
        assert_eq!(merged["sortBy"], json!("recent"));
    }
}
