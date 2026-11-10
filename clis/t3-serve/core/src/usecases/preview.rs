//! `preview.*` and `subscribePreviewEvents`: the phone's preview tabs over the agent API's `preview.sessions` and `preview.watch` groups. The
//! service keeps which tabs a thread has, their viewport, zoom and colour scheme, and what the phone reported of their navigation; the phone draws
//! the page itself (the service starts no browser), so a tab of the `server` runtime, which is a browser on the server, is refused with its reason
//! and never opened as something else.
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Follow, Op};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use t3_contracts::time::iso;

const NOT_FOUND: &str = "PREVIEW_SESSION_NOT_FOUND";

/// Protocols the app is told are no pages, for the error it declares.
fn protocol_of(raw: &str) -> Option<String> {
    let (scheme, _) = raw.split_once(':')?;
    let valid = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    // `host:3000` is an address with a port, not a scheme
    let port_like = raw.split_once(':').is_some_and(|(_, rest)| rest.chars().next().is_some_and(|c| c.is_ascii_digit()) && !rest.starts_with("//"));
    (valid && !port_like).then(|| format!("{}:", scheme.to_ascii_lowercase()))
}

fn invalid_url(raw: &str, reason: &str, protocol: Option<String>) -> QueryFailure {
    let mut error = json!({"_tag": "PreviewInvalidUrlError", "inputLength": raw.len(), "reason": reason, "cause": reason});
    if let Some(protocol) = &protocol {
        error["protocol"] = json!(protocol);
    }
    error["message"] = json!(format!("Invalid preview URL ({reason}{}; input length {}).", protocol.map_or(String::new(), |p| format!(": {p}")), raw.len()));
    QueryFailure::Error(error)
}

/// The address the tab loads: what the phone typed, with `http://` (a local address) or `https://` put in front of a bare host, and no
/// protocol but http and https.
pub fn normalize_url(raw: &str) -> Result<String, QueryFailure> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_url(raw, "empty", None));
    }
    let url = match protocol_of(trimmed) {
        Some(protocol) if matches!(protocol.as_str(), "http:" | "https:") => trimmed.to_string(),
        Some(protocol) => return Err(invalid_url(raw, "unsupported-protocol", Some(protocol))),
        None => {
            let host = trimmed.trim_start_matches("//").split(['/', '?', '#', ':']).next().unwrap_or_default().to_ascii_lowercase();
            let local = host == "localhost" || host.starts_with('[') || host.split('.').count() == 4 && host.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
            format!("{}://{}", if local { "http" } else { "https" }, trimmed.trim_start_matches("//"))
        }
    };
    if url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid_url(raw, "parse", None));
    }
    Ok(url)
}

fn viewport_to_agent(viewport: &Value) -> Result<Value, QueryFailure> {
    match viewport["_tag"].as_str() {
        Some("fill") => Ok(json!({"mode": "fill"})),
        Some("freeform") => Ok(json!({"mode": "freeform", "width": viewport["width"], "height": viewport["height"]})),
        Some("preset") => Ok(json!({"mode": "preset", "width": viewport["width"], "height": viewport["height"], "preset": viewport["presetId"]})),
        _ => Err(QueryFailure::Defect("Expected a viewport with _tag fill, freeform or preset\n  at [\"viewport\"]".into())),
    }
}

fn viewport_from_agent(viewport: &Value) -> Value {
    match viewport["mode"].as_str() {
        Some("freeform") => json!({"_tag": "freeform", "width": viewport["width"], "height": viewport["height"]}),
        Some("preset") => json!({"_tag": "preset", "width": viewport["width"], "height": viewport["height"], "presetId": viewport["preset"]}),
        _ => json!({"_tag": "fill"}),
    }
}

fn nav_to_agent(nav: &Value) -> Result<Value, QueryFailure> {
    let text = |key: &str| nav[key].as_str().unwrap_or_default();
    match nav["_tag"].as_str() {
        Some("Idle") => Ok(json!({"state": "idle"})),
        Some("Loading") => Ok(json!({"state": "loading", "url": text("url"), "title": text("title")})),
        Some("Success") => Ok(json!({"state": "success", "url": text("url"), "title": text("title")})),
        Some("LoadFailed") => Ok(json!({"state": "failed", "url": text("url"), "title": text("title"), "code": nav["code"], "description": text("description")})),
        _ => Err(QueryFailure::Defect("Expected navStatus with _tag Idle, Loading, Success or LoadFailed\n  at [\"navStatus\"]".into())),
    }
}

