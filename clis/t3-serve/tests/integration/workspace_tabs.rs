use crate::{rig::*, support::*};
use exec_testkit::devserver::free_port;
use std::{net::TcpStream, time::Duration};
use tungstenite::{Message, WebSocket};

fn media_ticket(case: &mut exec_testkit::Case, chain: &Chain, phone: &Phone) -> String {
    let reply = http_call(
        case,
        chain,
        &["http:auth.webSocketTicketFor"],
        (
            "POST",
            "/api/auth/websocket-ticket/media",
            &[("Authorization", &phone.bearer)],
            "",
        ),
        200,
    );
    reply.body["ticket"].as_str().unwrap().into()
}

fn viewer(chain: &Chain, ticket: &str, tab: &str) -> WebSocket<TcpStream> {
    let stream = TcpStream::connect(chain.addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(12)))
        .unwrap();
    tungstenite::client(
        format!(
            "ws://{}/api/preview-stream/ws?threadId=t1&tabId={tab}&wsTicket={ticket}",
            chain.addr
        ),
        stream,
    )
    .unwrap()
    .0
}

fn jpeg(socket: &mut WebSocket<TcpStream>) -> Vec<u8> {
    loop {
        match socket.read().unwrap() {
            Message::Binary(bytes) => {
                assert!(bytes.starts_with(&[255, 216]));
                return bytes.to_vec();
            }
            Message::Text(_) => {}
            other => panic!("Expected a browser frame, got {other:?}"),
        }
    }
}

#[test]
fn paired_server_preview_displays_desktop_content_accepts_input_and_reconnects() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired client and a page served only from the executor's test checkout
        let port = free_port();
        let allow = format!("127.0.0.1:{port}");
        let browsers = std::env::var("PLAYWRIGHT_BROWSERS_PATH")
            .unwrap_or_else(|_| format!("{}/.cache/ms-playwright", std::env::var("HOME").unwrap()));
        let chain = Chain::start_with_env(&[
            ("RCA_BROWSER", "1"),
            ("RCA_EXEC_BROWSER_ALLOW", &allow),
            ("RCA_EXEC_BROWSER_NO_SANDBOX", "1"),
            ("PLAYWRIGHT_BROWSERS_PATH", &browsers),
        ]);
        chain.repo.write("index.html","<title>Checkout preview</title><input autofocus oninput=\"document.body.style.background='green'\">");
        let mut desk = chain.direct();
        let server=desk.call("preview","preview.start",json!({"workspace":"w1","port":port,"command":format!("exec python3 -m http.server {port} --bind 127.0.0.1"),"ready_timeout_ms":10000})).unwrap();
        let scopes = t3_contracts::scopes::ALL_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let mut phone = Phone::connect_with(case, &chain, &scopes);
        phone.seed(case, &chain);
        let opened=phone.call(case,&chain,&["rpc:preview.open"],"preview.open",json!({"threadId":"t1","runtime":"server","url":format!("http://{allow}/"),"viewport":{"_tag":"freeform","width":390,"height":680}}));
        let tab = success(&opened)["tabId"].as_str().unwrap();
        // When the viewer receives a JPEG and types into the page, its next frame reflects the edit
        let mut ws = viewer(&chain, &media_ticket(case, &chain, &phone), tab);
        let first = jpeg(&mut ws);
        for input in [
            json!({"type":"text","text":"paired-RCAx"}),
            json!({"type":"key","key":"Backspace","action":"down"}),
            json!({"type":"ack"}),
        ] {
            ws.send(Message::text(input.to_string())).unwrap();
        }
        assert_ne!(first, jpeg(&mut ws));
        ws.send(Message::text(
            json!({"type":"resize","width":520,"height":680}).to_string(),
        ))
        .unwrap();
        let changed: Value = serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(changed, json!({"type":"viewport","width":520,"height":680}));
        ws.send(Message::text(json!({"type":"ack"}).to_string()))
            .unwrap();
        assert!(!jpeg(&mut ws).is_empty());

        let value=desk.call("browser","browser.eval",json!({"workspace":"w1","key":tab,"expression":"document.querySelector('input').value"})).unwrap();
        assert_eq!(value["value"], "paired-RCA");
        ws.close(None).unwrap();
        drop(ws);
        // Then a new media ticket reattaches to the same page and closing the tab releases Chromium
        let mut ws = viewer(&chain, &media_ticket(case, &chain, &phone), tab);
        assert!(!jpeg(&mut ws).is_empty());
        drop(ws);
        // An idle executor may release Chromium while its preview tab remains open on the phone.
        desk.call("browser", "browser.close", json!({"workspace":"w1","key":tab})).unwrap();
        let mut resumed = viewer(&chain, &media_ticket(case, &chain, &phone), tab);
        assert!(!jpeg(&mut resumed).is_empty());
        let location = desk.call("browser","browser.eval",json!({"workspace":"w1","key":tab,"expression":"location.href"})).unwrap();
        assert_eq!(location["value"], format!("http://{allow}/"));
        drop(resumed);
        success(&phone.call(
            case,
            &chain,
            &["rpc:preview.close"],
            "preview.close",
            json!({"threadId":"t1","tabId":tab}),
        ));
        assert_eq!(
            desk.call("browser", "browser.pages", json!({"workspace":"w1"}))
                .unwrap()["pages"],
            json!([])
        );
        desk.call(
            "preview",
            "preview.stop",
            json!({"workspace":"w1","id":server["id"]}),
        )
        .unwrap();
    });
}

