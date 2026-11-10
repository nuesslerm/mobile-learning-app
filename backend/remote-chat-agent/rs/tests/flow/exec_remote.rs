//! `/exec/v2` and `/mcp`, and the turns that use them: the executor channel, keyed by workspace. The service listens on a real loopback port; a
//! WebSocket client plays `rca-exec` (it performs the actions in its own temporary directory); a mapper starts a turn with `turn.start` on the agent
//! socket and follows the thread's events; a scripted program the Spawner starts plays the CLI and speaks MCP over HTTP to `/mcp/<thread>`.
//! The retired routes (`/exec/v1/*`, `POST /exec/v2/turns`) answer 404, and the turns they ran are `agent.threads` turns now.
use crate::harness::{call, get, post, service, service_spawning, Env, Options, Service, SECRET};
use crate::mapper::Agent;
use agent_api_proto::Event;
use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use futures_util::future::BoxFuture;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use rca_cli_driver::application::ports::{Running, Spawner};
use rca_cli_driver::testing::CliScript;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The CLI's view of its MCP server.
#[derive(Clone)]
struct Mcp {
    url: String,
}

impl Mcp {
    async fn rpc(&self, method: &str, params: Value) -> Value {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        reqwest::Client::new().post(&self.url).json(&body).send().await.expect("the MCP server answers").json::<Value>().await.expect("a JSON-RPC reply")["result"].clone()
    }

    async fn call(&self, tool: &str, args: Value) -> Value {
        self.rpc("tools/call", json!({"name": tool, "arguments": args})).await
    }

    async fn call_owned(self, tool: &'static str, args: Value) -> Value {
        self.call(tool, args).await
    }
}

type Script = Arc<dyn Fn(Mcp, mpsc::Sender<String>) -> BoxFuture<'static, ()> + Send + Sync>;

fn script<F, Fut>(f: F) -> Script
where
    F: Fn(Mcp, mpsc::Sender<String>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    Arc::new(move |mcp, out| Box::pin(f(mcp, out)))
}

/// A CLI that is a program of the flow's own: it finds its MCP server in the `--mcp-config` it was started with, as `claude` does.
struct ScriptedCli {
    script: Script,
    spawned: Mutex<Vec<(String, Vec<String>)>>,
}

#[async_trait]
impl Spawner for ScriptedCli {
    async fn spawn(&self, program: &str, args: &[String], _cwd: &Path) -> Result<Running, String> {
        self.spawned.lock().unwrap().push((program.to_string(), args.to_vec()));
        let config = args.iter().position(|a| a == "--mcp-config").and_then(|at| args.get(at + 1)).ok_or("started without an MCP config")?;
        let config: Value = serde_json::from_str(config).map_err(|e| e.to_string())?;
        let mcp = Mcp { url: config["mcpServers"]["executor"]["url"].as_str().ok_or("no executor server in the config")?.to_string() };
        let (lines, rx) = mpsc::channel(16);
        let task = tokio::spawn((self.script)(mcp, lines));
        let stop = task.abort_handle();
        Ok(Running {
            lines: Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)),
            finish: Box::pin(async move {
                let _ = task.await;
                Ok(())
            }),
            kill: Arc::new(move || stop.abort()),
        })
    }
}

fn tool_use(id: &str, name: &str, input: Value) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}}).to_string()
}

fn tool_result(id: &str, result: &Value) -> String {
    json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": id, "content": result["content"][0]["text"], "is_error": result["isError"]}]}}).to_string()
}

fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

fn cli(f: Script) -> Arc<ScriptedCli> {
    Arc::new(ScriptedCli { script: f, spawned: Mutex::new(Vec::new()) })
}

type Sink = Arc<tokio::sync::Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>;

/// `rca-exec`: a channel client that performs the actions in its own directory (or, when told to hold, never answers a `run_command`).
struct Executor {
    dir: tempfile::TempDir,
    requests: mpsc::UnboundedReceiver<Value>,
    sink: Sink,
    close_code: Arc<Mutex<Option<u16>>>,
}

