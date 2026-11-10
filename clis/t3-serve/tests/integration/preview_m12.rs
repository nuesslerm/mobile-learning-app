//! The `preview.*` rows and `subscribePreviewEvents` through the chain: a tab the phone opens is a tab of the real rca-service, which keeps its
//! viewport, zoom, colour scheme and reported navigation, lists it with the epoch and revision of its stream, and tells every phone following the
//! stream each change in order. The service starts no browser, so a server-runtime tab is refused, and a call counts for its row only when it was
//! answered and the agent API method ran in its window.
use crate::chain::last_window;
use crate::rig::*;
use crate::support::*;

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

/// The method answered in the last call's window.
fn asked(case: &exec_testkit::Case, method: &str) {
    let (agent, _) = last_window(case);
    assert!(agent.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {agent:?}");
}

#[test]
fn tabs_are_opened_listed_resized_adjusted_reported_and_closed_and_a_second_phone_following_the_stream_sees_each_change_in_order() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a paired phone
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        // When it follows the events and, on the same socket so that the service has the stream's cursor first, opens a tab
        let (stream, opened_first) = case.call(&chain.tap, &rows(&["rpc:subscribePreviewEvents"]), || {
            let stream = phone.watch.send("subscribePreviewEvents", json!({}));
            let open = phone.watch.send("preview.open", json!({"threadId": "t1", "url": "localhost:3000"}));
            let (mut exit, mut values) = (None, None);
            while exit.is_none() || values.is_none() {
                let frame = phone.watch.recv();
                match (frame["_tag"].as_str(), &frame["requestId"]) {
                    (Some("Exit"), id) if *id == open => exit = Some(frame["exit"].clone()),
                    (Some("Chunk"), id) if *id == stream => {
                        phone.watch.ack(&stream);
                        values = Some(frame["values"].as_array().unwrap().clone());
                    }
                    _ => panic!("unexpected frame {frame}"),
                }
            }
            let (exit, values) = (exit.unwrap(), values.unwrap());
            (exit["_tag"] == "Success" && !values.is_empty(), (stream, values))
        });
        asked(case, "preview.subscribe");
        // Then the first event is that tab's `opened`, with the address made whole and an epoch and revision of the service's
        assert_eq!((opened_first[0]["type"].clone(), opened_first[0]["snapshot"]["navStatus"].clone()), (json!("opened"), json!({"_tag": "Loading", "url": "http://localhost:3000", "title": ""})), "{opened_first:?}");
        let epoch = opened_first[0]["serverEpoch"].clone();
        assert!(epoch.is_string() && opened_first[0]["revision"].as_u64().unwrap() >= 1, "{opened_first:?}");
        // When the phone opens a second tab with a freeform viewport
        let opened = phone.call(case, &chain, &["rpc:preview.open"], "preview.open", json!({"threadId": "t1", "url": "https://example.test/app", "viewport": {"_tag": "freeform", "width": 800, "height": 600}}));
        asked(case, "preview.open");
        let tab = success(&opened)["tabId"].as_str().expect("rca named the tab").to_string();
        assert_eq!((success(&opened)["viewport"].clone(), success(&opened)["navStatus"]["url"].clone()), (json!({"_tag": "freeform", "width": 800, "height": 600}), json!("https://example.test/app")));
        // And resizes it to a preset, adjusts its colour scheme and zoom with a reload, and reports it loaded
        let resized = phone.call(case, &chain, &["rpc:preview.resize"], "preview.resize", json!({"threadId": "t1", "tabId": tab, "viewport": {"_tag": "preset", "width": 390, "height": 844, "presetId": "iphone-14"}}));
        asked(case, "preview.resize");
        let adjusted = phone.call(case, &chain, &["rpc:preview.adjust"], "preview.adjust", json!({"threadId": "t1", "tabId": tab, "colorScheme": "dark", "zoomFactor": 1.25, "hardReload": true}));
        asked(case, "preview.adjust");
        let reported = phone.call(case, &chain, &[], "preview.reportStatus", json!({"threadId": "t1", "tabId": tab, "navStatus": {"_tag": "Success", "url": "https://example.test/app", "title": "App"}, "canGoBack": true, "canGoForward": false}));
        asked(case, "preview.report");
        assert_eq!(*success(&reported), Value::Null);
        // Then the tab holds all of it
        assert_eq!((success(&resized)["viewport"]["presetId"].clone(), success(&adjusted)["colorScheme"].clone(), success(&adjusted)["zoomFactor"].clone()), (json!("iphone-14"), json!("dark"), json!(1.25)));
        // And the thread's list has both tabs, the second as reported, with this run's epoch
        let listed = phone.call(case, &chain, &["rpc:preview.list"], "preview.list", json!({"threadId": "t1"}));
        asked(case, "preview.list");
        let sessions = success(&listed)["sessions"].as_array().unwrap().clone();
        assert_eq!(sessions.len(), 2, "{listed}");
        let second = sessions.iter().find(|s| s["tabId"] == json!(tab)).expect("the second tab is listed");
        assert_eq!((second["navStatus"].clone(), second["canGoBack"].clone(), second["viewport"]["_tag"].clone(), second["zoomFactor"].clone()), (json!({"_tag": "Success", "url": "https://example.test/app", "title": "App"}), json!(true), json!("preset"), json!(1.25)));
        assert_eq!(success(&listed)["serverEpoch"], epoch);
        // When the phone closes the second tab
        let closed = phone.call(case, &chain, &["rpc:preview.close"], "preview.close", json!({"threadId": "t1", "tabId": tab}));
        asked(case, "preview.close");
        assert_eq!(*success(&closed), Value::Null);
        // Then the list has one tab, and the stream the phone follows told it every change, in order, of the kinds they were
        let after = phone.call(case, &chain, &["rpc:preview.list"], "preview.list", json!({"threadId": "t1"}));
        assert_eq!(success(&after)["sessions"].as_array().unwrap().len(), 1);
        let mut events = Vec::new();
        while events.len() < 5 {
            events.extend(phone.watch.chunk(&stream));
        }
        let kinds: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["opened", "resized", "resized", "navigated", "closed"], "{events:?}");
        let first_revision = opened_first[0]["revision"].as_u64().unwrap();
        let revisions: Vec<u64> = events.iter().map(|e| e["revision"].as_u64().unwrap()).collect();
        assert_eq!(revisions, (1..=5).map(|n| first_revision + n).collect::<Vec<_>>());
        assert!(events.iter().all(|e| e["tabId"] == json!(tab) && e["serverEpoch"] == epoch), "{events:?}");
        assert_eq!(events[2]["request"], json!({"hardReload": true}));
        assert_eq!(events[3]["snapshot"]["navStatus"]["title"], json!("App"));
    });
}

