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
            json!({"type":"text","text":"paired-RCA"}),
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
