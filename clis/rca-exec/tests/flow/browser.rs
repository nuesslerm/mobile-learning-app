//! The browser family through the real adapter: a real headless Chromium (found as the browser harness finds it) driven over CDP, and `fake-devserver`
//! (exec-testkit) as the web server, so every page load, click and key is a real one. Only what needs no browser (a refused address, a page nobody
//! opened) is also replayed from the contract's recorded exchanges.
use exec_browser::{Closed, Evaluated, Inputted, Listed, PageInfo, Policy, Shot, BROWSER_CLOSE, BROWSER_EVAL, BROWSER_GOTO, BROWSER_HISTORY, BROWSER_INPUT, BROWSER_LIST, BROWSER_OPEN, BROWSER_SCREENSHOT};
use exec_preview::{PreviewInfo, PREVIEW_START, PREVIEW_STOP};
use exec_proto::{Code, ExecResult, Stream};
use exec_testkit::devserver::{command, free_port};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// The tests share one process, and each counts the browsers it started: one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> ExecResult<Value> {
    execute(ex, "id-1", tool, &args, &mut |_: Stream, _: &str| {}, &mut |_| {})
}

fn typed<T: serde::de::DeserializeOwned>(ex: &LocalExecutor, tool: &str, args: Value) -> T {
    serde_json::from_value(call(ex, tool, args.clone()).unwrap_or_else(|e| panic!("{tool} {args}: {e:?}"))).unwrap()
}

fn eval(ex: &LocalExecutor, key: &str, expression: &str) -> Value {
    typed::<Evaluated>(ex, BROWSER_EVAL, json!({"key": key, "expression": expression})).value
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Chromium processes this test process started (their profile directory carries its pid).
fn chromium_processes() -> usize {
    let needle = format!("wf-browser-{}-", std::process::id());
    std::fs::read_dir("/proc").unwrap().flatten().filter(|e| std::fs::read(e.path().join("cmdline")).is_ok_and(|c| String::from_utf8_lossy(&c).contains(&needle))).count()
}

/// Page targets of the browser this test process started, read from its DevTools endpoint.
fn page_targets() -> usize {
    let prefix = format!("wf-browser-{}-", std::process::id());
    let port = std::fs::read_dir(std::env::temp_dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(&prefix)).find_map(|e| std::fs::read_to_string(e.path().join("DevToolsActivePort")).ok()).and_then(|t| t.lines().next().and_then(|l| l.parse::<u16>().ok()));
    let Some(port) = port else { return 0 };
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    std::io::Write::write_all(&mut stream, format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    stream.set_read_timeout(Some(Duration::from_millis(700))).unwrap();
    let mut raw = Vec::new();
    let _ = std::io::Read::read_to_end(&mut stream, &mut raw);
    let body = String::from_utf8_lossy(&raw);
    body.matches("\"type\": \"page\"").count()
}

/// A loopback listener that counts the connections made to it.
fn counting_listener() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = count.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming().flatten() {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            held.push(conn);
        }
    });
    (port, count)
}

struct Rig {
    _dir: tempfile::TempDir,
    ex: LocalExecutor,
    port: u16,
    server: String,
}

/// An executor whose browser may reach the dev server it starts (and nothing else on loopback), and that dev server.
fn rig(idle: Duration, max_pages: usize) -> Rig {
    rig_allowing(idle, max_pages, &[])
}

fn rig_allowing(idle: Duration, max_pages: usize, also: &[u16]) -> Rig {
    assert!(cdp::launch::find_chromium().is_some(), "no Chromium (set WF_CHROMIUM or install one with Playwright)");
    std::env::set_var("RCA_EXEC_BROWSER_NO_SANDBOX", "1");
    let port = free_port();
    let dir = tempfile::tempdir().unwrap();
    let allow: Vec<String> = std::iter::once(port).chain(also.iter().copied()).map(|p| format!("127.0.0.1:{p}")).collect();
    let ex = LocalExecutor::new(dir.path()).unwrap().with_browser(Policy::parse(&allow.join(",")).unwrap(), idle, max_pages, None);
    let started: PreviewInfo = typed(&ex, PREVIEW_START, json!({"command": command(""), "port": port, "ready_timeout_ms": 10_000}));
    Rig { _dir: dir, ex, port, server: started.id }
}

impl Rig {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = call(&self.ex, PREVIEW_STOP, json!({"id": self.server}));
    }
}

