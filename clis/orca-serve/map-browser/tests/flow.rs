//! The browser rows through the `Host` dispatcher with rca's `browser` group doubled by pages in memory: each reply is read the way the phone reads it,
//! and each input is read back from what the executor was sent. The refusals of a host without a browser are the recorded host's, word for word.
//! Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{AgentApi, Connection, Host, HostInfo, ServiceStatus};
use orca_map_browser::testing::Browsers;
use orca_map_browser::{BrowserArea, USES};
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    browsers: Browsers,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn with(browsers: Browsers) -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        Workspaces::default().workspace("main", "root", Some("/srv/main")).workspace("other", "root", Some("/srv/other")).install(&agent);
        browsers.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(BrowserArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, browsers, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn on() -> Rig {
        Rig::with(Browsers::on())
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        serde_json::from_str(&self.conn.on_message(&frame)[0]).unwrap()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> (String, String) {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }
}

fn pair(code: &str, message: &str) -> (String, String) {
    (code.to_string(), message.to_string())
}

#[test]
fn given_a_tab_created_in_a_worktree_when_the_phone_navigates_and_steps_through_history_then_the_page_of_that_tab_moves_and_each_answer_is_its_address_and_title() {
    // Given a service with the browser on and a worktree of the phone
    let mut rig = Rig::on();
    // When the phone creates a tab on a page, goes to another address, back, forward and reloads, naming the tab each time
    let created = rig.ok("browser.tabCreate", json!({"worktree": "main", "url": "https://example.com/a"}));
    let page = created["browserPageId"].as_str().unwrap().to_string();
    let to = |rig: &mut Rig, method: &str, extra: Value| {
        let mut params = json!({"page": page});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            params[k] = v;
        }
        rig.ok(method, params)
    };
    let second = to(&mut rig, "browser.goto", json!({"url": "https://example.com/b"}));
    let back = to(&mut rig, "browser.back", json!({}));
    let forward = to(&mut rig, "browser.forward", json!({}));
    let reload = to(&mut rig, "browser.reload", json!({}));
    // Then the tab is a page of the workspace, and each answer is the address and title the page shows
    assert!(page.starts_with("main:page"), "{page}");
    assert_eq!(rig.browsers.pages(), std::slice::from_ref(&page));
    assert_eq!(second, json!({"url": "https://example.com/b", "title": "Title of https://example.com/b"}));
    assert_eq!(back, json!({"url": "https://example.com/a", "title": "Title of https://example.com/a"}));
    assert_eq!((forward["url"].clone(), reload["url"].clone()), (json!("https://example.com/b"), json!("https://example.com/b")));
    // And a tab created without an address shows the blank page, and a second tab is a second page
    let blank = rig.ok("browser.tabCreate", json!({"worktree": "main"}))["browserPageId"].as_str().unwrap().to_string();
    assert_eq!(rig.browsers.url_of(&blank).as_deref(), Some("about:blank"));
    assert_ne!(blank, page);
}

#[test]
fn given_no_page_named_when_a_command_runs_then_it_acts_on_the_worktrees_own_page_or_says_no_tab_is_open_and_an_unknown_tab_is_not_found() {
    // Given a service with the browser on and no page
    let mut rig = Rig::on();
    // When a command names the worktree only, then a tab that was never created, then a worktree that is no worktree
    let none = rig.err("browser.goto", json!({"worktree": "main", "url": "https://example.com/"}));
    let unknown = rig.err("browser.reload", json!({"page": "main:page-gone"}));
    let stranger = rig.err("browser.back", json!({"worktree": "nowhere"}));
    // Then the first finds no tab, the second says the page was not found, and the third is the selector the host does not know
    assert_eq!(none, pair("browser_no_tab", "No browser tab open in this worktree"));
    assert_eq!(unknown, pair("browser_tab_not_found", "Browser page main:page-gone was not found"));
    assert_eq!(stranger, pair("selector_not_found", "selector_not_found"));
    // And once the workspace's own page is open, a command with only the worktree acts on it, and with no target at all on the first workspace's
    rig.ok("browser.tabCreate", json!({"worktree": "other"}));
    rig.agent.request("browser.open", json!({"workspace": "main", "key": "main", "url": "https://example.com/own"})).unwrap();
    assert_eq!(rig.ok("browser.reload", json!({"worktree": "main"}))["url"], json!("https://example.com/own"));
    assert_eq!(rig.ok("browser.reload", json!({}))["url"], json!("https://example.com/own"));
}

