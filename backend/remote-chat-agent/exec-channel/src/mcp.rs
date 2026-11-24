//! The tool server of a thread's CLI: the two tools only the service can answer (`approval_prompt` and `ask_user`, which wait for the person) and the
//! JSON-RPC methods of a streamable-HTTP server (`initialize`, `tools/list`, `tools/call`). The CLI has its own file and shell tools at the workspace
//! host (owner direction, 18 Nov): none of the executor's actions is a tool any more.
use async_trait::async_trait;
use exec_proto::Code;
use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "2025-03-26";

/// The permission prompt of Claude Code (`--permission-prompt-tool`): asks the person whether a tool call may run.
pub const APPROVAL_PROMPT: &str = "approval_prompt";
/// The tool a model calls to ask the person a question.
pub const ASK_USER: &str = "ask_user";
/// The tools the service answers itself, after the executor's: they reach no executor, they wait for the person's answer
/// (`approval.answer`, `input.answer`, `input.dismiss` of the agent API). Proposed in plan M4, awaiting the owner's confirmation.
pub const SERVICE_TOOLS: [&str; 2] = [APPROVAL_PROMPT, ASK_USER];

/// The service's side of the tools of `SERVICE_TOOLS`, as the thread's tool server asks it.
#[async_trait]
pub trait ServiceTools: Send + Sync {
    /// The MCP `tools/call` result (`{content, isError}`) of `tool` called with `args` by the model of `thread`: it comes when the person has
    /// answered, or when the question can no longer be answered.
    async fn call(&self, thread: &str, tool: &str, args: &Value) -> Value;

    /// Whether `token` is the one issued for a turn of `thread` that still runs: what lets a caller on another host reach the thread's tool server.
    fn admits(&self, _thread: &str, _token: &str) -> bool {
        false
    }
}

fn service_tool_definition(name: &str) -> Option<Value> {
    match name {
        APPROVAL_PROMPT => Some(json!({
            "name": APPROVAL_PROMPT,
            "description": "Asks the person whether a tool call may run, and waits for the answer. Claude Code calls it as its permission prompt tool, the model does not.",
            "inputSchema": {"type": "object", "properties": {"tool_name": {"type": "string", "description": "The tool that wants to run"}, "input": {"type": "object", "description": "What it is called with"}, "tool_use_id": {"type": "string", "description": "The call's id"}}, "required": ["tool_name", "input"]}
        })),
        ASK_USER => Some(json!({
            "name": ASK_USER,
            "description": "Asks the person a question and waits for the answer. Use it when the task cannot go on without a choice only they can make.",
            "inputSchema": {"type": "object", "properties": {"question": {"type": "string", "description": "What to ask"}, "options": {"type": "array", "items": {"type": "string"}, "description": "The answers to offer, when there is a short list"}}, "required": ["question"]}
        })),
        _ => None,
    }
}

/// The argument a service tool cannot go without, when the call lacks it.
fn missing_service_argument<'a>(name: &str, args: &Value) -> Option<&'a str> {
    let required = match name {
        APPROVAL_PROMPT => ["tool_name", "input"].as_slice(),
        _ => ["question"].as_slice(),
    };
    required.iter().copied().find(|r| args.get(*r).is_none_or(Value::is_null))
}

/// The `tools/list` result: the tools the service answers itself.
pub fn tool_list() -> Value {
    json!({ "tools": SERVICE_TOOLS.iter().filter_map(|name| service_tool_definition(name)).collect::<Vec<_>>() })
}

fn text(body: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": body}], "isError": is_error})
}

fn reply(id: &Value, outcome: Result<Value, (i64, String)>) -> Value {
    match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    }
}