#[test]
fn given_the_recorded_exchanges_of_the_family_when_replayed_then_each_response_is_what_was_recorded_and_no_browser_started() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given the family's recorded exchanges and an executor that holds no page
    let dir = tempfile::tempdir().unwrap();
    let ex = LocalExecutor::new(dir.path()).unwrap();
    for fixture in exec_browser::testing::fixtures() {
        for exchange in &fixture.exchanges {
            // When the recorded request runs
            let (tool, args) = (exchange.request["tool"].as_str().unwrap(), &exchange.request["args"]);
            // Then the answer is the recorded one, result or error code and message
            match (exchange.response["ok"].as_bool().unwrap(), call(&ex, tool, args.clone())) {
                (true, Ok(got)) => assert_eq!(got, exchange.response["result"], "{}", exchange.name),
                (false, Err(e)) => assert_eq!((json!(e.code.as_str()), json!(e.message)), (exchange.response["error"]["code"].clone(), exchange.response["error"]["message"].clone()), "{}", exchange.name),
                (_, other) => panic!("{}: {other:?}", exchange.name),
            }
        }
    }
    assert_eq!(chromium_processes(), 0);
}

#[test]
fn given_a_dev_server_when_a_page_is_opened_navigated_and_read_then_it_shows_what_the_server_sent_and_history_moves_through_it() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given the dev server and an executor that lets its browser reach it
    let rig = rig(Duration::from_secs(60), 3);
    // When a page is opened on the server
    let opened: PageInfo = typed(&rig.ex, BROWSER_OPEN, json!({"key": "t1", "url": rig.url("/")}));
    // Then it shows the server's address, and reading it gives what the server answered
    assert_eq!((opened.key.as_str(), opened.url.as_str()), ("t1", rig.url("/").as_str()));
    let body = eval(&rig.ex, "t1", "document.body.innerText");
    assert!(body.as_str().unwrap().contains(&format!("\"port_env\":\"{}\"", rig.port)), "{body}");
    // And navigating, going back, forward and reloading each report the address the page is at
    let at = |direction: &str| typed::<PageInfo>(&rig.ex, BROWSER_HISTORY, json!({"key": "t1", "direction": direction})).url;
    assert_eq!(typed::<PageInfo>(&rig.ex, BROWSER_GOTO, json!({"key": "t1", "url": rig.url("/second")})).url, rig.url("/second"));
    assert_eq!(at("back"), rig.url("/"));
    assert_eq!(at("forward"), rig.url("/second"));
    eval(&rig.ex, "t1", "window.marker = 1");
    assert_eq!(at("reload"), rig.url("/second"));
    assert_eq!(eval(&rig.ex, "t1", "typeof window.marker"), json!("undefined"));
    // And going back past the first entry (the blank page the browser opened with) is refused, and a picture of the page is a PNG
    assert_eq!(at("back"), rig.url("/"));
    assert_eq!(at("back"), "about:blank");
    let refused = call(&rig.ex, BROWSER_HISTORY, json!({"key": "t1", "direction": "back"})).unwrap_err();
    assert!(refused.code == Code::Io && refused.message.contains("no earlier entry"), "{refused:?}");
    let shot: Shot = typed(&rig.ex, BROWSER_SCREENSHOT, json!({"key": "t1"}));
    let png = base64_decode(&shot.data);
    assert_eq!((shot.mime.as_str(), &png[..8], png.len()), ("image/png", &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..], shot.bytes));
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text).unwrap()
}

