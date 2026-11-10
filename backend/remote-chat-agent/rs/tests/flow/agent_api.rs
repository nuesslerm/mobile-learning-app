//! The rca agent API socket (`GET /agent/v1/socket`) as a mapper uses it: one request per frame, one answer each, over a real WebSocket, opened
//! with a device bearer the service secret minted.

use crate::harness::{service, Env, Options, SECRET};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

async fn open(port: u16, bearer: Option<&str>) -> Result<WebSocketStream<TcpStream>, u16> {
    let mut request = format!("ws://127.0.0.1:{port}/agent/v1/socket").into_client_request().unwrap();
    if let Some(bearer) = bearer {
        request.headers_mut().insert("authorization", format!("Bearer {bearer}").parse().unwrap());
    }
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("the service is listening");
    match tokio_tungstenite::client_async(request, tcp).await {
        Ok((stream, _)) => Ok(stream),
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => Err(response.status().as_u16()),
        Err(e) => panic!("the handshake failed: {e}"),
    }
}

async fn ask(socket: &mut WebSocketStream<TcpStream>, frame: &str) -> Value {
    socket.send(Message::Text(frame.to_string().into())).await.unwrap();
    loop {
        match socket.next().await.expect("an answer").unwrap() {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            _ => continue,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_paired_mapper_when_it_asks_status_and_keeps_a_value_then_it_reads_it_back_after_reconnecting_and_a_stranger_is_refused() {
    // Given the service listening
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("mapper").await;
    // When a socket is opened without a bearer, with one nobody minted, and with the service secret, Then the upgrade is refused with 401 each time
    assert_eq!(open(port, None).await.err(), Some(401));
    assert_eq!(open(port, Some("wrong")).await.err(), Some(401));
    assert_eq!(open(port, Some(SECRET)).await.err(), Some(401));
    // When a mapper asks for the status, Then it gets the protocol and the groups this build serves
    let mut socket = open(port, Some(&token)).await.expect("the socket opens");
    let status = ask(&mut socket, r#"{"id": 1, "method": "service.status"}"#).await;
    assert_eq!((status["id"].clone(), status["result"]["protocol"].clone(), status["result"]["groups"].clone()), (json!(1), json!(1), json!(["service", "service.activity", "state", "state.watch", "files", "files.edit", "workspace.worktrees", "terminal", "git", "hosted.pr", "hosted.clone", "hosted.auth", "git.ai", "accounts", "agent.threads", "agent.requests", "agent.queue", "agent.lifecycle", "schedules", "schedules.watch", "attachments", "agent.catalog", "preview", "preview.sessions", "preview.watch"])));
    // When it keeps a device's quick commands and asks again on a new socket, Then the value comes back; another device sees none
    let set = ask(&mut socket, r#"{"id": "s", "method": "state.set", "params": {"scope": "phone-a", "key": "quickCommands", "value": ["git status", "ls"]}}"#).await;
    assert_eq!(set, json!({"id": "s", "result": {}}));
    drop(socket);
    let mut again = open(port, Some(&token)).await.expect("the socket opens");
    let back = ask(&mut again, r#"{"id": 2, "method": "state.get", "params": {"scope": "phone-a", "key": "quickCommands"}}"#).await;
    assert_eq!(back, json!({"id": 2, "result": {"value": ["git status", "ls"]}}));
    let other = ask(&mut again, r#"{"id": 3, "method": "state.get", "params": {"scope": "phone-b", "key": "quickCommands"}}"#).await;
    assert_eq!(other["result"]["value"], Value::Null);
    // When a frame is not JSON or names a method the service does not serve, Then the socket answers the error and stays open
    let garbage = ask(&mut again, "not json").await;
    assert_eq!(garbage["error"]["code"], json!("INVALID_REQUEST"));
    let unknown = ask(&mut again, r#"{"id": 4, "method": "terminal.destroy"}"#).await;
    assert_eq!((unknown["id"].clone(), unknown["error"]["code"].clone()), (json!(4), json!("UNKNOWN_METHOD")));
    let still = ask(&mut again, r#"{"id": 5, "method": "service.status"}"#).await;
    assert_eq!(still["id"], json!(5));
}

/// An executor double attached for `workspace`: answers `read_file` and `list_dir` from `files`, anything else as unsupported.
async fn attach_executor(port: u16, workspace: &str, files: &'static [(&'static str, &'static str)]) {
    attach_executor_as(port, &format!("workspace={workspace}"), files).await.expect("the executor channel opens");
}

/// The same double on the channel the query names; the HTTP status when the service refuses the upgrade.
async fn attach_executor_as(port: u16, query: &str, files: &'static [(&'static str, &'static str)]) -> Result<(), u16> {
    let mut request = format!("ws://127.0.0.1:{port}/exec/v2/executors?{query}").into_client_request().unwrap();
    request.headers_mut().insert("x-bot-secret", SECRET.parse().unwrap());
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut socket = match tokio_tungstenite::client_async(request, tcp).await {
        Ok((socket, _)) => socket,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => return Err(response.status().as_u16()),
        Err(e) => panic!("the handshake failed: {e}"),
    };
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let request: Value = serde_json::from_str(&text).unwrap();
            let (id, path) = (request["id"].clone(), request["args"]["path"].as_str().unwrap_or_default().to_string());
            let reply = match request["tool"].as_str() {
                Some("read_file") => match files.iter().find(|(name, _)| *name == path) {
                    Some((_, content)) => json!({"id": id, "ok": true, "result": {"content": content}}),
                    None => json!({"id": id, "ok": false, "error": {"code": "NOT_FOUND", "message": format!("{path} does not exist")}}),
                },
                Some("git_diff") => json!({"id": id, "ok": true, "result": {"diff": format!("diff of {} against {}", request["args"]["path"].as_str().unwrap_or("the tree"), request["args"]["base"].as_str().unwrap_or("the index"))}}),
                Some("list_dir") => json!({"id": id, "ok": true, "result": {"entries": files.iter().map(|(name, _)| json!({"name": name, "kind": "file"})).collect::<Vec<_>>()}}),
                _ => json!({"id": id, "ok": false, "error": {"code": "UNSUPPORTED", "message": "not in this executor"}}),
            };
            let _ = socket.send(Message::Text(reply.to_string().into())).await;
        }
    });
    // The channel is registered before the handshake answers, so the workspace is attached once the socket is open.
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_attached_executor_when_a_mapper_lists_workspaces_and_reads_files_then_they_come_from_that_workspace() {
    // Given the service listening and an executor attached for workspace w1 holding two files
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    attach_executor(port, "w1", &[("README.md", "hello"), ("src/main.rs", "fn main() {}")]).await;
    let token = svc.mint("mapper").await;
    let mut socket = open(port, Some(&token)).await.expect("the socket opens");
    // When the mapper lists the workspaces, Then w1 is the one attached
    assert_eq!(ask(&mut socket, r#"{"id": 1, "method": "workspace.list"}"#).await, json!({"id": 1, "result": {"workspaces": ["w1"]}}));
    // When it reads a file and lists the root, Then both come from the executor
    let read = ask(&mut socket, r#"{"id": 2, "method": "file.read", "params": {"workspace": "w1", "path": "README.md"}}"#).await;
    assert_eq!(read, json!({"id": 2, "result": {"content": "hello"}}));
    let listed = ask(&mut socket, r#"{"id": 3, "method": "file.list", "params": {"workspace": "w1", "path": "."}}"#).await;
    assert_eq!(listed["result"]["entries"].as_array().map(Vec::len), Some(2));
    // When the diff of the whole tree against HEAD is asked, Then the executor's git_diff answers with what it was given
    let diff = ask(&mut socket, r#"{"id": 7, "method": "file.diff", "params": {"workspace": "w1", "base": "HEAD"}}"#).await;
    assert_eq!(diff, json!({"id": 7, "result": {"diff": "diff of the tree against HEAD"}}));
    // When the file is missing or no executor is attached for the workspace, Then each is an error with its code
    let missing = ask(&mut socket, r#"{"id": 4, "method": "file.read", "params": {"workspace": "w1", "path": "nope.txt"}}"#).await;
    assert_eq!(missing["error"]["code"], json!("NOT_FOUND"));
    let detached = ask(&mut socket, r#"{"id": 5, "method": "file.read", "params": {"workspace": "w2", "path": "README.md"}}"#).await;
    assert_eq!(detached["error"]["code"], json!("NO_EXECUTOR"));
    // And the status lists the complete groups: files, agent.threads and its three sub-groups now, workspace not yet
    assert_eq!(ask(&mut socket, r#"{"id": 6, "method": "service.status"}"#).await["result"]["groups"], json!(["service", "service.activity", "state", "state.watch", "files", "files.edit", "workspace.worktrees", "terminal", "git", "hosted.pr", "hosted.clone", "hosted.auth", "git.ai", "accounts", "agent.threads", "agent.requests", "agent.queue", "agent.lifecycle", "schedules", "schedules.watch", "attachments", "agent.catalog", "preview", "preview.sessions", "preview.watch"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_state_file_that_cannot_be_written_when_a_value_is_set_then_the_socket_answers_state_unavailable_and_keeps_serving() {
    // Given a service with a bearer minted, and the file the device state lives in occupied by a directory that is not empty
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("mapper").await;
    std::fs::create_dir_all(env.home.join(".local/state/rca/agent-state.json/in-the-way")).unwrap();
    let mut socket = open(port, Some(&token)).await.expect("the socket opens");
    // When a mapper sets a value, and then reads one and asks for a bad key
    let set = ask(&mut socket, r#"{"id": 1, "method": "state.set", "params": {"scope": "a", "key": "k", "value": 1}}"#).await;
    let read = ask(&mut socket, r#"{"id": 2, "method": "state.get", "params": {"scope": "a", "key": "k"}}"#).await;
    let bad = ask(&mut socket, r#"{"id": 3, "method": "state.get", "params": {"scope": "a"}}"#).await;
    // Then the set is refused with STATE_UNAVAILABLE and the store's reason, the read finds nothing, a missing key is INVALID_PARAMS, and the socket stays open
    assert_eq!((set["id"].clone(), set["error"]["code"].clone()), (json!(1), json!("STATE_UNAVAILABLE")));
    assert!(set["error"]["message"].as_str().is_some_and(|why| !why.is_empty()));
    assert_eq!(read, json!({"id": 2, "result": {"value": null}}));
    assert_eq!((bad["error"]["code"].clone(), bad["error"]["message"].clone()), (json!("INVALID_PARAMS"), json!("scope and key are required")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_executors_attached_by_id_when_files_are_read_then_each_workspace_is_served_by_its_own_executor_and_a_taken_workspace_is_refused_without_replacing_its_owner() {
    // Given the service, an executor "box-a" with root workspace "main" and one "box-b" with root workspace "other", each holding a different file
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let token = svc.mint("mapper").await;
    attach_executor_as(port, "workspace=main&executor=box-a", &[("a.txt", "from a")]).await.unwrap();
    attach_executor_as(port, "workspace=other&executor=box-b", &[("b.txt", "from b")]).await.unwrap();
    let mut socket = open(port, Some(&token)).await.unwrap();
    // When the workspaces are listed and each is read
    let listed = ask(&mut socket, r#"{"id": 1, "method": "workspace.list"}"#).await;
    let a = ask(&mut socket, r#"{"id": 2, "method": "file.read", "params": {"workspace": "main", "path": "a.txt"}}"#).await;
    let b = ask(&mut socket, r#"{"id": 3, "method": "file.read", "params": {"workspace": "other", "path": "b.txt"}}"#).await;
    let crossed = ask(&mut socket, r#"{"id": 4, "method": "file.read", "params": {"workspace": "main", "path": "b.txt"}}"#).await;
    // Then both workspaces are listed, each file comes from the executor that owns the workspace, and the other executor's file is not there
    assert_eq!(listed["result"]["workspaces"], json!(["main", "other"]));
    assert_eq!((a["result"]["content"].clone(), b["result"]["content"].clone()), (json!("from a"), json!("from b")));
    assert_eq!(crossed["error"]["code"], json!("NOT_FOUND"));
    // When a third executor claims workspace "main" under another id
    let taken = attach_executor_as(port, "workspace=main&executor=box-c", &[("c.txt", "from c")]).await;
    // Then the upgrade is refused with 409 and the first executor still serves it
    assert_eq!(taken, Err(409));
    let again = ask(&mut socket, r#"{"id": 5, "method": "file.read", "params": {"workspace": "main", "path": "a.txt"}}"#).await;
    assert_eq!(again["result"]["content"], json!("from a"));
}
