//! The browser rows through the chain: the phone's commands reach the service's `browser` group, the real executor's headless Chromium loads a page of
//! `fake-devserver` (a real process on a real port), and the page shows what the phone sent. A row counts only when the group's method answered in its call's
//! window and the executor ran the browser action; with the browser off (the service's default) the rows are shown refused at the group.
use crate::agent_sessions::executor_ran;
use crate::rig::*;
use exec_testkit::devserver::{command, free_port};
use serde_json::{json, Value};

const WS: &str = "id:w1::";

fn eval(rca: &mut Direct, key: &str, expression: &str) -> Value {
    rca.call("browser", "browser.eval", json!({"workspace": "w1", "key": key, "expression": expression})).expect("the page answers")["value"].clone()
}

#[test]
fn the_phone_opens_a_tab_on_a_dev_server_and_navigates_clicks_scrolls_and_types_in_the_executors_chromium() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain with the browser on and an executor that may reach one dev server, started through the service
        let port = free_port();
        let allow = format!("127.0.0.1:{port}");
        let browsers = std::env::var("PLAYWRIGHT_BROWSERS_PATH").unwrap_or_else(|_| format!("{}/.cache/ms-playwright", std::env::var("HOME").unwrap_or_default()));
        let chain = Chain::start_with_env(None, &[("RCA_BROWSER", "1"), ("RCA_EXEC_BROWSER_ALLOW", &allow), ("RCA_EXEC_BROWSER_NO_SANDBOX", "1"), ("PLAYWRIGHT_BROWSERS_PATH", &browsers)]);
        let mut rca = chain.direct();
        let server = rca.call("preview", "preview.start", json!({"workspace": "w1", "command": command(""), "port": port, "ready_timeout_ms": 10_000})).expect("the dev server starts");
        let mut phone = Phone::connect(case, &chain);
        let url = format!("http://127.0.0.1:{port}/");
        // When the phone creates a tab on the server
        let created = phone.call(case, &chain, &["rpc:browser.tabCreate"], "browser.tabCreate", json!({"worktree": WS, "url": url}));
        // Then the service's group opened the page and the executor ran it, and the tab shows what the server answered
        assert_eq!(created["ok"], true, "{created}\n{}", chain.evidence());
        answered(case, "browser.open", true);
        assert!(executor_ran(case, "browser_open"), "{:?}", case.calls().last());
        let page = created["result"]["browserPageId"].as_str().expect("a tab id").to_string();
        assert!(page.starts_with("w1:"), "{created}");
        assert!(eval(&mut rca, &page, "document.body.innerText").as_str().unwrap().contains(&format!("\"port_env\":\"{port}\"")));
        // And the phone goes to another address, back, forward and reloads, each answered with the address the page is at
        let at = |reply: &Value| reply["result"]["url"].as_str().unwrap().to_string();
        let second = phone.call(case, &chain, &["rpc:browser.goto"], "browser.goto", json!({"page": page, "url": format!("{url}next")}));
        answered(case, "browser.goto", true);
        assert_eq!(at(&second), format!("{url}next"));
        let back = phone.call(case, &chain, &["rpc:browser.back"], "browser.back", json!({"page": page}));
        answered(case, "browser.history", true);
        assert_eq!(at(&back), url);
        let forward = phone.call(case, &chain, &["rpc:browser.forward"], "browser.forward", json!({"page": page}));
        assert_eq!(at(&forward), format!("{url}next"));
        let reload = phone.call(case, &chain, &["rpc:browser.reload"], "browser.reload", json!({"page": page}));
        answered(case, "browser.history", true);
        assert_eq!(at(&reload), format!("{url}next"));
        // And the page the phone drives has a button, a field and room to scroll
        eval(&mut rca, &page, "document.body.innerHTML = '<button id=b style=\"position:fixed;left:10px;top:10px;width:100px;height:50px\">go</button><input id=f style=\"position:fixed;left:10px;top:100px;width:200px\"><div style=\"height:5000px\"></div>'; window.log = []; const b = document.getElementById('b'); for (const t of ['mousedown', 'mouseup', 'click']) b.addEventListener(t, () => window.log.push(t)); true");
        // When the phone moves onto the button and presses and releases, clicks it, scrolls, and types into the field
        let params = |extra: Value| {
            let mut p = json!({"page": page});
            for (k, v) in extra.as_object().unwrap() {
                p[k] = v.clone();
            }
            p
        };
        let moved = phone.call(case, &chain, &["rpc:browser.mouseMove"], "browser.mouseMove", params(json!({"x": 30, "y": 30})));
        answered(case, "browser.input", true);
        let pressed = phone.call(case, &chain, &["rpc:browser.mouseDown"], "browser.mouseDown", params(json!({"button": "left"})));
        let released = phone.call(case, &chain, &["rpc:browser.mouseUp"], "browser.mouseUp", params(json!({})));
        let clicked = phone.call(case, &chain, &["rpc:browser.mouseClick"], "browser.mouseClick", params(json!({"x": 20, "y": 20, "modifiers": ["shift"]})));
        let scrolled = phone.call(case, &chain, &["rpc:browser.mouseWheel"], "browser.mouseWheel", params(json!({"dy": 300})));
        eval(&mut rca, &page, "document.getElementById('f').focus(); true");
        let typed = phone.call(case, &chain, &["rpc:browser.keyboardInsertText"], "browser.keyboardInsertText", params(json!({"text": "hello"})));
        let key = phone.call(case, &chain, &["rpc:browser.keypress"], "browser.keypress", params(json!({"key": "!"})));
        // Then each answer is the host's shape, and the page saw the events, the scroll and the text
        assert_eq!((moved["result"].clone(), pressed["result"].clone(), released["result"].clone()), (json!({"moved": true}), json!({"pressed": true}), json!({"released": true})));
        assert_eq!(clicked["result"]["clicked"]["button"], json!("left"));
        assert_eq!((scrolled["result"]["deltaY"].clone(), typed["result"].clone(), key["result"].clone()), (json!(300.0), json!({"inserted": true}), json!({"pressed": "!"})));
        assert_eq!(eval(&mut rca, &page, "window.log"), json!(["mousedown", "mouseup", "click", "mousedown", "mouseup", "click"]));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while eval(&mut rca, &page, "window.scrollY").as_f64().unwrap() <= 0.0 {
            assert!(std::time::Instant::now() < until, "the page did not scroll");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(eval(&mut rca, &page, "document.getElementById('f').value"), json!("hello!"));
        // When a second viewport subscribes to this page, then the executor captures JPEG pixels at the new CSS dimensions.
        for width in [390, 520] {
            let frame = rca.call("browser", "browser.frame", json!({"workspace":"w1","key":page,"width":width,"height":680,"quality":72,"mobile":false})).expect("the executor captures the page");
            assert_eq!(frame["mime"], "image/jpeg");
            assert_eq!(frame["navigation"]["url"], format!("{url}next"));
            assert_eq!(frame["navigation"]["can_go_back"], true);
            assert_eq!(frame["navigation"]["can_go_forward"], false);
            assert!(frame["data"].as_str().unwrap().starts_with("/9j/"));
            assert_eq!(eval(&mut rca, &page, "[innerWidth,innerHeight]"), json!([width,680]));
        }
        // When the paired phone subscribes, then it receives a JPEG in Orca's binary envelope and can acknowledge its sequence.
        let caps = phone.call(case, &chain, &["rpc:status.get"], "status.get", json!({}));
        assert!(caps["result"]["capabilities"].as_array().unwrap().contains(&json!("browser.screencast.v1")));
        let (_, ready) = phone.subscribe(case, &chain, &["rpc:browser.screencast"], "browser.screencast",
            json!({"worktree":WS,"page":page,"viewportWidth":390,"viewportHeight":680,"format":"jpeg","quality":72,"frameAck":1}));
        let subscription = &ready["result"]["subscriptionId"];
        assert_eq!(ready["result"]["frameAck"], true, "{ready}");
        let frame = phone.next_binary();
        assert_eq!(&frame[..8], &[0x62,1,1,1,0,0,0,0]);
        let meta_end = 16 + u32::from_le_bytes(frame[8..12].try_into().unwrap()) as usize;
        assert_eq!(serde_json::from_slice::<Value>(&frame[16..meta_end]).unwrap()["deviceWidth"],390);
        assert_eq!(&frame[meta_end..meta_end+2], &[0xff,0xd8]);
        let credit = phone.call(case, &chain, &["rpc:browser.screencast.ack"], "browser.screencast.ack", json!({"subscriptionId":subscription,"seq":0}));
        assert_eq!(credit["result"]["credited"],true);
        let end = phone.call(case, &chain, &["rpc:browser.screencast.unsubscribe"], "browser.screencast.unsubscribe", json!({"subscriptionId":subscription}));
        assert_eq!(end["ok"],true);
        // And a late acknowledgement of a screencast credits nothing, and the executor is left with no browser and no dev server
        let ack = phone.call(case, &chain, &["rpc:browser.screencast.ack"], "browser.screencast.ack", json!({"subscriptionId": "none", "seq": 3}));
        assert_eq!(ack["result"], json!({"credited": false}));
        // When the executor has reclaimed the idle page, a new phone reopens the kept tab at its last address.
        rca.call("browser","browser.close",json!({"workspace":"w1","key":page})).unwrap();
        drop(phone);
        let mut phone = Phone::connect(case, &chain);
        let (_, restored) = phone.subscribe(case, &chain, &["rpc:browser.screencast"], "browser.screencast",
            json!({"worktree":WS,"page":page,"viewportWidth":390,"viewportHeight":680,"frameAck":1}));
        assert_eq!(restored["result"]["tab"]["url"], format!("{url}next"), "{restored}");
        assert!(!phone.next_binary().is_empty());
        let close = phone.call(case, &chain, &["rpc:session.tabs.close"], "session.tabs.close", json!({"worktree":WS,"tabId":page,"reason":"user"}));
        assert_eq!(close["result"]["closed"],true,"{close}");
        assert_eq!(rca.call("browser","browser.pages",json!({"workspace":"w1"})).unwrap()["pages"],json!([]));
        rca.call("preview", "preview.stop", json!({"workspace": "w1", "id": server["id"]})).expect("the dev server stops");
    });
}

#[test]
fn the_phone_is_told_the_browser_is_unavailable_while_the_service_has_it_off_and_the_executor_is_never_asked() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain as the service starts by default, with no browser
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        // When the phone asks for a tab, and for the page of the worktree to go somewhere
        let tab = phone.refused(case, &chain, &["rpc:browser.tabCreate"], "browser.tabCreate", json!({"worktree": WS, "url": "https://example.com/"}));
        let goto = phone.refused(case, &chain, &["rpc:browser.goto"], "browser.goto", json!({"worktree": WS, "url": "https://example.com/"}));
        // Then each is the recorded host's answer for a host without a browser, refused at the service's group before any executor was asked
        for reply in [&tab, &goto] {
            assert_eq!((reply["error"]["code"].clone(), reply["error"]["message"].clone()), (json!("browser_unavailable"), json!("Browser automation is unavailable on this host.")), "{reply}");
        }
        answered(case, "browser.goto", false);
        assert!(!executor_ran(case, "browser_goto"));
    });
}