#[test]
fn given_a_page_with_a_button_and_a_field_when_the_pointer_and_keyboard_are_driven_then_the_page_receives_each_input_where_it_was_aimed() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given a page with a button at the top left, a field, and a document tall enough to scroll
    let rig = rig(Duration::from_secs(60), 3);
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "t1", "url": rig.url("/")}));
    eval(&rig.ex, "t1", "document.body.innerHTML = '<button id=b style=\"position:fixed;left:10px;top:10px;width:100px;height:50px\">go</button><input id=f style=\"position:fixed;left:10px;top:100px;width:200px\"><div style=\"height:5000px\"></div>'; window.clicks = []; document.getElementById('b').addEventListener('mousedown', e => window.clicks.push('down' + e.button)); document.getElementById('b').addEventListener('mouseup', e => window.clicks.push('up' + e.button)); document.getElementById('b').addEventListener('click', e => window.clicks.push('click' + (e.ctrlKey ? '+ctrl' : ''))); true");
    let input = |args: Value| -> Inputted {
        let mut all = json!({"key": "t1"});
        for (k, v) in args.as_object().unwrap() {
            all[k] = v.clone();
        }
        typed(&rig.ex, BROWSER_INPUT, all)
    };
    // When the pointer clicks the button, then moves onto it and presses and releases, then clicks it with ctrl held
    assert_eq!(input(json!({"kind": "mouse_click", "x": 20, "y": 20})), Inputted { x: 20.0, y: 20.0, buttons: 0 });
    input(json!({"kind": "mouse_move", "x": 30, "y": 30}));
    assert_eq!(input(json!({"kind": "mouse_down"})), Inputted { x: 30.0, y: 30.0, buttons: 1 });
    assert_eq!(input(json!({"kind": "mouse_up"})), Inputted { x: 30.0, y: 30.0, buttons: 0 });
    input(json!({"kind": "mouse_click", "x": 20, "y": 20, "modifiers": ["ctrl"]}));
    // Then the page saw each, in order, with the position the executor kept for the press and release
    assert_eq!(eval(&rig.ex, "t1", "window.clicks"), json!(["down0", "up0", "click", "down0", "up0", "click", "down0", "up0", "click+ctrl"]));
    // And typing goes to the focused field: text inserted, a key pressed, a named key pressed
    eval(&rig.ex, "t1", "document.getElementById('f').focus(); true");
    input(json!({"kind": "insert_text", "text": "hello"}));
    input(json!({"kind": "key_press", "text": "!"}));
    input(json!({"kind": "key_press", "text": "Backspace"}));
    input(json!({"kind": "key_press", "text": "Home"}));
    input(json!({"kind": "insert_text", "text": ">"}));
    assert_eq!(eval(&rig.ex, "t1", "document.getElementById('f').value"), json!(">hello"));
    // And a wheel scroll over the page moves it, and a key the executor does not press is refused by name
    input(json!({"kind": "mouse_wheel", "dy": 300}));
    wait_until("the page to scroll", || eval(&rig.ex, "t1", "window.scrollY").as_f64().unwrap() > 0.0);
    let unknown = call(&rig.ex, BROWSER_INPUT, json!({"key": "t1", "kind": "key_press", "text": "Control+Shift+K"})).unwrap_err();
    assert!(unknown.code == Code::InvalidArgs && unknown.message.contains("Control+Shift+K"), "{unknown:?}");
}

#[test]
fn given_a_page_that_asks_for_addresses_the_executor_does_not_allow_when_it_loads_them_then_each_request_fails_and_the_allowed_one_succeeds() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given a page on the dev server and a second server on loopback that the executor's allowlist does not name
    let rig = rig(Duration::from_secs(60), 3);
    let other = free_port();
    let second: PreviewInfo = typed(&rig.ex, PREVIEW_START, json!({"command": command(""), "port": other, "ready_timeout_ms": 10_000}));
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "t1", "url": rig.url("/")}));
    let reach = |url: &str| eval(&rig.ex, "t1", &format!("fetch('{url}', {{mode: 'no-cors'}}).then(() => 'reached', () => 'blocked')"));
    // When the page fetches the named server, the other loopback server, the cloud metadata address and a private one
    // Then only the named one is reached
    assert_eq!(reach(&rig.url("/x")), json!("reached"));
    assert_eq!(reach(&format!("http://127.0.0.1:{other}/")), json!("blocked"));
    assert_eq!(reach("http://169.254.169.254/latest/meta-data/"), json!("blocked"));
    assert_eq!(reach("http://10.0.0.1/"), json!("blocked"));
    // And an address the policy refuses is refused as the page's address, before the browser is asked
    for url in [format!("http://127.0.0.1:{other}/"), "http://169.254.169.254/".to_string(), "file:///etc/passwd".to_string(), "http://localhost:4000/".to_string()] {
        let refused = call(&rig.ex, BROWSER_GOTO, json!({"key": "t1", "url": url})).unwrap_err();
        assert_eq!(refused.code, Code::InvalidArgs, "{url}: {refused:?}");
    }
    call(&rig.ex, PREVIEW_STOP, json!({"id": second.id})).unwrap();
}