#[test]
fn checkout_note_creation_skips_collisions_and_a_stale_writer_keeps_the_desktop_edit() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired checkout with an existing default note and no chat thread
        let chain = Chain::start();
        chain.repo.write("untitled.md", "existing");
        let scopes: Vec<_> = t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect();
        let phone = Phone::connect_with(case, &chain, &scopes);
        let auth = [("Authorization", phone.bearer.as_str()), ("Content-Type", "application/json")];
        let checkout = format!("w1::{}", std::fs::canonicalize(chain.repo.path()).unwrap().display());
        let route = "/api/mobile/notes";
        let create = http_call(case, &chain, &[], ("POST", route, &auth, &json!({"checkout":checkout}).to_string()), 200);
        assert_eq!(create.body, json!({"path":"untitled-2.md","content":""}));
        let path = "untitled-2.md";
        let save = |before: &str, content: &str| json!({"checkout":checkout,"path":path,"previous":before,"content":content}).to_string();
        // When the phone saves and reopens, the persisted file includes its trailing newline
        let saved = http_call(case, &chain, &[], ("PUT", route, &auth, &save("", "phone\n")), 200);
        assert_eq!(saved.status, 200, "{:?}", saved.body);
        let read = format!("{route}?checkout={checkout}&path={path}");
        let reopened = http_call(case, &chain, &[], ("GET", &read, &auth, ""), 200);
        assert_eq!(reopened.body["content"], "phone\n");
        // Then a competing desktop save survives the phone's stale update
        chain.repo.write(path, "desktop\n");
        let conflict = http_call(case, &chain, &[], ("PUT", route, &auth, &save("phone\n", "unsaved draft")), 409);
        assert_eq!(conflict.status, 409, "{:?}", conflict.body);
        let latest = http_call(case, &chain, &[], ("GET", &read, &auth, ""), 200);
        assert_eq!(latest.body["content"], "desktop\n");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("untitled.md")).unwrap(), "existing");
    });
}

#[test]
fn a_checkout_without_chat_restores_its_active_tab_and_retries_creation_without_duplicates() {
    exec_testkit::case!(SUITE, |case| {
        // Given the paired T3 API and an empty checkout
        let browsers = std::env::var("PLAYWRIGHT_BROWSERS_PATH").unwrap_or_else(|_| format!("{}/.cache/ms-playwright", std::env::var("HOME").unwrap()));
        let chain = Chain::start_with_env(&[("RCA_BROWSER", "1"), ("RCA_EXEC_BROWSER_NO_SANDBOX","1"), ("PLAYWRIGHT_BROWSERS_PATH", &browsers)]);
        let scopes: Vec<_> = t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect();
        let phone = Phone::connect_with(case, &chain, &scopes);
        let headers = [("Authorization", phone.bearer.as_str()), ("Content-Type", "application/json")];
        let root = std::fs::canonicalize(chain.repo.path()).unwrap();
        let route = format!("/api/mobile/workspace-tabs?checkout=w1::{}", root.display());
        let mut request = |action: Option<Value>| {
            let body = action.as_ref().map(Value::to_string).unwrap_or_default();
            let method = if action.is_some() { "POST" } else { "GET" };
            let reply = http_call(case, &chain, &[], (method, &route, &headers, &body), 200);
            assert_eq!(reply.status, 200, "{:?}", reply.body);
            reply.body
        };
        assert_eq!(request(None)["tabs"], json!([]));
        // When creation is retried and another tab takes focus
        let create = json!({"action":"create","kind":"markdown","id":"note1"});
        request(Some(create.clone()));
        let note = request(Some(create));
        assert_eq!(note["tabs"].as_array().unwrap().len(), 1);
        assert!(!root.join("untitled-2.md").exists());
        let terminal = request(Some(json!({"action":"create","kind":"terminal","id":"pty1"})));
        assert_eq!(terminal["active"], "pty1");
        let thread = terminal["threadId"].as_str().unwrap().to_owned();
        // Then a fresh read restores selection, and closing the note keeps its real file
        request(Some(json!({"action":"select","id":"note1"})));
        assert_eq!(request(None)["active"], "note1");
        assert_eq!(request(Some(json!({"action":"close","id":"note1"})))["active"], "pty1");
        assert!(root.join("untitled.md").is_file());
        let mut desktop = chain.direct();
        let listed = desktop.call("terminal","terminal.list",json!({"workspace":"w1"})).unwrap();
        assert!(listed["terminals"].as_array().unwrap().iter().any(|t| t["terminal"] == format!("{thread}.pty1")));
        let browser = request(Some(json!({"action":"create","kind":"browser","id":"web1"})));
        assert_eq!(browser["active"], "web1");
        assert_eq!(desktop.call("browser","browser.pages",json!({"workspace":"w1"})).unwrap()["pages"].as_array().unwrap().len(), 1);
        request(Some(json!({"action":"close","id":"web1"})));
        assert_eq!(desktop.call("browser","browser.pages",json!({"workspace":"w1"})).unwrap()["pages"], json!([]));
        assert_eq!(request(Some(json!({"action":"close","id":"pty1"})))["tabs"], json!([]));
    });
}
