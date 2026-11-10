//! The `preview.sessions` group: a thread's preview tabs. The service numbers each tab (`tab`), keeps its viewport, zoom, colour scheme and
//! navigation as the app reported it, and records every change on the `preview` stream (`watch`).
use agent_api_proto::{text, GroupName, Method, ParamError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

pub const GROUP: GroupName = GroupName::new("preview.sessions");

pub const OPEN: Method = Method::new(GROUP, "preview.open");
pub const REPORT: Method = Method::new(GROUP, "preview.report");
pub const RESIZE: Method = Method::new(GROUP, "preview.resize");
pub const ADJUST: Method = Method::new(GROUP, "preview.adjust");
pub const CLOSE: Method = Method::new(GROUP, "preview.close");
pub const LIST: Method = Method::new(GROUP, "preview.list");

/// Every method of the group.
pub const METHODS: &[Method] = &[OPEN, REPORT, RESIZE, ADJUST, CLOSE, LIST];

pub const URL_MAX: usize = 2048;
pub const TITLE_MAX: usize = 512;
pub const ID_MAX: usize = 128;
pub const VIEWPORT_MIN: u32 = 240;
pub const VIEWPORT_MAX: u32 = 3840;
pub const VIEWPORT_MAX_AREA: u64 = 3840 * 2160;
/// Tabs a thread keeps, and tabs the service keeps in all.
pub const MAX_TABS_PER_THREAD: usize = 16;
pub const MAX_TABS: usize = 128;
/// The zoom factors an app steps through.
pub const ZOOM_LEVELS: [f64; 17] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];

pub mod code {
    /// No tab of that thread has that id.
    pub const SESSION_NOT_FOUND: &str = "PREVIEW_SESSION_NOT_FOUND";
    /// The thread or the service holds as many tabs as it may.
    pub const SESSION_LIMIT: &str = "PREVIEW_SESSION_LIMIT";

    pub const ALL: [&str; 2] = [SESSION_NOT_FOUND, SESSION_LIMIT];
}

pub mod message {
    pub fn not_found(thread: &str, tab: &str) -> String {
        format!("thread {thread} has no preview tab {tab}")
    }

    pub fn thread_limit(thread: &str) -> String {
        format!("thread {thread} already has {} preview tabs", super::MAX_TABS_PER_THREAD)
    }

    pub fn service_limit() -> String {
        format!("the service already holds {} preview tabs", super::MAX_TABS)
    }
}

fn read<T: DeserializeOwned>(params: &Value) -> Result<T, ParamError> {
    let params = if params.is_null() { json!({}) } else { params.clone() };
    serde_json::from_value(params).map_err(|e| ParamError(e.to_string()))
}

fn id(name: &str, value: &str) -> Result<(), ParamError> {
    if value.trim().is_empty() || value.len() > ID_MAX {
        return Err(ParamError(format!("{name} must be 1 to {ID_MAX} characters")));
    }
    Ok(())
}

/// A page address an app may show: http or https, at most `URL_MAX` bytes, no blank or control character.
pub fn check_url(url: &str) -> Result<(), ParamError> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    if rest.is_none_or(str::is_empty) || url.len() > URL_MAX || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ParamError(format!("url must be an http or https address of at most {URL_MAX} bytes")));
    }
    Ok(())
}

fn check_title(title: &str) -> Result<(), ParamError> {
    if title.len() > TITLE_MAX {
        return Err(ParamError(format!("title is at most {TITLE_MAX} bytes")));
    }
    Ok(())
}

/// How much of the page the tab shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Viewport {
    Fill {},
    Freeform { width: u32, height: u32 },
    Preset { width: u32, height: u32, preset: String },
}

