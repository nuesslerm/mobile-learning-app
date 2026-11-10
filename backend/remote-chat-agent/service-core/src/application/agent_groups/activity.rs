//! The `service.activity` sub-group: `service.clientActivity` keeps one lease per client, renewed by each report and gone when its time ends, so
//! the service can ask whether anyone is looking (`ActivityGroup::foreground`) before it does optional work.
use agent_api_proto::{GroupName, Response};
use agent_api_service::activity::{self, ClientActivityParams};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The service's time in milliseconds since the epoch.
pub trait ActivityClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct SystemClock;

impl ActivityClock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
    }
}

impl rca_group_terminal::Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        ActivityClock::now_ms(self)
    }
}

const SERVED: [Served; 1] = [Served::service(activity::CLIENT_ACTIVITY, &[])];

#[derive(Debug, Clone, PartialEq)]
struct Lease {
    params: ClientActivityParams,
    expires_at_ms: u64,
}

pub struct ActivityGroup {
    clock: Arc<dyn ActivityClock>,
    leases: Mutex<HashMap<String, Lease>>,
}

impl ActivityGroup {
    pub fn new(clock: Arc<dyn ActivityClock>) -> ActivityGroup {
        ActivityGroup { clock, leases: Mutex::default() }
    }

    /// The clients whose lease has not ended and that are visible: someone is looking at the service through them.
    pub fn foreground(&self) -> Vec<String> {
        let now = self.clock.now_ms();
        let mut clients: Vec<String> = self.leases.lock().unwrap().iter().filter(|(_, l)| l.expires_at_ms > now && l.params.visible).map(|(c, _)| c.clone()).collect();
        clients.sort();
        clients
    }

    /// The scopes the live leases look at, each once.
    pub fn scopes(&self) -> Vec<String> {
        let now = self.clock.now_ms();
        let mut scopes: Vec<String> = self.leases.lock().unwrap().values().filter(|l| l.expires_at_ms > now).flat_map(|l| l.params.scopes.clone()).collect();
        scopes.sort();
        scopes.dedup();
        scopes
    }
}

#[async_trait]
impl Group for ActivityGroup {
    fn name(&self) -> GroupName {
        activity::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        match method {
            activity::CLIENT_ACTIVITY => match ClientActivityParams::parse(params) {
                Ok(report) => {
                    let now = self.clock.now_ms();
                    let expires_at_ms = now + report.ttl_ms();
                    let mut leases = self.leases.lock().unwrap();
                    leases.retain(|_, l| l.expires_at_ms > now);
                    leases.insert(report.client.clone(), Lease { params: report, expires_at_ms });
                    Response::result(id, activity::result(expires_at_ms))
                }
                Err(e) => e.response(id),
            },
            other => unreachable!("the service.activity group does not serve {}", other.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_agent_core::{AgentApi, Build};
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Clock(AtomicU64);

    impl ActivityClock for Clock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn report(client: &str, visible: bool, extra: Value) -> Value {
        let mut params = json!({"client": client, "kind": "mobile", "visible": visible, "focused": visible, "recently_interacted": visible, "scopes": ["server-config"], "observed_at_ms": 0});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        json!({"id": 1, "method": "service.clientActivity", "params": params})
    }

    async fn send(api: &AgentApi, frame: Value) -> Value {
        api.dispatch_text(Build { version: "v", commit: "c" }, &frame.to_string()).await
    }

    fn setup(now: u64) -> (Arc<Clock>, Arc<ActivityGroup>, AgentApi) {
        let clock = Arc::new(Clock(AtomicU64::new(now)));
        let group = Arc::new(ActivityGroup::new(clock.clone()));
        let api = AgentApi::new(vec![group.clone() as Arc<dyn Group>]).unwrap();
        (clock, group, api)
    }

    #[tokio::test]
    async fn given_a_report_when_the_lease_runs_out_then_the_client_is_no_longer_in_the_foreground_and_a_new_report_renews_it() {
        // Given a phone that reported visible with the default ttl, at time 10_000
        let (clock, group, api) = setup(10_000);
        let answered = send(&api, report("phone", true, json!({}))).await;
        // Then the answer says when the lease ends and the phone is in the foreground
        assert_eq!(answered, json!({"id": 1, "result": {"expires_at_ms": 55_000}}));
        assert_eq!((group.foreground(), group.scopes()), (vec!["phone".to_string()], vec!["server-config".to_string()]));
        // When the time passes the end of the lease
        clock.0.store(55_000, Ordering::SeqCst);
        // Then nobody is in the foreground, and a renewal with a ttl below the minimum lasts the minimum
        assert!(group.foreground().is_empty() && group.scopes().is_empty());
        assert_eq!(send(&api, report("phone", true, json!({"ttl_ms": 5}))).await["result"]["expires_at_ms"], json!(56_000));
        assert_eq!(group.foreground(), ["phone"]);
    }

    #[tokio::test]
    async fn given_two_clients_when_one_reports_hidden_then_only_the_visible_one_is_foreground_and_a_repeat_replaces_its_own_lease() {
        // Given a visible phone and a hidden laptop
        let (_, group, api) = setup(0);
        send(&api, report("phone", true, json!({"scopes": ["thread:a"]}))).await;
        send(&api, report("laptop", false, json!({"scopes": ["thread:b"]}))).await;
        // Then only the phone is foreground, and both clients' scopes are live
        assert_eq!((group.foreground(), group.scopes()), (vec!["phone".to_string()], vec!["thread:a".to_string(), "thread:b".to_string()]));
        // When the phone reports again, hidden, with other scopes, Then it replaces its own lease and keeps the laptop's
        send(&api, report("phone", false, json!({"scopes": ["thread:c"]}))).await;
        assert_eq!((group.foreground(), group.scopes()), (Vec::<String>::new(), vec!["thread:b".to_string(), "thread:c".to_string()]));
    }

    #[tokio::test]
    async fn given_a_report_that_breaks_the_contract_when_sent_then_it_is_invalid_params_and_no_lease_is_kept() {
        let (_, group, api) = setup(0);
        // Given a report with no client, and one with a client over the limit
        let mut no_client = report("x", true, json!({}));
        no_client["params"].as_object_mut().unwrap().remove("client");
        let long = report(&"x".repeat(129), true, json!({}));
        // Then both are INVALID_PARAMS and neither is kept
        assert_eq!(send(&api, no_client).await["error"]["code"], json!("INVALID_PARAMS"));
        assert_eq!(send(&api, long).await["error"]["code"], json!("INVALID_PARAMS"));
        assert!(group.foreground().is_empty());
    }
}
