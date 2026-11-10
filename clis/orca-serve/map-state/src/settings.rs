//! `settings.update`: a patch of the keys the pinned `SettingsUpdate` schema names, each held to its kind, merged into the kept object.
use crate::store::{self, SETTINGS};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

enum Kind {
    Bool,
    Text,
    OneOf(&'static [&'static str]),
    /// A list of strings, or null for none chosen.
    Selection,
    /// Kept as sent: its shape is the phone's own (nested records the schema normalises per field).
    Record,
}

const KEYS: &[(&str, Kind)] = &[
    ("worktreeVisibilityDefaults", Kind::Record),
    ("defaultTuiAgent", Kind::Record),
    ("disabledTuiAgents", Kind::Record),
    ("agentDefaultArgs", Kind::Record),
    ("agentDefaultEnv", Kind::Record),
    ("defaultTaskSource", Kind::Text),
    ("visibleTaskProviders", Kind::Record),
    ("defaultTaskViewPreset", Kind::OneOf(&["issues", "my-issues", "prs", "my-prs", "review", "all"])),
    ("experimentalNewWorktreeCardStyle", Kind::Bool),
    ("experimentalConnectors", Kind::Bool),
    ("agentStatusHooksEnabled", Kind::Bool),
    ("codexShareHistoryAcrossAccounts", Kind::Bool),
    ("claudeSubagentNavigationEnabled", Kind::Bool),
    ("defaultRepoSelection", Kind::Selection),
    ("defaultLinearTeamSelection", Kind::Selection),
    ("compactWorktreeCards", Kind::Bool),
    ("minimaxGroupId", Kind::Text),
    ("minimaxUsageModels", Kind::Text),
    ("minimaxEndpoint", Kind::OneOf(&["overseas", "cn"])),
    ("zcodePlanSite", Kind::OneOf(&["zai", "bigmodel"])),
    ("githubProjects", Kind::Record),
    ("prBotAuthorOverrides", Kind::Record),
    ("nativeChatAutoContinueAfterUsageLimit", Kind::Bool),
    ("nativeChatPermissionMode", Kind::Text),
];

/// The host's own name is set where it runs (`--label`), so the phone cannot rename it: a rename that was kept would change nothing the phone reads back.
const MACHINE_NAME: &str = "machineName";

fn check(key: &str, kind: &Kind, value: &Value) -> Result<(), Failure> {
    let fits = match kind {
        Kind::Bool => value.is_boolean(),
        Kind::Text => value.is_string(),
        Kind::OneOf(allowed) => value.as_str().is_some_and(|v| allowed.contains(&v)),
        Kind::Selection => value.is_null() || value.as_array().is_some_and(|l| l.iter().all(Value::is_string)),
        Kind::Record => true,
    };
    if fits {
        Ok(())
    } else {
        Err(Failure::invalid(format!("Invalid value for setting {key}")))
    }
}

pub(crate) fn update(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let patch = match params {
        Value::Null => return Ok(json!({"settings": store::with_defaults(SETTINGS, store::read_object(agent, SETTINGS)?)})),
        Value::Object(patch) => patch,
        _ => return Err(Failure::invalid("settings.update takes an object of the settings to change")),
    };
    for (key, value) in patch {
        if key == MACHINE_NAME {
            return Err(Failure::invalid("The host's name is set where it runs, not from a phone"));
        }
        let (_, kind) = KEYS.iter().find(|(k, _)| k == key).ok_or_else(|| Failure::invalid(format!("Unknown setting: {key}")))?;
        check(key, kind, value)?;
    }
    let mut settings = store::read_object(agent, SETTINGS)?;
    settings.extend(patch.iter().map(|(k, v)| (k.clone(), v.clone())));
    store::write(agent, SETTINGS, Value::Object(settings.clone()))?;
    Ok(json!({"settings": store::with_defaults(SETTINGS, settings)}))
}
