//! The browser actions of the executor contract: a headless Chromium run where the workspace is, one page per key (a thread or a workspace), opened,
//! navigated, read, driven with the pointer and keyboard, and closed. The executor keeps at most `MAX_PAGES` pages (Chromium is the heavy part of a
//! VM with no swap, `docs/gaps/vm-memory-saturation.md`) and closes a page idle for `IDLE_MS`. An address is validated here, before any browser starts:
//! http or https, no credentials, and a host that is no internal name and, by the executor's own test of an address (it holds the sockets), no loopback,
//! unspecified, link-local, private, shared-address or metadata address, unless the executor's own `Policy` names `host:port`. The executor checks the
//! same rule again for every request the page makes.
use exec_proto::args::{opt_string, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;

pub const BROWSER_OPEN: &str = "browser_open";
pub const BROWSER_GOTO: &str = "browser_goto";
pub const BROWSER_HISTORY: &str = "browser_history";
pub const BROWSER_EVAL: &str = "browser_eval";
pub const BROWSER_SCREENSHOT: &str = "browser_screenshot";
pub const BROWSER_INPUT: &str = "browser_input";
pub const BROWSER_CLOSE: &str = "browser_close";
pub const BROWSER_LIST: &str = "browser_list";

/// The actions that wait on the page or start the browser: they run beside the reader, not in it.
pub const LONG: &[&str] = &[BROWSER_OPEN, BROWSER_GOTO, BROWSER_HISTORY, BROWSER_EVAL, BROWSER_SCREENSHOT];

pub const KEY_MAX: usize = 128;
pub const URL_MAX: usize = 2048;
pub const EXPRESSION_MAX: usize = 65_536;
pub const TEXT_MAX: usize = 4096;
/// Pages an executor keeps at once.
pub const MAX_PAGES: usize = 3;
/// How long a page may sit unused before the executor closes it, and the browser with the last one.
pub const IDLE_MS: u64 = 300_000;
/// The largest value `browser_eval` returns and the largest picture `browser_screenshot` returns, in bytes.
pub const VALUE_MAX: usize = 262_144;
pub const SCREENSHOT_MAX: usize = 4_194_304;

pub const BUTTONS: &[&str] = &["left", "middle", "right"];
pub const MODIFIERS: &[&str] = &["cmd", "ctrl", "alt", "shift"];

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
    Action {
        name: BROWSER_OPEN,
        about: "Open the page of a key (the executor's headless Chromium starts when it is the first) and, with `url`, load it; a page that is open stays. Refused beyond three pages.",
        properties: &[Property { name: "key", kind: "string", about: "The thread or workspace the page belongs to: 1 to 128 letters, digits, dots, dashes, underscores or colons" }, Property { name: "url", kind: "string", about: "An http or https address to load" }],
        required: &["key"],
    },
    Action {
        name: BROWSER_GOTO,
        about: "Load an address in the page of a key and return the page's address and title once it has loaded.",
        properties: &[Property { name: "key", kind: "string", about: "The page's key" }, Property { name: "url", kind: "string", about: "An http or https address" }],
        required: &["key", "url"],
    },
    Action {
        name: BROWSER_HISTORY,
        about: "Go back or forward in the page's history, or reload it, and return its address and title.",
        properties: &[Property { name: "key", kind: "string", about: "The page's key" }, Property { name: "direction", kind: "string", about: "back, forward or reload" }],
        required: &["key", "direction"],
    },
    Action {
        name: BROWSER_EVAL,
        about: "Evaluate a JavaScript expression in the page of a key and return its JSON value (at most 262144 bytes).",
        properties: &[Property { name: "key", kind: "string", about: "The page's key" }, Property { name: "expression", kind: "string", about: "The expression, at most 65536 bytes" }],
        required: &["key", "expression"],
    },
    Action {
        name: BROWSER_SCREENSHOT,
        about: "A PNG of the page's viewport, base64, at most 4 MiB.",
        properties: &[Property { name: "key", kind: "string", about: "The page's key" }],
        required: &["key"],
    },
    Action {
        name: BROWSER_INPUT,
        about: "One pointer or keyboard input to the page of a key; the executor tracks the pointer's position and held button for the page.",
        properties: &[
            Property { name: "key", kind: "string", about: "The page's key" },
            Property { name: "kind", kind: "string", about: "mouse_move, mouse_down, mouse_up, mouse_click, mouse_wheel, key_press or insert_text" },
            Property { name: "x", kind: "number", about: "Pointer x in CSS pixels (mouse_move, mouse_click)" },
            Property { name: "y", kind: "number", about: "Pointer y in CSS pixels (mouse_move, mouse_click)" },
            Property { name: "button", kind: "string", about: "left, middle or right (the mouse kinds; default left)" },
            Property { name: "modifiers", kind: "array", about: "cmd, ctrl, alt or shift held for a mouse_click" },
            Property { name: "dx", kind: "number", about: "Horizontal wheel delta (mouse_wheel, default 0)" },
            Property { name: "dy", kind: "number", about: "Vertical wheel delta, positive scrolls down (mouse_wheel)" },
            Property { name: "text", kind: "string", about: "The key to press or the text to insert, at most 4096 bytes" },
        ],
        required: &["key", "kind"],
    },
    Action { name: BROWSER_CLOSE, about: "Close the page of a key; the browser stops with the last page.", properties: &[Property { name: "key", kind: "string", about: "The page's key" }], required: &["key"] },
    Action { name: BROWSER_LIST, about: "The pages this executor keeps, with how long each has been idle and the limits.", properties: &[], required: &[] },
];