#[test]
fn given_a_page_when_the_pointer_and_keyboard_rows_run_then_each_answers_as_the_host_does_and_the_executor_is_sent_the_input_with_the_pointer_it_keeps() {
    // Given a tab on a page
    let mut rig = Rig::on();
    let page = rig.ok("browser.tabCreate", json!({"worktree": "main", "url": "https://example.com/"}))["browserPageId"].clone();
    let target = |extra: Value| {
        let mut params = json!({"page": page});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            params[k] = v;
        }
        params
    };
    // When the phone moves the pointer, presses, releases, clicks with ctrl held, scrolls, presses a key and inserts text
    let moved = rig.ok("browser.mouseMove", target(json!({"x": 12, "y": 34.5})));
    let pressed = rig.ok("browser.mouseDown", target(json!({"button": "left"})));
    let released = rig.ok("browser.mouseUp", target(json!({})));
    let clicked = rig.ok("browser.mouseClick", target(json!({"x": 5, "y": 6, "button": "right", "modifiers": ["ctrl"], "radius": 4})));
    let scrolled = rig.ok("browser.mouseWheel", target(json!({"dy": 120, "dx": -3})));
    let key = rig.ok("browser.keypress", target(json!({"key": "Enter"})));
    let text = rig.ok("browser.keyboardInsertText", target(json!({"text": "héllo"})));
    // Then each answer is the shape the host gives
    assert_eq!((moved, pressed, released), (json!({"moved": true}), json!({"pressed": true}), json!({"released": true})));
    assert_eq!(clicked, json!({"clicked": {"x": 5.0, "y": 6.0, "button": "right", "adjusted": false, "handled": false}}));
    assert_eq!(scrolled, json!({"scrolled": true, "deltaX": -3.0, "deltaY": 120.0}));
    assert_eq!((key, text), (json!({"pressed": "Enter"}), json!({"inserted": true})));
    // And the executor was sent each input for the tab's page, in order
    let sent: Vec<Value> = rig.browsers.inputs().into_iter().map(|(k, p)| {
        assert_eq!(&json!(k), &page);
        let mut p = p;
        p.as_object_mut().unwrap().retain(|k, _| !["workspace", "key"].contains(&k.as_str()));
        p
    }).collect();
    assert_eq!(sent, [
        json!({"kind": "mouse_move", "x": 12.0, "y": 34.5}),
        json!({"kind": "mouse_down", "button": "left"}),
        json!({"kind": "mouse_up"}),
        json!({"kind": "mouse_click", "x": 5.0, "y": 6.0, "button": "right", "modifiers": ["ctrl"]}),
        json!({"kind": "mouse_wheel", "dx": -3.0, "dy": 120.0}),
        json!({"kind": "key_press", "text": "Enter"}),
        json!({"kind": "insert_text", "text": "héllo"}),
    ]);
}

#[test]
fn given_a_service_started_without_the_browser_when_each_row_runs_then_it_answers_what_the_recorded_host_answers_without_a_browser() {
    // Given a service with the browser off, as it is by default
    let mut rig = Rig::with(Browsers::off());
    let unavailable = pair("browser_unavailable", "Browser automation is unavailable on this host.");
    // When each row is called with the smallest request the host accepts, and with a worktree that is no worktree
    for (method, params) in [
        ("browser.goto", json!({"url": "a"})),
        ("browser.reload", json!({})),
        ("browser.back", json!({})),
        ("browser.forward", json!({})),
        ("browser.tabCreate", json!({})),
        ("browser.keypress", json!({"key": "a"})),
        ("browser.keyboardInsertText", json!({"text": "a"})),
        ("browser.mouseMove", json!({"x": 1, "y": 2})),
        ("browser.mouseDown", json!({})),
        ("browser.mouseUp", json!({})),
        ("browser.mouseClick", json!({"x": 1, "y": 2})),
        ("browser.mouseWheel", json!({"dy": 3})),
    ] {
        // Then it is browser_unavailable, and the unknown worktree is the selector the host does not know (a new tab asks for the browser first)
        assert_eq!(rig.err(method, params.clone()), unavailable, "{method}");
        let mut stranger = params;
        stranger["worktree"] = json!("wrong");
        let expected = if method == "browser.tabCreate" { unavailable.clone() } else { pair("selector_not_found", "selector_not_found") };
        assert_eq!(rig.err(method, stranger), expected, "{method}");
    }
    assert!(rig.browsers.pages().is_empty());
}

