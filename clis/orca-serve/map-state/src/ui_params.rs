//! The `ui.set` parameters as the pin's `UiUpdate` schema reads them: strict about keys (an unknown key refuses the call), tolerant about values (a value of
//! the wrong kind is dropped, the rest applies), pairing-local keys stripped. The answer is the object of updates to merge.
use crate::ui_cleanup;
use crate::ui_values::*;
use orca_core::wire::Failure;
use serde_json::{json, Map, Value};

type Field = (&'static str, fn(&Value) -> bool, bool);

enum Spec {
    Bool,
    Num,
    Str,
    NullStr,
    StrList,
    OneOf(&'static [&'static str]),
    NullOneOf(&'static [&'static str]),
    NullNum,
    /// Per-client view choices: accepted by the schema, never stored.
    Local,
    Custom(fn(&Value) -> Option<Value>),
}

const TUI_VIEWS: &[&str] = &["terminal", "settings", "tasks", "activity", "automations", "space", "skills", "artifacts", "mobile"];
const STATUS_BAR_ITEMS: &[&str] = &["claude", "codex", "gemini", "antigravity", "opencode-go", "kimi", "minimax", "grok", "cursor", "zcode", "muse", "ssh", "resource-usage", "ports"];
const RELEASE_CHANNELS: &[&str] = &["stable", "rc", "hourly", "daily", "adhoc"];

fn record_of(v: &Value, ok: impl Fn(&Value) -> bool) -> Option<Value> {
    let o = v.as_object()?;
    o.values().all(ok).then(|| v.clone())
}

fn object_fields(v: &Value, fields: &[Field], strict: bool) -> Option<Map<String, Value>> {
    let o = v.as_object()?;
    let mut out = Map::new();
    for (key, ok, required) in fields {
        match o.get(*key) {
            Some(x) if ok(x) => {
                out.insert((*key).into(), x.clone());
            }
            Some(_) => return None,
            None if *required => return None,
            None => {}
        }
    }
    if strict && o.keys().any(|k| !fields.iter().any(|(f, _, _)| f == k)) {
        return None;
    }
    Some(out)
}

fn is_str(v: &Value) -> bool {
    v.is_string()
}
fn is_num(v: &Value) -> bool {
    v.is_number()
}
fn is_bool(v: &Value) -> bool {
    v.is_boolean()
}
fn is_str_list(v: &Value) -> bool {
    v.as_array().is_some_and(|l| l.iter().all(Value::is_string))
}
fn is_nonempty_str(v: &Value) -> bool {
    v.as_str().is_some_and(|s| !s.is_empty())
}