impl Viewport {
    pub fn validate(&self) -> Result<(), ParamError> {
        let (width, height) = match self {
            Viewport::Fill {} => return Ok(()),
            Viewport::Freeform { width, height } => (*width, *height),
            Viewport::Preset { width, height, preset } => {
                id("viewport.preset", preset)?;
                (*width, *height)
            }
        };
        if ![width, height].iter().all(|d| (VIEWPORT_MIN..=VIEWPORT_MAX).contains(d)) {
            return Err(ParamError(format!("viewport sides must be between {VIEWPORT_MIN} and {VIEWPORT_MAX}")));
        }
        if width as u64 * height as u64 > VIEWPORT_MAX_AREA {
            return Err(ParamError(format!("viewport area is at most {VIEWPORT_MAX_AREA}")));
        }
        Ok(())
    }
}

/// How far a tab's navigation has got, as the app reported it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Nav {
    Idle {},
    Loading { url: String, #[serde(default)] title: String },
    Success { url: String, #[serde(default)] title: String },
    Failed { url: String, #[serde(default)] title: String, code: i32, description: String },
}

impl Nav {
    pub fn validate(&self) -> Result<(), ParamError> {
        match self {
            Nav::Idle {} => Ok(()),
            Nav::Loading { url, title } | Nav::Success { url, title } | Nav::Failed { url, title, .. } => {
                check_url(url)?;
                check_title(title)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clear {
    Cookies,
    Cache,
}

/// A tab, as `preview.open`, `preview.resize`, `preview.adjust` and `preview.list` answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub thread: String,
    pub tab: String,
    pub nav: Nav,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub viewport: Viewport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<ColorScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<bool>,
    pub updated_ms: u64,
}

/// `preview.open`: a new tab of `thread`, loading `url` when one is given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<bool>,
}

impl OpenParams {
    pub fn parse(params: &Value) -> Result<OpenParams, ParamError> {
        let parsed: OpenParams = read(params)?;
        id("thread", &parsed.thread)?;
        if let Some(url) = &parsed.url {
            check_url(url)?;
        }
        if let Some(viewport) = &parsed.viewport {
            viewport.validate()?;
        }
        if let Some(profile) = &parsed.profile {
            id("profile", profile)?;
        }
        Ok(parsed)
    }
}

/// `preview.report`: the app says how the tab's navigation stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub tab: String,
    pub nav: Nav,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

impl ReportParams {
    pub fn parse(params: &Value) -> Result<ReportParams, ParamError> {
        let parsed: ReportParams = read(params)?;
        id("thread", &parsed.thread)?;
        id("tab", &parsed.tab)?;
        parsed.nav.validate()?;
        Ok(parsed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResizeParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub tab: String,
    pub viewport: Viewport,
}

impl ResizeParams {
    pub fn parse(params: &Value) -> Result<ResizeParams, ParamError> {
        let parsed: ResizeParams = read(params)?;
        id("thread", &parsed.thread)?;
        id("tab", &parsed.tab)?;
        parsed.viewport.validate()?;
        Ok(parsed)
    }
}

/// `preview.adjust`: colour scheme and zoom to keep, and a reload or a clearing the app is asked to do (the service keeps neither).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default)]
    pub tab: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<ColorScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_reload: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<Clear>,
}

impl AdjustParams {
    pub fn parse(params: &Value) -> Result<AdjustParams, ParamError> {
        let parsed: AdjustParams = read(params)?;
        id("thread", &parsed.thread)?;
        id("tab", &parsed.tab)?;
        if parsed.zoom.is_some_and(|z| !ZOOM_LEVELS.contains(&z)) {
            return Err(ParamError("zoom must be one of the zoom levels".into()));
        }
        Ok(parsed)
    }
}

/// `preview.close`: one tab, or every tab of the thread when `tab` is left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseParams {
    #[serde(default)]
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
}

impl CloseParams {
    pub fn parse(params: &Value) -> Result<CloseParams, ParamError> {
        let parsed: CloseParams = read(params)?;
        id("thread", &parsed.thread)?;
        if let Some(tab) = &parsed.tab {
            id("tab", tab)?;
        }
        Ok(parsed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    #[serde(default)]
    pub thread: String,
}

impl ListParams {
    pub fn parse(params: &Value) -> Result<ListParams, ParamError> {
        let parsed: ListParams = read(params)?;
        id("thread", &parsed.thread)?;
        Ok(parsed)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionResult {
    pub session: SessionView,
}

/// `preview.list`: the thread's tabs, oldest change first, and where the stream stands (`epoch` names this run of the service, `revision` counts
/// the changes it recorded).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub sessions: Vec<SessionView>,
    pub epoch: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseResult {
    pub closed: Vec<String>,
}

/// A thread's text param, for a caller that has no use for the rest of the params.
pub fn thread_of(params: &Value) -> Option<&str> {
    text(params, "thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_viewports_around_the_bounds_when_validated_then_only_sides_and_areas_inside_are_accepted() {
        for (width, height, ok) in [(239, 800, false), (240, 240, true), (3840, 1000, true), (3841, 800, false), (3840, 2160, true), (3840, 2161, false)] {
            assert_eq!(Viewport::Freeform { width, height }.validate().is_ok(), ok, "{width}x{height}");
        }
        assert!(Viewport::Fill {}.validate().is_ok());
        assert!(Viewport::Preset { width: 390, height: 844, preset: String::new() }.validate().is_err());
    }

    #[test]
    fn given_urls_when_checked_then_only_http_and_https_addresses_without_blanks_pass() {
        for (url, ok) in [("http://127.0.0.1:3000/", true), ("https://a.test/p?q=1", true), ("javascript:alert(1)", false), ("file:///etc/passwd", false), ("http://", false), ("http://a b", false), ("ftp://a", false)] {
            assert_eq!(check_url(url).is_ok(), ok, "{url}");
        }
        assert!(check_url(&format!("http://{}", "a".repeat(URL_MAX))).is_err());
    }

    #[test]
    fn given_open_and_adjust_params_when_read_then_unknown_fields_bad_zoom_and_missing_threads_are_refused() {
        assert_eq!(OpenParams::parse(&json!({"thread": "t", "url": "http://a.test/", "viewport": {"mode": "fill"}})).unwrap().viewport, Some(Viewport::Fill {}));
        assert_eq!(OpenParams::parse(&json!({"url": "http://a.test/"})).unwrap_err().0, "thread must be 1 to 128 characters");
        assert!(OpenParams::parse(&json!({"thread": "t", "runtime": "server"})).unwrap_err().0.contains("unknown field `runtime`"));
        assert_eq!(AdjustParams::parse(&json!({"thread": "t", "tab": "a", "zoom": 1.3})).unwrap_err().0, "zoom must be one of the zoom levels");
        assert_eq!(AdjustParams::parse(&json!({"thread": "t", "tab": "a", "zoom": 1.25, "clear": "cache"})).unwrap().clear, Some(Clear::Cache));
        assert!(ReportParams::parse(&json!({"thread": "t", "tab": "a", "nav": {"state": "loading", "url": "data:x"}, "can_go_back": false, "can_go_forward": false})).is_err());
    }

    #[test]
    fn given_a_session_when_encoded_then_the_fields_keep_the_contract_names_and_unset_ones_are_absent() {
        let view = SessionView { thread: "t".into(), tab: "tab-1".into(), nav: Nav::Idle {}, can_go_back: false, can_go_forward: false, viewport: Viewport::Fill {}, color_scheme: None, zoom: None, profile: None, reveal: None, updated_ms: 9 };
        assert_eq!(serde_json::to_string(&view).unwrap(), r#"{"thread":"t","tab":"tab-1","nav":{"state":"idle"},"can_go_back":false,"can_go_forward":false,"viewport":{"mode":"fill"},"updated_ms":9}"#);
        assert_eq!(METHODS.len(), 6);
    }
}
