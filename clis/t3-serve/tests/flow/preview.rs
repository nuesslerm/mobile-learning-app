//! The `preview.*` rows and `subscribePreviewEvents` through the real router over the agent double: what a phone's call becomes (the `Op::Preview`
//! the agent API is asked: snake_case, the phone's viewport and navigation in the contract's shape), what the phone is told (the snapshot with its
//! own names), what rca does not offer (the server runtime, refused by name, nothing sent), and the stream from the cursor. The chain
//! (`tests/integration/preview_m12.rs`) runs the same calls through the real service.
use super::support::*;
use t3_core::ports::{AgentError, Op};

fn started() -> (Harness, Rpc) {
    let h = Harness::start();
    let all = PairingRequest { scopes: t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect(), ..PairingRequest::default() };
    let rpc = connect_with(&h, all);
    (h, rpc)
}

fn asked(h: &Harness) -> Vec<(String, Value)> {
    h.agent.ops.lock().unwrap().iter().filter_map(|op| if let Op::Preview { method, params } = op { Some((method.to_string(), params.clone())) } else { None }).collect()
}

fn reply(h: &Harness, method: &'static str, answer: Result<Value, AgentError>) {
    h.agent.replies.lock().unwrap().insert(method, answer);
}

fn session(extra: Value) -> Value {
    let mut view = json!({"thread": "t1", "tab": "tab-1", "nav": {"state": "loading", "url": "http://localhost:3000/", "title": ""}, "can_go_back": false, "can_go_forward": false, "viewport": {"mode": "fill"}, "updated_ms": 1_700_000_000_000_i64});
    view.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    view
}

#[test]
fn opening_a_tab_sends_the_address_made_whole_and_the_viewport_in_the_contracts_shape_and_shows_the_phone_the_snapshot_the_service_kept() {
    // Given a service that answers an open with a tab loading the address it was given
    let (h, mut rpc) = started();
    reply(&h, "preview.open", Ok(json!({"session": session(json!({"viewport": {"mode": "preset", "width": 390, "height": 844, "preset": "iphone-14"}, "profile": "p1"}))})));
    // When the phone opens a tab on a bare local address with a preset viewport and a profile
    let exit = rpc.call("preview.open", json!({"threadId": "t1", "url": "localhost:3000", "viewport": {"_tag": "preset", "width": 390, "height": 844, "presetId": "iphone-14"}, "profileId": "p1", "reveal": true}));
    // Then the service was asked once with the address made whole and the viewport as the contract has it
    assert_eq!(asked(&h), [("preview.open".to_string(), json!({"thread": "t1", "url": "http://localhost:3000", "viewport": {"mode": "preset", "width": 390, "height": 844, "preset": "iphone-14"}, "profile": "p1", "reveal": true}))]);
    // And the phone is shown the snapshot with its own names
    let tab = success(&exit);
    assert_eq!((tab["threadId"].clone(), tab["tabId"].clone(), tab["navStatus"].clone(), tab["viewport"].clone(), tab["profileId"].clone(), tab["updatedAt"].clone()), (json!("t1"), json!("tab-1"), json!({"_tag": "Loading", "url": "http://localhost:3000/", "title": ""}), json!({"_tag": "preset", "width": 390, "height": 844, "presetId": "iphone-14"}), json!("p1"), json!("2023-11-14T22:13:20.000Z")));
}

#[test]
fn what_rca_does_not_offer_or_the_phone_got_wrong_is_refused_by_name_and_nothing_is_sent() {
    let (h, mut rpc) = started();
    // When the phone opens a server-runtime tab, a file address, an empty address, and resizes with a viewport that is no viewport
    let server = rpc.call("preview.open", json!({"threadId": "t1", "runtime": "server"}));
    let file = rpc.call("preview.open", json!({"threadId": "t1", "url": "file:///etc/passwd"}));
    let empty = rpc.call("preview.open", json!({"threadId": "t1", "url": "  "}));
    let bad = rpc.call("preview.resize", json!({"threadId": "t1", "tabId": "a", "viewport": {"_tag": "wide"}}));
    // Then the server runtime is an UnsupportedOperationError that says why, the addresses are PreviewInvalidUrlError with their reason, the viewport a defect
    assert_eq!((failure(&server)["_tag"].clone(), failure(&server)["method"].clone()), (json!("UnsupportedOperationError"), json!("preview.open")));
    assert!(failure(&server)["message"].as_str().unwrap().contains("starts no browser"), "{server}");
    assert_eq!((failure(&file)["_tag"].clone(), failure(&file)["reason"].clone(), failure(&file)["protocol"].clone()), (json!("PreviewInvalidUrlError"), json!("unsupported-protocol"), json!("file:")));
    assert_eq!(failure(&empty)["reason"], json!("empty"));
    assert!(bad.to_string().contains("viewport"), "{bad}");
    // And the service was asked nothing
    assert!(asked(&h).is_empty());
}

