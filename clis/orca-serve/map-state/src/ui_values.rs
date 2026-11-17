//! The small normalisers behind `ui.get` and `ui.set`, ported from `references/orca/src/shared` at the pin: id catalogs, execution host ids,
//! numeric clamps and the workspace status list. Each function takes what the persisted state may hold (anything) and answers what Orca derives.
use serde_json::{json, Map, Value};

/// A JS number as JSON: whole values print without a fraction, as `JSON.stringify` does.
pub(crate) fn num(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9e15 {
        json!(x as i64)
    } else {
        json!(x)
    }
}

fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

pub(crate) fn js_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// `String.prototype.trim`.
pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(js_whitespace)
}

pub(crate) fn one_of(value: Option<&Value>, allowed: &[&str]) -> Option<String> {
    value.and_then(Value::as_str).filter(|v| allowed.contains(v)).map(String::from)
}

pub(crate) fn as_record(value: Option<&Value>) -> Map<String, Value> {
    value.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn pick<'a>(value: Option<&'a Value>, fallback: &'a str, allowed: &[&str]) -> Value {
    json!(value.and_then(Value::as_str).filter(|v| allowed.contains(v)).unwrap_or(fallback))
}

pub(crate) fn group_by(v: Option<&Value>, default: &Value) -> Value {
    match v.and_then(Value::as_str) {
        Some("flat") => json!("none"),
        _ => pick(v, default.as_str().unwrap_or("none"), &["none", "workspace-status", "repo", "pr-status"]),
    }
}

pub(crate) fn sort_by(v: Option<&Value>, default: &Value) -> Value {
    pick(v, default.as_str().unwrap_or("smart"), &["smart", "recent", "repo", "name", "manual"])
}

pub(crate) fn project_order_by(v: Option<&Value>, default: &Value) -> Value {
    pick(v, default.as_str().unwrap_or("manual"), &["manual", "recent"])
}

pub(crate) const STATIC_RIGHT_SIDEBAR_TABS: &[&str] = &["explorer", "search", "vault", "workspaces", "pr-checks", "source-control", "checks", "ports"];

