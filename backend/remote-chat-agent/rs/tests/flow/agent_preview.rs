//! The `preview` groups end to end: a dev server started by the real `rca-exec` (`fake-devserver` is the server, a real process on a real port)
//! through the service, and the preview tabs and their stream as two mappers see them on sockets of their own. Every recorded exchange of the
//! server methods is replayed against the real executor.
use crate::agent_threads::chain;
use crate::harness::Env;
use exec_testkit::devserver::{ask, command, free_port};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn alive(pid: i64) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "))
}

async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_workspace_with_an_executor_when_a_dev_server_is_started_through_the_service_then_it_runs_there_without_the_executors_secrets_and_stops_when_asked() {
    // Given the real chain, whose executor holds a token and a key, and a free port
    let env = Env::process(&[("GH_TOKEN", "ghp_canary"), ("ANTHROPIC_API_KEY", "sk-canary")]);
    let chain = chain(&env).await;
    let mut agent = chain.agent().await;
    let port = free_port();
    // When the server is started in the workspace
    let started = agent.ask("preview.start", json!({"workspace": "w1", "command": command(""), "port": port, "name": "web", "env": {"FEATURE": "on"}, "ready_timeout_ms": 10000})).await;
    let info = started["result"].clone();
    // Then it listens on the port at the executor, was told its port and the flag, and saw neither secret
    assert_eq!((info["port"].clone(), info["url"].clone(), info["state"].clone(), info["name"].clone()), (json!(port), json!(format!("http://127.0.0.1:{port}/")), json!("running"), json!("web")), "{started}");
    let seen = ask(port);
    let names: Vec<&str> = seen["env"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(names.contains(&"FEATURE") && names.contains(&"PORT"), "{names:?}");
    assert!(!names.contains(&"GH_TOKEN") && !names.contains(&"ANTHROPIC_API_KEY"), "a secret reached the server: {names:?}");
    assert_eq!(std::fs::canonicalize(seen["cwd"].as_str().unwrap()).unwrap(), std::fs::canonicalize(chain.repo.path()).unwrap());
    // And the list shows it running, a second start on the port is refused with the executor's code, and a stop ends the process
    let listed = agent.ask("preview.servers", json!({"workspace": "w1"})).await;
    assert_eq!((listed["result"]["previews"][0]["id"].clone(), listed["result"]["previews"][0]["state"].clone()), (info["id"].clone(), json!("running")), "{listed}");
    let again = agent.ask("preview.start", json!({"workspace": "w1", "command": command(""), "port": port})).await;
    assert_eq!(again["error"]["code"], json!("ALREADY_EXISTS"), "{again}");
    let stopped = agent.ask("preview.stop", json!({"workspace": "w1", "id": info["id"]})).await;
    assert_eq!(stopped["result"], json!({"stopped": true}));
    let pid = info["pid"].as_i64().unwrap();
    wait_until("the server's process to end", || !alive(pid)).await;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
    let after = agent.ask("preview.servers", json!({"workspace": "w1"})).await;
    assert_eq!(after["result"]["previews"][0]["state"], json!("exited"), "{after}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_the_server_methods_recorded_in_the_contract_when_replayed_on_the_real_executor_then_each_response_is_what_was_recorded() {
    for fixture in agent_api_preview::testing::fixtures().iter().filter(|f| agent_api_preview::METHODS.iter().any(|m| m.name() == f.method)) {
        for exchange in &fixture.exchanges {
            // When an exchange's request is made on a service and executor of their own
            let env = Env::process(&[]);
            let chain = chain(&env).await;
            let mut agent = chain.agent().await;
            let mut answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
            answer["id"] = exchange.response["id"].clone();
            // Then the answer is the recorded one
            assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
        }
    }
}

async fn expect(agent: &mut crate::mapper::Agent, kind: &str) -> Value {
    let event = agent.event().await;
    assert_eq!(event.kind, kind, "{event:?}");
    event.to_value()["event"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_two_mappers_when_one_changes_a_preview_tab_then_the_other_follows_each_change_in_order_and_can_resume_after_a_gap() {
    // Given the real chain and a mapper following the preview stream on a socket of its own
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let mut watcher = chain.agent().await;
    let followed = watcher.ask("preview.subscribe", json!({})).await;
    let epoch = followed["result"]["epoch"].as_str().unwrap().to_string();
    assert_eq!((followed["result"]["topic"].clone(), followed["result"]["seq"].clone(), followed["result"]["reset"].clone()), (json!("preview"), json!(0), json!(false)), "{followed}");
    let mut agent = chain.agent().await;
    // When the other mapper opens a tab, the app reports it loaded, the viewport and the zoom change, and the tab closes
    let opened = agent.ask("preview.open", json!({"thread": "t1", "url": "http://127.0.0.1:3000/"})).await;
    let tab = opened["result"]["session"]["tab"].as_str().unwrap().to_string();
    agent.ask("preview.report", json!({"thread": "t1", "tab": tab, "nav": {"state": "success", "url": "http://127.0.0.1:3000/", "title": "App"}, "can_go_back": false, "can_go_forward": false})).await;
    agent.ask("preview.resize", json!({"thread": "t1", "tab": tab, "viewport": {"mode": "freeform", "width": 800, "height": 600}})).await;
    agent.ask("preview.adjust", json!({"thread": "t1", "tab": tab, "zoom": 1.5, "hard_reload": true})).await;
    let listed = agent.ask("preview.list", json!({"thread": "t1"})).await;
    agent.ask("preview.close", json!({"thread": "t1", "tab": tab})).await;
    // Then the watcher saw each change as the event of its kind, numbered one after the other, naming the tab and this run's epoch
    let (a, b, c, d, e) = (expect(&mut watcher, "preview.opened").await, expect(&mut watcher, "preview.navigated").await, expect(&mut watcher, "preview.resized").await, expect(&mut watcher, "preview.resized").await, expect(&mut watcher, "preview.closed").await);
    assert_eq!([&a, &b, &c, &d, &e].map(|ev| ev["seq"].clone()), [json!(1), json!(2), json!(3), json!(4), json!(5)]);
    assert!([&a, &b, &c, &d, &e].iter().all(|ev| ev["tab"] == json!(tab) && ev["epoch"] == json!(epoch) && ev["revision"] == ev["seq"]));
    assert_eq!((b["session"]["nav"]["title"].clone(), c["session"]["viewport"].clone(), d["request"].clone(), d["session"]["zoom"].clone()), (json!("App"), json!({"mode": "freeform", "width": 800, "height": 600}), json!({"hard_reload": true}), json!(1.5)));
    // And the list the other mapper asked for held the tab at revision four with the zoom, and the close left none
    assert_eq!((listed["result"]["revision"].clone(), listed["result"]["sessions"][0]["zoom"].clone(), listed["result"]["epoch"].clone()), (json!(4), json!(1.5), json!(epoch)), "{listed}");
    assert_eq!(agent.ask("preview.list", json!({"thread": "t1"})).await["result"]["sessions"], json!([]));
    // And a third mapper that saw the second event resumes with the three it missed, while one of another run is reset
    let mut resumed = chain.agent().await;
    let back = resumed.ask("preview.subscribe", json!({"epoch": epoch, "after_seq": 2})).await;
    assert_eq!((back["result"]["reset"].clone(), back["result"]["seq"].clone()), (json!(false), json!(5)), "{back}");
    assert_eq!([expect(&mut resumed, "preview.resized").await["seq"].clone(), expect(&mut resumed, "preview.resized").await["seq"].clone(), expect(&mut resumed, "preview.closed").await["seq"].clone()], [json!(3), json!(4), json!(5)]);
    let stale = chain.agent().await.ask("preview.subscribe", json!({"epoch": "epoch-of-an-earlier-run", "after_seq": 2})).await;
    assert_eq!(stale["result"]["reset"], json!(true), "{stale}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_changes_to_tabs_that_do_not_exist_or_addresses_that_are_not_pages_when_asked_then_each_is_refused_and_the_stream_is_unchanged() {
    // Given the real chain and a mapper that follows the stream
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let mut watcher = chain.agent().await;
    watcher.ask("preview.subscribe", json!({})).await;
    let mut agent = chain.agent().await;
    // When a tab that was never opened is resized, a page that is a file is opened, and a viewport too large is set
    let missing = agent.ask("preview.resize", json!({"thread": "t1", "tab": "nope", "viewport": {"mode": "fill"}})).await;
    let file = agent.ask("preview.open", json!({"thread": "t1", "url": "file:///etc/passwd"})).await;
    let huge = agent.ask("preview.resize", json!({"thread": "t1", "tab": "nope", "viewport": {"mode": "freeform", "width": 3840, "height": 2161}})).await;
    // Then each is refused with its code, and the next change is the stream's first event
    assert_eq!((missing["error"]["code"].clone(), file["error"]["code"].clone(), huge["error"]["code"].clone()), (json!("PREVIEW_SESSION_NOT_FOUND"), json!("INVALID_PARAMS"), json!("INVALID_PARAMS")));
    agent.ask("preview.open", json!({"thread": "t1"})).await;
    assert_eq!(expect(&mut watcher, "preview.opened").await["seq"], json!(1));
}
