//! The `Tap` adapter through the thin binary: `mimic-toy-wire` is a reference whose target is an HTTP and WebSocket server, reached
//! through a tap, so recording it proves what the tap saw and replaying it proves the same behaviour reads the same.
use crate::support::*;

const HTTP_AND_WS: [(&str, &str); 4] = [("http", "http"), ("ws", "ws"), ("refused", "http"), ("missing", "http")];

fn wire_world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let set = dir.path().join("wire-set");
    std::fs::create_dir_all(&set).unwrap();
    let w = World { dir, set };
    let toy = env!("CARGO_BIN_EXE_mimic-toy-wire");
    w.write("pin.json", r#"{"version":"toy-wire-1.0"}"#);
    w.write("rules.json", RULES);
    let case = |id: &str, op: &str, input: Value| json!({"kind": "corpus", "name": id, "op": op, "cases": [{"id": id, "row": format!("wire:{id}"), "input": input}]});
    let corpus = json!({"schema": "mimic-corpus/1", "seed": 1, "generators": [
        case("http", "http", json!({"method": "POST", "path": "/echo?ticket=abcdefgh", "body": {"say": "hello"}})),
        case("ws", "ws", json!({"path": "/ws?wsTicket=zzzzzzzz", "frames": [{"id": 1, "method": "ping"}, {"id": 2, "method": "pong"}, {"binary": 200}]})),
        case("refused", "http", json!({"method": "GET", "path": "/ws"})),
        case("missing", "http", json!({"method": "GET", "path": "/nowhere"})),
    ]});
    w.write("corpus.json", &corpus.to_string());
    w.write(
        "mimic.toml",
        &format!("[reference]\ncommand = [{toy:?}]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n\n[candidate.same]\ncommand = [{toy:?}]\n\n[candidate.change]\ncommand = [{toy:?}, \"change-value\"]\n"),
    );
    w
}

fn steps_of(w: &World, id: &str) -> Vec<Value> {
    w.interactions().into_iter().find(|i| i["id"] == id).unwrap_or_else(|| panic!("no interaction {id}"))["steps"].as_array().unwrap().clone()
}

fn on(steps: &[Value], channel: &str) -> Vec<Value> {
    steps.iter().filter(|s| s["channel"] == channel).cloned().collect()
}

#[test]
fn recording_a_http_exchange_keeps_request_and_answer_and_masks_what_the_server_made_up() {
    // Given a reference that answers a POST with a made-up session id and time, and a ticket in the query
    let w = wire_world();
    // When it is recorded
    w.record();
    // Then the call and the return are both steps, the body is JSON, and the ticket, uuid and time are masks
    let steps = steps_of(&w, "http");
    let http = on(&steps, "http");
    assert_eq!(http.len(), 2, "{steps:?}");
    assert_eq!((http[0]["dir"].clone(), http[0]["payload"]["method"].clone(), http[0]["payload"]["body"].clone()), (json!("call"), json!("POST"), json!({"say": "hello"})));
    assert_eq!(http[0]["payload"]["path"], json!("/echo?ticket=<ws-ticket>"));
    let answer = &http[1]["payload"];
    assert_eq!((answer["status"].clone(), answer["body"]["echo"].clone(), answer["body"]["at"].clone()), (json!(200), json!("hello"), json!("<ts>")));
    assert_eq!(answer["body"]["session"], json!("<uuid:1>"));
    // and an unrouted path is an ordinary 404 with its text body
    let missing = on(&steps_of(&w, "missing"), "http");
    assert_eq!((missing[1]["payload"]["status"].clone(), missing[1]["payload"]["body"].clone()), (json!(404), json!("no such route")));
}

#[test]
fn recording_a_websocket_session_keeps_every_frame_whole_in_order_with_the_open_and_both_closes() {
    // Given a reference whose socket echoes each request, reverses a 200-byte binary frame and then closes
    let w = wire_world();
    // When it is recorded
    w.record();
    // Then the open, three frames each way, the close handshake and the dropped connection are the steps
    let steps = steps_of(&w, "ws");
    let shape: Vec<(String, String)> = steps.iter().map(|s| (s["channel"].as_str().unwrap().into(), s["dir"].as_str().unwrap().into())).collect();
    let open = shape.iter().position(|s| s.0 == "ws:open").expect("an open");
    assert_eq!(open, 0, "{shape:?}");
    assert_eq!(steps[0]["payload"], json!({"path": "/ws?wsTicket=<ws-ticket>", "status": 101}));
    let frames = on(&steps, "ws");
    let to = |dir: &str| frames.iter().filter(|f| f["dir"] == dir).cloned().collect::<Vec<_>>();
    assert_eq!((to("c2s").len(), to("s2c").len()), (3, 3), "{frames:?}");
    // a text frame is JSON under its request id, a binary frame is kept whole by length
    assert_eq!(to("s2c")[0]["rid"], json!("1"));
    assert_eq!(to("s2c")[0]["payload"]["result"]["echo"], json!("ping"));
    let binary: Vec<&Value> = frames.iter().filter(|f| f["kind"] == "binary").collect();
    assert_eq!(binary.iter().map(|f| f["len"].clone()).collect::<Vec<_>>(), vec![json!(200), json!(200)]);
    assert_ne!(binary[0]["payload"], binary[1]["payload"], "the server reversed the bytes");
    let closes: Vec<(String, Value, Value)> = on(&steps, "ws:close").iter().map(|c| (c["dir"].as_str().unwrap().into(), c["payload"]["code"].clone(), c["payload"]["tcp"].clone())).collect();
    assert_eq!(closes, vec![("c2s".into(), json!(1000), json!(false)), ("s2c".into(), json!(1000), json!(false)), ("s2c".into(), Value::Null, json!(true))]);
}

#[test]
fn a_refused_upgrade_is_recorded_as_the_http_answer_the_client_got() {
    // Given a reference that answers a plain GET of its socket path with 426
    let w = wire_world();
    // When it is recorded
    w.record();
    // Then the exchange is an http call and return with the refusal and its body
    let http = on(&steps_of(&w, "refused"), "http");
    assert_eq!((http[1]["payload"]["status"].clone(), http[1]["payload"]["body"]["code"].clone()), (json!(426), json!("upgrade_required")));
}

#[test]
fn replaying_the_same_target_is_clean_and_a_changed_reply_is_a_drift_in_each_kind_of_interaction() {
    // Given a recorded set
    let w = wire_world();
    w.record();
    // When the same program is replayed, it made up other ids and times
    let same = w.run("replay", &["--candidate", "same"]);
    // Then every interaction is clean
    assert_eq!((same.code, same.json["severity"].clone()), (0, json!("none")), "{}", same.json);
    assert_eq!(same.json["summary"]["total"], json!(HTTP_AND_WS.len()));
    // When a candidate answers other words
    let changed = w.run("replay", &["--candidate", "change"]);
    // Then the http echo and the websocket result are both reported
    assert_eq!((changed.code, changed.json["severity"].clone()), (1, json!("drift")), "{}", changed.json);
    let broken: Vec<&str> = changed.json["results"].as_array().unwrap().iter().filter(|r| r["diffs"].as_array().is_some_and(|d| !d.is_empty())).map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(broken, vec!["http", "ws"], "{}", changed.json);
}
