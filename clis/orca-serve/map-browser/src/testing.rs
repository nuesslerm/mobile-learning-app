//! A double of rca's `browser` group: pages in memory, each with a history and the pointer the executor keeps, answering as the service does (the
//! address check is the contract's own), and every input kept for the test to read. Off, it refuses every method as the service started without the
//! browser does.
use exec_browser::Policy;
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

struct Page {
    history: Vec<String>,
    at: usize,
    x: f64,
    y: f64,
    buttons: u8,
}

#[derive(Default)]
struct State {
    enabled: bool,
    pages: BTreeMap<String, Page>,
    inputs: Vec<(String, Value)>,
}

#[derive(Clone, Default)]
pub struct Browsers(Arc<Mutex<State>>);

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

fn key(p: &Value) -> Result<String, AgentError> {
    p["key"].as_str().map(str::to_string).ok_or_else(|| refused("INVALID_PARAMS", "key is required"))
}

fn title(url: &str) -> String {
    format!("Title of {url}")
}

fn shown(url: &str) -> Value {
    json!({"url": url, "title": title(url)})
}

impl Browsers {
    /// A service started with the browser on.
    pub fn on() -> Browsers {
        let b = Browsers::default();
        b.0.lock().unwrap().enabled = true;
        b
    }

    /// A service started without it.
    pub fn off() -> Browsers {
        Browsers::default()
    }

    /// The keys of the pages held now.
    pub fn pages(&self) -> Vec<String> {
        self.0.lock().unwrap().pages.keys().cloned().collect()
    }

    /// The address a page shows.
    pub fn url_of(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().pages.get(key).map(|p| p.history[p.at].clone())
    }

    /// Every input sent, as `(page key, the input params)`.
    pub fn inputs(&self) -> Vec<(String, Value)> {
        self.0.lock().unwrap().inputs.clone()
    }

    pub fn install(&self, agent: &ScriptedAgent) {
        for method in ["browser.open", "browser.goto", "browser.history", "browser.input", "browser.pages"] {
            let me = self.clone();
            agent.reply(method, move |p| me.answer(method, p));
        }
    }

    fn answer(&self, method: &str, p: &Value) -> Result<Value, AgentError> {
        let mut s = self.0.lock().unwrap();
        if !s.enabled {
            return Err(refused("BROWSER_DISABLED", agent_api_browser::message::DISABLED));
        }
        if p["workspace"].as_str().is_none() {
            return Err(refused("INVALID_PARAMS", "workspace is required"));
        }
        if method == "browser.pages" {
            return Ok(json!({"pages": s.pages.iter().map(|(k, pg)| json!({"key": k, "url": pg.history[pg.at], "idle_ms": 0})).collect::<Vec<_>>(), "max_pages": 3, "idle_limit_ms": 300_000}));
        }
        let key = key(p)?;
        let url = p["url"].as_str();
        if let Some(url) = url {
            Policy::default().check_with(url, |t| t.host.starts_with("127.") || t.host.starts_with("169.254.") || t.host.starts_with("10.")).map_err(|e| refused(e.code.as_str(), e.message))?;
        }
        if method == "browser.open" {
            if !s.pages.contains_key(&key) && s.pages.len() >= 3 {
                return Err(refused("ALREADY_EXISTS", "3 browser pages are already open; close one first"));
            }
            let page = s.pages.entry(key.clone()).or_insert(Page { history: vec!["about:blank".into()], at: 0, x: 0.0, y: 0.0, buttons: 0 });
            if let Some(url) = url {
                page.history.truncate(page.at + 1);
                page.history.push(url.to_string());
                page.at += 1;
            }
            let at = page.history[page.at].clone();
            return Ok(json!({"key": key, "url": at, "title": title(&at)}));
        }
        let Some(page) = s.pages.get_mut(&key) else { return Err(refused("NOT_FOUND", format!("no page is open for key {key}"))) };
        let at = match method {
            "browser.goto" => {
                page.history.truncate(page.at + 1);
                page.history.push(url.unwrap_or_default().to_string());
                page.at += 1;
                None
            }
            "browser.history" => match p["direction"].as_str() {
                Some("back") if page.at > 0 => {
                    page.at -= 1;
                    None
                }
                Some("forward") if page.at + 1 < page.history.len() => {
                    page.at += 1;
                    None
                }
                Some("reload") => None,
                Some("back" | "forward") => return Err(refused("IO_ERROR", "page.history: the page has no such entry in its history")),
                _ => return Err(refused("INVALID_ARGS", "`direction` must be back, forward or reload")),
            },
            _ => Some(()),
        };
        if at.is_none() {
            let now = page.history[page.at].clone();
            return Ok(json!({"key": key, "url": now, "title": title(&now)}));
        }
        match p["kind"].as_str() {
            Some("mouse_move" | "mouse_click") => {
                page.x = p["x"].as_f64().ok_or_else(|| refused("INVALID_ARGS", "`x` must be a number"))?;
                page.y = p["y"].as_f64().ok_or_else(|| refused("INVALID_ARGS", "`y` must be a number"))?;
            }
            Some("mouse_down") => page.buttons |= 1,
            Some("mouse_up") => page.buttons = 0,
            Some("key_press") if p["text"].as_str().is_some_and(|k| k.contains('+')) => return Err(refused("INVALID_ARGS", format!("`text` names a key this executor does not press: {}", p["text"].as_str().unwrap_or_default()))),
            _ => {}
        }
        let out = json!({"x": page.x, "y": page.y, "buttons": page.buttons});
        s.inputs.push((key, p.clone()));
        Ok(out)
    }
}

/// What a page shows after `url` was loaded in it.
pub fn page_of(url: &str) -> Value {
    shown(url)
}
