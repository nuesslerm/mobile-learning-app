//! The start-up cases of plan section 3 ("Start-up"): what the service says when it starts and whom it asks, that its
//! first Engine request reaches the case's fake Engine (so a missing `DOCKER_SOCKET` seam fails here instead of reaching a
//! real daemon), and that nothing runs in a container on its own at start.
use crate::{engine_calls, exec_creates, get, with_engine, world, AGENT_SHELL};
use serde_json::{json, Value};
use std::time::Duration;
use vm_ops_api_integration::{case, Exec, Service, HOST_LABEL};

const LEETCODE_BACKEND: &str =
    r#"[{"Id":"l1","Names":["/leetcode-backend"],"State":"running","Status":"Up 2 hours","Image":"ns/leetcode-backend:sha-1"}]"#;

fn port(svc: &Service) -> u16 {
    svc.base().rsplit(':').next().and_then(|p| p.parse().ok()).expect("a base URL with a port")
}

/// The argv of every call of the `wf` double so far.
fn wf_calls(svc: &Service) -> Vec<Vec<String>> {
    let argv = |c: &Value| c["argv"].as_array().map(|a| a.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect());
    svc.world().calls().iter().filter(|c| c["tool"] == "wf").filter_map(argv).collect()
}

case!(start_up_says_it_listens_and_beats_and_asks_the_vault_for_nothing, {
    // Given the service in its world, with nothing in the vault to ask for
    let svc = world().start();
    let port = port(&svc);
    // Then stdout has vm-ops-api.listening with its port and host
    let listening = json!({ "level": "info", "message": "vm-ops-api.listening", "port": port, "host": HOST_LABEL });
    svc.wait_line(&listening.to_string(), Duration::from_secs(5));
    // And health.heartbeat for its host, with its uptime and memory in whole numbers
    let beat = svc.wait_event("health.heartbeat", Duration::from_secs(5));
    let keys: Vec<&str> = beat.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
    assert_eq!(keys, ["level", "message", "host", "uptimeSec", "rssMb", "ok"], "{beat}");
    assert_eq!((&beat["level"], &beat["host"], &beat["ok"]), (&json!("info"), &json!(HOST_LABEL), &json!(true)), "{beat}");
    assert!(beat["uptimeSec"].is_u64() && beat["rssMb"].is_u64(), "{beat}");
    // And the wf double was never asked for a secret: the service reads only its own environment
    let asked: Vec<Vec<String>> = wf_calls(&svc).into_iter().filter(|argv| argv.first().is_some_and(|a| a == "secret")).collect();
    assert_eq!(asked, Vec::<Vec<String>>::new());
});

case!(the_first_ps_request_reaches_the_fake_engines_socket, {
    // Given the fake Engine on the case's DOCKER_SOCKET, listing leetcode-backend
    let svc = with_engine(LEETCODE_BACKEND).start();
    // When the first ps request is made
    let r = get(&svc, "/services/leetcode-backend/ps");
    // Then it is answered from the fake Engine's list, and the Engine recorded that list request: the only one so far
    assert_eq!((r.status, &r.json()["containers"][0]["name"]), (200, &json!("leetcode-backend")), "{}", r.body);
    assert_eq!(engine_calls(&svc), ["GET /containers/json?all=true"]);
});

case!(nothing_runs_in_agent_shell_in_the_first_10_s, {
    // Given agent-shell and leetcode-backend running in the fake Engine, which would answer any exec a timer ran
    let mut list: Vec<Value> = serde_json::from_str(AGENT_SHELL).expect("JSON");
    list.extend(serde_json::from_str::<Vec<Value>>(LEETCODE_BACKEND).expect("JSON"));
    let w = with_engine(&Value::Array(list).to_string());
    w.fake_engine().exec(Exec::prefix(&["sh", "-c"], "").answer("", 0));
    let svc = w.start();
    // When the service has run for 10 s with no request
    std::thread::sleep(Duration::from_secs(10));
    // Then no exec reached any container
    assert_eq!(exec_creates(&svc), Vec::<Value>::new(), "{:?}", engine_calls(&svc));
    // When a ps request is made, Then it is answered from this fake Engine, whose only request so far is that list: the
    // service talks to it, and asked it nothing in the first 10 s
    let r = get(&svc, "/services/leetcode-backend/ps");
    assert_eq!((r.status, &r.json()["containers"][0]["name"]), (200, &json!("leetcode-backend")), "{}", r.body);
    assert_eq!(engine_calls(&svc), ["GET /containers/json?all=true"]);
});
