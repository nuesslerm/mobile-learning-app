//! The UI state as the pin keeps it: `apply` is `updatePersistedUI` (merge an update over the kept state and normalise the fields it names) and `project` is
//! `getPersistedUI` minus the pairing-local keys (what `ui.get` and `ui.set` answer).
use crate::ui_cleanup;
use crate::ui_values::*;
use serde_json::{json, Map, Value};

const PAIRING_LOCAL: &[&str] = &[
    "automationHostFilter", "hideWorkspacesFromOtherDevices", "manualRepoOrder", "workspaceHostOrder", "sidebarOpen", "activityBarPosition", "showAgentModels", "agentsVisibleHostIds", "agentsFilterRepoIds",
    "agentsHideWorkspacesFromOtherDevices", "agentsHideAutomationGeneratedWorkspaces", "agentsHideCliCreatedWorkspaces", "agentsShowChildAgents", "agentsCompactMode", "agentsShowSearch", "agentsReadFilter",
    "agentsGroupBy", "activityClearedAtByPaneKey", "manuallyUnreadTurnsByPaneKey",
];

fn defaults() -> Map<String, Value> {
    let all: Value = serde_json::from_str(include_str!("defaults.json")).expect("defaults.json parses");
    all["ui"].as_object().cloned().unwrap_or_default()
}

fn default_of(key: &str) -> Value {
    defaults().get(key).cloned().unwrap_or(Value::Null)
}

fn present<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(k)
}

/// `a ?? b`: a value that is absent or null falls through.
fn either<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    a.filter(|v| !v.is_null()).or(b)
}

