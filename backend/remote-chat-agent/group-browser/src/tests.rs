//! The relay over doubles of the executors' channels.
use super::*;
use agent_api_proto::Method;
use rca_agent_core::{AgentApi, Build};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::Registry;
use serde_json::json;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

fn ok(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

async fn ask(api: &AgentApi, method: Method, params: Value) -> Value {
    api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
}

fn rig(channel: Arc<dyn ExecutorChannel>, enabled: bool) -> AgentApi {
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", channel).unwrap();
    AgentApi::new(vec![Arc::new(BrowserGroup::new(executors, enabled)) as Arc<dyn Group>]).unwrap()
}

#[test]
fn given_the_served_rows_when_compared_with_the_contract_then_each_method_has_one_in_order_naming_the_action_of_its_spec() {
    let rows: Vec<(Method, Vec<&str>)> = SERVED.iter().map(|s| (s.method, s.actions.to_vec())).collect();
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), api::METHODS);
    for spec in &api::SPECS {
        assert_eq!(rows.iter().find(|r| r.0 == spec.method).unwrap().1, [spec.action]);
    }
}

#[tokio::test]
async fn given_a_service_started_without_the_browser_when_any_method_is_called_then_it_is_refused_and_no_executor_is_asked() {
    // Given an executor that would answer anything, behind a service with the browser off
    let channel = ScriptedChannel::new(|tool, _| ok(json!({"action": tool})));
    let api = rig(channel.clone(), false);
    for spec in &api::SPECS {
        // When each method is called
        let answer = ask(&api, spec.method, json!({"workspace": "root", "key": "k", "url": "https://example.com", "direction": "back", "expression": "1", "kind": "mouse_down"})).await;
        // Then it is BROWSER_DISABLED naming how to turn the browser on
        assert_eq!((answer["error"]["code"].clone(), answer["error"]["message"].clone()), (json!("BROWSER_DISABLED"), json!(message::DISABLED)), "{}", spec.method.name());
    }
    assert!(channel.calls().is_empty());
}

#[tokio::test]
async fn given_every_method_when_called_then_the_executor_is_asked_its_action_with_the_forwarded_params_and_the_pages_key_and_its_answer_comes_back_as_it_came() {
    // Given an executor that answers each action with a marker naming it
    let channel = ScriptedChannel::new(|tool, _| ok(json!({"action": tool})));
    let api = rig(channel.clone(), true);
    for spec in &api::SPECS {
        // When the method is called with every key it forwards, a thread and one param it does not forward
        let mut params = json!({"workspace": "root", "thread": "t9", "unforwarded": 1});
        for key in spec.keys {
            params[*key] = json!("v");
        }
        params.as_object_mut().unwrap().remove("key");
        let answer = ask(&api, spec.method, params).await;
        // Then the answer is the executor's, and the executor saw the keys of the spec, the page's key being the thread's, and nothing else
        assert_eq!(answer["result"], json!({"action": spec.action}), "{}", spec.method.name());
        let (tool, args) = channel.calls().pop().unwrap();
        assert_eq!(tool, spec.action);
        assert!(args.get("unforwarded").is_none() && args.get("workspace").is_none() && args.get("thread").is_none(), "{args}");
        assert_eq!(args.get("key"), spec.keys.contains(&"key").then(|| json!("t9")).as_ref(), "{}: {args}", spec.method.name());
        assert_eq!(args.as_object().unwrap().len(), spec.keys.len(), "{args}");
    }
}

#[tokio::test]
async fn given_a_refusal_a_missing_workspace_and_a_detached_one_when_asked_then_each_answer_keeps_its_code() {
    // Given an executor that refuses with the code of an address it does not allow
    let channel = ScriptedChannel::new(|_, _| Err(RemoteError::Action { code: "INVALID_ARGS".into(), message: "`url` points at 10.0.0.1:80, an address the executor does not let a page reach".into() }));
    let api = rig(channel.clone(), true);
    // When a page is opened, one call has no workspace, and one names a workspace nobody attached
    let refused = ask(&api, api::OPEN, json!({"workspace": "root", "url": "http://10.0.0.1/"})).await;
    let none = ask(&api, api::OPEN, json!({"url": "https://example.com"})).await;
    let detached = ask(&api, api::PAGES, json!({"workspace": "other"})).await;
    // Then the refusal is the executor's, the others are INVALID_PARAMS and NO_EXECUTOR, and only the first reached the executor
    assert_eq!((refused["error"]["code"].clone(), refused["error"]["message"].clone()), (json!("INVALID_ARGS"), json!("`url` points at 10.0.0.1:80, an address the executor does not let a page reach")));
    assert_eq!((none["error"]["code"].clone(), none["error"]["message"].clone()), (json!("INVALID_PARAMS"), json!("workspace is required")));
    assert_eq!(detached["error"]["code"], json!("NO_EXECUTOR"));
    assert_eq!(channel.calls().len(), 1);
}