#[test]
fn a_server_runtime_tab_a_file_address_and_a_tab_that_is_not_there_are_refused_and_nothing_is_opened() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a paired phone
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        // When it opens a server-runtime tab and a file address, and resizes a tab nobody opened (these are refusals, not calls the rows count)
        let server = phone.calls.call("preview.open", json!({"threadId": "t9", "runtime": "server"}));
        let file = phone.calls.call("preview.open", json!({"threadId": "t9", "url": "file:///etc/passwd"}));
        let missing = phone.calls.call("preview.resize", json!({"threadId": "t9", "tabId": "nope", "viewport": {"_tag": "fill"}}));
        // Then each is refused with its error
        assert_eq!(failure(&server)["_tag"], json!("UnsupportedOperationError"));
        assert_eq!((failure(&file)["_tag"].clone(), failure(&file)["reason"].clone()), (json!("PreviewInvalidUrlError"), json!("unsupported-protocol")));
        assert_eq!((failure(&missing)["_tag"].clone(), failure(&missing)["tabId"].clone()), (json!("PreviewSessionLookupError"), json!("nope")));
        // And the thread has no tab
        let listed = phone.calls.call("preview.list", json!({"threadId": "t9"}));
        assert_eq!(success(&listed)["sessions"], json!([]));
    });
}