fn plugin_id_part(s: &str) -> bool {
    !s.is_empty() && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// `plugin:<publisher>.<id>/<panel>`: both halves of the qualified key are safe plugin ids (at most 64 characters), the panel a manifest id.
pub(crate) fn is_plugin_panel_tab_key(tab: &str) -> bool {
    let Some(rest) = tab.strip_prefix("plugin:") else { return false };
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() != 2 {
        return false;
    }
    let key: Vec<&str> = parts[0].split('.').collect();
    key.len() == 2 && key.iter().all(|p| p.len() <= 64 && plugin_id_part(p)) && plugin_id_part(parts[1])
}

pub(crate) fn is_right_sidebar_tab(v: &Value) -> bool {
    v.as_str().is_some_and(|t| STATIC_RIGHT_SIDEBAR_TABS.contains(&t) || is_plugin_panel_tab_key(t))
}

pub(crate) fn right_sidebar_tab(v: Option<&Value>, default: &Value) -> Value {
    match v {
        Some(t) if is_right_sidebar_tab(t) => t.clone(),
        _ => default.clone(),
    }
}

pub(crate) fn right_sidebar_explorer_view(view: Option<&Value>, tab: Option<&Value>, default: &Value) -> Value {
    match one_of(view, &["files", "search"]) {
        Some(v) => json!(v),
        None if tab.and_then(Value::as_str) == Some("search") => json!("search"),
        None => default.clone(),
    }
}

const CARD_PROPERTIES: &[&str] = &["status", "unread", "ci", "branch", "issue", "linear-issue", "jira-issue", "pr", "automation", "cli", "comment", "ports", "inline-agents", "host"];
const DEFAULT_CARD_PROPERTIES: &[&str] = &["status", "unread", "issue", "linear-issue", "jira-issue", "pr", "automation", "cli", "comment", "ports", "inline-agents", "host"];

pub(crate) fn is_card_property(v: &Value) -> bool {
    v.as_str().is_some_and(|p| CARD_PROPERTIES.contains(&p))
}

/// The fixed pair first, then the known properties in catalog order.
pub(crate) fn worktree_card_properties(v: Option<&Value>) -> Value {
    let source: Vec<&str> = match v {
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
        _ => DEFAULT_CARD_PROPERTIES.to_vec(),
    };
    let mut out: Vec<&str> = vec!["status", "unread"];
    for p in CARD_PROPERTIES {
        if source.contains(p) && !out.contains(p) {
            out.push(p);
        }
    }
    json!(out)
}

pub(crate) fn agent_activity_display_mode(v: Option<&Value>) -> Value {
    pick(v, "compact", &["compact", "full"])
}

pub(crate) fn usage_percentage_display(v: Option<&Value>) -> Value {
    pick(v, "used", &["used", "remaining"])
}

pub(crate) fn status_bar_usage_mode(v: Option<&Value>) -> Value {
    pick(v, "compact", &["verbose", "compact"])
}

pub(crate) const USAGE_PROVIDERS: &[&str] = &["claude", "codex", "gemini", "opencode-go", "kimi", "minimax", "grok", "antigravity", "cursor", "zcode", "muse"];

pub(crate) fn status_bar_usage_format(v: Option<&Value>) -> Value {
    let Some(Value::Object(raw)) = v else { return json!({"template": ""}) };
    let template = raw.get("template").and_then(Value::as_str).unwrap_or("");
    let by_provider: Map<String, Value> = match raw.get("byProvider") {
        Some(Value::Object(m)) => m.iter().filter(|(k, v)| USAGE_PROVIDERS.contains(&k.as_str()) && v.is_string()).map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Map::new(),
    };
    if by_provider.is_empty() {
        json!({"template": template})
    } else {
        json!({"template": template, "byProvider": by_provider})
    }
}

pub(crate) fn clamp_workspace_board_opacity(v: Option<&Value>) -> Value {
    match v.and_then(Value::as_f64) {
        Some(x) => num(1f64.min(0.2f64.max(js_round(x * 100.0) / 100.0))),
        None => json!(1),
    }
}

pub(crate) fn clamp_workspace_board_column_width(v: Option<&Value>) -> Value {
    match v.and_then(Value::as_f64) {
        Some(x) => num(520f64.min(220f64.max(js_round(x)))),
        None => json!(308),
    }
}

pub(crate) fn clamp_markdown_toc_panel_width(v: Option<&Value>) -> Value {
    match v.and_then(Value::as_f64) {
        Some(x) => num(600f64.min(200f64.max(x))),
        None => json!(240),
    }
}

pub(crate) fn clamp_combined_diff_file_tree_width(v: Option<&Value>) -> Value {
    match v.and_then(Value::as_f64) {
        Some(x) => num(640f64.min(200f64.max(x))),
        None => json!(256),
    }
}

const BROWSER_ZOOM_PERCENTS: [f64; 17] = [25.0, 33.0, 50.0, 67.0, 75.0, 80.0, 90.0, 100.0, 110.0, 125.0, 150.0, 175.0, 200.0, 250.0, 300.0, 400.0, 500.0];

/// Snaps any level to the nearest preset `ln(percent / 100) / ln(1.2)`; a non-number counts as 100%.
pub(crate) fn browser_page_zoom_level(v: Option<&Value>) -> Value {
    let levels: Vec<f64> = BROWSER_ZOOM_PERCENTS.iter().map(|p| (p / 100.0).ln() / 1.2f64.ln()).collect();
    let Some(level) = v.and_then(Value::as_f64) else { return num(levels[7]) };
    let mut nearest = 0;
    for i in 1..levels.len() {
        if (levels[i] - level).abs() < (levels[nearest] - level).abs() {
            nearest = i;
        }
    }
    num(levels[nearest])
}

fn host_part(id: &str, prefix: &str) -> bool {
    let Some(encoded) = id.strip_prefix(prefix) else { return false };
    !encoded.is_empty() && !encoded.contains('|') && percent_decodes(encoded)
}

/// `decodeURIComponent` succeeds: every `%` takes two hex digits and the bytes are UTF-8.
fn percent_decodes(s: &str) -> bool {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
            match hex {
                Some(byte) => out.push(byte),
                None => return false,
            }
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).is_ok_and(|d| !d.is_empty())
}

/// `local`, `ssh:<id>` or `runtime:<id>`, trimmed.
pub(crate) fn execution_host_id(v: &str) -> Option<String> {
    let id = js_trim(v);
    (id == "local" || host_part(id, "ssh:") || host_part(id, "runtime:")).then(|| id.to_string())
}