fn visible_hosts(v: &Value) -> Option<Value> {
    (v.is_null() || is_str_list(v)).then(|| v.clone())
}
fn right_sidebar(v: &Value) -> Option<Value> {
    is_right_sidebar_tab(v).then(|| v.clone())
}
fn card_properties(v: &Value) -> Option<Value> {
    let list = v.as_array()?;
    list.iter().all(is_card_property).then(|| worktree_card_properties(Some(v)))
}
fn status_definitions(v: &Value) -> Option<Value> {
    let list = v.as_array()?;
    let mut out = Vec::new();
    for item in list {
        let f: &[Field] = &[("id", is_str, true), ("label", is_str, true), ("color", is_str, false), ("icon", is_str, false)];
        out.push(Value::Object(object_fields(item, f, false)?));
    }
    Some(json!(out))
}
fn status_bar_items(v: &Value) -> Option<Value> {
    let list = v.as_array()?;
    list.iter().all(|i| i.as_str().is_some_and(|s| STATUS_BAR_ITEMS.contains(&s))).then(|| v.clone())
}
fn usage_format(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let mut out = Map::new();
    out.insert("template".into(), o.get("template").filter(|t| t.is_string())?.clone());
    if let Some(by) = o.get("byProvider") {
        let by = by.as_object()?;
        let mut kept = Map::new();
        for p in USAGE_PROVIDERS {
            match by.get(*p) {
                Some(x) if x.is_string() => {
                    kept.insert((*p).into(), x.clone());
                }
                Some(_) => return None,
                None => {}
            }
        }
        out.insert("byProvider".into(), Value::Object(kept));
    }
    Some(Value::Object(out))
}
fn dev_tools_dock(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let side = o.get("side")?.as_str().filter(|s| ["bottom", "right"].contains(s))?;
    let (b, r) = (o.get("bottomFraction").filter(|x| x.is_number())?, o.get("rightFraction").filter(|x| x.is_number())?);
    Some(json!({"side": side, "bottomFraction": b, "rightFraction": r}))
}
fn window_bounds(v: &Value) -> Option<Value> {
    if v.is_null() {
        return Some(Value::Null);
    }
    let f: &[Field] = &[("x", is_num, true), ("y", is_num, true), ("width", is_num, true), ("height", is_num, true)];
    object_fields(v, f, false).map(Value::Object)
}
fn record_of_objects(v: &Value) -> Option<Value> {
    let l = v.as_array()?;
    l.iter().all(Value::is_object).then(|| v.clone())
}
fn record_any(v: &Value) -> Option<Value> {
    v.is_object().then(|| v.clone())
}
fn record_nums(v: &Value) -> Option<Value> {
    record_of(v, is_num)
}
fn record_strs(v: &Value) -> Option<Value> {
    record_of(v, is_str)
}
fn record_bools(v: &Value) -> Option<Value> {
    record_of(v, is_bool)
}
fn graph_filters(v: &Value) -> Option<Value> {
    record_of(v, |x| match x {
        Value::String(s) => s == "all" || s == "head",
        Value::Array(l) => l.len() <= 1000 && l.iter().all(|n| n.as_str().is_some_and(is_git_history_ref_name)),
        _ => false,
    })
}
fn task_resume_state(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let in_set = |k: &str, allowed: &'static [&'static str]| o.get(k).is_none_or(|x| x.as_str().is_some_and(|s| allowed.contains(&s)));
    let str_or_absent = |k: &str| o.get(k).is_none_or(Value::is_string);
    let ok = in_set("githubMode", &["items", "project"])
        && o.get("githubItemsPreset").is_none_or(|x| x.is_null() || x.is_string())
        && str_or_absent("githubItemsQuery")
        && o.get("githubProjectHiddenFieldIdsByView").is_none_or(|x| record_of(x, is_str_list).is_some())
        && in_set("linearMode", &["issues", "projects", "views", "in-orca"])
        && in_set("linearPreset", &["assigned", "created", "all", "completed"])
        && str_or_absent("linearQuery")
        && o.get("linearContext").is_none_or(|c| {
            c.as_object().is_some_and(|c| {
                c.keys().all(|k| ["kind", "id", "workspaceId", "model"].contains(&k.as_str()))
                    && c.get("kind").and_then(Value::as_str).is_some_and(|k| k == "project" || k == "view")
                    && c.get("id").is_some_and(Value::is_string)
                    && c.get("workspaceId").is_some_and(Value::is_string)
                    && c.get("model").is_none_or(|m| m.as_str().is_some_and(|m| m == "issue" || m == "project"))
            })
        })
        && in_set("jiraPreset", &["assigned", "reported", "all", "done"])
        && str_or_absent("jiraQuery")
        && o.keys().all(|k| ["githubMode", "githubItemsPreset", "githubItemsQuery", "githubProjectHiddenFieldIdsByView", "linearMode", "linearPreset", "linearQuery", "linearContext", "jiraPreset", "jiraQuery"].contains(&k.as_str()));
    ok.then(|| v.clone())
}
fn workspace_cleanup(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    let dismissals = o.get("dismissals")?;
    let ok = record_of(dismissals, |d| {
        let f: &[Field] =
            &[("worktreeId", is_str, true), ("dismissedAt", is_num, true), ("fingerprint", is_str, true), ("classifierVersion", is_num, true), ("executionHostId", is_nonempty_str, false)];
        object_fields(d, f, false).is_some()
    });
    ok?;
    let mut out = Map::new();
    out.insert("dismissals".into(), dismissals.clone());
    if let Some(b) = o.get("browse") {
        out.insert("browse".into(), ui_cleanup::browse(b));
    }
    Some(Value::Object(out))
}
fn feature_tips(v: &Value) -> Option<Value> {
    v.as_array()?.iter().all(is_feature_tip_seen_id).then(|| v.clone())
}
fn feature_interaction_records(v: &Value) -> Option<Value> {
    let o = v.as_object()?;
    for (id, rec) in o {
        if !FEATURE_INTERACTION_IDS.contains(&id.as_str()) {
            return None;
        }
        let r = rec.as_object()?;
        if r.keys().any(|k| k != "firstInteractedAt" && k != "interactionCount") {
            return None;
        }
        r.get("firstInteractedAt").and_then(Value::as_f64).filter(|f| *f >= 0.0)?;
        if let Some(c) = r.get("interactionCount") {
            c.as_f64().filter(|c| c.fract() == 0.0 && *c > 0.0)?;
        }
    }
    Some(v.clone())
}

use Spec::*;