#[test]
fn a_change_to_a_tab_the_service_does_not_have_is_the_phones_lookup_error_naming_the_tab_and_resizing_adjusting_reporting_and_closing_are_one_request_each() {
    let (h, mut rpc) = started();
    // Given a service that has no such tab
    for method in ["preview.resize", "preview.adjust", "preview.report", "preview.close"] {
        reply(&h, method, Err(AgentError::Refused { code: "PREVIEW_SESSION_NOT_FOUND".into(), message: "thread t1 has no preview tab nope".into() }));
    }
    // When the phone resizes, adjusts, reports and closes it
    let resize = rpc.call("preview.resize", json!({"threadId": "t1", "tabId": "nope", "viewport": {"_tag": "freeform", "width": 800, "height": 600}}));
    let adjust = rpc.call("preview.adjust", json!({"threadId": "t1", "tabId": "nope", "colorScheme": "dark", "zoomFactor": 1.25, "hardReload": true, "clear": "cache"}));
    let report = rpc.call("preview.reportStatus", json!({"threadId": "t1", "tabId": "nope", "navStatus": {"_tag": "LoadFailed", "url": "http://a.test/", "title": "", "code": -105, "description": "no"}, "canGoBack": true, "canGoForward": false}));
    let close = rpc.call("preview.close", json!({"threadId": "t1", "tabId": "nope"}));
    // Then each is a PreviewSessionLookupError naming the thread and the tab
    for exit in [&resize, &adjust, &report, &close] {
        assert_eq!((failure(exit)["_tag"].clone(), failure(exit)["threadId"].clone(), failure(exit)["tabId"].clone()), (json!("PreviewSessionLookupError"), json!("t1"), json!("nope")), "{exit}");
    }
    // And each was one request whose params are the contract's
    assert_eq!(asked(&h), [
        ("preview.resize".to_string(), json!({"thread": "t1", "tab": "nope", "viewport": {"mode": "freeform", "width": 800, "height": 600}})),
        ("preview.adjust".to_string(), json!({"thread": "t1", "tab": "nope", "color_scheme": "dark", "zoom": 1.25, "hard_reload": true, "clear": "cache"})),
        ("preview.report".to_string(), json!({"thread": "t1", "tab": "nope", "nav": {"state": "failed", "url": "http://a.test/", "title": "", "code": -105, "description": "no"}, "can_go_back": true, "can_go_forward": false})),
        ("preview.close".to_string(), json!({"thread": "t1", "tab": "nope"})),
    ]);
}

#[test]
fn listing_shows_the_threads_tabs_with_the_epoch_and_revision_and_closing_a_thread_names_no_tab() {
    let (h, mut rpc) = started();
    reply(&h, "preview.list", Ok(json!({"sessions": [session(json!({"zoom": 1.5, "color_scheme": "dark"}))], "epoch": "epoch-7", "revision": 12})));
    reply(&h, "preview.close", Ok(json!({"closed": ["tab-1"]})));
    // When the phone lists a thread's tabs and closes the thread's tabs
    let listed_exit = rpc.call("preview.list", json!({"threadId": "t1"}));
    let listed = success(&listed_exit);
    let closed = rpc.call("preview.close", json!({"threadId": "t1"}));
    // Then the list has the snapshot with zoom and colour scheme, the epoch as serverEpoch and the revision, and the close sent no tab and answered nothing
    assert_eq!((listed["sessions"][0]["zoomFactor"].clone(), listed["sessions"][0]["colorScheme"].clone(), listed["serverEpoch"].clone(), listed["revision"].clone()), (json!(1.5), json!("dark"), json!("epoch-7"), json!(12)));
    assert_eq!(asked(&h)[1], ("preview.close".to_string(), json!({"thread": "t1"})));
    assert_eq!(*success(&closed), Value::Null);
}

#[test]
fn the_event_stream_starts_from_the_cursor_the_service_gave_and_sends_each_event_as_the_phones_and_none_for_what_is_no_preview_event() {
    // Given a service whose stream stands at epoch e1 number 4, and a stream that reports a tab opened, an event that is none, and a close
    let (h, mut rpc) = started();
    reply(&h, "preview.subscribe", Ok(json!({"topic": "preview", "epoch": "e1", "seq": 4, "reset": false})));
    let base = |seq: u64, kind: &str| json!({"seq": seq, "type": kind, "thread": "t1", "tab": "tab-1", "at_ms": 1_700_000_000_000_i64, "epoch": "e1", "revision": seq});
    let mut opened = base(5, "preview.opened");
    opened["session"] = session(json!({}));
    *h.agent.preview_events.lock().unwrap() = vec![opened, base(6, "schedule.run"), base(7, "preview.closed")];
    // When the phone subscribes
    let stream = rpc.send("subscribePreviewEvents", json!({}));
    let values = rpc.chunk(&stream);
    // Then the stream was followed from the cursor, and the phone got the open with its snapshot and then the close, in one chunk: the other event made none
    assert_eq!(*h.agent.preview_cursor.lock().unwrap(), Some(("e1".to_string(), 4)));
    assert_eq!(values.len(), 2, "{values:?}");
    let (first, second) = (&values[0], &values[1]);
    assert_eq!((first["type"].clone(), first["serverEpoch"].clone(), first["revision"].clone(), first["snapshot"]["tabId"].clone()), (json!("opened"), json!("e1"), json!(5), json!("tab-1")));
    assert_eq!((second["type"].clone(), second["revision"].clone()), (json!("closed"), json!(7)));
    assert!(rpc.try_recv(std::time::Duration::from_millis(300)).is_none());
}