/// Letters, digits, dots, dashes, underscores and colons, 1 to `KEY_MAX`.
pub fn check_key(key: &str) -> ExecResult<()> {
    if key.is_empty() || key.len() > KEY_MAX || !key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':')) {
        return Err(ExecError::invalid_args(format!("`key` must be 1 to {KEY_MAX} letters, digits, dots, dashes, underscores or colons")));
    }
    Ok(())
}

/// Where an address points: its lower-case host (an IPv6 literal without brackets) and port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
}

/// A host name that is only ever the machine or its network: no public name ends this way.
fn internal_name(host: &str) -> bool {
    host == "localhost" || host == "metadata" || [".localhost", ".local", ".internal", ".localdomain", ".home.arpa"].iter().any(|s| host.ends_with(s))
}

/// Four decimal parts of 0 to 255 with no leading zero: the one spelling of an IPv4 address no browser reads another way.
fn plain_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0')) && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

/// Breaks an http or https address into its target. Refuses anything else, credentials, a missing host, a port of 0, a control or blank character,
/// and a host written as a number a browser would read as an address (`2130706433`, `0x7f.1`), whatever the policy.
pub fn parse_url(url: &str) -> ExecResult<Target> {
    let bad = |why: &str| ExecError::invalid_args(format!("`url` must be an http or https address of at most {URL_MAX} bytes: {why}"));
    if url.len() > URL_MAX || url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return Err(bad("no blank, control character or backslash"));
    }
    let (default_port, rest) = match (url.strip_prefix("http://"), url.strip_prefix("https://")) {
        (Some(r), _) => (80, r),
        (_, Some(r)) => (443, r),
        _ => return Err(bad("the scheme is not http or https")),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(bad("a host is needed and credentials are not allowed"));
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(inner) => {
            let (host, after) = inner.split_once(']').ok_or_else(|| bad("an unclosed IPv6 literal"))?;
            (host, after.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port = match port {
        None | Some("") => default_port,
        Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(|| bad("the port is not 1 to 65535"))?,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(bad("a host is needed"));
    }
    if host.contains(':') {
        if !host.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') {
            return Err(bad("the host is no address"));
        }
    } else if !plain_ipv4(&host) && host.rsplit('.').next().is_some_and(|l| !l.is_empty() && (l.chars().all(|c| c.is_ascii_digit()) || l.starts_with("0x"))) {
        return Err(bad("the host is a number that is no plain address"));
    }
    Ok(Target { host, port })
}

/// What the executor lets a page reach beyond the public internet: `host:port` pairs the operator names (the test's own dev server, a staging
/// host). Empty by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    allow: Vec<(String, u16)>,
}

impl Policy {
    /// `host:port,host:port` (an IPv6 host in brackets); an entry that is not one is an error naming it.
    pub fn parse(list: &str) -> Result<Policy, String> {
        let mut allow = Vec::new();
        for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let target = parse_url(&format!("http://{entry}")).map_err(|e| format!("browser allowlist entry {entry}: {}", e.message))?;
            if !entry.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) {
                return Err(format!("browser allowlist entry {entry} needs a port"));
            }
            allow.push((target.host, target.port));
        }
        Ok(Policy { allow })
    }

    pub fn names(&self, target: &Target) -> bool {
        self.allow.iter().any(|(h, p)| *h == target.host && *p == target.port)
    }

    /// `parse_url`, then the policy: the refusal an action answers before it starts anything. An address the policy names always passes; any other
    /// does unless its host is an internal name or `blocked` (the executor's test of an address, which holds the sockets) says its host is no place a page
    /// may reach. A public name that resolves to a blocked address is the executor's to catch with the same test.
    pub fn check_with(&self, url: &str, blocked: impl Fn(&Target) -> bool) -> ExecResult<Target> {
        let target = parse_url(url)?;
        if !self.names(&target) && (internal_name(&target.host) || blocked(&target)) {
            return Err(ExecError::invalid_args(format!("`url` points at {}:{}, an address the executor does not let a page reach", target.host, target.port)));
        }
        Ok(target)
    }

    /// Whether the host is an internal name, which no policy but a named `host:port` lets through.
    pub fn internal(&self, target: &Target) -> bool {
        !self.names(target) && internal_name(&target.host)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Back,
    Forward,
    Reload,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    MouseMove { x: f64, y: f64 },
    MouseDown { button: String },
    MouseUp { button: Option<String> },
    MouseClick { x: f64, y: f64, button: String, modifiers: Vec<String> },
    MouseWheel { dx: f64, dy: f64 },
    KeyPress { key: String },
    InsertText { text: String },
}