const FIELDS: &[(&str, Spec)] = &[
    ("lastActiveRepoId", NullStr),
    ("lastActiveWorktreeId", NullStr),
    ("activeView", OneOf(TUI_VIEWS)),
    ("sidebarWidth", Num),
    ("sidebarOpen", Local),
    ("rightSidebarOpen", Bool),
    ("rightSidebarTab", Custom(right_sidebar)),
    ("rightSidebarExplorerView", OneOf(&["files", "search"])),
    ("rightSidebarWidth", Num),
    ("activityBarPosition", Local),
    ("markdownTocPanelWidth", Num),
    ("combinedDiffFileTreeWidth", Num),
    ("groupBy", OneOf(&["none", "workspace-status", "repo", "pr-status"])),
    ("showWorkspaceLineage", Bool),
    ("sortBy", OneOf(&["name", "smart", "recent", "repo", "manual"])),
    ("projectOrderBy", OneOf(&["manual", "recent"])),
    ("showActiveOnly", Bool),
    ("hideSleepingWorkspaces", Bool),
    ("showSleepingWorkspaces", Bool),
    ("showInactiveWorkspaces", Bool),
    ("workspaceHostScope", Str),
    ("visibleWorkspaceHostIds", Custom(visible_hosts)),
    ("agentsVisibleHostIds", Local),
    ("agentsFilterRepoIds", Local),
    ("agentsHideWorkspacesFromOtherDevices", Local),
    ("agentsHideAutomationGeneratedWorkspaces", Local),
    ("agentsHideCliCreatedWorkspaces", Local),
    ("agentsShowChildAgents", Local),
    ("agentsCompactMode", Local),
    ("agentsShowSearch", Local),
    ("agentsReadFilter", Local),
    ("agentsGroupBy", Local),
    ("workspaceHostOrder", Local),
    ("automationHostFilter", Local),
    ("manualRepoOrder", Local),
    ("hideDefaultBranchWorkspace", Bool),
    ("hideAutomationGeneratedWorkspaces", Bool),
    ("hideCliCreatedWorkspaces", Bool),
    ("hideDetachedHeadWorkspaces", Bool),
    ("hideWorkspacesFromOtherDevices", Local),
    ("alwaysShowDefaultBranchWorkspace", Bool),
    ("filterRepoIds", StrList),
    ("explorerDisplayRootByWorktree", Custom(record_strs)),
    ("gitGraphFiltersByWorktree", Custom(graph_filters)),
    ("showDotfilesByWorktree", Custom(record_bools)),
    ("collapsedGroups", StrList),
    ("uiZoomLevel", Num),
    ("editorFontZoomLevel", Num),
    ("worktreeCardProperties", Custom(card_properties)),
    ("_worktreeCardModeDefaulted", Bool),
    ("showAgentModels", Local),
    ("agentActivityDisplayMode", OneOf(&["compact", "full"])),
    ("workspaceStatuses", Custom(status_definitions)),
    ("workspaceBoardOpacity", Num),
    ("workspaceBoardColumnWidth", Num),
    ("syncTaskStatusFromWorkspaceBoard", Bool),
    ("_workspaceStatusesDefaultOrderMigrated", Bool),
    ("_workspaceStatusesReorderedDefaultRepaired", Bool),
    ("_workspaceStatusesDefaultWorkflowMigrated", Bool),
    ("_workspaceStatusesDefaultVisualsMigrated", Bool),
    ("statusBarItems", Custom(status_bar_items)),
    ("_portsStatusBarDefaultAdded", Bool),
    ("_kimiStatusBarDefaultAdded", Bool),
    ("_minimaxStatusBarDefaultAdded", Bool),
    ("_antigravityStatusBarDefaultAdded", Bool),
    ("_grokStatusBarDefaultAdded", Bool),
    ("_cursorStatusBarDefaultAdded", Bool),
    ("_zcodeStatusBarDefaultAdded", Bool),
    ("_museStatusBarDefaultAdded", Bool),
    ("statusBarVisible", Bool),
    ("usagePercentageDisplay", OneOf(&["used", "remaining"])),
    ("statusBarUsageMode", OneOf(&["verbose", "compact"])),
    ("statusBarUsageFormat", Custom(usage_format)),
    ("dismissedUpdateVersion", NullStr),
    ("dismissedUnexpectedSignoutVersion", NullStr),
    ("lastUpdateCheckAt", NullNum),
    ("pendingUpdateNudgeId", NullStr),
    ("dismissedUpdateNudgeId", NullStr),
    ("releaseChannelOverride", NullOneOf(RELEASE_CHANNELS)),
    ("notificationPermissionRequested", Bool),
    ("updateReassuranceSeen", Bool),
    ("seenMacosUpdateRequirement", NullStr),
    ("osc52ClipboardDefaultOnNoticePending", Bool),
    ("acknowledgedAgentsByPaneKey", Custom(record_nums)),
    ("activityClearedAtByPaneKey", Local),
    ("manuallyUnreadTurnsByPaneKey", Local),
    ("browserDefaultUrl", NullStr),
    ("browserDefaultSearchEngine", NullOneOf(&["google", "duckduckgo", "bing", "kagi"])),
    ("browserDefaultZoomLevel", Num),
    ("browserKagiSessionLink", NullStr),
    ("browserDevToolsDock", Custom(dev_tools_dock)),
    ("windowBounds", Custom(window_bounds)),
    ("windowMaximized", Bool),
    ("_sortBySmartMigrated", Bool),
    ("_inlineAgentsDefaultedForExperiment", Bool),
    ("_inlineAgentsDefaultedForAllUsers", Bool),
    ("trustedOrcaHooks", Custom(record_any)),
    ("setupScriptPromptDismissedRepoIds", StrList),
    ("setupGuideSidebarDismissed", Bool),
    ("setupGuideBrowserMilestoneMigrated", Bool),
    ("setupGuideBrowserMilestoneLegacyComplete", Bool),
    ("browserImportHintHidden", Bool),
    ("mobileEmulatorTabIntroDismissed", Bool),
    ("mobileEmulatorAgentSetupDismissed", Bool),
    ("projectOrderManualDefaultNoticeDismissed", Bool),
    ("usagePercentageDisplayChangeNoticeDismissed", Bool),
    ("statusBarCompactChangeNoticeDismissed", Bool),
    ("usageEmptyStateDismissed", Bool),
    ("pluginsAlphaNoticeDismissed", Bool),
    ("codexTerminalServerIsolationNoticeSeen", Bool),
    ("codexSharedSettingsNoticeSeen", Bool),
    ("speechModelUpdateNoticeSeen", Bool),
    ("claudeDiffPanelNoticeSeen", Bool),
    ("claudeAccountSignInNoticeSeen", Bool),
    ("petVisible", Bool),
    ("petId", Str),
    ("customPets", Custom(record_of_objects)),
    ("petSize", Num),
    ("sidekickVisible", Bool),
    ("sidekickId", Str),
    ("customSidekicks", Custom(record_of_objects)),
    ("sidekickSize", Num),
    ("taskResumeState", Custom(task_resume_state)),
    ("workspaceCleanup", Custom(workspace_cleanup)),
    ("featureTipsSeenIds", Custom(feature_tips)),
    ("featureInteractions", Custom(feature_interaction_records)),
    ("contextualToursSeenIds", StrList),
    ("contextualToursAutoEligible", Bool),
];