pub(crate) fn execution_host_scope(v: Option<&Value>) -> String {
    let s = v.and_then(Value::as_str).map(js_trim).unwrap_or("");
    if s.is_empty() || s == "all" {
        return "all".into();
    }
    execution_host_id(s).unwrap_or_else(|| "all".into())
}

/// Deduplicated valid host ids; an array with none is null.
pub(crate) fn visible_execution_host_ids(v: Option<&Value>) -> Value {
    let Some(Value::Array(list)) = v else { return Value::Null };
    let mut ids: Vec<String> = Vec::new();
    for id in list.iter().filter_map(Value::as_str).filter_map(execution_host_id) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        Value::Null
    } else {
        json!(ids)
    }
}

/// The scope a paired client's visible-host list implies: all hosts for none, the one host for one, otherwise the scope it had.
pub(crate) fn workspace_host_scope_for_visible_hosts(visible: &Value, current: String) -> String {
    match visible.as_array() {
        None => "all".into(),
        Some(list) if list.len() == 1 => list[0].as_str().unwrap_or("all").to_string(),
        Some(_) => current,
    }
}

pub(crate) fn is_workspace_host_scope(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s == "all" || s == "local" || s.starts_with("ssh:") || s.starts_with("runtime:"))
}

fn is_unsafe_key(k: &str) -> bool {
    k.is_empty() || matches!(k, "__proto__" | "constructor" | "prototype")
}

pub(crate) fn show_dotfiles_by_worktree(v: Option<&Value>) -> Value {
    let kept: Map<String, Value> = as_record(v).into_iter().filter(|(k, x)| !is_unsafe_key(k) && x.is_boolean()).collect();
    Value::Object(kept)
}

pub(crate) fn explorer_display_root_by_worktree(v: Option<&Value>) -> Value {
    let kept: Map<String, Value> = as_record(v).into_iter().filter(|(k, x)| !is_unsafe_key(k) && x.is_string()).collect();
    Value::Object(kept)
}

fn is_js_space(c: char) -> bool {
    js_whitespace(c)
}

/// `/^refs\/(heads|remotes|tags)\/[^\s~^:?*[\\]+$/` and no control character.
pub(crate) fn is_git_history_ref_name(name: &str) -> bool {
    ["refs/heads/", "refs/remotes/", "refs/tags/"].iter().any(|p| {
        name.strip_prefix(p).is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| !is_js_space(c) && !"~^:?*[\\".contains(c) && c as u32 > 32 && c as u32 != 127))
    })
}

pub(crate) fn git_graph_filters(v: Option<&Value>) -> Value {
    let mut out = Map::new();
    for (key, refs) in as_record(v) {
        match &refs {
            Value::String(s) if s == "all" || s == "head" => {
                out.insert(key, refs.clone());
            }
            Value::Array(list) if list.iter().all(Value::is_string) => {
                let names: Vec<&str> = list.iter().filter_map(Value::as_str).collect();
                if names.len() <= 1000 && names.iter().all(|n| is_git_history_ref_name(n)) {
                    let mut unique: Vec<&str> = Vec::new();
                    for n in names {
                        if !unique.contains(&n) {
                            unique.push(n);
                        }
                    }
                    unique.sort_by_key(|s| s.encode_utf16().collect::<Vec<u16>>());
                    out.insert(key, json!(unique));
                }
            }
            _ => {}
        }
    }
    Value::Object(out)
}

const FEATURE_TIP_SEEN_IDS: &[&str] = &["chat-ui-here", "agent-session-search", "orca-cli", "cmd-j-palette", "voice-dictation", "chat-ui-here-rollout-v1", "chat-ui-here-v2"];

const CONTEXTUAL_TOUR_IDS: &[&str] = &[
    "workspace-board",
    "workspace-agent-sessions",
    "browser",
    "browser-extensions",
    "client-hosted-browser",
    "tasks",
    "automations",
    "floating-workspace",
    "workspace-creation",
    "projects-collapse-all",
];