/// One JSON-RPC message to the thread's server: the reply, or `None` for a notification (the HTTP answer is then 202 with no body).
/// `service` answers the calls of `SERVICE_TOOLS`.
pub async fn handle(thread: &str, service: Option<&dyn ServiceTools>, message: &Value) -> Option<Value> {
    let id = message.get("id")?;
    let params = &message["params"];
    Some(match message["method"].as_str() {
        Some("initialize") => reply(
            id,
            Ok(json!({"protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION), "capabilities": {"tools": {"listChanged": false}}, "serverInfo": {"name": "rca-executor", "version": "1"}})),
        ),
        Some("ping") => reply(id, Ok(json!({}))),
        Some("tools/list") => reply(id, Ok(tool_list())),
        Some("tools/call") => {
            let name = params["name"].as_str().unwrap_or_default();
            if SERVICE_TOOLS.contains(&name) {
                let args = if params["arguments"].is_null() { json!({}) } else { params["arguments"].clone() };
                let answer = match (service, missing_service_argument(name, &args)) {
                    (_, Some(missing)) => text(format!("{}: {name} needs {missing}", Code::InvalidArguments.as_str()), true),
                    (None, None) => text(format!("{}: this service answers no {name}", Code::InvalidArguments.as_str()), true),
                    (Some(service), None) => service.call(thread, name, &args).await,
                };
                return Some(reply(id, Ok(answer)));
            }
            reply(id, Ok(text(format!("{}: unknown tool {name}", Code::InvalidArguments.as_str()), true)))
        }
        other => reply(id, Err((-32601, format!("method {} is not supported", other.unwrap_or("(none)"))))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": name, "arguments": args}})
    }

    #[test]
    fn the_tool_list_is_the_two_tools_the_service_answers_and_no_file_or_shell_tool() {
        let names: Vec<String> = tool_list()["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, SERVICE_TOOLS);
    }

    #[tokio::test]
    async fn given_a_tool_that_is_not_the_service_s_when_called_then_it_is_refused_as_unknown_and_a_notification_gets_no_reply() {
        // Given a call for a file tool the CLI has of its own
        let refused = handle("t", None, &call("read_file", json!({"path": "a"}))).await.unwrap();
        // Then the model reads an error that names the tool
        assert!(refused["result"]["isError"] == true && refused["result"]["content"][0]["text"].as_str().unwrap().starts_with("INVALID_ARGUMENTS: unknown tool read_file"), "{refused}");
        assert!(handle("t", None, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await.is_none());
    }

    /// A service that answers what it was asked with the thread, the tool and the arguments, and how often it was called.
    struct Echo(std::sync::Mutex<Vec<(String, String, Value)>>);

    #[async_trait]
    impl ServiceTools for Echo {
        async fn call(&self, thread: &str, tool: &str, args: &Value) -> Value {
            self.0.lock().unwrap().push((thread.into(), tool.into(), args.clone()));
            text(format!("{tool} for {thread}"), false)
        }
    }

    #[tokio::test]
    async fn given_the_two_tools_the_service_answers_when_called_then_the_service_gets_the_thread_and_arguments_and_a_call_missing_an_argument_is_refused_first() {
        // Given a service that echoes
        let service = Echo(Default::default());
        // When the permission prompt and the question are called, and each again without what it needs, and with no service at all
        let approval = handle("t9", Some(&service), &call("approval_prompt", json!({"tool_name": "write_file", "input": {"path": "a"}}))).await.unwrap();
        let question = handle("t9", Some(&service), &call("ask_user", json!({"question": "Which?", "options": ["a", "b"]}))).await.unwrap();
        let no_tool = handle("t9", Some(&service), &call("approval_prompt", json!({"input": {}}))).await.unwrap();
        let no_question = handle("t9", Some(&service), &call("ask_user", json!({}))).await.unwrap();
        let unserved = handle("t9", None, &call("ask_user", json!({"question": "x"}))).await.unwrap();
        // Then the service answered the first two, with the thread and the arguments as given
        assert_eq!((approval["result"]["content"][0]["text"].clone(), question["result"]["content"][0]["text"].clone()), (json!("approval_prompt for t9"), json!("ask_user for t9")));
        assert_eq!(service.0.lock().unwrap().iter().map(|(t, n, a)| (t.as_str(), n.as_str(), a["question"].clone())).collect::<Vec<_>>(), [("t9", "approval_prompt", json!(null)), ("t9", "ask_user", json!("Which?"))]);
        // And a call missing its argument, and one with no service, are refused as errors the model reads, without reaching the service
        for (what, refused, says) in [("no tool_name", &no_tool, "approval_prompt needs tool_name"), ("no question", &no_question, "ask_user needs question"), ("no service", &unserved, "this service answers no ask_user")] {
            assert!(refused["result"]["isError"] == true && refused["result"]["content"][0]["text"].as_str().unwrap().contains(says), "{what}: {refused}");
        }
        assert_eq!(service.0.lock().unwrap().len(), 2);
    }
}