fn number(args: &Value, key: &str) -> ExecResult<f64> {
    args[key].as_f64().filter(|n| n.is_finite() && n.abs() <= 1.0e7).ok_or_else(|| ExecError::invalid_args(format!("`{key}` must be a number between -10000000 and 10000000")))
}

fn button(args: &Value) -> ExecResult<Option<String>> {
    let b = opt_string(args, "button")?;
    if let Some(b) = &b {
        if !BUTTONS.contains(&b.as_str()) {
            return Err(ExecError::invalid_args("`button` must be left, middle or right"));
        }
    }
    Ok(b)
}

fn text(args: &Value, what: &str) -> ExecResult<String> {
    let t = string(args, "text")?;
    if t.is_empty() || t.len() > TEXT_MAX || t.contains('\0') {
        return Err(ExecError::invalid_args(format!("`text` ({what}) must be 1 to {TEXT_MAX} bytes")));
    }
    Ok(t)
}

impl Input {
    pub fn parse(args: &Value) -> ExecResult<Input> {
        match string(args, "kind")?.as_str() {
            "mouse_move" => Ok(Input::MouseMove { x: number(args, "x")?, y: number(args, "y")? }),
            "mouse_down" => Ok(Input::MouseDown { button: button(args)?.unwrap_or_else(|| "left".into()) }),
            "mouse_up" => Ok(Input::MouseUp { button: button(args)? }),
            "mouse_click" => {
                let modifiers = match &args["modifiers"] {
                    Value::Null => Vec::new(),
                    Value::Array(all) => all.iter().map(|m| m.as_str().filter(|m| MODIFIERS.contains(m)).map(str::to_string).ok_or_else(|| ExecError::invalid_args("`modifiers` may hold cmd, ctrl, alt and shift"))).collect::<ExecResult<Vec<_>>>()?,
                    _ => return Err(ExecError::invalid_args("`modifiers` must be an array")),
                };
                Ok(Input::MouseClick { x: number(args, "x")?, y: number(args, "y")?, button: button(args)?.unwrap_or_else(|| "left".into()), modifiers })
            }
            "mouse_wheel" => Ok(Input::MouseWheel { dx: if args["dx"].is_null() { 0.0 } else { number(args, "dx")? }, dy: number(args, "dy")? }),
            "key_press" => Ok(Input::KeyPress { key: text(args, "key")? }),
            "insert_text" => Ok(Input::InsertText { text: text(args, "text")? }),
            other => Err(ExecError::invalid_args(format!("`kind` {other} is not one of mouse_move, mouse_down, mouse_up, mouse_click, mouse_wheel, key_press, insert_text"))),
        }
    }
}

/// One request of the family, read from its tool name and args. Every address is checked against the executor's policy by the executor, which owns it.
#[derive(Debug, Clone, PartialEq)]
pub enum BrowserOp {
    Open { key: String, url: Option<String> },
    Goto { key: String, url: String },
    History { key: String, direction: Direction },
    Eval { key: String, expression: String },
    Screenshot { key: String },
    Input { key: String, input: Input },
    Close { key: String },
    List,
}

fn keyed(args: &Value) -> ExecResult<String> {
    let key = string(args, "key")?;
    check_key(&key)?;
    Ok(key)
}

