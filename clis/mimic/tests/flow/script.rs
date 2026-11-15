//! `driver = "script"` through the thin binary: the step interpreter plays a corpus of HTTP cases against a server that is already running
//! (a `url` target), the recording keeps the wire as steps with what the server makes up masked, and a replay against a server that
//! behaves the same reads clean while one that answers differently is breaking.
use crate::support::*;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

fn answer(mut stream: TcpStream, tweak: bool, counter: std::sync::Arc<std::sync::atomic::AtomicU64>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let path = line.split(' ').nth(1).unwrap_or("/").to_string();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
            break;
        }
    }
    let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    // a made-up id on every answer, as a real server makes up one
    let body = match path.as_str() {
        "/hello" => format!(r#"{{"greeting":"{}","session":"{:08x}-0000-4000-8000-{:012x}"}}"#, if tweak { "changed" } else { "hello" }, n, n),
        _ => String::new(),
    };
    let status = if path == "/hello" { "200 OK" } else { "404 Not Found" };
    let _ = stream.write_all(format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
}

fn server(tweak: bool) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let counter = counter.clone();
            std::thread::spawn(move || answer(stream, tweak, counter));
        }
    });
    addr
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let set = dir.path().join("script-set");
    std::fs::create_dir_all(&set).unwrap();
    let w = World { dir, set };
    let (reference, same, changed) = (server(false), server(false), server(true));
    w.write("pin.json", r#"{"version":"fake-1.0"}"#);
    w.write("rules.json", RULES);
    let case = |id: &str, path: &str| json!({"kind": "corpus", "name": id, "op": "script", "cases": [{"id": id, "row": format!("http:{id}"), "input": {"script": [{"http": {"path": path}}], "settle_ms": 20}}]});
    w.write("corpus.json", &json!({"schema": "mimic-corpus/1", "seed": 1, "generators": [case("hello", "/hello"), case("missing", "/nowhere")]}).to_string());
    let target = |addr: SocketAddr| format!("driver = \"script\"\n\n[%.script]\ntarget = \"url\"\naddr = \"{addr}\"\nworkspace = \"/w\"\n");
    w.write(
        "mimic.toml",
        &format!(
            "[reference]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n{}\n[candidate.same]\n{}\n[candidate.change]\n{}",
            target(reference).replace("[%.script]", "[reference.script]"),
            target(same).replace("[%.script]", "[candidate.same.script]"),
            target(changed).replace("[%.script]", "[candidate.change.script]"),
        ),
    );
    w
}

#[test]
fn a_corpus_run_through_the_interpreter_records_the_wire_with_what_the_server_made_up_masked() {
    // Given a server that answers /hello with a made-up session id and 404s the rest
    let w = world();

    // When the corpus is recorded with the script driver
    w.record();

    // Then each case is its HTTP call and return, and the id is a mask
    let steps = |id: &str| w.interactions().into_iter().find(|i| i["id"] == id).unwrap()["steps"].as_array().unwrap().clone();
    let hello = steps("hello");
    assert_eq!((hello[0]["dir"].clone(), hello[0]["payload"]["path"].clone(), hello[1]["payload"]["status"].clone()), (json!("call"), json!("/hello"), json!(200)));
    assert_eq!(hello[1]["payload"]["body"]["session"], json!("<uuid:1>"));
    assert_eq!(steps("missing")[1]["payload"]["status"], json!(404));
}

#[test]
fn a_server_that_behaves_the_same_replays_clean_and_one_that_answers_differently_drifts_at_the_changed_value() {
    // Given a recorded set
    let w = world();
    w.record();

    // When it is replayed against a server that makes up other ids and one that greets differently
    let same = w.run("replay", &["--candidate", "same"]);
    let changed = w.run("replay", &["--candidate", "change"]);

    // Then the first is clean and the second names the value that changed
    assert_eq!((same.code, same.json["ok"].clone(), same.json["severity"].clone()), (0, json!(true), json!("none")), "{}", same.json);
    assert_eq!((changed.code, changed.json["severity"].clone()), (1, json!("drift")), "{}", changed.json);
    assert!(changed.json.to_string().contains("greeting"), "{}", changed.json);
}

#[test]
fn a_script_driver_without_its_script_table_is_refused_when_the_file_is_read() {
    // Given a mimic.toml that names the driver and no table
    let w = world();
    w.write("mimic.toml", "[reference]\ndriver = \"script\"\npin = \"pin.json\"\n");

    // When it is used, Then the refusal names the missing table
    let r = w.run("record", &[]);
    assert_ne!(r.code, 0);
    assert!(r.json["error"].as_str().unwrap_or_default().contains("needs a [reference.script] table"), "{}", r.json);
}

