//! The MCP face of the executor channel: the tool list (the contract table), how a `tools/call` becomes a contract request, how the result
//! becomes MCP content, and the JSON-RPC methods of a streamable-HTTP server (`initialize`, `tools/list`, `tools/call`).
use crate::contract_actions;
use crate::remote::{ActionOutput, ExecutorChannel, RemoteError};
use async_trait::async_trait;
use exec_proto::{Action, Code};
use serde_json::{json, Value};
use std::sync::Arc;

const PROTOCOL_VERSION: &str = "2025-03-26";

/// The tools every model gets, in the contract's order. Only the actions a model may take: the others stay behind agent API methods that an
/// app calls (`docs/plans/rca-parity/plan.md`, section 2), and a test asserts this list.
pub const TOOLS: [&str; 10] = [exec_proc::RUN_COMMAND, exec_fs::READ_FILE, exec_fs::WRITE_FILE, exec_fs::EDIT_FILE, exec_fs::LIST_DIR, exec_fs::SEARCH, exec_proc::CANCEL, exec_fs::STAT, exec_fs::READ_CHUNK, exec_fs::CREATE_FILE];

fn tool(name: &str) -> Option<&'static Action> {
    TOOLS.contains(&name).then(|| contract_actions().find(|a| a.name == name)).flatten()
}

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

/// The `tools/list` result: the contract table as MCP tool definitions, then the tools the service answers itself.
pub fn tool_list() -> Value {
    let mut tools: Vec<Value> = TOOLS
        .iter()
        .filter_map(|name| tool(name))
        .map(|action| {
            let props: serde_json::Map<String, Value> = action.properties.iter().map(|p| (p.name.to_string(), json!({"type": p.kind, "description": p.about}))).collect();
            json!({"name": action.name, "description": action.about, "inputSchema": {"type": "object", "properties": props, "required": action.required}})
        })
        .collect();
    tools.extend(SERVICE_TOOLS.iter().filter_map(|name| service_tool_definition(name)));
    json!({ "tools": tools })
}

/// The contract request for a `tools/call`: the tool and its arguments, or why the call is not one (unknown tool, a required argument missing).
pub fn request_for(name: &str, args: &Value) -> Result<(&'static str, Value), RemoteError> {
    let invalid = |message: String| RemoteError::Action { code: Code::InvalidArguments.as_str().into(), message };
    let action = tool(name).ok_or_else(|| invalid(format!("unknown tool {name}")))?;
    let args = if args.is_null() { json!({}) } else { args.clone() };
    if !args.is_object() {
        return Err(invalid("arguments must be an object".into()));
    }
    match action.required.iter().find(|r| args.get(**r).is_none_or(Value::is_null)) {
        Some(missing) => Err(invalid(format!("{} needs {missing}", action.name))),
        None => Ok((action.name, args)),
    }
}

fn text(body: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": body}], "isError": is_error})
}

/// The MCP `tools/call` result for what the executor answered: output as text, a failure (a non-zero exit, a timeout, an error frame) as `isError`.
pub fn content_for(tool: &str, outcome: Result<ActionOutput, RemoteError>) -> Value {
    match outcome {
        Err(e) => text(format!("{}: {}", e.code(), e.message()), true),
        Ok(ActionOutput { result, output }) => match tool {
            exec_proc::RUN_COMMAND => {
                let code = result["exit_code"].as_i64();
                let timed_out = result["timed_out"] == true;
                let mut body = output;
                if result["truncated"] == true {
                    body.push_str("\n[output truncated]");
                }
                if timed_out {
                    body.push_str("\n[timed out]");
                }
                if code != Some(0) {
                    body.push_str(&format!("\n[exit code {}]", code.map_or("unknown".into(), |c| c.to_string())));
                }
                text(body, code != Some(0) || timed_out)
            }
            exec_fs::READ_FILE => text(result["content"].as_str().map_or_else(|| result.to_string(), str::to_string), false),
            exec_fs::WRITE_FILE => text(format!("wrote {} bytes", result["bytes"]), false),
            exec_fs::EDIT_FILE => text(format!("{} replacement(s) made", result["replacements"]), false),
            _ => text(result.to_string(), false),
        },
    }
}