fn nav_from_agent(nav: &Value) -> Value {
    let (url, title) = (&nav["url"], &nav["title"]);
    match nav["state"].as_str() {
        Some("loading") => json!({"_tag": "Loading", "url": url, "title": title}),
        Some("success") => json!({"_tag": "Success", "url": url, "title": title}),
        Some("failed") => json!({"_tag": "LoadFailed", "url": url, "title": title, "code": nav["code"], "description": nav["description"]}),
        _ => json!({"_tag": "Idle"}),
    }
}

/// The phone's `PreviewSessionSnapshot` for the agent API's session.
pub fn snapshot_from_agent(session: &Value) -> Value {
    let mut out = json!({
        "threadId": session["thread"], "tabId": session["tab"], "navStatus": nav_from_agent(&session["nav"]),
        "canGoBack": session["can_go_back"], "canGoForward": session["can_go_forward"], "viewport": viewport_from_agent(&session["viewport"]),
        "updatedAt": iso(session["updated_ms"].as_i64().unwrap_or(0)),
    });
    for (from, to) in [("color_scheme", "colorScheme"), ("zoom", "zoomFactor"), ("profile", "profileId"), ("reveal", "reveal")] {
        if !session[from].is_null() {
            out[to] = session[from].clone();
        }
    }
    out
}

/// The phone's `PreviewEvent` for an event of the agent API's `preview` stream (`{seq, type, thread, tab, at_ms, epoch, revision, ...}`).
pub fn event_from_agent(event: &Value) -> Option<Value> {
    let kind = event["type"].as_str()?.strip_prefix("preview.")?;
    let mut out = json!({"type": kind, "threadId": event["thread"], "tabId": event["tab"], "createdAt": iso(event["at_ms"].as_i64().unwrap_or(0)), "serverEpoch": event["epoch"], "revision": event["revision"]});
    match kind {
        "opened" | "navigated" => out["snapshot"] = snapshot_from_agent(&event["session"]),
        "resized" => {
            out["snapshot"] = snapshot_from_agent(&event["session"]);
            if let Some(request) = event.get("request").filter(|r| r.is_object()) {
                let mut mapped = json!({});
                if request["hard_reload"] == true {
                    mapped["hardReload"] = json!(true);
                }
                if !request["clear"].is_null() {
                    mapped["clear"] = request["clear"].clone();
                }
                out["request"] = mapped;
            }
        }
        "failed" => {
            for key in ["url", "title", "code", "description"] {
                out[key] = event[key].clone();
            }
        }
        "closed" => {}
        _ => return None,
    }
    Some(out)
}

fn refused(e: &AgentError, thread: &str, tab: &str) -> QueryFailure {
    match e {
        AgentError::Refused { code, message } if code == NOT_FOUND => QueryFailure::Error(json!({"_tag": "PreviewSessionLookupError", "threadId": thread, "tabId": tab, "message": message})),
        other => QueryFailure::Error(json!({"_tag": "UnsupportedOperationError", "message": other.message(), "method": "preview"})),
    }
}

/// Where the phone's stream of events starts: the run of the service and the last number it had given out.
pub struct PreviewCursor {
    epoch: String,
    seq: u64,
}

impl Service {
    fn preview_run(&self, method: &'static str, params: Value) -> Result<Value, AgentError> {
        self.agent().run(&Op::Preview { method, params })
    }

    /// `preview.open`
    pub fn preview_open(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        if payload["runtime"] == "server" {
            return Err(QueryFailure::Error(json!({"_tag": "UnsupportedOperationError", "method": "preview.open", "message": "preview.open with the server runtime is not offered: the service starts no browser, the app draws the page itself."})));
        }
        let mut params = json!({"thread": thread});
        if let Some(url) = payload.get("url").filter(|u| !u.is_null()) {
            params["url"] = json!(normalize_url(url.as_str().unwrap_or_default())?);
        }
        if let Some(viewport) = payload.get("viewport").filter(|v| !v.is_null()) {
            params["viewport"] = viewport_to_agent(viewport)?;
        }
        for (from, to) in [("profileId", "profile"), ("reveal", "reveal")] {
            if let Some(value) = payload.get(from).filter(|v| !v.is_null()) {
                params[to] = value.clone();
            }
        }
        let answer = self.preview_run("preview.open", params).map_err(|e| refused(&e, thread, ""))?;
        Ok(snapshot_from_agent(&answer["session"]))
    }