pub(crate) const FEATURE_INTERACTION_IDS: &[&str] = &[
    "workspace-board",
    "workspace-agent-sessions",
    "workspace-board-actions",
    "cmd-j",
    "cmd-j-workspace-open",
    "cmd-j-browser-page-open",
    "cmd-j-settings-open",
    "cmd-j-quick-action",
    "cmd-j-create-workspace",
    "browser",
    "browser-extensions",
    "client-hosted-browser",
    "browser-tab-created",
    "tasks",
    "github-tasks",
    "gitlab-tasks",
    "linear-tasks",
    "jira-tasks",
    "automations",
    "automation-created",
    "automation-run",
    "browser-annotations",
    "browser-annotations-sent-to-agent",
    "browser-grab",
    "markdown-file-created",
    "workspace-creation",
    "projects-collapse-all",
    "agent-browser-setup",
    "agent-browser-use",
    "ephemeral-vm-setup",
    "agent-orchestration-setup",
    "agent-orchestration",
    "mobile-emulator-agent-setup",
    "ai-commit-generation",
    "ai-pr-generation",
    "claude-account-switching",
    "computer-use-setup",
    "computer-use",
    "codex-account-switching",
    "cookie-import",
    "floating-workspace",
    "floating-workspace-hidden",
    "mobile-pairing",
    "notifications",
    "ports",
    "quick-commands",
    "resource-manager",
    "review-notes",
    "ssh",
    "terminal-pane-split",
    "terminal-panes",
    "terminal-tabs",
    "tab-splits",
    "usage-tracking",
    "voice-dictation",
    "workspace-cleanup",
];

pub(crate) fn is_feature_tip_seen_id(v: &Value) -> bool {
    v.as_str().is_some_and(|s| FEATURE_TIP_SEEN_IDS.contains(&s))
}

fn known_ids(v: Option<&Value>, known: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|s| known.contains(s)) {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    out
}

pub(crate) fn feature_tip_ids(v: Option<&Value>) -> Value {
    json!(known_ids(v, FEATURE_TIP_SEEN_IDS))
}

pub(crate) fn contextual_tour_ids(v: Option<&Value>) -> Value {
    json!(known_ids(v, CONTEXTUAL_TOUR_IDS))
}

/// The union of what is kept and what a client sends, known ids only, so a stale client cannot revive a seen tip or tour.
pub(crate) fn merge_ids(current: Option<&Value>, incoming: Option<&Value>, normalise: fn(Option<&Value>) -> Value) -> Value {
    let mut out: Vec<Value> = normalise(current).as_array().cloned().unwrap_or_default();
    for id in normalise(incoming).as_array().cloned().unwrap_or_default() {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    json!(out)
}

fn feature_interaction_record(v: Option<&Value>) -> Option<(f64, f64)> {
    let o = v?.as_object()?;
    let first = o.get("firstInteractedAt")?.as_f64().filter(|f| *f >= 0.0)?;
    let count = o.get("interactionCount").and_then(Value::as_f64).filter(|c| c.fract() == 0.0 && *c > 0.0).unwrap_or(1.0);
    Some((first, count))
}

pub(crate) fn feature_interactions(v: Option<&Value>) -> Value {
    let input = as_record(v);
    let mut out = Map::new();
    for id in FEATURE_INTERACTION_IDS {
        if let Some((first, count)) = feature_interaction_record(input.get(*id)) {
            out.insert((*id).into(), json!({"firstInteractedAt": num(first), "interactionCount": num(count)}));
        }
    }
    Value::Object(out)
}

/// Keep the earliest first interaction and the highest count per id, so a stale client cannot erase what the host recorded.
pub(crate) fn merge_feature_interactions(current: Option<&Value>, incoming: Option<&Value>) -> Value {
    let mut merged = feature_interactions(current).as_object().cloned().unwrap_or_default();
    for (id, record) in feature_interactions(incoming).as_object().cloned().unwrap_or_default() {
        let combined = match feature_interaction_record(merged.get(&id)) {
            Some((first, count)) => {
                let (in_first, in_count) = feature_interaction_record(Some(&record)).unwrap_or((first, count));
                json!({"firstInteractedAt": num(first.min(in_first)), "interactionCount": num(count.max(in_count))})
            }
            None => record,
        };
        merged.insert(id, combined);
    }
    Value::Object(merged)
}

const STATUS_COLORS: &[&str] = &["neutral", "blue", "sky", "violet", "amber", "emerald", "rose", "zinc", "conductor-done", "conductor-review", "conductor-progress"];
const STATUS_ICONS: &[&str] = &[
    "circle", "circle-dot", "circle-progress", "circle-dashed", "circle-ellipsis", "git-pull-request", "timer", "flag", "circle-alert", "circle-pause", "circle-play", "circle-check", "ban",
    "conductor-done", "conductor-review", "conductor-progress",
];

fn default_status_visual(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "todo" => Some(("neutral", "circle")),
        "in-progress" => Some(("conductor-progress", "conductor-progress")),
        "in-review" => Some(("conductor-review", "conductor-review")),
        "completed" => Some(("conductor-done", "conductor-done")),
        _ => None,
    }
}

