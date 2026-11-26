//! Maps Orca browser commands and acknowledged screencast frames to pages owned by the checkout executor.
mod screencast;

use agent_api_browser::{GOTO, HISTORY, INPUT, OPEN, PAGES};
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{received, Failure, INVALID_ARGUMENT, METHOD_NOT_FOUND};
use orca_core::{AgentApi, AgentError, Area, Ctx, Kind, Method};
use orca_map_workspace::{resolve, targets};
use serde_json::{json, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// The agent methods this crate sends.
pub const USES: &[AgentMethod] = &[agent_api_browser::FRAME, agent_api_browser::CLOSE, orca_map_workspace::layout::SET,OPEN, GOTO, HISTORY, INPUT, PAGES];

const METHODS: &[Method] = &[
    Method { name: "browser.goto", kind: Kind::Unary },
    Method { name: "browser.reload", kind: Kind::Unary },
    Method { name: "browser.back", kind: Kind::Unary },
    Method { name: "browser.forward", kind: Kind::Unary },
    Method { name: "browser.tabCreate", kind: Kind::Unary },
    Method { name: "browser.keypress", kind: Kind::Unary },
    Method { name: "browser.keyboardInsertText", kind: Kind::Unary },
    Method { name: "browser.mouseMove", kind: Kind::Unary },
    Method { name: "browser.mouseDown", kind: Kind::Unary },
    Method { name: "browser.mouseUp", kind: Kind::Unary },
    Method { name: "browser.mouseClick", kind: Kind::Unary },
    Method { name: "browser.mouseWheel", kind: Kind::Unary },
    Method { name: "browser.screencast", kind: Kind::Streaming },
    Method { name: "browser.screencast.unsubscribe", kind: Kind::Unary },
    Method { name: "browser.screencast.ack", kind: Kind::Unary },
];

const UNAVAILABLE: &str = "Browser automation is unavailable on this host.";

pub struct BrowserArea;

impl Area for BrowserArea {
    fn name(&self) -> &'static str {
        "browser"
    }

    fn capabilities(&self) -> &'static [&'static str] { &["browser.screencast.v1"] }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn open(&self, ctx: &mut Ctx, _: &str, params: &Value) -> Result<Box<dyn orca_core::Subscription>, Failure> { screencast::open(ctx, params) }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            "browser.goto" => goto(agent, params),
            "browser.reload" => history(agent, params, "reload"),
            "browser.back" => history(agent, params, "back"),
            "browser.forward" => history(agent, params, "forward"),
            "browser.tabCreate" => tab_create(ctx, params),
            "browser.keypress" => keypress(agent, params),
            "browser.keyboardInsertText" => insert_text(agent, params),
            "browser.mouseMove" => mouse_move(agent, params),
            "browser.mouseDown" => mouse_button(agent, params, "mouse_down", "pressed"),
            "browser.mouseUp" => mouse_button(agent, params, "mouse_up", "released"),
            "browser.mouseClick" => mouse_click(agent, params),
            "browser.mouseWheel" => mouse_wheel(agent, params),
            "browser.screencast.ack" => screencast_ack(ctx, params),
            "browser.screencast.unsubscribe" => {
                let id = plain(params, "subscriptionId")?.ok_or_else(|| Failure::invalid("Invalid input: expected string, received undefined"))?;
                if id.starts_with("browser-screencast:") && ctx.subscriptions.cancel(id) {
                    return Ok(json!({"unsubscribed":true}));
                }
                let page = locate(agent, None, None)?;
                ask(agent, PAGES, &page, json!({}))?;
                Ok(json!({"unsubscribed":false}))
            },
            other => Err(Failure::new(METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

/// Where a command acts: the executor's workspace and the page's key, and whether the phone named the page.
struct Page {
    workspace: String,
    key: String,
    named: Option<String>,
}

pub fn available(agent: &dyn AgentApi) -> bool {
    targets(agent).unwrap_or_default().iter().any(|target| {
        agent.request(PAGES.name(), json!({"workspace":target.workspace})).is_ok()
    })
}

pub fn close_page(agent: &dyn AgentApi, workspace: &str, key: &str) -> Result<(), Failure> {
    let page = Page { workspace: workspace.to_owned(), key: key.to_owned(), named: Some(key.to_owned()) };
    ask(agent, agent_api_browser::CLOSE, &page, json!({})).map(|_| ())
}

fn plain<'a>(params: &'a Value, key: &str) -> Result<Option<&'a str>, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
    }
}