/// The kept state after `updates` (already sanitised) are applied; an update of only `activeView` changes nothing else.
pub(crate) fn apply(kept: &Map<String, Value>, updates: &Map<String, Value>) -> Map<String, Value> {
    let mut state = kept.clone();
    if let Some(view) = updates.get("activeView") {
        state.insert("activeView".into(), view.clone());
    }
    let durable: Map<String, Value> = updates.iter().filter(|(k, _)| *k != "activeView").map(|(k, v)| (k.clone(), v.clone())).collect();
    if durable.is_empty() {
        return state;
    }
    let mut current = defaults();
    current.extend(state.clone());
    let (stored, up) = (&state, &durable);
    let tab_default = default_of("rightSidebarTab");
    let next_tab = right_sidebar_tab(present(up, "rightSidebarTab").or(stored.get("rightSidebarTab")), &tab_default);
    let view_default = default_of("rightSidebarExplorerView");
    let next_view = if present(up, "rightSidebarExplorerView").is_some() {
        right_sidebar_explorer_view(present(up, "rightSidebarExplorerView"), Some(&next_tab), &view_default)
    } else if present(up, "rightSidebarTab").and_then(Value::as_str) == Some("search") {
        json!("search")
    } else {
        right_sidebar_explorer_view(stored.get("rightSidebarExplorerView"), Some(&next_tab), &view_default)
    };
    let next_visible = visible_execution_host_ids(present(up, "visibleWorkspaceHostIds").or(stored.get("visibleWorkspaceHostIds")));
    let mut next = current.clone();
    next.extend(durable.clone());
    let cleanup = match up.get("workspaceCleanup") {
        None => current.get("workspaceCleanup").cloned(),
        Some(incoming) => {
            let mut merged = current.get("workspaceCleanup").and_then(Value::as_object).cloned().unwrap_or_default();
            merged.extend(incoming.as_object().cloned().unwrap_or_default());
            Some(Value::Object(merged))
        }
    };
    if let Some(c) = cleanup {
        next.insert("workspaceCleanup".into(), c);
    }
    let by_update = |k: &str| present(up, k).or(stored.get(k));
    next.insert("groupBy".into(), group_by(by_update("groupBy"), &default_of("groupBy")));
    next.insert("sortBy".into(), sort_by(by_update("sortBy"), &default_of("sortBy")));
    next.insert("projectOrderBy".into(), project_order_by(by_update("projectOrderBy"), &default_of("projectOrderBy")));
    next.insert("rightSidebarTab".into(), next_tab);
    next.insert("rightSidebarExplorerView".into(), next_view);
    next.insert("worktreeCardProperties".into(), worktree_card_properties(by_update("worktreeCardProperties")));
    next.insert("agentActivityDisplayMode".into(), agent_activity_display_mode(by_update("agentActivityDisplayMode")));
    next.insert("workspaceStatuses".into(), workspace_statuses(by_update("workspaceStatuses")));
    next.insert("workspaceBoardOpacity".into(), clamp_workspace_board_opacity(either(present(up, "workspaceBoardOpacity"), stored.get("workspaceBoardOpacity"))));
    next.insert("workspaceBoardColumnWidth".into(), clamp_workspace_board_column_width(either(present(up, "workspaceBoardColumnWidth"), stored.get("workspaceBoardColumnWidth"))));
    next.insert("syncTaskStatusFromWorkspaceBoard".into(), json!(by_update("syncTaskStatusFromWorkspaceBoard") == Some(&json!(true))));
    next.insert("usagePercentageDisplay".into(), usage_percentage_display(either(present(up, "usagePercentageDisplay"), stored.get("usagePercentageDisplay"))));
    next.insert("statusBarUsageMode".into(), status_bar_usage_mode(either(present(up, "statusBarUsageMode"), stored.get("statusBarUsageMode"))));
    next.insert("statusBarUsageFormat".into(), status_bar_usage_format(either(present(up, "statusBarUsageFormat"), stored.get("statusBarUsageFormat"))));
    next.insert("markdownTocPanelWidth".into(), clamp_markdown_toc_panel_width(either(present(up, "markdownTocPanelWidth"), stored.get("markdownTocPanelWidth"))));
    next.insert("combinedDiffFileTreeWidth".into(), clamp_combined_diff_file_tree_width(either(present(up, "combinedDiffFileTreeWidth"), stored.get("combinedDiffFileTreeWidth"))));
    if present(up, "visibleWorkspaceHostIds").is_some() && present(up, "workspaceHostScope").is_none() {
        let scope = workspace_host_scope_for_visible_hosts(&next_visible, execution_host_scope(current.get("workspaceHostScope")));
        next.insert("workspaceHostScope".into(), json!(scope));
    }
    next.insert("visibleWorkspaceHostIds".into(), next_visible);
    next.insert("browserDefaultZoomLevel".into(), browser_page_zoom_level(either(present(up, "browserDefaultZoomLevel"), stored.get("browserDefaultZoomLevel"))));
    next.insert("explorerDisplayRootByWorktree".into(), explorer_display_root_by_worktree(by_update("explorerDisplayRootByWorktree")));
    if let Some(filters) = present(up, "gitGraphFiltersByWorktree") {
        next.insert("gitGraphFiltersByWorktree".into(), git_graph_filters(Some(filters)));
    } else if let Some(filters) = stored.get("gitGraphFiltersByWorktree") {
        next.insert("gitGraphFiltersByWorktree".into(), filters.clone());
    }
    next.insert("showDotfilesByWorktree".into(), show_dotfiles_by_worktree(by_update("showDotfilesByWorktree")));
    next.insert(
        "featureTipsSeenIds".into(),
        match present(up, "featureTipsSeenIds") {
            Some(incoming) => merge_ids(stored.get("featureTipsSeenIds"), Some(incoming), feature_tip_ids),
            None => feature_tip_ids(stored.get("featureTipsSeenIds")),
        },
    );
    next.insert(
        "contextualToursSeenIds".into(),
        match present(up, "contextualToursSeenIds") {
            Some(incoming) => merge_ids(stored.get("contextualToursSeenIds"), Some(incoming), contextual_tour_ids),
            None => contextual_tour_ids(stored.get("contextualToursSeenIds")),
        },
    );
    next.insert(
        "featureInteractions".into(),
        match present(up, "featureInteractions") {
            Some(incoming) => merge_feature_interactions(stored.get("featureInteractions"), Some(incoming)),
            None => feature_interactions(stored.get("featureInteractions")),
        },
    );
    next
}