fn read(spec: &Spec, v: &Value) -> Option<Value> {
    let keep = |ok: bool| ok.then(|| v.clone());
    match spec {
        Bool => keep(v.is_boolean()),
        Num => keep(v.is_number()),
        Str => keep(v.is_string()),
        NullStr => keep(v.is_null() || v.is_string()),
        StrList => keep(is_str_list(v)),
        OneOf(allowed) => keep(v.as_str().is_some_and(|s| allowed.contains(&s))),
        NullOneOf(allowed) => keep(v.is_null() || v.as_str().is_some_and(|s| allowed.contains(&s))),
        NullNum => keep(v.is_null() || v.is_number()),
        Local => None,
        Custom(f) => f(v),
    }
}

pub(crate) fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub(crate) fn parse(params: &Value) -> Result<Map<String, Value>, Failure> {
    let given = match params {
        Value::Null => return Ok(Map::new()),
        Value::Object(o) => o,
        other => return Err(Failure::invalid(format!("Invalid input: expected object, received {}", type_name(other)))),
    };
    let unknown: Vec<String> = given.keys().filter(|k| !FIELDS.iter().any(|(f, _)| f == k)).map(|k| format!("\"{k}\"")).collect();
    match unknown.len() {
        0 => {}
        1 => return Err(Failure::invalid(format!("Unrecognized key: {}", unknown[0]))),
        _ => return Err(Failure::invalid(format!("Unrecognized keys: {}", unknown.join(", ")))),
    }
    let mut out = Map::new();
    for (key, spec) in FIELDS {
        if let Some(v) = given.get(*key).and_then(|v| read(spec, v)) {
            out.insert((*key).into(), v);
        }
    }
    if out.get("workspaceHostScope").is_some_and(|s| !is_workspace_host_scope(s)) {
        out.remove("workspaceHostScope");
    }
    Ok(out)
}