mod orca_target {
    use crate::support::*;

    /// A sidecar that answers every control line by its id, every request with `echo`, and ends on `quit`.
    fn sidecar(echo: &str) -> String {
        let script = r#"while read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([0-9]*\)".*/\1/p')
  case "$line" in
    *'"op":"connect"'*) echo "{\"id\":\"$id\",\"ok\":{\"state\":\"connected\"}}" ;;
    *'"op":"request"'*) echo "{\"id\":\"$id\",\"ok\":{\"response\":{\"ok\":true,\"result\":{\"echo\":ECHO}}}}" ;;
    *'"op":"quit"'*) echo "{\"id\":\"$id\",\"ok\":{}}"; exit 0 ;;
    *) echo "{\"id\":\"$id\",\"ok\":{}}" ;;
  esac
done"#;
        script.replace("ECHO", &format!("\\\"{echo}\\\""))
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let set = dir.path().join("orca-set");
        std::fs::create_dir_all(&set).unwrap();
        let w = World { dir, set };
        w.write("pin.json", r#"{"version":"fake-1.0"}"#);
        w.write("rules.json", RULES);
        w.write("same.sh", &sidecar("hello"));
        w.write("reference.sh", &sidecar("hello"));
        w.write("changed.sh", &sidecar("changed"));
        let case = |id: &str, method: &str| json!({"kind": "corpus", "name": id, "op": "script", "cases": [{"id": id, "row": format!("rpc:{method}"), "input": {"requests": [{"method": method, "params": {}}]}}]});
        w.write("corpus.json", &json!({"schema": "mimic-corpus/1", "seed": 1, "generators": [case("files-list", "files.list"), case("repo-list", "repo.list")]}).to_string());
        let target = |table: &str, script: &str| format!("driver = \"script\"\n\n[{table}]\ntarget = \"orca\"\nsidecar = [\"sh\", \"{script}\"]\n");
        w.write(
            "mimic.toml",
            &format!(
                "[reference]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n{}\n[candidate.same]\n{}\n[candidate.change]\n{}",
                target("reference.script", "reference.sh"),
                target("candidate.same.script", "same.sh"),
                target("candidate.change.script", "changed.sh"),
            ),
        );
        w
    }

    #[test]
    fn a_legacy_orca_corpus_through_the_sidecar_records_the_request_and_the_answer_as_rows() {
        // Given a sidecar standing for the phone's client and a corpus of two Orca requests
        let w = world();

        // When the corpus is recorded with the orca target
        w.record();

        // Then each case is the request step and the answer step, both with the request's index as their rid
        let steps = w.interactions().into_iter().find(|i| i["id"] == "files-list").unwrap()["steps"].as_array().unwrap().clone();
        assert_eq!(steps.len(), 2, "{steps:?}");
        assert_eq!((steps[0]["dir"].clone(), steps[0]["rid"].clone(), steps[0]["payload"]["method"].clone()), (json!("c2s"), json!("0"), json!("files.list")));
        assert_eq!((steps[1]["dir"].clone(), steps[1]["payload"]["result"]["echo"].clone()), (json!("s2c"), json!("hello")));
    }

    #[test]
    fn a_host_that_answers_the_same_replays_clean_and_one_that_answers_differently_is_breaking_at_the_changed_value() {
        // Given a recorded set
        let w = world();
        w.record();

        // When it is replayed against a candidate that answers the same and one that does not
        let same = w.run("replay", &["--candidate", "same"]);
        let changed = w.run("replay", &["--candidate", "change"]);

        // Then the first is clean and the second names the value that changed
        assert_eq!((same.code, same.json["severity"].clone()), (0, json!("none")), "{}", same.json);
        assert_eq!(changed.code, 1, "{}", changed.json);
        assert!(changed.json.to_string().contains("echo"), "{}", changed.json);
    }

    #[test]
    fn an_orca_target_without_a_sidecar_is_refused_when_the_file_is_read() {
        let w = world();
        w.write("mimic.toml", "[reference]\ndriver = \"script\"\npin = \"pin.json\"\n\n[reference.script]\ntarget = \"orca\"\n");

        let r = w.run("record", &[]);

        assert_ne!(r.code, 0);
        assert!(r.json["error"].as_str().unwrap_or_default().contains("needs a sidecar command"), "{}", r.json);
    }
}