pub(crate) fn default_workspace_statuses() -> Value {
    json!([
        {"id": "todo", "label": "Todo", "color": "neutral", "icon": "circle"},
        {"id": "in-progress", "label": "In progress", "color": "conductor-progress", "icon": "conductor-progress"},
        {"id": "in-review", "label": "In review", "color": "conductor-review", "icon": "conductor-review"},
        {"id": "completed", "label": "Done", "color": "conductor-done", "icon": "conductor-done"},
    ])
}

/// Runs of characters outside `keep` become one `-`, and `-` at either end goes.
fn dashed(s: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in s.chars() {
        if keep(c) {
            if gap {
                out.push('-');
                gap = false;
            }
            out.push(c);
        } else {
            gap = true;
        }
    }
    if gap {
        out.push('-');
    }
    out.trim_matches('-').to_string()
}

fn slug(label: &str) -> String {
    let s = dashed(&js_trim(label).to_lowercase(), |c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if s.is_empty() {
        "status".into()
    } else {
        s
    }
}

fn status_label(v: Option<&Value>, fallback: &str) -> String {
    let Some(s) = v.and_then(Value::as_str) else { return fallback.into() };
    let collapsed = js_trim(s).split(js_whitespace).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return fallback.into();
    }
    let mut units = 0;
    let mut out = String::new();
    for c in collapsed.chars() {
        if units + c.len_utf16() > 32 {
            break;
        }
        units += c.len_utf16();
        out.push(c);
    }
    out
}

fn status_id(v: Option<&Value>, label: &str) -> String {
    let Some(s) = v.and_then(Value::as_str) else { return slug(label) };
    let lower = js_trim(s).to_lowercase();
    if lower.is_empty() {
        return slug(label);
    }
    let id = dashed(&lower, |c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if id.is_empty() {
        "status".into()
    } else {
        id
    }
}

fn make_status_id(label: &str, taken: &[String]) -> String {
    let base = slug(label);
    if !taken.contains(&base) {
        return base;
    }
    for i in 2..100 {
        let candidate = format!("{base}-{i}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("status-{ms}")
}

/// Each entry gets a unique id, a label of at most 32 characters and a known color and icon; an empty or non-array list is the defaults.
pub(crate) fn workspace_statuses(v: Option<&Value>) -> Value {
    let Some(Value::Array(list)) = v else { return default_workspace_statuses() };
    let mut out: Vec<Value> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    for raw in list.iter().filter_map(Value::as_object) {
        let label = status_label(raw.get("label"), &format!("Status {}", out.len() + 1));
        let mut id = status_id(raw.get("id"), &label);
        if used.contains(&id) {
            id = make_status_id(&label, &ids);
        }
        used.push(id.clone());
        ids.push(id.clone());
        let color = match raw.get("color").and_then(Value::as_str).filter(|c| STATUS_COLORS.contains(c)) {
            Some(c) => c.to_string(),
            None => default_status_visual(&id).map(|(c, _)| c.to_string()).unwrap_or_else(|| STATUS_COLORS[out.len() % STATUS_COLORS.len()].to_string()),
        };
        let icon = match raw.get("icon").and_then(Value::as_str).filter(|i| STATUS_ICONS.contains(i)) {
            Some(i) => i.to_string(),
            None => default_status_visual(&id).map(|(_, i)| i.to_string()).unwrap_or_else(|| "circle-dot".into()),
        };
        out.push(json!({"id": id, "label": label, "color": color, "icon": icon}));
    }
    if out.is_empty() {
        default_workspace_statuses()
    } else {
        json!(out)
    }
}
