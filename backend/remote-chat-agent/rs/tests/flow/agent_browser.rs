//! The `browser` group end to end: a real headless Chromium started by the real `rca-exec`, loading `fake-devserver` (a real process on a real port),
//! asked through the service on a mapper's socket. The group is off unless the service is started with the browser on, and every recorded exchange
//! of its methods is replayed against the real executor.
use crate::agent_threads::{chain, chain_browser};
use crate::harness::Env;
use exec_testkit::devserver::{command, free_port};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_service_started_without_the_browser_when_a_mapper_asks_for_a_page_then_every_method_is_refused_and_no_browser_starts() {
    // Given the real chain with the browser off (the default)
    let env = Env::new(&[]);
    let chain = chain(&env).await;
    let mut agent = chain.agent().await;
    // When the mapper opens a page, lists pages and evaluates in one
    let opened = agent.ask("browser.open", json!({"workspace": "w1", "url": "https://example.com/"})).await;
    let pages = agent.ask("browser.pages", json!({"workspace": "w1"})).await;
    let eval = agent.ask("browser.eval", json!({"workspace": "w1", "expression": "1+1"})).await;
    // Then each is BROWSER_DISABLED with the way to turn it on
    for answer in [opened, pages, eval] {
        assert_eq!(answer["error"]["code"], json!("BROWSER_DISABLED"), "{answer}");
        assert!(answer["error"]["message"].as_str().unwrap().contains("RCA_BROWSER=1"), "{answer}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_the_browser_methods_recorded_in_the_contract_when_replayed_on_the_real_executor_then_each_response_is_what_was_recorded() {
    for fixture in agent_api_browser::testing::fixtures().iter().filter(|f| agent_api_browser::METHODS.iter().any(|m| m.name() == f.method)) {
        for exchange in &fixture.exchanges {
            // When an exchange's request is made on a service with the browser on and an executor of their own
            let env = Env::new(&[]);
            let chain = chain_browser(&env, "").await;
            let mut agent = chain.agent().await;
            let mut answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
            answer["id"] = exchange.response["id"].clone();
            // Then the answer is the recorded one
            assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_dev_server_when_a_thread_opens_a_page_through_the_service_then_the_executors_chromium_loads_it_and_the_thread_drives_and_closes_it() {
    // Given the real chain with the browser on, whose executor may reach one dev server, and that server started through the service
    let port = free_port();
    let env = Env::new(&[]);
    let chain = chain_browser(&env, &format!("127.0.0.1:{port}")).await;
    let mut agent = chain.agent().await;
    let started = agent.ask("preview.start", json!({"workspace": "w1", "command": command(""), "port": port, "ready_timeout_ms": 10000})).await;
    assert!(started.get("result").is_some(), "{started}");
    let url = format!("http://127.0.0.1:{port}/");
    // When thread t1 opens a page on it and reads the page
    let opened = agent.ask("browser.open", json!({"workspace": "w1", "thread": "t1", "url": url})).await;
    let body = agent.ask("browser.eval", json!({"workspace": "w1", "thread": "t1", "expression": "document.body.innerText"})).await;
    // Then the page shows the server's address and what it sent, under the thread's key
    assert_eq!((opened["result"]["key"].clone(), opened["result"]["url"].clone()), (json!("t1"), json!(url)), "{opened}");
    assert!(body["result"]["value"].as_str().unwrap().contains(&format!("\"port_env\":\"{port}\"")), "{body}");
    // And the thread moves the page, goes back, drives the pointer, and another thread sees none of it
    let second = agent.ask("browser.goto", json!({"workspace": "w1", "thread": "t1", "url": format!("{url}next")})).await;
    let back = agent.ask("browser.history", json!({"workspace": "w1", "thread": "t1", "direction": "back"})).await;
    let moved = agent.ask("browser.input", json!({"workspace": "w1", "thread": "t1", "kind": "mouse_move", "x": 12, "y": 34})).await;
    let other = agent.ask("browser.eval", json!({"workspace": "w1", "thread": "t2", "expression": "1"})).await;
    assert_eq!((second["result"]["url"].clone(), back["result"]["url"].clone(), moved["result"].clone()), (json!(format!("{url}next")), json!(url), json!({"x": 12.0, "y": 34.0, "buttons": 0})), "{second} {back} {moved}");
    assert_eq!((other["error"]["code"].clone(), other["error"]["message"].clone()), (json!("NOT_FOUND"), json!("no page is open for key t2")));
    // And an address the executor does not allow is refused before the browser is asked, the page list names the thread's page, and closing it empties the list
    let refused = agent.ask("browser.goto", json!({"workspace": "w1", "thread": "t1", "url": "http://169.254.169.254/latest/meta-data/"})).await;
    assert_eq!(refused["error"]["code"], json!("INVALID_ARGS"), "{refused}");
    let listed = agent.ask("browser.pages", json!({"workspace": "w1"})).await;
    assert_eq!((listed["result"]["pages"][0]["key"].clone(), listed["result"]["max_pages"].clone()), (json!("t1"), json!(3)), "{listed}");
    assert_eq!(agent.ask("browser.close", json!({"workspace": "w1", "thread": "t1"})).await["result"], json!({"closed": true}));
    assert_eq!(agent.ask("browser.pages", json!({"workspace": "w1"})).await["result"]["pages"], json!([]));
    // And the dev server, which outlives the executor that started it, is stopped
    let stopped = agent.ask("preview.stop", json!({"workspace": "w1", "id": started["result"]["id"]})).await;
    assert_eq!(stopped["result"], json!({"stopped": true}));
}