impl BrowserOp {
    /// None for a tool that is not a browser action.
    pub fn parse(tool: &str, args: &Value) -> Option<ExecResult<BrowserOp>> {
        let parsed = match tool {
            BROWSER_OPEN => keyed(args).and_then(|key| Ok(BrowserOp::Open { key, url: opt_string(args, "url")? })),
            BROWSER_GOTO => keyed(args).and_then(|key| Ok(BrowserOp::Goto { key, url: string(args, "url")? })),
            BROWSER_HISTORY => keyed(args).and_then(|key| {
                let direction = match string(args, "direction")?.as_str() {
                    "back" => Direction::Back,
                    "forward" => Direction::Forward,
                    "reload" => Direction::Reload,
                    _ => return Err(ExecError::invalid_args("`direction` must be back, forward or reload")),
                };
                Ok(BrowserOp::History { key, direction })
            }),
            BROWSER_EVAL => keyed(args).and_then(|key| {
                let expression = string(args, "expression")?;
                if expression.trim().is_empty() || expression.len() > EXPRESSION_MAX {
                    return Err(ExecError::invalid_args(format!("`expression` must be text of 1 to {EXPRESSION_MAX} bytes")));
                }
                Ok(BrowserOp::Eval { key, expression })
            }),
            BROWSER_SCREENSHOT => keyed(args).map(|key| BrowserOp::Screenshot { key }),
            BROWSER_INPUT => keyed(args).and_then(|key| Ok(BrowserOp::Input { key, input: Input::parse(args)? })),
            BROWSER_CLOSE => keyed(args).map(|key| BrowserOp::Close { key }),
            BROWSER_LIST => Ok(BrowserOp::List),
            _ => return None,
        };
        Some(parsed)
    }

    pub fn action(&self) -> &'static str {
        match self {
            BrowserOp::Open { .. } => BROWSER_OPEN,
            BrowserOp::Goto { .. } => BROWSER_GOTO,
            BrowserOp::History { .. } => BROWSER_HISTORY,
            BrowserOp::Eval { .. } => BROWSER_EVAL,
            BrowserOp::Screenshot { .. } => BROWSER_SCREENSHOT,
            BrowserOp::Input { .. } => BROWSER_INPUT,
            BrowserOp::Close { .. } => BROWSER_CLOSE,
            BrowserOp::List => BROWSER_LIST,
        }
    }
}

/// A page as `browser_open`, `browser_goto` and `browser_history` report it: the address and title it shows now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageInfo {
    pub key: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evaluated {
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    pub mime: String,
    pub data: String,
    pub bytes: usize,
}