async fn open_channel(port: u16, workspace: &str, secret: Option<&str>) -> Result<(SplitSink<WebSocketStream<TcpStream>, Message>, futures_util::stream::SplitStream<WebSocketStream<TcpStream>>), (u16, String)> {
    let mut request = format!("ws://127.0.0.1:{port}/exec/v2/executors?workspace={workspace}").into_client_request().unwrap();
    if let Some(secret) = secret {
        request.headers_mut().insert("x-bot-secret", secret.parse().unwrap());
    }
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("the service is listening");
    match tokio_tungstenite::client_async(request, tcp).await {
        Ok((stream, _)) => Ok(stream.split()),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err((response.status().as_u16(), String::new())),
        Err(e) => panic!("the handshake failed: {e}"),
    }
}

impl Executor {
    async fn connect(port: u16, workspace: &str, hold: bool) -> Executor {
        let (write, mut read) = open_channel(port, workspace, Some(SECRET)).await.expect("the channel opens");
        let dir = tempfile::tempdir().expect("an executor directory");
        let sink: Sink = Arc::new(tokio::sync::Mutex::new(write));
        let (seen, requests) = mpsc::unbounded_channel();
        let close_code = Arc::new(Mutex::new(None));
        let (root, reply, code) = (dir.path().to_path_buf(), Arc::clone(&sink), Arc::clone(&close_code));
        tokio::spawn(async move {
            while let Some(Ok(message)) = read.next().await {
                match message {
                    Message::Close(frame) => {
                        *code.lock().unwrap() = frame.map(|f| u16::from(f.code));
                        break;
                    }
                    Message::Text(text) => {
                        let request: Value = serde_json::from_str(&text).expect("a JSON request");
                        let _ = seen.send(request.clone());
                        let (id, args) = (request["id"].clone(), &request["args"]);
                        let frames = match request["tool"].as_str() {
                            Some("write_file") => {
                                let target = root.join(args["path"].as_str().unwrap());
                                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                                std::fs::write(&target, args["content"].as_str().unwrap()).unwrap();
                                vec![json!({"id": id, "ok": true, "result": {"bytes": args["content"].as_str().unwrap().len()}})]
                            }
                            Some("run_command") if hold => vec![],
                            Some("run_command") => {
                                let out = std::process::Command::new("sh").arg("-c").arg(args["command"].as_str().unwrap()).current_dir(&root).output().unwrap();
                                vec![
                                    json!({"id": id, "stream": "stdout", "data": String::from_utf8_lossy(&out.stdout)}),
                                    json!({"id": id, "ok": true, "result": {"exit_code": out.status.code(), "timed_out": false, "truncated": false}}),
                                ]
                            }
                            _ => vec![json!({"id": id, "ok": false, "error": {"code": "UNSUPPORTED", "message": "not in this executor"}})],
                        };
                        for frame in frames {
                            let _ = reply.lock().await.send(Message::Text(frame.to_string().into())).await;
                        }
                    }
                    _ => {}
                }
            }
        });
        Executor { dir, requests, sink, close_code }
    }

    async fn request(&mut self, tool: &str) -> Value {
        loop {
            let request = tokio::time::timeout(Duration::from_secs(5), self.requests.recv()).await.expect("a request arrives").expect("the channel is open");
            if request["tool"] == tool {
                return request;
            }
        }
    }

    async fn hang_up(&self) {
        let _ = self.sink.lock().await.close().await;
    }