    /// `preview.resize`
    pub fn preview_resize(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (thread, tab) = (field(payload, "threadId")?, field(payload, "tabId")?);
        let viewport = viewport_to_agent(&payload["viewport"])?;
        let answer = self.preview_run("preview.resize", json!({"thread": thread, "tab": tab, "viewport": viewport})).map_err(|e| refused(&e, thread, tab))?;
        Ok(snapshot_from_agent(&answer["session"]))
    }

    /// `preview.adjust`
    pub fn preview_adjust(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (thread, tab) = (field(payload, "threadId")?, field(payload, "tabId")?);
        let mut params = json!({"thread": thread, "tab": tab});
        for (from, to) in [("colorScheme", "color_scheme"), ("zoomFactor", "zoom"), ("hardReload", "hard_reload"), ("clear", "clear")] {
            if let Some(value) = payload.get(from).filter(|v| !v.is_null()) {
                params[to] = value.clone();
            }
        }
        let answer = self.preview_run("preview.adjust", params).map_err(|e| refused(&e, thread, tab))?;
        Ok(snapshot_from_agent(&answer["session"]))
    }

    /// `preview.close`
    pub fn preview_close(&self, payload: &Value) -> Result<(), QueryFailure> {
        let thread = field(payload, "threadId")?;
        let mut params = json!({"thread": thread});
        if let Some(tab) = payload.get("tabId").filter(|t| !t.is_null()) {
            params["tab"] = tab.clone();
        }
        self.preview_run("preview.close", params).map(|_| ()).map_err(|e| refused(&e, thread, payload["tabId"].as_str().unwrap_or_default()))
    }

    /// `preview.list`
    pub fn preview_list(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        let answer = self.preview_run("preview.list", json!({"thread": thread})).map_err(|e| refused(&e, thread, ""))?;
        let sessions: Vec<Value> = answer["sessions"].as_array().into_iter().flatten().map(snapshot_from_agent).collect();
        Ok(json!({"sessions": sessions, "serverEpoch": answer["epoch"], "revision": answer["revision"]}))
    }

    /// `preview.reportStatus`: what the phone's page reports of its navigation.
    pub fn preview_report_status(&self, payload: &Value) -> Result<(), QueryFailure> {
        let (thread, tab) = (field(payload, "threadId")?, field(payload, "tabId")?);
        let nav = nav_to_agent(&payload["navStatus"])?;
        let flag = |key: &str| payload[key].as_bool().ok_or_else(|| QueryFailure::Defect(format!("Missing key\n  at [\"{key}\"]")));
        let params = json!({"thread": thread, "tab": tab, "nav": nav, "can_go_back": flag("canGoBack")?, "can_go_forward": flag("canGoForward")?});
        self.preview_run("preview.report", params).map(|_| ()).map_err(|e| refused(&e, thread, tab))
    }

    /// `subscribePreviewEvents`: asks where the stream stands now, so no change after this call is missed.
    pub fn open_preview_events(&self) -> Result<PreviewCursor, QueryFailure> {
        let answer = self.preview_run("preview.subscribe", json!({})).map_err(|e| refused(&e, "", ""))?;
        Ok(PreviewCursor { epoch: answer["epoch"].as_str().unwrap_or_default().to_string(), seq: answer["seq"].as_u64().unwrap_or(0) })
    }