/// Reads the target fields in the order the host's schema does: worktree and page, then `expectUrl`.
fn target_fields(params: &Value) -> Result<(Option<&str>, Option<&str>), Failure> {
    let (worktree, page) = (plain(params, "worktree")?, plain(params, "page")?);
    match params.get("expectUrl") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if is_absolute_url(s) => {
            return Err(Failure::invalid("--expect-url is not supported by this host: it does not check the page's address before a command"));
        }
        Some(_) => return Err(Failure::invalid("--expect-url must be an absolute URL, like https://example.com/chat/a")),
    }
    Ok((worktree.filter(|w| !w.is_empty()), page))
}

fn is_absolute_url(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else { return false };
    !rest.is_empty() && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn number(params: &Value, key: &str, message: &str) -> Result<f64, Failure> {
    params.get(key).and_then(Value::as_f64).ok_or_else(|| Failure::invalid(message))
}

fn locate(agent: &dyn AgentApi, worktree: Option<&str>, page: Option<&str>) -> Result<Page, Failure> {
    if let Some(page) = page {
        let workspace = match (page.rsplit_once(':'), worktree) {
            (Some((workspace, _)), _) => workspace.to_string(),
            (None, Some(selector)) => resolve(agent, selector)?.workspace,
            (None, None) => return Err(Failure::new("browser_tab_not_found", format!("Browser page {page} was not found"))),
        };
        return Ok(Page { workspace, key: page.to_string(), named: Some(page.to_string()) });
    }
    let workspace = match worktree {
        Some(selector) => resolve(agent, selector)?.workspace,
        None => targets(agent)?.into_iter().next().map(|t| t.workspace).ok_or_else(|| Failure::new("browser_unavailable", UNAVAILABLE))?,
    };
    Ok(Page { key: workspace.clone(), workspace, named: None })
}

fn refusal(error: AgentError, page: &Page) -> Failure {
    match error {
        AgentError::Refused { code, .. } if code == agent_api_browser::code::BROWSER_DISABLED => Failure::new("browser_unavailable", UNAVAILABLE),
        AgentError::Refused { code, .. } if code == "NOT_FOUND" || code == "NO_EXECUTOR" => match &page.named {
            Some(id) => Failure::new("browser_tab_not_found", format!("Browser page {id} was not found")),
            None => Failure::new("browser_no_tab", "No browser tab open in this worktree"),
        },
        AgentError::Refused { code, message } if code == "INVALID_ARGS" || code == "INVALID_PARAMS" => Failure::new(INVALID_ARGUMENT, message),
        AgentError::Refused { message, .. } => Failure::new("browser_error", message),
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

fn ask(agent: &dyn AgentApi, method: AgentMethod, page: &Page, mut params: Value) -> Result<Value, Failure> {
    params["workspace"] = json!(page.workspace);
    params["key"] = json!(page.key);
    agent.request(method.name(), params).map_err(|e| refusal(e, page))
}

fn navigated(answer: &Value) -> Value {
    json!({"url": answer["url"], "title": answer["title"]})
}

fn goto(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let url = params.get("url").and_then(Value::as_str).filter(|u| !u.is_empty()).ok_or_else(|| Failure::invalid("Missing required --url"))?;
    let page = locate(agent, worktree, page)?;
    Ok(navigated(&ask(agent, GOTO, &page, json!({"url": url}))?))
}

fn history(agent: &dyn AgentApi, params: &Value, direction: &str) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let page = locate(agent, worktree, page)?;
    Ok(navigated(&ask(agent, HISTORY, &page, json!({"direction": direction}))?))
}

fn tab_create(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    let agent = ctx.agent;
    // Whether this host has a browser at all comes first, as it does at the recorded host: before the worktree is looked up.
    let first = targets(agent)?.into_iter().next().map(|t| t.workspace).ok_or_else(|| Failure::new("browser_unavailable", UNAVAILABLE))?;
    if let Err(AgentError::Refused { code, .. }) = agent.request(PAGES.name(), json!({"workspace": first})) {
        if code == agent_api_browser::code::BROWSER_DISABLED {
            return Err(Failure::new("browser_unavailable", UNAVAILABLE));
        }
    }
    let worktree = plain(params, "worktree")?.filter(|w| !w.is_empty());
    let url = plain(params, "url")?;
    for key in ["page", "profileId", "targetGroupId", "splitFromTabId"] {
        plain(params, key)?;
    }
    if let Some(profile) = plain(params, "profileId")? {
        return Err(Failure::invalid(format!("Browser profile {profile} was not found")));
    }
    let desktop = "Opening a browser tab in a split pane needs an Orca desktop window on this host.";
    if params.get("split").is_some_and(|s| !s.is_null()) || params.pointer("/placement/kind").and_then(Value::as_str) == Some("client") {
        return Err(Failure::invalid(desktop));
    }
    let workspace = match worktree {
        Some(selector) => resolve(agent, selector)?.workspace,
        None => first,
    };
    let id = format!("{workspace}:{}", ctx.subscription_id("page"));
    let page = Page { key: id.clone(), workspace, named: None };
    let info = ask(agent, OPEN, &page, match url.filter(|u| *u != "about:blank") {
        Some(url) => json!({"url": url}),
        None => json!({}),
    })?;
    if let Err(err) = orca_map_workspace::layout::open_browser(agent, &page.workspace, &id, &info) {
        ask(agent, agent_api_browser::CLOSE, &page, json!({}))?;
        return Err(err);
    }
    Ok(json!({"browserPageId": id}))
}

fn keypress(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let key = params.get("key").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or_else(|| Failure::invalid("Missing required --key"))?;
    let page = locate(agent, worktree, page)?;
    ask(agent, INPUT, &page, json!({"kind": "key_press", "text": key}))?;
    Ok(json!({"pressed": key}))
}

fn insert_text(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let text = params.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or_else(|| Failure::invalid("Missing required --text"))?;
    let page = locate(agent, worktree, page)?;
    ask(agent, INPUT, &page, json!({"kind": "insert_text", "text": text}))?;
    Ok(json!({"inserted": true}))
}

const COORDINATES: &str = "Missing required x and y coordinates";

fn mouse_move(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let (x, y) = (number(params, "x", COORDINATES)?, number(params, "y", COORDINATES)?);
    let page = locate(agent, worktree, page)?;
    ask(agent, INPUT, &page, json!({"kind": "mouse_move", "x": x, "y": y}))?;
    Ok(json!({"moved": true}))
}

/// `left`, `middle` or `right` (the host normalises the phone's word for it); an empty or absent button is left.
fn button(params: &Value) -> Result<String, Failure> {
    match plain(params, "button")?.filter(|b| !b.is_empty()) {
        None => Ok("left".into()),
        Some(b) if ["left", "middle", "right"].contains(&b) => Ok(b.to_string()),
        Some(b) => Err(Failure::invalid(format!("Unsupported mouse button: {b}"))),
    }
}

fn mouse_button(agent: &dyn AgentApi, params: &Value, kind: &str, done: &str) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let button = plain(params, "button")?.is_some().then(|| button(params)).transpose()?;
    let page = locate(agent, worktree, page)?;
    let mut input = json!({"kind": kind});
    if let Some(button) = button {
        input["button"] = json!(button);
    }
    ask(agent, INPUT, &page, input)?;
    Ok(json!({ done: true }))
}

fn mouse_click(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let (x, y) = (number(params, "x", COORDINATES)?, number(params, "y", COORDINATES)?);
    let button = button(params)?;
    let modifiers = match params.get("modifiers") {
        None | Some(Value::Null) => json!([]),
        Some(Value::Array(all)) if all.iter().all(|m| m.as_str().is_some_and(|m| ["cmd", "ctrl", "alt", "shift"].contains(&m))) => Value::Array(all.clone()),
        Some(_) => return Err(Failure::invalid("Invalid option: expected one of \"cmd\"|\"ctrl\"|\"alt\"|\"shift\"")),
    };
    let page = locate(agent, worktree, page)?;
    ask(agent, INPUT, &page, json!({"kind": "mouse_click", "x": x, "y": y, "button": button, "modifiers": modifiers}))?;
    Ok(json!({"clicked": {"x": x, "y": y, "button": button, "adjusted": false, "handled": false}}))
}

fn mouse_wheel(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (worktree, page) = target_fields(params)?;
    let dy = number(params, "dy", "Missing required --dy")?;
    let dx = params.get("dx").and_then(Value::as_f64).unwrap_or(0.0);
    let page = locate(agent, worktree, page)?;
    ask(agent, INPUT, &page, json!({"kind": "mouse_wheel", "dx": dx, "dy": dy}))?;
    Ok(json!({"scrolled": true, "deltaX": dx, "deltaY": dy}))
}

fn screencast_ack(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    match params.get("subscriptionId") {
        Some(Value::String(s)) if !s.is_empty() => {}
        Some(Value::String(_)) => return Err(Failure::invalid("Missing required --subscription-id")),
        other => return Err(Failure::invalid(format!("Invalid input: expected string, received {}", other.map_or("undefined", received)))),
    }
    match params.get("seq").and_then(Value::as_f64) {
        Some(seq) if seq >= 0.0 && seq.fract() == 0.0 => Ok(json!({"credited": params["seq"].as_u64().and_then(|n| u32::try_from(n).ok()).is_some_and(|seq| ctx.subscriptions.acknowledge(params["subscriptionId"].as_str().unwrap_or(""), seq))})),
        Some(seq) if seq < 0.0 => Err(Failure::invalid("Too small: expected number to be >=0")),
        Some(_) => Err(Failure::invalid("Invalid input: expected int, received number")),
        None => Err(Failure::invalid(format!("Invalid input: expected number, received {}", params.get("seq").map_or("undefined", received)))),
    }
}