    async fn closed_with(&self) -> Option<u16> {
        for _ in 0..100 {
            if let Some(code) = *self.close_code.lock().unwrap() {
                return Some(code);
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        None
    }
}

fn turn(thread: &str, workspace: &str, driver: &str) -> Value {
    json!({"thread": thread, "workspace": workspace, "driver": driver, "model": "m1", "prompt": "make a note"})
}

/// A mapper of its own: a bearer minted with the service secret, and the agent socket it opens.
async fn mapper(svc: &Service, port: u16) -> Agent {
    Agent::open(port, &svc.mint("flow mapper").await).await
}

/// A thread of `workspace` that a mapper follows, ready for a turn.
async fn followed(agent: &mut Agent, thread: &str, workspace: &str, driver: &str) {
    let made = agent.ask("thread.create", json!({"thread": thread, "workspace": workspace, "driver": driver})).await;
    assert!(made.get("result").is_some(), "{made}");
    let followed = agent.ask("thread.subscribe", json!({"thread": thread})).await;
    assert!(followed.get("result").is_some(), "{followed}");
}

fn kinds(events: &[Event]) -> Vec<&str> {
    events.iter().map(|e| e.kind.as_str()).collect()
}

/// A CLI that stays up until the test ends, holding its stream open.
fn idle_cli() -> Arc<ScriptedCli> {
    cli(script(|_mcp, out| async move {
        let _open = out;
        tokio::time::sleep(Duration::from_secs(30)).await;
    }))
}

/// Starts a turn of `thread` on workspace `w1` and returns once its CLI runs, so `/mcp/<thread>` resolves to the workspace's executor.
async fn running_turn(svc: &Service, port: u16, cli: &Arc<ScriptedCli>, thread: &str) -> Agent {
    let mut agent = mapper(svc, port).await;
    followed(&mut agent, thread, "w1", "claudeAgent").await;
    let started = agent.ask("turn.start", turn(thread, "w1", "claudeAgent")).await;
    assert!(started.get("result").is_some(), "{started}");
    for _ in 0..200 {
        if !cli.spawned.lock().unwrap().is_empty() {
            return agent;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the turn's CLI never started");
}

fn text_of(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

async fn listening(env: &Env, cli: Arc<ScriptedCli>) -> (Service, u16) {
    let svc = service_spawning(env, Options::default(), cli);
    let port = svc.listen().await;
    (svc, port)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_the_retired_routes_when_called_with_the_service_secret_then_each_is_404_and_no_cli_ran_while_a_live_route_answers_the_same_caller() {
    // Given a service with a secret
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    // When every route of /exec/v1 and the turn routes of /exec/v2 are called with the secret, as the caller that used them did
    let routes = [
        (Method::GET, "/exec/v1/catalog"),
        (Method::POST, "/exec/v1/turns"),
        (Method::POST, "/exec/v1/turns/t1/interrupt"),
        (Method::GET, "/exec/v1/fs/list"),
        (Method::GET, "/exec/v1/fs/read"),
        (Method::PUT, "/exec/v1/fs/write"),
        (Method::POST, "/exec/v2/turns"),
        (Method::POST, "/exec/v2/turns/t1/interrupt"),
    ];
    for (method, path) in routes {
        let answer = svc.send(call(method, path).secret().json(json!({"threadId": "t1", "thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "p", "cwd": "/", "root": "/", "path": "x", "contents": "x"}))).await;
        // Then each is the router's own 404, and not a refusal of the caller
        assert_eq!((answer.status, answer.json()), (StatusCode::NOT_FOUND, json!({"error": "not_found"})), "{path}");
    }
    // And no CLI started and nothing was written, while a route that is served answers the same secret
    assert!(svc.spawner.spawned().is_empty());
    assert_eq!(svc.send(get("/agent/v1/bearers").secret()).await.status, StatusCode::OK);
    assert_eq!(svc.send(post("/agent/v1/bearers").secret().json(json!({"label": "later"}))).await.status, StatusCode::CREATED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_executor_channel_when_the_cli_writes_a_file_and_runs_a_command_then_they_happen_in_the_executors_directory_and_not_in_rcas() {
    // Given a service whose CLI writes a note and then reads it back with a command that fails, an executor connected for the workspace, and a mapper following a thread of it
    let env = Env::new(&[]);
    let cli = cli(script(|mcp, out| async move {
        let args = json!({"path": "notes/a.txt", "content": "hello from the model", "create_dirs": true});
        out.send(tool_use("k1", "mcp__executor__write_file", args.clone())).await.unwrap();
        out.send(tool_result("k1", &mcp.call("write_file", args).await)).await.unwrap();
        out.send(tool_use("k2", "mcp__executor__run_command", json!({"command": "cat notes/a.txt; exit 3"}))).await.unwrap();
        out.send(tool_result("k2", &mcp.call("run_command", json!({"command": "cat notes/a.txt; exit 3"})).await)).await.unwrap();
        out.send(said("done")).await.unwrap();
    }));
    let (svc, port) = listening(&env, cli.clone()).await;
    let executor = Executor::connect(port, "w1", false).await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    // When a turn runs on the workspace
    let started = agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    let events = agent.events_through("turn.finished").await;
    // Then the file is in the executor's directory and rca's workspace has none of it
    assert!(started.get("result").is_some(), "{started}");
    assert_eq!(std::fs::read_to_string(executor.dir.path().join("notes/a.txt")).unwrap(), "hello from the model");
    assert!(!env.home.join("notes").exists() && !Path::new(&svc.default_cwd).join("notes/a.txt").exists());
    // And the events are each tool's start, output and finish (and the file the write changed), then the text and the finish, the command's output concatenated with its exit code and its call a failure
    assert_eq!(kinds(&events), ["turn.started", "message.delta", "tool.started", "tool.output", "tool.finished", "file.changed", "tool.started", "tool.output", "tool.finished", "message.delta", "turn.finished"]);
    assert_eq!((events[3].data["data"].clone(), events[4].data["ok"].clone(), events[5].data["path"].clone()), (json!("wrote 20 bytes"), json!(true), json!("notes/a.txt")));
    assert_eq!((events[7].data["data"].clone(), events[8].data["ok"].clone()), (json!("hello from the model\n[exit code 3]"), json!(false)));
    assert_eq!(events[10].data["reason"], json!("completed"));
    // And the CLI was started with the built-in tools off and only the thread's MCP server
    let (program, args) = cli.spawned.lock().unwrap()[0].clone();
    assert_eq!(program, "claude");
    let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert!(after("--mcp-config").contains(&format!("http://127.0.0.1:{port}/mcp/t1")) && after("--disallowedTools").split(',').all(|t| !t.is_empty()) && after("--disallowedTools").contains("Bash"));
    assert!(args.contains(&"--strict-mcp-config".to_string()) && args.contains(&"make a note".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_clis_that_print_their_streams_when_turns_run_then_each_becomes_the_same_neutral_events_in_the_thread_log() {
    // Given a service with an executor for the workspace and two scripted CLIs: Claude Code printing a tool call and its answer, opencode printing text
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let _executor = Executor::connect(port, "w1", false).await;
    svc.spawner.script(
        "claude",
        CliScript {
            lines: [
                r#"{"type":"system","subtype":"init"}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"k1","name":"Bash","input":{"command":"ls"}}]}}"#,
                r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"k1","content":"a.txt","is_error":false}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done: a.txt"}]}}"#,
                r#"{"type":"result","is_error":false,"result":"Done: a.txt"}"#,
            ]
            .map(String::from)
            .to_vec(),
            ..CliScript::default()
        },
    );
    // opencode runs as `env OPENCODE_CONFIG_CONTENT=... opencode ...`, so the double is keyed by `env`
    svc.spawner.script("env", CliScript { lines: vec![r#"{"type":"text","part":{"text":"from opencode"}}"#.into()], ..CliScript::default() });
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    followed(&mut agent, "t2", "w1", "opencode").await;
    // When a turn runs on each
    agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    let claude = agent.events_through("turn.finished").await;
    agent.ask("turn.start", turn("t2", "w1", "opencode")).await;
    let opencode = agent.events_through("turn.finished").await;
    // Then Claude Code's are its tool call, the call's output and finish, the text and a completed finish, each of the thread that ran
    assert_eq!(kinds(&claude), ["turn.started", "message.delta", "tool.started", "tool.output", "tool.finished", "message.delta", "turn.finished"]);
    assert_eq!((claude[2].data["tool"].clone(), claude[2].data["args"].clone(), claude[3].data["data"].clone()), (json!("Bash"), json!({"command": "ls"}), json!("a.txt")));
    assert_eq!((claude[5].data["role"].clone(), claude[5].data["text"].clone(), claude[6].data["reason"].clone()), (json!("assistant"), json!("Done: a.txt"), json!("completed")));
    assert!(claude.iter().all(|e| e.thread.as_deref() == Some("t1")));
    // And opencode's text is the same kind of event
    assert_eq!(kinds(&opencode), ["turn.started", "message.delta", "message.delta", "turn.finished"]);
    assert_eq!((opencode[2].data["role"].clone(), opencode[2].data["text"].clone()), (json!("assistant"), json!("from opencode")));
    // And the CLIs ran in the service's directory with the model and the prompt, not interactively
    let spawned = svc.spawner.spawned();
    assert_eq!((spawned[0].program.as_str(), spawned[0].cwd.clone()), ("claude", env.home.clone()));
    assert!(spawned[0].args.windows(2).any(|w| w == ["--model", "m1"]) && spawned[0].args.contains(&"stream-json".to_string()) && spawned[0].args.contains(&"make a note".to_string()), "{:?}", spawned[0].args);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_clis_that_fail_when_turns_run_then_each_thread_logs_an_error_with_the_reason_and_finishes_failed() {
    // Given an executor for the workspace, Claude Code reporting an error result, opencode exiting with stderr and no JSON, and a CLI that is not installed
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let _executor = Executor::connect(port, "w1", false).await;
    svc.spawner.script("claude", CliScript { lines: vec![r#"{"type":"result","is_error":true,"result":"rate limited"}"#.into()], ..CliScript::default() });
    svc.spawner.script("env", CliScript { exit: Some("exited with 1: not logged in".into()), ..CliScript::default() });
    let mut agent = mapper(&svc, port).await;
    for (thread, driver) in [("t1", "claudeAgent"), ("t2", "opencode")] {
        followed(&mut agent, thread, "w1", driver).await;
    }
    // When a turn runs on each
    agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    let claude = agent.events_through("turn.finished").await;
    agent.ask("turn.start", turn("t2", "w1", "opencode")).await;
    let opencode = agent.events_through("turn.finished").await;
    // Then each ends with an error that says why, and a failed finish
    let error = |events: &[Event]| events.iter().find(|e| e.kind == "error").map(|e| (e.data["code"].clone(), e.data["message"].clone()));
    assert_eq!(error(&claude), Some((json!("CLI_ERROR"), json!("rate limited"))));
    assert_eq!(error(&opencode), Some((json!("CLI_ERROR"), json!("exited with 1: not logged in"))));
    assert!(claude.last().unwrap().data["reason"] == json!("failed") && opencode.last().unwrap().data["reason"] == json!("failed"));
    // When a turn names a CLI that cannot be started
    svc.spawner.script("claude", CliScript { not_started: Some("not found".into()), ..CliScript::default() });
    followed(&mut agent, "t3", "w1", "claudeAgent").await;
    agent.ask("turn.start", turn("t3", "w1", "claudeAgent")).await;
    let missing = agent.events_through("turn.finished").await;
    // Then the thread says it could not start and why, and finished failed
    assert_eq!(error(&missing).map(|(code, _)| code), Some(json!("CLI_START_FAILED")));
    assert!(error(&missing).unwrap().1.as_str().unwrap().contains("not found"), "{:?}", error(&missing));
    assert_eq!(missing.last().unwrap().data["reason"], json!("failed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_the_mcp_server_when_a_client_initializes_and_lists_tools_then_it_gets_the_contract_table_and_the_two_tools_the_service_answers() {
    // Given a service (no executor needed to list tools)
    let env = Env::new(&[]);
    let (_svc, port) = listening(&env, cli(script(|_, _| async {}))).await;
    let mcp = Mcp { url: format!("http://127.0.0.1:{port}/mcp/t1") };
    // When the client initializes and lists the tools
    let init = mcp.rpc("initialize", json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})).await;
    let listed = mcp.rpc("tools/list", json!({})).await;
    // Then the server answers as a tool server, and the tools are the contract's and then the two the service answers itself, each with an input schema
    assert_eq!((init["protocolVersion"].clone(), init["capabilities"]["tools"].is_object()), (json!("2025-03-26"), true));
    let tools = listed["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["run_command", "read_file", "write_file", "edit_file", "list_dir", "search", "cancel", "stat", "read_chunk", "create_file", "approval_prompt", "ask_user"]);
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_no_executor_for_the_workspace_when_a_turn_starts_or_a_tool_is_called_then_both_say_no_executor_and_no_cli_runs() {
    // Given a service, an executor for another workspace, and a thread whose workspace has none
    let env = Env::new(&[]);
    let cli = cli(script(|_, _| async {}));
    let (svc, port) = listening(&env, cli.clone()).await;
    let _other = Executor::connect(port, "elsewhere", false).await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "lonely", "lonely", "claudeAgent").await;
    // When a turn starts and a tool is called for it
    let refused = agent.ask("turn.start", turn("lonely", "lonely", "claudeAgent")).await;
    let called = Mcp { url: format!("http://127.0.0.1:{port}/mcp/lonely") }.call("write_file", json!({"path": "a", "content": "x"})).await;
    // Then the turn is refused with NO_EXECUTOR, the tool call is an error that says so, and the CLI never started
    assert_eq!(refused["error"]["code"], json!("NO_EXECUTOR"));
    assert_eq!(called["isError"], true);
    assert!(text_of(&called).starts_with("NO_EXECUTOR"), "{called}");
    assert!(cli.spawned.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_command_running_at_the_executor_when_the_channel_closes_then_the_call_is_executor_gone_and_the_turn_ends_failed_with_an_error_event() {
    // Given a CLI that runs a command which the executor never finishes, and a second MCP client with its own command pending on the same thread once the turn runs
    let env = Env::new(&[]);
    let cli = cli(script(|mcp, out| async move {
        out.send(tool_use("k1", "mcp__executor__run_command", json!({"command": "sleep 100"}))).await.unwrap();
        let result = mcp.call("run_command", json!({"command": "sleep 100"})).await;
        let _ = out.send(tool_result("k1", &result)).await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    }));
    let (svc, port) = listening(&env, cli).await;
    let mut executor = Executor::connect(port, "w1", true).await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    executor.request("run_command").await;
    let pending = tokio::spawn(Mcp { url: format!("http://127.0.0.1:{port}/mcp/t1") }.call_owned("run_command", json!({"command": "sleep 100"})));
    executor.request("run_command").await;
    // When the executor's channel closes mid-command
    executor.hang_up().await;
    let end = agent.events_through("turn.finished").await;
    // Then the thread logged an error event with EXECUTOR_GONE and the turn finished failed
    let error = end.iter().find(|e| e.kind == "error").unwrap_or_else(|| panic!("no error event in {:?}", kinds(&end)));
    assert_eq!(error.data["code"], json!("EXECUTOR_GONE"));
    assert_eq!(end.last().unwrap().data["reason"], json!("failed"));
    // And the tool call that was pending failed with EXECUTOR_GONE
    let result = pending.await.unwrap();
    assert_eq!(result["isError"], true);
    assert!(text_of(&result).starts_with("EXECUTOR_GONE"), "{result}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_non_loopback_caller_when_it_posts_to_the_mcp_server_then_it_is_refused_with_403_and_nothing_reached_the_executor() {
    // Given an executor connected for the workspace and a running turn of the thread
    let env = Env::new(&[]);
    let idle = idle_cli();
    let (svc, port) = listening(&env, idle.clone()).await;
    let mut executor = Executor::connect(port, "w1", false).await;
    let _agent = running_turn(&svc, port, &idle, "t1").await;
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "write_file", "arguments": {"path": "x.txt", "content": "x"}}});
    // When a caller that is not loopback posts a tool call, and a loopback one posts the same
    let refused = svc.send(post("/mcp/t1").from([10, 1, 2, 3]).json(call.clone())).await;
    let allowed = reqwest::Client::new().post(format!("http://127.0.0.1:{port}/mcp/t1")).json(&call).send().await.unwrap();
    // Then only the loopback call is served: the refusal is 403 with its own code, and the file exists once, from the allowed call
    assert_eq!((refused.status, refused.json()["code"].clone()), (StatusCode::FORBIDDEN, json!("MCP_LOOPBACK_ONLY")));
    assert_eq!(allowed.status().as_u16(), 200);
    assert_eq!(executor.request("write_file").await["args"]["path"], "x.txt");
    assert!(tokio::time::timeout(Duration::from_millis(200), executor.requests.recv()).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_channel_for_a_workspace_when_a_second_opens_then_the_first_closes_with_4001_and_calls_go_to_the_second() {
    // Given two channels opened for one workspace, one after the other, and a running turn on it
    let env = Env::new(&[]);
    let idle = idle_cli();
    let (svc, port) = listening(&env, idle.clone()).await;
    let first = Executor::connect(port, "w1", false).await;
    let mut second = Executor::connect(port, "w1", false).await;
    let _agent = running_turn(&svc, port, &idle, "t1").await;
    // When a tool is called
    let result = Mcp { url: format!("http://127.0.0.1:{port}/mcp/t1") }.call("write_file", json!({"path": "b.txt", "content": "second"})).await;
    // Then the first was closed with 4001 and wrote nothing, and the second did the write
    assert_eq!(first.closed_with().await, Some(4001));
    assert_eq!(text_of(&result), "wrote 6 bytes");
    assert_eq!(std::fs::read_to_string(second.dir.path().join("b.txt")).unwrap(), "second");
    assert!(!first.dir.path().join("b.txt").exists());
    second.request("write_file").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_channel_or_turn_request_that_cannot_run_when_it_is_made_then_it_is_refused_with_its_reason_and_no_cli_starts() {
    // Given an executor connected for the workspace and a thread of it
    let env = Env::new(&[]);
    let cli = cli(script(|_, _| async {}));
    let (svc, port) = listening(&env, cli.clone()).await;
    let _executor = Executor::connect(port, "w1", false).await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    // When the channel is opened without the secret, a turn names Codex (which cannot run through an executor), a turn misses its prompt, and one misses its workspace
    let unauthorised = open_channel(port, "w1", None).await.err();
    let codex = agent.ask("turn.start", turn("t1", "w1", "codex")).await;
    let no_prompt = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent"})).await;
    let no_workspace = agent.ask("turn.start", json!({"thread": "t1", "driver": "claudeAgent", "prompt": "x"})).await;
    // Then the open is 401, Codex is UNKNOWN_DRIVER, the missing prompt and workspace are INVALID_PARAMS naming the field, and no CLI started
    assert_eq!(unauthorised.map(|(status, _)| status), Some(401));
    assert_eq!(codex["error"]["code"], json!("UNKNOWN_DRIVER"));
    assert_eq!((no_prompt["error"]["code"].clone(), no_prompt["error"]["message"].clone()), (json!("INVALID_PARAMS"), json!("prompt is required")));
    assert_eq!((no_workspace["error"]["code"].clone(), no_workspace["error"]["message"].clone()), (json!("INVALID_PARAMS"), json!("workspace is required")));
    assert!(cli.spawned.lock().unwrap().is_empty());
}

struct Stopped(Arc<std::sync::atomic::AtomicBool>);

impl Drop for Stopped {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_waiting_on_a_command_at_the_executor_when_it_is_interrupted_then_the_cli_stops_and_the_pending_command_is_cancelled() {
    // Given a CLI whose command the executor holds forever, running a turn of thread t1 on workspace w1
    let env = Env::new(&[]);
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cli = cli(script({
        let stopped = stopped.clone();
        move |mcp, out| {
            let guard = Stopped(stopped.clone());
            async move {
                let _alive = guard;
                out.send(tool_use("k1", "mcp__executor__run_command", json!({"command": "sleep 100"}))).await.unwrap();
                let result = mcp.call("run_command", json!({"command": "sleep 100"})).await;
                let _ = out.send(tool_result("k1", &result)).await;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    }));
    let (svc, port) = listening(&env, cli).await;
    let mut executor = Executor::connect(port, "w1", true).await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    let started = agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    let command = executor.request("run_command").await;
    // When the turn is interrupted
    let interrupted = agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    let end = agent.events_through("turn.finished").await;
    // Then the interrupt named the running turn, the executor was told to cancel its command, and the CLI is stopped
    assert_eq!(interrupted["result"], json!({"thread": "t1", "turn": started["result"]["turn"].clone()}));
    assert_eq!(executor.request("cancel").await["args"]["id"], command["id"]);
    for _ in 0..100 {
        if stopped.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(stopped.load(std::sync::atomic::Ordering::SeqCst), "the CLI is still running");
    // And the turn's events end with its finish as interrupted
    assert_eq!(end.last().unwrap().data["reason"], json!("interrupted"));
}