#[test]
fn given_the_page_limit_when_pages_are_opened_closed_and_left_idle_then_the_cap_holds_and_an_idle_page_and_then_the_browser_are_closed() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given an executor that keeps two pages and closes one idle for 1.5 s
    let rig = rig(Duration::from_millis(1500), 2);
    // When three keys are opened
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "a", "url": rig.url("/")}));
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "b"}));
    let over = call(&rig.ex, BROWSER_OPEN, json!({"key": "c"})).unwrap_err();
    // Then the third is refused naming the cap, and the list shows the two with their limits
    assert!(over.code == Code::AlreadyExists && over.message.contains("2 browser pages"), "{over:?}");
    let listed: Listed = typed(&rig.ex, BROWSER_LIST, json!({}));
    assert_eq!((listed.pages.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), listed.max_pages, listed.idle_limit_ms), (vec!["a", "b"], 2, 1500));
    assert!(chromium_processes() > 0);
    // And closing one frees a place, and a key that is open again keeps its page
    assert_eq!(typed::<Closed>(&rig.ex, BROWSER_CLOSE, json!({"key": "b"})), Closed { closed: true });
    assert_eq!(typed::<Closed>(&rig.ex, BROWSER_CLOSE, json!({"key": "b"})), Closed { closed: false });
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "c"}));
    eval(&rig.ex, "a", "window.mine = 7");
    assert_eq!(typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "a"})).url, rig.url("/"));
    assert_eq!(eval(&rig.ex, "a", "window.mine"), json!(7));
    // And left alone past the limit the pages close by themselves and the browser stops with the last
    wait_until("the idle pages and the browser to be closed", || chromium_processes() == 0);
    assert_eq!(typed::<Listed>(&rig.ex, BROWSER_LIST, json!({})).pages.len(), 0);
    let gone = call(&rig.ex, BROWSER_EVAL, json!({"key": "a", "expression": "1"})).unwrap_err();
    assert_eq!(gone.code, Code::NotFound);
}

const SOCKET_PROBE: &str = "new Promise(done => { const w = new WebSocket('ws://127.0.0.1:PORT/'); w.onerror = () => done('failed'); w.onopen = () => done('open'); setTimeout(() => done('pending'), 3000); })";

#[test]
fn given_a_loopback_server_the_allowlist_does_not_name_when_a_page_opens_a_websocket_or_fetches_it_then_nothing_connects_and_when_it_is_named_the_connection_is_made() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given a loopback listener that counts connections, and an executor whose allowlist names only its dev server
    let (other, connections) = counting_listener();
    let rig = rig(Duration::from_secs(60), 3);
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "t1", "url": rig.url("/")}));
    // When the page opens a WebSocket to the listener and fetches it
    let socket = eval(&rig.ex, "t1", &SOCKET_PROBE.replace("PORT", &other.to_string()));
    let fetched = eval(&rig.ex, "t1", &format!("fetch('http://127.0.0.1:{other}/', {{mode: 'no-cors'}}).then(() => 'reached', () => 'blocked')"));
    // Then both fail and the listener saw no connection
    assert_eq!((socket, fetched), (json!("failed"), json!("blocked")));
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 0);
    // And when an executor names the listener, the same WebSocket reaches it
    let named = rig_allowing(Duration::from_secs(60), 3, &[other]);
    typed::<PageInfo>(&named.ex, BROWSER_OPEN, json!({"key": "t1", "url": named.url("/")}));
    eval(&named.ex, "t1", &SOCKET_PROBE.replace("PORT", &other.to_string()));
    assert!(connections.load(std::sync::atomic::Ordering::SeqCst) > 0);
}

#[test]
fn given_a_page_in_use_when_it_is_closed_or_reaped_then_its_browser_target_is_closed_too() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Given two open pages (the browser also holds its blank start page) and a long call running on one of them
    let rig = rig(Duration::from_secs(60), 3);
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "keep", "url": rig.url("/")}));
    typed::<PageInfo>(&rig.ex, BROWSER_OPEN, json!({"key": "busy", "url": rig.url("/")}));
    let before = page_targets();
    let ex = &rig.ex;
    std::thread::scope(|scope| {
        let running = scope.spawn(|| eval(ex, "busy", "new Promise(done => setTimeout(() => done('slept'), 1500))"));
        std::thread::sleep(Duration::from_millis(300));
        // When the busy page is closed while the call still holds it
        assert_eq!(typed::<Closed>(ex, BROWSER_CLOSE, json!({"key": "busy"})), Closed { closed: true });
        running.join().unwrap();
    });
    // Then once the call ends the page's target is gone from the browser, and the other page stays
    wait_until("the closed page's target to go", || page_targets() == before - 1);
    assert_eq!(eval(&rig.ex, "keep", "1 + 1"), json!(2));
}
