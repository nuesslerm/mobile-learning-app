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

fn cli(f: Script) -> Arc<ScriptedCli> {
    Arc::new(ScriptedCli { script: f, spawned: Mutex::new(Vec::new()) })
}

type Sink = Arc<tokio::sync::Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>;

/// `rca-exec` as a channel client: it holds the channel open and answers every action as unsupported, so a flow sees what the service does with the channel itself.
struct Executor {
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
    async fn connect(port: u16, workspace: &str) -> Executor {
        Executor::connect_having(port, workspace, None).await
    }

    /// An executor that answers `cli_detect` for the CLIs it `has` (and says nothing else is supported), as the executor of a host does.
    async fn connect_having(port: u16, workspace: &str, has: Option<Vec<String>>) -> Executor {
        let (write, mut read) = open_channel(port, workspace, Some(SECRET)).await.expect("the channel opens");
        let sink: Sink = Arc::new(tokio::sync::Mutex::new(write));
        let close_code = Arc::new(Mutex::new(None));
        let (reply, code) = (Arc::clone(&sink), Arc::clone(&close_code));
        tokio::spawn(async move {
            while let Some(Ok(message)) = read.next().await {
                match message {
                    Message::Close(frame) => {
                        *code.lock().unwrap() = frame.map(|f| u16::from(f.code));
                        break;
                    }
                    Message::Text(text) => {
                        let request: Value = serde_json::from_str(&text).expect("a JSON request");
                        let detect = has.as_ref().filter(|_| request["tool"] == "cli_detect");
                        let frame = match detect {
                            Some(has) => {
                                let asked = request["args"]["programs"].as_array().cloned().unwrap_or_default();
                                let clis: Vec<Value> = asked.iter().filter_map(Value::as_str).map(|p| json!({"program": p, "installed": has.iter().any(|h| h == p)})).collect();
                                json!({"id": request["id"].clone(), "ok": true, "result": {"clis": clis}, "output": ""})
                            }
                            None => json!({"id": request["id"].clone(), "ok": false, "error": {"code": "UNSUPPORTED", "message": "not in this executor"}}),
                        };
                        let _ = reply.lock().await.send(Message::Text(frame.to_string().into())).await;
                    }
                    _ => {}
                }
            }
        });
        Executor { sink, close_code }
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
async fn given_clis_that_print_their_streams_when_turns_run_then_each_becomes_the_same_neutral_events_in_the_thread_log() {
    // Given a service with an executor for the workspace and two scripted CLIs: Claude Code printing a tool call and its answer, opencode printing text
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let _executor = Executor::connect(port, "w1").await;
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
    followed(&mut agent, "t3", "w1", "codex").await;
    svc.spawner.script("codex", CliScript { lines: vec![r#"{"type":"item.completed","item":{"type":"agent_message","text":"from codex"}}"#.into()], ..CliScript::default() });
    // When a turn runs on each
    agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    let claude = agent.events_through("turn.finished").await;
    agent.ask("turn.start", turn("t2", "w1", "opencode")).await;
    let opencode = agent.events_through("turn.finished").await;
    agent.ask("turn.start", turn("t3", "w1", "codex")).await;
    agent.events_through("turn.finished").await;
    // Then Claude Code's are its tool call, the call's output and finish, the text and a completed finish, each of the thread that ran
    assert_eq!(kinds(&claude), ["turn.started", "message.delta", "tool.started", "tool.output", "tool.finished", "message.delta", "turn.finished"]);
    assert_eq!((claude[2].data["tool"].clone(), claude[2].data["args"].clone(), claude[3].data["data"].clone()), (json!("Bash"), json!({"command": "ls"}), json!("a.txt")));
    assert_eq!((claude[5].data["role"].clone(), claude[5].data["text"].clone(), claude[6].data["reason"].clone()), (json!("assistant"), json!("Done: a.txt"), json!("completed")));
    assert!(claude.iter().all(|e| e.thread.as_deref() == Some("t1")));
    // And opencode's text is the same kind of event
    assert_eq!(kinds(&opencode), ["turn.started", "message.delta", "message.delta", "turn.finished"]);
    assert_eq!((opencode[2].data["role"].clone(), opencode[2].data["text"].clone()), (json!("assistant"), json!("from opencode")));
    // And the CLIs were started at the workspace host's root with the model and the prompt, not interactively
    let spawned = svc.spawner.spawned();
    assert_eq!((spawned[0].program.as_str(), spawned[0].cwd.clone()), ("claude", std::path::PathBuf::from(".")));
    assert!(spawned[0].args.windows(2).any(|w| w == ["--model", "m1"]) && spawned[0].args.contains(&"stream-json".to_string()) && spawned[0].args.contains(&"make a note".to_string()), "{:?}", spawned[0].args);
    // And all three run at that root with the prompt, none with its own tools switched off or its tool list narrowed
    assert_eq!(spawned.iter().map(|s| (s.program.as_str(), s.cwd.clone())).collect::<Vec<_>>(), [("claude", ".".into()), ("env", ".".into()), ("codex", ".".into())]);
    assert!(spawned.iter().all(|s| s.args.contains(&"make a note".to_string())), "{spawned:?}");
    let switched_off = ["--tools", "--allowedTools", "--allowed-tools", "--disallowedTools", "--disallowed-tools", "--no-builtin-tools"];
    assert!(spawned.iter().all(|s| !s.args.iter().any(|a| switched_off.contains(&a.as_str()) || a.contains("\"tools\":"))), "{spawned:?}");
    assert!(spawned[1].args.iter().any(|a| a == "opencode") && spawned[2].args.contains(&"exec".to_string()), "{spawned:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_clis_that_fail_when_turns_run_then_each_thread_logs_an_error_with_the_reason_and_finishes_failed() {
    // Given an executor for the workspace, Claude Code reporting an error result, opencode exiting with stderr and no JSON, and a CLI that is not installed
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let _executor = Executor::connect(port, "w1").await;
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
async fn given_an_executor_without_claude_when_a_turn_names_claude_code_then_it_is_refused_naming_the_cli_and_the_cli_it_has_still_runs() {
    // Given an executor for the workspace that reports codex installed and claude missing
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let _executor = Executor::connect_having(port, "w1", Some(vec!["codex".into()])).await;
    svc.spawner.script("codex", CliScript { lines: vec![r#"{"type":"item.completed","item":{"type":"agent_message","text":"from codex"}}"#.into()], ..CliScript::default() });
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    followed(&mut agent, "t2", "w1", "codex").await;
    // When a turn names claude code, and another names codex
    let refused = agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    let started = agent.ask("turn.start", turn("t2", "w1", "codex")).await;
    let ran = agent.events_through("turn.finished").await;
    // Then the first is refused naming claude and nothing ran for it, and the second ran to its finish
    assert_eq!(refused["error"], json!({"code": "CLI_NOT_INSTALLED", "message": "claude is not installed on the executor of workspace w1"}));
    assert!(started.get("result").is_some(), "{started}");
    assert_eq!(ran.last().map(|e| e.data["reason"].clone()), Some(json!("completed")));
    assert_eq!(svc.spawner.spawned().iter().map(|s| s.program.clone()).collect::<Vec<_>>(), ["codex"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_the_mcp_server_when_a_client_initializes_and_lists_tools_then_it_gets_only_the_two_tools_the_service_answers() {
    // Given a service (no executor needed to list tools)
    let env = Env::new(&[]);
    let (_svc, port) = listening(&env, cli(script(|_, _| async {}))).await;
    let mcp = Mcp { url: format!("http://127.0.0.1:{port}/mcp/t1") };
    // When the client initializes and lists the tools
    let init = mcp.rpc("initialize", json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}})).await;
    let listed = mcp.rpc("tools/list", json!({})).await;
    // Then the server answers as a tool server, and the tools are the two the service answers itself, each with an input schema
    assert_eq!((init["protocolVersion"].clone(), init["capabilities"]["tools"].is_object()), (json!("2025-03-26"), true));
    let tools = listed["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["approval_prompt", "ask_user"]);
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_no_executor_for_the_workspace_when_a_turn_starts_then_it_says_no_executor_and_no_cli_runs() {
    // Given a service, an executor for another workspace, and a thread whose workspace has none
    let env = Env::new(&[]);
    let cli = cli(script(|_, _| async {}));
    let (svc, port) = listening(&env, cli.clone()).await;
    let _other = Executor::connect(port, "elsewhere").await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "lonely", "lonely", "claudeAgent").await;
    // When a turn starts
    let refused = agent.ask("turn.start", turn("lonely", "lonely", "claudeAgent")).await;
    // Then the turn is refused with NO_EXECUTOR and the CLI never started
    assert_eq!(refused["error"]["code"], json!("NO_EXECUTOR"));
    assert!(cli.spawned.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_running_when_the_executors_channel_closes_then_the_turn_ends_failed_with_an_executor_gone_error_event() {
    // Given a CLI that goes on running, a connected executor, and a running turn
    let env = Env::new(&[]);
    let cli = cli(script(|_, _| async {
        tokio::time::sleep(Duration::from_secs(30)).await;
    }));
    let (svc, port) = listening(&env, cli).await;
    let executor = Executor::connect(port, "w1").await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    // When the executor's channel closes mid-turn
    executor.hang_up().await;
    let end = agent.events_through("turn.finished").await;
    // Then the thread logged an error event with EXECUTOR_GONE and the turn finished failed
    let error = end.iter().find(|e| e.kind == "error").unwrap_or_else(|| panic!("no error event in {:?}", kinds(&end)));
    assert_eq!(error.data["code"], json!("EXECUTOR_GONE"));
    assert_eq!(end.last().unwrap().data["reason"], json!("failed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_non_loopback_caller_when_it_posts_to_the_mcp_server_then_it_is_refused_with_403() {
    // Given an executor connected for the workspace and a running turn of the thread
    let env = Env::new(&[]);
    let idle = idle_cli();
    let (svc, port) = listening(&env, idle.clone()).await;
    let _executor = Executor::connect(port, "w1").await;
    let _agent = running_turn(&svc, port, &idle, "t1").await;
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    // When a caller that is not loopback posts a request, and a loopback one posts the same
    let refused = svc.send(post("/mcp/t1").from([10, 1, 2, 3]).json(call.clone())).await;
    let allowed = reqwest::Client::new().post(format!("http://127.0.0.1:{port}/mcp/t1")).json(&call).send().await.unwrap();
    // Then only the loopback call is served: the refusal is 403 with its own code
    assert_eq!((refused.status, refused.json()["code"].clone()), (StatusCode::FORBIDDEN, json!("MCP_LOOPBACK_ONLY")));
    assert_eq!(allowed.status().as_u16(), 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_cli_on_another_host_when_it_presents_the_turns_token_then_the_tool_server_answers_it_and_a_wrong_token_or_a_finished_turn_is_refused() {
    // Given a running turn whose CLI was told the tool server's token, as one on the workspace host is
    let env = Env::new(&[]);
    let idle = idle_cli();
    let (svc, port) = listening(&env, idle.clone()).await;
    let _executor = Executor::connect(port, "w1").await;
    let mut agent = running_turn(&svc, port, &idle, "t1").await;
    let args = idle.spawned.lock().unwrap()[0].1.clone();
    let config: Value = serde_json::from_str(&args[args.iter().position(|a| a == "--mcp-config").unwrap() + 1]).unwrap();
    let bearer = config["mcpServers"]["executor"]["headers"]["Authorization"].as_str().unwrap().to_string();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    let remote = |auth: &str| svc.send(post("/mcp/t1").from([10, 1, 2, 3]).header("authorization", auth).json(call.clone()));
    // When the same caller from another address presents that token, a wrong one, and the token for another thread's route
    let served = remote(&bearer).await;
    let wrong = remote("Bearer not-the-token").await;
    let bare = svc.send(post("/mcp/t1").from([10, 1, 2, 3]).json(call.clone())).await;
    let elsewhere = svc.send(post("/mcp/t2").from([10, 1, 2, 3]).header("authorization", &bearer).json(call.clone())).await;
    // Then only the right token on its own thread's route is served
    assert_eq!(served.status, StatusCode::OK, "{}", served.json());
    assert_eq!((wrong.status, bare.status, elsewhere.status), (StatusCode::FORBIDDEN, StatusCode::FORBIDDEN, StatusCode::FORBIDDEN));
    // And once the turn is over the token no longer opens the route
    agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    agent.events_through("turn.finished").await;
    assert_eq!(remote(&bearer).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_channel_for_a_workspace_when_a_second_opens_then_the_first_closes_with_4001() {
    // Given two channels opened for one workspace, one after the other
    let env = Env::new(&[]);
    let idle = idle_cli();
    let (svc, port) = listening(&env, idle.clone()).await;
    let first = Executor::connect(port, "w1").await;
    let _second = Executor::connect(port, "w1").await;
    let _agent = running_turn(&svc, port, &idle, "t1").await;
    // Then the first was closed with 4001
    assert_eq!(first.closed_with().await, Some(4001));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_channel_or_turn_request_that_cannot_run_when_it_is_made_then_it_is_refused_with_its_reason_and_no_cli_starts() {
    // Given an executor connected for the workspace and a thread of it
    let env = Env::new(&[]);
    let cli = cli(script(|_, _| async {}));
    let (svc, port) = listening(&env, cli.clone()).await;
    let _executor = Executor::connect(port, "w1").await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    // When the channel is opened without the secret, a turn names a driver nobody runs, a turn misses its prompt, and one misses its workspace
    let unauthorised = open_channel(port, "w1", None).await.err();
    let unknown = agent.ask("turn.start", turn("t1", "w1", "nobody")).await;
    let no_prompt = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent"})).await;
    let no_workspace = agent.ask("turn.start", json!({"thread": "t1", "driver": "claudeAgent", "prompt": "x"})).await;
    // Then the open is 401, the unknown driver is UNKNOWN_DRIVER, the missing prompt and workspace are INVALID_PARAMS naming the field, and no CLI started
    assert_eq!(unauthorised.map(|(status, _)| status), Some(401));
    assert_eq!(unknown["error"]["code"], json!("UNKNOWN_DRIVER"));
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
async fn given_a_running_turn_when_it_is_interrupted_then_the_cli_stops_and_the_turn_ends_interrupted() {
    // Given a CLI that goes on running, a turn of thread t1 on workspace w1
    let env = Env::new(&[]);
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cli = cli(script({
        let stopped = stopped.clone();
        move |_, _| {
            let guard = Stopped(stopped.clone());
            async move {
                let _alive = guard;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    }));
    let (svc, port) = listening(&env, cli).await;
    let _executor = Executor::connect(port, "w1").await;
    let mut agent = mapper(&svc, port).await;
    followed(&mut agent, "t1", "w1", "claudeAgent").await;
    let started = agent.ask("turn.start", turn("t1", "w1", "claudeAgent")).await;
    // When the turn is interrupted
    let interrupted = agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    let end = agent.events_through("turn.finished").await;
    // Then the interrupt named the running turn and the CLI is stopped
    assert_eq!(interrupted["result"], json!({"thread": "t1", "turn": started["result"]["turn"].clone()}));
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