fn reply(id: &Value, outcome: Result<Value, (i64, String)>) -> Value {
    match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    }
}

/// One JSON-RPC message to the thread's server: the reply, or `None` for a notification (the HTTP answer is then 202 with no body).
/// `channel` is the thread's executor, when one is open; `service` answers the calls of `SERVICE_TOOLS`.
pub async fn handle(thread: &str, channel: Option<Arc<dyn ExecutorChannel>>, service: Option<&dyn ServiceTools>, message: &Value) -> Option<Value> {
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
            let outcome = match (request_for(name, &params["arguments"]), channel) {
                (Err(e), _) => Err(e),
                (Ok(_), None) => Err(RemoteError::NoExecutor),
                (Ok((tool, args)), Some(channel)) => channel.call(tool, args).await,
            };
            reply(id, Ok(content_for(name, outcome)))
        }
        other => reply(id, Err((-32601, format!("method {} is not supported", other.unwrap_or("(none)"))))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedChannel;

    #[test]
    fn the_tool_list_is_the_contract_table_in_order() {
        let names: Vec<String> = tool_list()["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["run_command", "read_file", "write_file", "edit_file", "list_dir", "search", "cancel", "stat", "read_chunk", "create_file", "approval_prompt", "ask_user"]);
        // And the executor's tools are the first ten, the two the service answers itself the last
        assert_eq!(names[..TOOLS.len()], TOOLS);
        assert_eq!(names[TOOLS.len()..], SERVICE_TOOLS);
    }

    #[test]
    fn given_the_pty_actions_when_a_model_calls_them_as_tools_then_none_is_listed_and_each_is_refused_as_an_unknown_tool() {
        // Given the actions of the pty family and the tools a model is given
        let names: Vec<String> = tool_list()["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for action in exec_pty::ACTIONS {
            // Then no pty action is a tool, and calling one by name is refused
            assert!(!names.iter().any(|n| n == action.name) && !TOOLS.contains(&action.name), "{} is a tool", action.name);
            assert_eq!(request_for(action.name, &json!({"pty": "p", "data": "rm -rf /"})).unwrap_err().code(), "INVALID_ARGUMENTS", "{}", action.name);
        }
    }

    #[test]
    fn given_the_hosted_actions_when_a_model_calls_them_as_tools_then_the_tools_are_exactly_the_ten_and_each_hosted_action_is_refused_as_an_unknown_tool() {
        // Given the tools a model is given
        let names: Vec<String> = tool_list()["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        // Then they are the ten executor tools and the two service ones, and nothing else
        assert_eq!(TOOLS, ["run_command", "read_file", "write_file", "edit_file", "list_dir", "search", "cancel", "stat", "read_chunk", "create_file"]);
        assert_eq!(names.len(), TOOLS.len() + SERVICE_TOOLS.len());
        for action in exec_hosted::ACTIONS {
            // And no hosted action is a tool, and calling one by name (a model holding the machine's gh sign-in) is refused
            assert!(!names.iter().any(|n| n == action.name) && !TOOLS.contains(&action.name), "{} is a tool", action.name);
            assert_eq!(request_for(action.name, &json!({"provider": "github", "repo": "octo/hello"})).unwrap_err().code(), "INVALID_ARGUMENTS", "{}", action.name);
        }
    }

    #[test]
    fn given_the_preview_actions_when_a_model_calls_them_as_tools_then_none_is_a_tool_and_each_is_refused_as_an_unknown_tool() {
        // Given the tools a model is given
        let names: Vec<String> = tool_list()["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for action in exec_preview::ACTIONS {
            // Then no preview action is a tool (a model could not start a server on the machine), and calling one by name is refused
            assert!(!names.iter().any(|n| n == action.name) && !TOOLS.contains(&action.name), "{} is a tool", action.name);
            assert_eq!(request_for(action.name, &json!({"command": "x", "port": 4000, "id": "p"})).unwrap_err().code(), "INVALID_ARGUMENTS", "{}", action.name);
        }
    }

    #[test]
    fn a_call_names_a_known_tool_and_its_required_arguments() {
        assert_eq!(request_for("write_file", &json!({"path": "a", "content": ""})).unwrap().0, "write_file");
        for (name, args) in [("write_file", json!({"path": "a"})), ("nope", json!({})), ("read_file", json!("a"))] {
            assert_eq!(request_for(name, &args).unwrap_err().code(), "INVALID_ARGUMENTS", "{name}");
        }
    }

    #[test]
    fn command_output_is_concatenated_and_the_exit_code_decides_is_error() {
        let ok = content_for("run_command", Ok(ActionOutput { result: json!({"exit_code": 0, "timed_out": false, "truncated": false}), output: "ab".into() }));
        assert_eq!((ok["content"][0]["text"].clone(), ok["isError"].clone()), (json!("ab"), json!(false)));
        let failed = content_for("run_command", Ok(ActionOutput { result: json!({"exit_code": 2, "timed_out": false}), output: "boom".into() }));
        assert_eq!(failed["isError"], true);
        assert!(failed["content"][0]["text"].as_str().unwrap().ends_with("[exit code 2]"));
        let gone = content_for("run_command", Err(RemoteError::ExecutorGone));
        assert_eq!((gone["isError"].clone(), gone["content"][0]["text"].as_str().unwrap().starts_with("EXECUTOR_GONE")), (json!(true), true));
    }

    #[tokio::test]
    async fn a_tool_call_with_a_channel_is_performed_at_the_executor_and_its_answer_is_what_the_model_reads() {
        // Given the executor of a thread, which reads a file as asked
        let executor = ScriptedChannel::new(|tool, args| match tool {
            "read_file" => Ok(ActionOutput { result: json!({"content": format!("contents of {}", args["path"].as_str().unwrap_or_default())}), output: String::new() }),
            other => Err(RemoteError::Action { code: "UNEXPECTED".into(), message: other.into() }),
        });
        let channel: Arc<dyn ExecutorChannel> = executor.clone();
        // When the model's tools/call for read_file arrives, and one for a tool that is not a model's
        let call = |name: &str| json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": name, "arguments": {"path": "a.txt"}}});
        let read = handle("t", Some(channel.clone()), None, &call("read_file")).await.unwrap();
        let refused = handle("t", Some(channel), None, &call("git_push")).await.unwrap();
        // Then the executor was asked once for that path and the model reads its contents, and the other tool never reached the executor
        assert_eq!((read["id"].clone(), read["result"]["content"][0]["text"].clone(), read["result"]["isError"].clone()), (json!(4), json!("contents of a.txt"), json!(false)));
        assert_eq!(executor.calls(), [("read_file".to_string(), json!({"path": "a.txt"}))]);
        assert!(refused["result"]["isError"] == true && refused["result"]["content"][0]["text"].as_str().unwrap().starts_with("INVALID_ARGUMENTS"));
    }

    #[tokio::test]
    async fn the_model_can_stat_read_a_byte_range_of_and_create_a_file_and_each_needs_its_path() {
        // Given the executor of a thread that answers each of the three as the contract says
        let executor = ScriptedChannel::new(|tool, _| match tool {
            "stat" => Ok(ActionOutput { result: json!({"kind": "file", "size": 5, "modified_ms": 1}), output: String::new() }),
            "read_chunk" => Ok(ActionOutput { result: json!({"data": "aGVsbG8=", "offset": 0, "bytes": 5, "total_bytes": 5, "eof": true}), output: String::new() }),
            "create_file" => Err(RemoteError::Action { code: "ALREADY_EXISTS".into(), message: "a.txt already exists".into() }),
            other => Err(RemoteError::Action { code: "UNEXPECTED".into(), message: other.into() }),
        });
        let channel: Arc<dyn ExecutorChannel> = executor.clone();
        let call = |name: &str, args: Value| json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}});
        // When the model calls each with a path, and each without one
        let stat = handle("t", Some(channel.clone()), None, &call("stat", json!({"path": "a.txt"}))).await.unwrap();
        let chunk = handle("t", Some(channel.clone()), None, &call("read_chunk", json!({"path": "a.txt", "length": 5}))).await.unwrap();
        let exists = handle("t", Some(channel.clone()), None, &call("create_file", json!({"path": "a.txt", "content": "x"}))).await.unwrap();
        let mut pathless = Vec::new();
        for name in ["stat", "read_chunk", "create_file"] {
            pathless.push(handle("t", Some(channel.clone()), None, &call(name, json!({}))).await.unwrap());
        }
        // Then the executor was asked for the three with their arguments, the model reads the executor's answers (a refusal as an error with its code), and a call with no path never reached it
        assert_eq!(executor.calls().iter().map(|(tool, _)| tool.as_str()).collect::<Vec<_>>(), ["stat", "read_chunk", "create_file"]);
        assert_eq!(executor.calls()[1].1, json!({"path": "a.txt", "length": 5}));
        assert_eq!((stat["result"]["isError"].clone(), stat["result"]["content"][0]["text"].as_str().unwrap().contains("\"size\":5")), (json!(false), true));
        assert!(chunk["result"]["content"][0]["text"].as_str().unwrap().contains("aGVsbG8="));
        assert!(exists["result"]["isError"] == true && exists["result"]["content"][0]["text"].as_str().unwrap().starts_with("ALREADY_EXISTS"));
        for answer in pathless {
            assert!(answer["result"]["isError"] == true && answer["result"]["content"][0]["text"].as_str().unwrap().starts_with("INVALID_ARGUMENTS"));
        }
    }

    #[tokio::test]
    async fn a_tool_call_without_a_channel_says_no_executor_and_a_notification_gets_no_reply() {
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "read_file", "arguments": {"path": "a"}}});
        let answer = handle("t", None, None, &call).await.unwrap();
        assert_eq!((answer["result"]["isError"].clone(), answer["result"]["content"][0]["text"].as_str().unwrap().starts_with("NO_EXECUTOR")), (json!(true), true));
        assert!(handle("t", None, None, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await.is_none());
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
    async fn given_the_two_tools_the_service_answers_when_called_then_the_service_gets_the_thread_and_arguments_and_no_executor_is_asked_and_a_call_missing_an_argument_is_refused_first() {
        // Given an executor that fails the test if asked, and a service that echoes
        let executor = ScriptedChannel::new(|tool, _| Err(RemoteError::Action { code: "UNEXPECTED".into(), message: tool.into() }));
        let channel: Arc<dyn ExecutorChannel> = executor.clone();
        let service = Echo(Default::default());
        let call = |name: &str, args: Value| json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": name, "arguments": args}});
        // When the permission prompt and the question are called, and each again without what it needs, and with no service at all
        let approval = handle("t9", Some(channel.clone()), Some(&service), &call("approval_prompt", json!({"tool_name": "write_file", "input": {"path": "a"}}))).await.unwrap();
        let question = handle("t9", Some(channel.clone()), Some(&service), &call("ask_user", json!({"question": "Which?", "options": ["a", "b"]}))).await.unwrap();
        let no_tool = handle("t9", Some(channel.clone()), Some(&service), &call("approval_prompt", json!({"input": {}}))).await.unwrap();
        let no_question = handle("t9", Some(channel.clone()), Some(&service), &call("ask_user", json!({}))).await.unwrap();
        let unserved = handle("t9", Some(channel), None, &call("ask_user", json!({"question": "x"}))).await.unwrap();
        // Then the service answered the first two, with the thread and the arguments as given, and the executor was never asked
        assert_eq!((approval["result"]["content"][0]["text"].clone(), question["result"]["content"][0]["text"].clone()), (json!("approval_prompt for t9"), json!("ask_user for t9")));
        assert_eq!(service.0.lock().unwrap().iter().map(|(t, n, a)| (t.as_str(), n.as_str(), a["question"].clone())).collect::<Vec<_>>(), [("t9", "approval_prompt", json!(null)), ("t9", "ask_user", json!("Which?"))]);
        assert!(executor.calls().is_empty());
        // And a call missing its argument, and one with no service, are refused as errors the model reads, without reaching the service
        for (what, refused, says) in [("no tool_name", &no_tool, "approval_prompt needs tool_name"), ("no question", &no_question, "ask_user needs question"), ("no service", &unserved, "this service answers no ask_user")] {
            assert!(refused["result"]["isError"] == true && refused["result"]["content"][0]["text"].as_str().unwrap().contains(says), "{what}: {refused}");
        }
        assert_eq!(service.0.lock().unwrap().len(), 2);
    }
}