    /// Follows the stream on a thread of its own from the cursor and sends each change as the phone's event.
    pub fn follow_preview_events(&self, cursor: PreviewCursor, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let result = svc.agent().preview_follow(&cursor.epoch, cursor.seq, &mut |event| {
                if stop.load(Ordering::SeqCst) {
                    return Follow::Stop;
                }
                match event.as_ref().and_then(event_from_agent) {
                    Some(mapped) => match to.send((id.clone(), mapped)) {
                        Ok(()) => Follow::Continue,
                        Err(_) => Follow::Stop,
                    },
                    None => Follow::Continue,
                }
            });
            if let Err(e) = result {
                svc.log("preview.follow.stopped", &[("message", e.message())]);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_addresses_the_phone_typed_when_normalized_then_bare_hosts_get_a_scheme_and_only_http_and_https_pass() {
        // Given a local host with a port, a local address, a public host and addresses that are no pages
        let ok = |raw: &str| normalize_url(raw).ok();
        // Then the local ones get http, the public one https, and an address with a scheme is kept
        assert_eq!(ok("localhost:3000/x"), Some("http://localhost:3000/x".into()));
        assert_eq!(ok("127.0.0.1:8081"), Some("http://127.0.0.1:8081".into()));
        assert_eq!(ok("example.com/a?b=1"), Some("https://example.com/a?b=1".into()));
        assert_eq!(ok(" https://a.test/ "), Some("https://a.test/".into()));
        let reason = |raw: &str| match normalize_url(raw) {
            Err(QueryFailure::Error(e)) => (e["reason"].clone(), e["protocol"].clone()),
            other => panic!("{raw}: {other:?}"),
        };
        assert_eq!(reason("file:///etc/passwd"), (json!("unsupported-protocol"), json!("file:")));
        assert_eq!(reason("javascript:alert(1)"), (json!("unsupported-protocol"), json!("javascript:")));
        assert_eq!(reason("   "), (json!("empty"), Value::Null));
        assert_eq!(reason("http://a b"), (json!("parse"), Value::Null));
    }

    #[test]
    fn given_the_agent_apis_session_when_shown_to_the_phone_then_it_is_the_snapshot_with_the_phones_names_and_unset_fields_absent() {
        // Given a session with a failed navigation, a preset viewport, a zoom and a profile
        let session = json!({"thread": "t", "tab": "tab-1", "nav": {"state": "failed", "url": "http://a.test/", "title": "", "code": -105, "description": "no"}, "can_go_back": true, "can_go_forward": false,
            "viewport": {"mode": "preset", "width": 390, "height": 844, "preset": "iphone-14"}, "zoom": 1.25, "profile": "p1", "updated_ms": 1_000});
        // When it is shown, Then the names are the phone's and the unset colour scheme and reveal are absent
        assert_eq!(snapshot_from_agent(&session), json!({"threadId": "t", "tabId": "tab-1", "navStatus": {"_tag": "LoadFailed", "url": "http://a.test/", "title": "", "code": -105, "description": "no"},
            "canGoBack": true, "canGoForward": false, "viewport": {"_tag": "preset", "width": 390, "height": 844, "presetId": "iphone-14"}, "updatedAt": "1970-01-01T00:00:01.000Z", "zoomFactor": 1.25, "profileId": "p1"}));
    }

    #[test]
    fn given_the_events_of_the_stream_when_mapped_then_each_kind_is_the_phones_event_and_an_unknown_one_is_dropped() {
        let session = json!({"thread": "t", "tab": "tab-1", "nav": {"state": "idle"}, "can_go_back": false, "can_go_forward": false, "viewport": {"mode": "fill"}, "updated_ms": 0});
        let base = |kind: &str| json!({"seq": 7, "type": kind, "thread": "t", "tab": "tab-1", "at_ms": 0, "epoch": "e1", "revision": 7});
        let with = |mut event: Value, extra: Value| {
            event.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            event
        };
        let resized = event_from_agent(&with(base("preview.resized"), json!({"session": session, "request": {"hard_reload": true, "clear": "cache"}}))).unwrap();
        assert_eq!((resized["type"].clone(), resized["request"].clone(), resized["serverEpoch"].clone(), resized["revision"].clone()), (json!("resized"), json!({"hardReload": true, "clear": "cache"}), json!("e1"), json!(7)));
        let failed = event_from_agent(&with(base("preview.failed"), json!({"url": "http://a.test/", "title": "", "code": -2, "description": "x"}))).unwrap();
        assert_eq!((failed["type"].clone(), failed["code"].clone(), failed.get("snapshot").is_none()), (json!("failed"), json!(-2), true));
        assert_eq!(event_from_agent(&base("preview.closed")).unwrap(), json!({"type": "closed", "threadId": "t", "tabId": "tab-1", "createdAt": "1970-01-01T00:00:00.000Z", "serverEpoch": "e1", "revision": 7}));
        assert!(event_from_agent(&with(base("preview.opened"), json!({"session": session}))).unwrap()["snapshot"]["navStatus"] == json!({"_tag": "Idle"}));
        assert!(event_from_agent(&base("schedule.run")).is_none() && event_from_agent(&base("preview.moved")).is_none());
    }
}