/// What an input did: the pointer where the executor now holds it, and the buttons held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inputted {
    pub x: f64,
    pub y: f64,
    pub buttons: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Closed {
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageRow {
    pub key: String,
    pub url: String,
    pub idle_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub pages: Vec<PageRow>,
    pub max_pages: usize,
    pub idle_limit_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::Code;
    use serde_json::json;

    #[test]
    fn given_addresses_when_parsed_then_only_http_and_https_with_a_host_and_no_credentials_are_targets() {
        // Given addresses that are fine and addresses that are not web pages or hide a host
        let fine = [("http://example.com", ("example.com", 80)), ("https://Example.COM:8443/a?b#c", ("example.com", 8443)), ("http://[2001:db8::1]:9/x", ("2001:db8::1", 9)), ("https://example.com./", ("example.com", 443))];
        let refused = ["file:///etc/passwd", "ftp://example.com", "javascript:alert(1)", "http://", "http://user:pw@example.com", "http://example.com:0", "http://example.com:99999", "http:// example.com", "http://example.com\\@evil", "http://2130706433/", "http://0x7f.1/", "http://127.1/", "http://010.0.0.1/", "http://[zz::1]/", "about:blank"];
        for (url, (host, port)) in fine {
            // Then a fine one gives its lower-case host and port
            assert_eq!(parse_url(url).unwrap(), Target { host: host.into(), port }, "{url}");
        }
        for url in refused {
            // And each of the others is INVALID_ARGS naming `url`
            let e = parse_url(url).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("`url`"), "{url}: {e:?}");
        }
    }

    /// The executor's test of an address, stood in for here: the loopback ones.
    fn loopback(t: &Target) -> bool {
        t.host.starts_with("127.") || t.host == "::1"
    }

    #[test]
    fn given_the_default_policy_when_addresses_are_checked_then_internal_names_and_the_addresses_the_executor_blocks_are_refused_by_name() {
        // Given the empty policy and an executor that blocks loopback addresses
        let policy = Policy::default();
        for url in ["http://localhost/", "http://app.localhost/", "http://metadata.google.internal/", "http://printer.local/", "http://127.0.0.1:3000/", "http://[::1]/"] {
            // Then each is refused and the refusal names the host
            let e = policy.check_with(url, loopback).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("does not let a page reach"), "{url}: {e:?}");
        }
        for url in ["https://example.com/", "http://93.184.216.34/", "http://[2001:4860:4860::8888]/", "http://100.63.0.1/"] {
            assert!(policy.check_with(url, loopback).is_ok(), "{url}");
        }
    }

    #[test]
    fn given_an_allowlist_when_addresses_are_checked_then_exactly_the_named_host_and_port_pass() {
        // Given a policy that names the test's dev server
        let policy = Policy::parse("127.0.0.1:4100, [::1]:4200").unwrap();
        // Then that host and port pass and another port of the same host does not
        assert!(policy.check_with("http://127.0.0.1:4100/x", loopback).is_ok() && policy.check_with("http://[::1]:4200/", loopback).is_ok());
        assert!(policy.check_with("http://127.0.0.1:4101/", loopback).is_err() && policy.check_with("http://localhost:4100/", loopback).is_err());
        assert!(policy.internal(&Target { host: "localhost".into(), port: 80 }) && !policy.internal(&Target { host: "127.0.0.1".into(), port: 4100 }));
        // And an entry without a port or that is no host is an error naming it
        assert!(Policy::parse("127.0.0.1").unwrap_err().contains("needs a port"));
        assert!(Policy::parse("a b:1").unwrap_err().contains("a b:1"));
        assert_eq!(Policy::parse("").unwrap(), Policy::default());
    }

    #[test]
    fn given_requests_when_parsed_then_keys_directions_inputs_and_limits_are_checked_by_name() {
        // Given the family's requests
        let ok = |tool: &str, args: Value| BrowserOp::parse(tool, &args).unwrap().unwrap();
        assert_eq!(ok(BROWSER_OPEN, json!({"key": "t1"})), BrowserOp::Open { key: "t1".into(), url: None });
        assert_eq!(ok(BROWSER_HISTORY, json!({"key": "t1", "direction": "reload"})), BrowserOp::History { key: "t1".into(), direction: Direction::Reload });
        assert_eq!(ok(BROWSER_INPUT, json!({"key": "t1", "kind": "mouse_click", "x": 1, "y": 2.5, "modifiers": ["ctrl"]})), BrowserOp::Input { key: "t1".into(), input: Input::MouseClick { x: 1.0, y: 2.5, button: "left".into(), modifiers: vec!["ctrl".into()] } });
        assert_eq!(ok(BROWSER_INPUT, json!({"key": "t1", "kind": "mouse_wheel", "dy": 40})), BrowserOp::Input { key: "t1".into(), input: Input::MouseWheel { dx: 0.0, dy: 40.0 } });
        // Then each bad request is INVALID_ARGS naming what is wrong
        for (tool, args, named) in [
            (BROWSER_OPEN, json!({"key": "a b"}), "`key`"),
            (BROWSER_OPEN, json!({}), "`key`"),
            (BROWSER_HISTORY, json!({"key": "k", "direction": "up"}), "`direction`"),
            (BROWSER_EVAL, json!({"key": "k", "expression": "  "}), "`expression`"),
            (BROWSER_INPUT, json!({"key": "k", "kind": "tap"}), "`kind`"),
            (BROWSER_INPUT, json!({"key": "k", "kind": "mouse_move", "x": "1", "y": 2}), "`x`"),
            (BROWSER_INPUT, json!({"key": "k", "kind": "mouse_down", "button": "back"}), "`button`"),
            (BROWSER_INPUT, json!({"key": "k", "kind": "mouse_click", "x": 1, "y": 1, "modifiers": ["super"]}), "`modifiers`"),
            (BROWSER_INPUT, json!({"key": "k", "kind": "key_press", "text": ""}), "`text`"),
            (BROWSER_INPUT, json!({"key": "k", "kind": "mouse_wheel"}), "`dy`"),
        ] {
            let e = BrowserOp::parse(tool, &args).unwrap().unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains(named), "{tool} {args}: {e:?}");
        }
        assert!(BrowserOp::parse("run_command", &json!({})).is_none());
    }

    #[test]
    fn given_the_actions_when_listed_then_every_tool_is_an_op_and_names_its_own_action() {
        for action in ACTIONS {
            let args = json!({"key": "k", "url": "https://example.com", "direction": "back", "expression": "1", "kind": "key_press", "text": "a"});
            let op = BrowserOp::parse(action.name, &args).unwrap_or_else(|| panic!("{} is no op", action.name)).unwrap();
            assert_eq!(op.action(), action.name);
            for required in action.required {
                assert!(action.properties.iter().any(|p| p.name == *required), "{} requires {required}, which it lacks", action.name);
            }
        }
        assert!(LONG.iter().all(|l| ACTIONS.iter().any(|a| a.name == *l)));
    }
}