#[test]
fn given_requests_the_host_refuses_before_it_looks_for_a_browser_when_sent_then_each_is_invalid_argument_with_the_hosts_words_and_nothing_reaches_the_executor() {
    // Given a service with the browser on
    let mut rig = Rig::on();
    let expect = "--expect-url must be an absolute URL, like https://example.com/chat/a";
    for (method, params, message) in [
        ("browser.goto", json!({}), "Missing required --url"),
        ("browser.goto", json!({"url": 7}), "Missing required --url"),
        ("browser.goto", json!({"expectUrl": "a", "worktree": "a", "page": "a", "url": "a"}), expect),
        ("browser.reload", json!({"expectUrl": "a"}), expect),
        ("browser.keypress", json!({}), "Missing required --key"),
        ("browser.keyboardInsertText", json!({"text": 7}), "Missing required --text"),
        ("browser.mouseMove", json!({"y": "a"}), "Missing required x and y coordinates"),
        ("browser.mouseClick", json!({"x": "wrong", "y": "a"}), "Missing required x and y coordinates"),
        ("browser.mouseWheel", json!({"dy": "a"}), "Missing required --dy"),
        ("browser.back", json!({"worktree": 7}), "Invalid input: expected string, received number"),
        ("browser.screencast.ack", json!({}), "Invalid input: expected string, received undefined"),
        ("browser.screencast.ack", json!({"seq": 0, "subscriptionId": 7}), "Invalid input: expected string, received number"),
    ] {
        assert_eq!(rig.err(method, params.clone()), pair("invalid_argument", message), "{method} {params}");
    }
    assert!(rig.agent.requests().iter().all(|(m, _)| m != "browser.open" && m != "browser.goto" && m != "browser.history" && m != "browser.input"));
}

#[test]
fn given_what_the_host_does_not_do_when_the_phone_asks_for_it_then_it_is_refused_by_name_and_no_page_changes() {
    // Given a service with the browser on and a tab
    let mut rig = Rig::on();
    let page = rig.ok("browser.tabCreate", json!({"worktree": "main", "url": "https://example.com/"}))["browserPageId"].clone();
    // When the phone checks the page's address first, names a browser profile, asks for a split pane or a page of its own, a chord, or an address that is no web page
    let expect = rig.err("browser.reload", json!({"page": page, "expectUrl": "https://example.com/"}));
    let profile = rig.err("browser.tabCreate", json!({"worktree": "main", "profileId": "work"}));
    let split = rig.err("browser.tabCreate", json!({"worktree": "main", "split": "right"}));
    let client = rig.err("browser.tabCreate", json!({"worktree": "main", "placement": {"kind": "client"}}));
    let chord = rig.err("browser.keypress", json!({"page": page, "key": "Control+Shift+K"}));
    let file = rig.err("browser.goto", json!({"page": page, "url": "file:///etc/passwd"}));
    // Then each is refused with its reason, and the tab is where it was
    assert_eq!(expect.0, "invalid_argument");
    assert!(expect.1.starts_with("--expect-url is not supported by this host"), "{expect:?}");
    assert_eq!(profile, pair("invalid_argument", "Browser profile work was not found"));
    assert_eq!(split, pair("invalid_argument", "Opening a browser tab in a split pane needs an Orca desktop window on this host."));
    assert_eq!(client, split);
    assert_eq!((chord.0.as_str(), chord.1.contains("Control+Shift+K")), ("invalid_argument", true), "{chord:?}");
    assert_eq!(file.0, "invalid_argument");
    assert!(file.1.contains("http or https"), "{file:?}");
    assert_eq!(rig.browsers.pages().len(), 1);
    assert_eq!(rig.browsers.url_of(page.as_str().unwrap()).as_deref(), Some("https://example.com/"));
}

#[test]
fn given_a_late_ack_for_a_screencast_when_sent_then_nothing_is_credited_because_this_host_opens_none() {
    // Given a service with the browser on
    let mut rig = Rig::on();
    // When the phone acknowledges a frame of a subscription, and one with a seq that is no count
    let late = rig.ok("browser.screencast.ack", json!({"subscriptionId": "a", "seq": 0}));
    let bad = rig.err("browser.screencast.ack", json!({"subscriptionId": "a", "seq": -1}));
    // Then the ack credits nothing and the other is refused
    assert_eq!(late, json!({"credited": false}));
    assert_eq!(bad, pair("invalid_argument", "Too small: expected number to be >=0"));
}

#[test]
fn given_the_rows_when_run_then_the_agent_methods_sent_are_listed_in_uses() {
    // Given every row run once with the browser on, and once without
    let mut rig = Rig::on();
    let page = rig.ok("browser.tabCreate", json!({"worktree": "main"}))["browserPageId"].clone();
    for (method, extra) in [("browser.goto", json!({"url": "https://example.com/"})), ("browser.reload", json!({})), ("browser.back", json!({})), ("browser.forward", json!({})), ("browser.keypress", json!({"key": "a"})), ("browser.mouseMove", json!({"x": 1, "y": 1}))] {
        let mut params = json!({"page": page});
        for (k, v) in extra.as_object().unwrap() {
            params[k] = v.clone();
        }
        let _ = rig.call(method, params);
    }
    // Then every agent method sent is one of USES, and the pages probe of tabCreate is among them
    let declared: BTreeSet<String> = USES.iter().map(|m| m.name().to_string()).chain(["repo.list".to_string()]).collect();
    let sent: BTreeSet<String> = rig.agent.requests().into_iter().map(|(m, _)| m).collect();
    assert!(sent.is_subset(&declared), "{sent:?} is not within {declared:?}");
    assert!(sent.contains("browser.pages") && sent.contains("browser.open") && sent.contains("browser.input"));
}