/// What the phone reads: the pinned defaults under the kept state, every field normalised, the pairing-local keys left out.
pub(crate) fn project(kept: &Map<String, Value>) -> Value {
    let mut ui = defaults();
    ui.extend(kept.clone());
    let s = |k: &str| kept.get(k);
    let d = default_of;
    let tab = s("rightSidebarTab");
    ui.insert("groupBy".into(), group_by(s("groupBy"), &d("groupBy")));
    ui.insert("sortBy".into(), sort_by(s("sortBy"), &d("sortBy")));
    ui.insert("projectOrderBy".into(), project_order_by(s("projectOrderBy"), &d("projectOrderBy")));
    ui.insert("rightSidebarTab".into(), right_sidebar_tab(tab, &d("rightSidebarTab")));
    ui.insert("rightSidebarExplorerView".into(), right_sidebar_explorer_view(s("rightSidebarExplorerView"), tab, &d("rightSidebarExplorerView")));
    ui.insert("worktreeCardProperties".into(), worktree_card_properties(s("worktreeCardProperties")));
    ui.insert("agentActivityDisplayMode".into(), agent_activity_display_mode(s("agentActivityDisplayMode")));
    ui.insert("workspaceStatuses".into(), workspace_statuses(s("workspaceStatuses")));
    ui.insert("workspaceBoardOpacity".into(), clamp_workspace_board_opacity(s("workspaceBoardOpacity")));
    ui.insert("workspaceBoardColumnWidth".into(), clamp_workspace_board_column_width(s("workspaceBoardColumnWidth")));
    ui.insert("syncTaskStatusFromWorkspaceBoard".into(), json!(s("syncTaskStatusFromWorkspaceBoard") == Some(&json!(true))));
    ui.insert("usagePercentageDisplay".into(), usage_percentage_display(s("usagePercentageDisplay")));
    ui.insert("statusBarUsageMode".into(), status_bar_usage_mode(s("statusBarUsageMode")));
    ui.insert("statusBarUsageFormat".into(), status_bar_usage_format(s("statusBarUsageFormat")));
    ui.insert("statusBarCompactChangeNoticeDismissed".into(), json!(s("statusBarCompactChangeNoticeDismissed") != Some(&json!(false))));
    ui.insert("trayMinimizeNoticeShown".into(), json!(s("trayMinimizeNoticeShown") == Some(&json!(true))));
    ui.insert("osc52ClipboardDefaultOnNoticePending".into(), json!(s("osc52ClipboardDefaultOnNoticePending") == Some(&json!(true))));
    ui.insert("markdownTocPanelWidth".into(), clamp_markdown_toc_panel_width(s("markdownTocPanelWidth")));
    ui.insert("combinedDiffFileTreeWidth".into(), clamp_combined_diff_file_tree_width(s("combinedDiffFileTreeWidth")));
    ui.insert("visibleWorkspaceHostIds".into(), visible_execution_host_ids(s("visibleWorkspaceHostIds")));
    ui.insert("browserDefaultZoomLevel".into(), browser_page_zoom_level(s("browserDefaultZoomLevel")));
    ui.insert("explorerDisplayRootByWorktree".into(), explorer_display_root_by_worktree(s("explorerDisplayRootByWorktree")));
    ui.insert("gitGraphFiltersByWorktree".into(), git_graph_filters(s("gitGraphFiltersByWorktree")));
    ui.insert("showDotfilesByWorktree".into(), show_dotfiles_by_worktree(s("showDotfilesByWorktree")));
    ui.insert("featureTipsSeenIds".into(), feature_tip_ids(s("featureTipsSeenIds")));
    ui.insert("contextualToursSeenIds".into(), contextual_tour_ids(s("contextualToursSeenIds")));
    ui.insert("featureInteractions".into(), feature_interactions(s("featureInteractions")));
    ui.retain(|k, _| !PAIRING_LOCAL.contains(&k.as_str()));
    let _ = ui_cleanup::browse;
    Value::Object(ui)
}
