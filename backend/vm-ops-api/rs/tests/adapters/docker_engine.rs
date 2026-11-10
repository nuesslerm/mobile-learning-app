use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;
use vm_ops_api::infrastructure::DockerEngine;
use vm_ops_api_core::application::ports::{Containers, ContainersError, EngineReply, ExecOptions, ExecResult, LogFollow};
use vm_ops_api_core::domain::container_stats::{cpu_percent, working_set_bytes};
use vm_ops_api_core::domain::containers::DockerContainer;

/// The pause between a stream's chunks, so each reaches the client as a read of its own.
const CHUNK_GAP: Duration = Duration::from_millis(20);
const ASSERTED_WITHIN: Duration = Duration::from_secs(5);
const STREAM_TYPE: &str = "application/vnd.docker.multiplexed-stream";

/// One request as the fake Engine read it off the socket: the request line, the headers with lower-case names, the body bytes.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn call(&self) -> String {
        format!("{} {}", self.method, self.target)
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The headers that frame a body: the content type, the length and the transfer encoding.
    fn framing(&self) -> [Option<&str>; 3] {
        ["content-type", "content-length", "transfer-encoding"].map(|name| self.header(name))
    }
}

#[derive(Clone, Copy)]
enum StreamEnd {
    /// The last chunk of a chunked body, so the Engine's stream ends cleanly.
    Finish,
    /// The connection drops with the chunked body unfinished.
    Cut,
    /// The stream stays open until the client closes the connection.
    HoldOpen,
}

#[derive(Clone)]
enum Answer {
    Full { status: u16, body: Vec<u8> },
    Raw(Vec<u8>),
    Stream { chunks: Vec<Vec<u8>>, end: StreamEnd },
    After(Duration, Box<Answer>),
    Hangup,
}

impl Answer {
    fn json(status: u16, body: &str) -> Answer {
        Answer::Full { status, body: body.as_bytes().to_vec() }
    }

    fn frames(bytes: &[u8]) -> Answer {
        Answer::Full { status: 200, body: bytes.to_vec() }
    }

    fn stream(chunks: Vec<Vec<u8>>, end: StreamEnd) -> Answer {
        Answer::Stream { chunks, end }
    }
}

type Script = Arc<dyn Fn(&Seen) -> Answer + Send + Sync>;

struct Engine {
    _dir: TempDir,
    socket: PathBuf,
    seen: Arc<Mutex<Vec<Seen>>>,
    closed_by_client: Arc<AtomicUsize>,
    accepting: JoinHandle<()>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

impl Engine {
    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    fn calls(&self) -> Vec<String> {
        self.seen().iter().map(Seen::call).collect()
    }

    fn docker(&self) -> DockerEngine {
        DockerEngine::new(&self.socket).expect("a client for the socket")
    }

    async fn closed_by_client(&self) -> usize {
        let waited = tokio::time::timeout(ASSERTED_WITHIN, async {
            while self.closed_by_client.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(waited.is_ok(), "the client never closed the stream's connection");
        self.closed_by_client.load(Ordering::SeqCst)
    }
}

/// An Engine listening on a Unix socket in a temp dir, answering each request from `script` and keeping what it was asked.
async fn start(script: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> Engine {
    let dir = tempfile::tempdir().expect("a temp dir");
    let socket = dir.path().join("docker.sock");
    let listener = UnixListener::bind(&socket).expect("the fake Engine binds its socket");
    let (seen, closed_by_client): (Arc<Mutex<Vec<Seen>>>, Arc<AtomicUsize>) = Default::default();
    let script: Script = Arc::new(script);
    let (kept, closed, answers) = (seen.clone(), closed_by_client.clone(), script);
    let accepting = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve(stream, answers.clone(), kept.clone(), closed.clone()));
        }
    });
    Engine { _dir: dir, socket, seen, closed_by_client, accepting }
}

async fn serve(mut stream: UnixStream, script: Script, seen: Arc<Mutex<Vec<Seen>>>, closed: Arc<AtomicUsize>) {
    let mut buffered = Vec::new();
    while let Some(request) = read_request(&mut stream, &mut buffered).await {
        seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(request.clone());
        if !respond(&mut stream, script(&request), &closed).await {
            return;
        }
    }
}

async fn read_request(stream: &mut UnixStream, buffered: &mut Vec<u8>) -> Option<Seen> {
    let head_end = loop {
        if let Some(at) = buffered.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => buffered.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buffered[..head_end]).into_owned();
    let mut lines = head.trim_end().split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let (method, target) = (request_line.next()?.to_string(), request_line.next()?.to_string());
    let headers: Vec<(String, String)> =
        lines.filter_map(|line| line.split_once(':')).map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string())).collect();
    let length: usize = headers.iter().find(|(name, _)| name == "content-length").and_then(|(_, value)| value.parse().ok()).unwrap_or(0);
    while buffered.len() < head_end + length {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => buffered.extend_from_slice(&chunk[..n]),
        }
    }
    let body = buffered[head_end..head_end + length].to_vec();
    buffered.drain(..head_end + length);
    Some(Seen { method, target, headers, body })
}

/// Writes the answer; false when the connection is no longer to be read from.
async fn respond(stream: &mut UnixStream, answer: Answer, closed: &Arc<AtomicUsize>) -> bool {
    match answer {
        Answer::Full { status, body } => {
            let head = format!("HTTP/1.1 {status} Scripted\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).await.is_ok() && stream.write_all(&body).await.is_ok()
        }
        Answer::Raw(bytes) => stream.write_all(&bytes).await.is_ok(),
        Answer::After(wait, inner) => {
            tokio::time::sleep(wait).await;
            Box::pin(respond(stream, *inner, closed)).await
        }
        Answer::Hangup => false,
        Answer::Stream { chunks, end } => {
            let head = format!("HTTP/1.1 200 OK\r\ncontent-type: {STREAM_TYPE}\r\ntransfer-encoding: chunked\r\n\r\n");
            if stream.write_all(head.as_bytes()).await.is_err() {
                return false;
            }
            for chunk in chunks {
                let framed = [format!("{:x}\r\n", chunk.len()).into_bytes(), chunk, b"\r\n".to_vec()].concat();
                if stream.write_all(&framed).await.is_err() {
                    return false;
                }
                tokio::time::sleep(CHUNK_GAP).await;
            }
            match end {
                StreamEnd::Finish => stream.write_all(b"0\r\n\r\n").await.is_ok(),
                StreamEnd::Cut => false,
                StreamEnd::HoldOpen => {
                    let mut ignored = [0u8; 64];
                    while matches!(stream.read(&mut ignored).await, Ok(n) if n > 0) {}
                    closed.fetch_add(1, Ordering::SeqCst);
                    false
                }
            }
        }
    }
}

fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![stream, 0, 0, 0];
    bytes.extend_from_slice(&u32::try_from(payload.len()).expect("a small frame").to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn words(cmd: &[&str]) -> Vec<String> {
    cmd.iter().map(|word| (*word).to_string()).collect()
}

fn ids(containers: &[DockerContainer]) -> Vec<&str> {
    containers.iter().map(|c| c.id.as_str()).collect()
}

/// Exec create, start and inspect of one exec named `exec-1`; the start prints `frames` and the inspect answers `inspect`.
fn one_exec(frames: Vec<u8>, inspect: &'static str) -> impl Fn(&Seen) -> Answer + Send + Sync + 'static {
    move |seen| match seen.call().as_str() {
        "POST /containers/as1/exec" => Answer::json(201, r#"{"Id":"exec-1"}"#),
        "POST /exec/exec-1/start" => Answer::frames(&frames),
        "GET /exec/exec-1/json" => Answer::json(200, inspect),
        other => Answer::json(404, &format!(r#"{{"message":"the fake Engine has no rule for {other}"}}"#)),
    }
}

const LISTED: &str = r#"[
    {"Id":"b","Names":["/remote-chat-agent-tailscale"],"State":"exited","Status":"Exited (0) 1 hour ago","Image":"tailscale/tailscale:v1"},
    {"Id":"x","Names":["/remote-chat-agent-old"],"State":"running","Status":"Up 1 hour","Image":"ns/old:1"},
    {"Id":"a","Names":["/remote-chat-agent"],"State":"running","Status":"Up 2 hours","Image":"ns/remote-chat-agent:sha-1"},
    {"Id":"o","Names":["/reporting-service"],"State":"running"},
    {"Id":"n"}
]"#;

#[tokio::test]
async fn given_look_alike_names_when_an_apps_containers_are_listed_then_only_its_exact_names_come_back_in_the_engines_order() {
    // Given an Engine whose list holds the app's two containers, a look-alike, another app's container and a nameless one
    let engine = start(|_| Answer::json(200, LISTED)).await;

    // When the app's containers are listed
    let found = engine.docker().list_by_app("remote-chat-agent").await.expect("the list");

    // Then only the exact names come back, in the Engine's order, with the fields the service reads
    assert_eq!(ids(&found), ["b", "a"]);
    assert_eq!(found[1].image.as_deref(), Some("ns/remote-chat-agent:sha-1"));
    assert_eq!(found[1].status.as_deref(), Some("Up 2 hours"));
    // And the Engine was asked for every state, with no body headers
    assert_eq!(engine.calls(), ["GET /containers/json?all=true"]);
    assert_eq!(engine.seen()[0].framing(), [None, None, None]);
}

#[tokio::test]
async fn given_an_engine_that_redirects_when_a_list_is_read_then_the_redirect_is_not_followed_and_counts_as_a_refusal() {
    // Given an Engine whose list answers 307 to another path, which would list a container
    let engine = start(|seen| match seen.target.as_str() {
        "/elsewhere" => Answer::json(200, r#"[{"Id":"a","Names":["/agent-shell"]}]"#),
        _ => Answer::Raw(b"HTTP/1.1 307 Temporary Redirect\r\nlocation: /elsewhere\r\ncontent-length: 0\r\n\r\n".to_vec()),
    })
    .await;

    // When the app's containers are listed
    let error = engine.docker().list_by_app("agent-shell").await.expect_err("refused");

    // Then the 307 is the answer, as Node reads it, and only one request was made
    assert!(matches!(error, ContainersError::Refused { status: 307, .. }), "{error:?}");
    assert_eq!(engine.calls(), ["GET /containers/json?all=true"]);
}

#[tokio::test]
async fn given_an_app_with_no_known_names_when_it_is_listed_then_it_has_no_containers_but_the_engine_was_asked() {
    // Given an Engine with containers
    let engine = start(|_| Answer::json(200, LISTED)).await;

    // When the removed metro-runner's containers are listed
    let found = engine.docker().list_by_app("metro-runner").await.expect("the list");

    // Then it has none, and the list was still requested first
    assert!(found.is_empty());
    assert_eq!(engine.calls(), ["GET /containers/json?all=true"]);
}

#[tokio::test]
async fn given_an_engine_that_refuses_a_list_when_it_is_read_then_the_error_has_the_status_and_the_first_200_units_of_the_body() {
    // Given an Engine whose list answers 500, once with a short body and once with a body whose 200th unit is half a pair
    let short = start(|_| Answer::json(500, r#"{"message":"engine down"}"#)).await;
    let long = format!("{}\u{1f600}{}", "x".repeat(199), "y".repeat(50));
    let cut = start(move |_| Answer::json(500, &long)).await;

    // When the app's containers and then the running containers are listed
    let engine_down = short.docker().list_by_app("agent-shell").await.expect_err("refused");
    let running_down = short.docker().list_running().await.expect_err("refused");
    let half_a_pair = cut.docker().list_by_app("agent-shell").await.expect_err("refused");

    // Then the message is what Node's Error carried and the status is kept
    let down = r#"docker API returned 500: {"message":"engine down"}"#;
    assert_eq!((engine_down.to_string(), running_down.to_string()), (down.to_string(), down.to_string()));
    assert!(matches!(engine_down, ContainersError::Refused { status: 500, .. }));
    assert_eq!(half_a_pair.message().to_json(), format!("\"docker API returned 500: {}\\ud83d\"", "x".repeat(199)));
}

#[tokio::test]
async fn given_an_engine_that_answers_a_list_with_any_status_but_200_when_it_is_read_then_it_is_refused() {
    // Given an Engine whose list answers 204 with no body
    let engine = start(|_| Answer::json(204, "")).await;

    // When the app's containers are listed
    let error = engine.docker().list_by_app("agent-shell").await.expect_err("refused");

    // Then it is refused as Node refuses it, whatever the status
    assert_eq!(error.to_string(), "docker API returned 204: ");
    assert!(matches!(error, ContainersError::Refused { status: 204, .. }));
}

#[tokio::test]
async fn given_running_containers_when_they_are_listed_then_the_request_has_no_query_and_all_of_them_come_back() {
    // Given an Engine that lists two running containers of different apps
    let engine = start(|seen| match seen.target.as_str() {
        "/containers/json" => Answer::json(200, r#"[{"Id":"l1","Names":["/leetcode-backend"]},{"Id":"r1","Names":["/reporting-service"]}]"#),
        _ => Answer::json(404, r#"{"message":"unexpected"}"#),
    })
    .await;

    // When the running containers are listed
    let running = engine.docker().list_running().await.expect("the list");

    // Then both come back and the Engine got the bare path
    assert_eq!(ids(&running), ["l1", "r1"]);
    assert_eq!(engine.calls(), ["GET /containers/json"]);
}

#[tokio::test]
async fn given_an_attached_exec_when_it_runs_then_create_start_and_inspect_are_sent_as_node_sends_them_and_the_output_and_exit_code_come_back() {
    // Given an Engine whose exec prints a stdout and a stderr frame and exits 3
    let wire = [frame(1, b"out\n"), frame(2, b"err\n")].concat();
    let engine = start(one_exec(wire, r#"{"ID":"exec-1","Running":false,"ExitCode":3}"#)).await;
    let cmd = words(&["cat", "/workspace/vm-ledger/pool-cooldowns.json"]);

    // When the command runs attached with no environment
    let result = engine.docker().exec("as1", &cmd, &ExecOptions::default()).await.expect("ran");

    // Then both streams are joined and the exit code is the inspect's
    assert_eq!(result, ExecResult::attached(Some(3), "out\nerr\n".to_string()));
    // And the calls are create, start, inspect, with the JSON bodies byte for byte and no Env
    assert_eq!(engine.calls(), ["POST /containers/as1/exec", "POST /exec/exec-1/start", "GET /exec/exec-1/json"]);
    let seen = engine.seen();
    let create = r#"{"AttachStdout":true,"AttachStderr":true,"Cmd":["cat","/workspace/vm-ledger/pool-cooldowns.json"]}"#;
    assert_eq!((seen[0].text(), seen[1].text(), seen[2].text()), (create.to_string(), r#"{"Detach":false}"#.to_string(), String::new()));
    // And each body is framed as JSON with its exact length, never chunked, and the inspect has no body headers
    let json = |body: &Seen| [Some("application/json".to_string()), Some(body.body.len().to_string()), None];
    assert_eq!(seen[0].framing().map(|h| h.map(str::to_string)), json(&seen[0]));
    assert_eq!(seen[1].framing().map(|h| h.map(str::to_string)), json(&seen[1]));
    assert_eq!(seen[2].framing(), [None, None, None]);
}

#[tokio::test]
async fn given_an_environment_when_an_exec_runs_then_env_follows_cmd_in_the_create_body() {
    // Given an Engine for one exec
    let engine = start(one_exec(Vec::new(), r#"{"ExitCode":0}"#)).await;
    let options = ExecOptions { detach: false, env: vec!["A=1".to_string(), "B=two \"words\"".to_string()] };

    // When a command runs with two environment entries
    let result = engine.docker().exec("as1", &words(&["env"]), &options).await.expect("ran");

    // Then the create body lists them last, escaped as JSON.stringify writes them
    assert_eq!(result, ExecResult::attached(Some(0), String::new()));
    let create = r#"{"AttachStdout":true,"AttachStderr":true,"Cmd":["env"],"Env":["A=1","B=two \"words\""]}"#;
    assert_eq!(engine.seen()[0].text(), create);
}

#[tokio::test]
async fn given_a_detached_exec_when_it_runs_then_it_is_started_detached_and_never_inspected() {
    // Given an Engine that creates an exec and starts it
    let engine = start(|seen| match seen.call().as_str() {
        "POST /containers/as1/exec" => Answer::json(201, r#"{"Id":"exec-3"}"#),
        "POST /exec/exec-3/start" => Answer::frames(b""),
        other => Answer::json(404, &format!(r#"{{"message":"no rule for {other}"}}"#)),
    })
    .await;
    let options = ExecOptions { detach: true, env: Vec::new() };

    // When a command runs detached
    let result = engine.docker().exec("as1", &words(&["setsid", "sh", "-c", "job"]), &options).await.expect("started");

    // Then it returns the exec id with no exit code and no output
    assert_eq!(result, ExecResult::detached("exec-3".to_string()));
    // And nothing was attached or inspected: the create attaches no stream and the start says Detach
    assert_eq!(engine.calls(), ["POST /containers/as1/exec", "POST /exec/exec-3/start"]);
    let seen = engine.seen();
    assert_eq!(seen[0].text(), r#"{"AttachStdout":false,"AttachStderr":false,"Cmd":["setsid","sh","-c","job"]}"#);
    assert_eq!(seen[1].text(), r#"{"Detach":true}"#);
}

#[tokio::test]
async fn given_a_start_the_engine_refuses_when_a_detached_exec_runs_then_its_status_is_not_read_as_in_node() {
    // Given an Engine whose exec start answers 500
    let engine = start(|seen| match seen.call().as_str() {
        "POST /containers/as1/exec" => Answer::json(201, r#"{"Id":"exec-1"}"#),
        _ => Answer::json(500, r#"{"message":"start failed"}"#),
    })
    .await;

    // When a command runs detached
    let result = engine.docker().exec("as1", &words(&["true"]), &ExecOptions { detach: true, env: Vec::new() }).await;

    // Then the exec still counts as started
    assert_eq!(result, Ok(ExecResult::detached("exec-1".to_string())));
}

#[tokio::test]
async fn given_a_container_that_cannot_exec_when_the_create_is_refused_then_the_error_says_so_and_nothing_is_started() {
    // Given an Engine whose exec create answers 409 for an exited container, and another whose refusal is long
    let exited = start(|_| Answer::json(409, r#"{"message":"container as1 is not running"}"#)).await;
    let long = format!("{}\u{20ac}\u{1f600}{}", "x".repeat(298), "y".repeat(100));
    let wordy = start(move |_| Answer::json(500, &long)).await;

    // When a command runs in each
    let refused = exited.docker().exec("as1", &words(&["true"]), &ExecOptions::default()).await.expect_err("refused");
    let cut = wordy.docker().exec("as1", &words(&["true"]), &ExecOptions::default()).await.expect_err("refused");

    // Then the message names the create, the status and the body, as the detail of the 502 Node answers
    assert_eq!(refused.to_string(), r#"docker exec create returned 409: {"message":"container as1 is not running"}"#);
    assert!(matches!(refused, ContainersError::Refused { status: 409, .. }));
    // And the body is cut at 300 UTF-16 units, through the middle of a surrogate pair
    let expected = format!("\"docker exec create returned 500: {}\u{20ac}\\ud83d\"", "x".repeat(298));
    assert_eq!(cut.message().to_json(), expected);
    // And only the create was sent
    assert_eq!(exited.calls(), ["POST /containers/as1/exec"]);
}

#[tokio::test]
async fn given_a_create_that_answers_200_when_an_exec_runs_then_it_is_refused_because_only_201_is_a_created_exec() {
    // Given an Engine whose exec create answers 200 with an exec id
    let engine = start(|_| Answer::json(200, r#"{"Id":"exec-1"}"#)).await;

    // When a command runs
    let error = engine.docker().exec("as1", &words(&["true"]), &ExecOptions::default()).await.expect_err("refused");

    // Then it is refused with the body, and nothing was started
    assert_eq!(error.to_string(), r#"docker exec create returned 200: {"Id":"exec-1"}"#);
    assert_eq!(engine.calls(), ["POST /containers/as1/exec"]);
}

#[tokio::test]
async fn given_a_character_cut_across_two_frames_when_an_exec_prints_it_then_each_frame_is_decoded_alone() {
    // Given an exec that prints a euro sign cut after its second byte across two frames
    let wire = [frame(1, b"a\xe2\x82"), frame(1, b"\xacb")].concat();
    let engine = start(one_exec(wire, r#"{"ExitCode":0}"#)).await;

    // When it runs
    let result = engine.docker().exec("as1", &words(&["printf"]), &ExecOptions::default()).await.expect("ran");

    // Then the two halves come back as two replacement characters
    assert_eq!(result.output, "a\u{fffd}\u{fffd}b");
}

#[tokio::test]
async fn given_inspect_answers_of_every_shape_when_an_exec_ends_then_the_exit_code_is_a_number_or_none_and_unreadable_json_fails() {
    // Given Engines whose inspect answers a number, null, nothing named, a refusal and then text that is no JSON
    let cases: [(&str, u16, Option<i64>); 4] = [
        (r#"{"ExitCode":137}"#, 200, Some(137)),
        (r#"{"ExitCode":null,"Running":true}"#, 200, None),
        (r#"{"ID":"exec-1"}"#, 200, None),
        (r#"{"message":"No such exec instance"}"#, 404, None),
    ];
    for (inspect, status, expected) in cases {
        let engine = start(move |seen| match seen.call().as_str() {
            "POST /containers/as1/exec" => Answer::json(201, r#"{"Id":"exec-1"}"#),
            "POST /exec/exec-1/start" => Answer::frames(b""),
            _ => Answer::json(status, inspect),
        })
        .await;

        // When a command runs attached
        let result = engine.docker().exec("as1", &words(&["true"]), &ExecOptions::default()).await;

        // Then the exit code is the number, or none when the inspect names no number, whatever its status
        assert_eq!(result.map(|r| r.exit_code), Ok(expected), "{inspect}");
    }
    for (inspect, message) in [("oops", None), ("", Some("Unexpected end of JSON input"))] {
        let engine = start(move |seen| match seen.call().as_str() {
            "POST /containers/as1/exec" => Answer::json(201, r#"{"Id":"exec-1"}"#),
            "POST /exec/exec-1/start" => Answer::frames(b""),
            _ => Answer::json(200, inspect),
        })
        .await;

        let error = engine.docker().exec("as1", &words(&["true"]), &ExecOptions::default()).await.expect_err("unreadable");

        assert!(matches!(error, ContainersError::Failed { .. }), "{inspect:?}: {error:?}");
        assert!(message.map_or(!error.to_string().is_empty(), |text| error.to_string() == text), "{inspect:?}: {error}");
    }
}

#[tokio::test]
async fn given_a_tar_when_it_is_put_then_the_request_is_the_archive_put_node_sends_with_the_bytes_untouched() {
    // Given an Engine that takes archives, and a tar that is no UTF-8
    let engine = start(|_| Answer::json(200, "")).await;
    let tar: Vec<u8> = [vec![0xff, 0xfe, 0x00], vec![0u8; 1533]].concat();

    // When it is put into a job dir
    let reply = engine.docker().put_file("as1", "/tmp/exec-jobs/0a1b-2c", &tar).await.expect("put");

    // Then the Engine answered 200 and the reply says so
    assert_eq!(reply, EngineReply { status: 200, body: String::new() });
    // And the request is a PUT to the archive path with the dir in the query, a tar content type and the exact length
    let seen = engine.seen();
    assert_eq!(engine.calls(), ["PUT /containers/as1/archive?path=%2Ftmp%2Fexec-jobs%2F0a1b-2c"]);
    assert_eq!(seen[0].framing(), [Some("application/x-tar"), Some("1536"), None]);
    assert_eq!(seen[0].body, tar);
}

#[tokio::test]
async fn given_an_engine_that_refuses_the_archive_when_it_is_put_then_the_status_and_body_come_back_as_a_reply() {
    // Given an Engine whose archive PUT answers 500 with a message
    let engine = start(|_| Answer::json(500, "no space left on device")).await;

    // When a tar is put
    let reply = engine.docker().put_file("as1", "/workspace/home/.claude", b"tar").await;

    // Then the refusal is a reply for the caller to read, not an error
    assert_eq!(reply, Ok(EngineReply { status: 500, body: "no space left on device".to_string() }));
}

#[tokio::test]
async fn given_dirs_with_every_kind_of_character_when_they_are_put_then_the_query_is_encode_uri_component_of_the_dir() {
    // Given an Engine that takes archives
    let engine = start(|_| Answer::json(200, "")).await;
    let cases = [
        ("/workspace/home/.claude", "%2Fworkspace%2Fhome%2F.claude"),
        ("/a b/\u{fc}&=+#%?x", "%2Fa%20b%2F%C3%BC%26%3D%2B%23%25%3Fx"),
        ("!~*()._-", "!~*()._-"),
        ("\u{1f600}", "%F0%9F%98%80"),
        // The url crate under reqwest writes this one as %27 whatever it is given, so the adapter escapes it itself.
        ("it's", "it%27s"),
    ];

    // When a tar is put into each
    for (dir, _) in cases {
        engine.docker().put_file("as1", dir, b"t").await.expect("put");
    }

    // Then each request's query is the dir as JavaScript's encodeURIComponent writes it
    let expected: Vec<String> = cases.iter().map(|(_, encoded)| format!("PUT /containers/as1/archive?path={encoded}")).collect();
    assert_eq!(engine.calls(), expected);
}

#[tokio::test]
async fn given_a_containers_logs_when_the_snapshot_is_read_then_the_frames_are_joined_and_the_request_names_both_streams_and_the_tail() {
    // Given an Engine whose logs hold a stdout and a stderr frame
    let wire = [frame(1, b"started\n"), frame(2, b"warn: slow\n")].concat();
    let engine = start(move |_| Answer::frames(&wire)).await;

    // When the snapshot is read with a tail of 5
    let text = engine.docker().logs("l1", 5).await.expect("logs");

    // Then the text is the frames joined and the request is Node's
    assert_eq!(text, "started\nwarn: slow\n");
    assert_eq!(engine.calls(), ["GET /containers/l1/logs?stdout=true&stderr=true&tail=5"]);
    assert_eq!(engine.seen()[0].framing(), [None, None, None]);
}

#[tokio::test]
async fn given_an_engine_that_refuses_the_logs_when_the_snapshot_is_read_then_the_error_carries_the_status() {
    // Given Engines whose logs answer 500 and 404
    for status in [500, 404] {
        let engine = start(move |_| Answer::json(status, r#"{"message":"logs failed"}"#)).await;

        // When the snapshot is read
        let error = engine.docker().logs("l2", 200).await.expect_err("refused");

        // Then the status is the one a reply prints as <docker API returned 500>
        assert!(matches!(error, ContainersError::Refused { status: got, .. } if got == status), "{error:?}");
    }
}

async fn texts_until_the_end(follow: &mut Box<dyn LogFollow>) -> Vec<String> {
    let mut texts = Vec::new();
    while let Some(text) = tokio::time::timeout(ASSERTED_WITHIN, follow.next_text()).await.expect("a text within 5 s").expect("no error") {
        texts.push(text);
    }
    texts
}

#[tokio::test]
async fn given_a_follow_stream_cut_anywhere_when_it_is_read_then_whole_frames_come_out_and_it_ends_when_the_engine_ends_it() {
    // Given a stream that cuts a frame's header across two chunks and a three-byte character inside a frame across the next two
    let wire = [frame(1, b"line one\n"), frame(2, "line two \u{20ac}\n".as_bytes())].concat();
    let euro = wire.len() - 3;
    let chunks = vec![wire[..5].to_vec(), wire[5..euro].to_vec(), wire[euro..].to_vec()];
    let engine = start(move |_| Answer::stream(chunks.clone(), StreamEnd::Finish)).await;

    // When the logs are followed with a tail of 5 and read to the end
    let mut follow = engine.docker().follow_logs("l1", 5).await.expect("the stream opens");
    let texts = texts_until_the_end(&mut follow).await;

    // Then each frame's text is whole, and the stream ended with the Engine's
    assert_eq!(texts, ["line one\n", "line two \u{20ac}\n"]);
    // And the Engine was asked to follow both streams from the tail
    assert_eq!(engine.calls(), ["GET /containers/l1/logs?follow=true&stdout=true&stderr=true&tail=5"]);
    assert_eq!(engine.seen()[0].framing(), [None, None, None]);
}

#[tokio::test]
async fn given_an_engine_that_refuses_a_follow_when_it_is_opened_then_the_error_is_the_node_text_and_no_stream_exists() {
    // Given Engines whose follow answers 500 and 404
    for (status, body) in [(500, r#"{"message":"logs failed"}"#), (404, r#"{"message":"No such container: r1"}"#)] {
        let engine = start(move |_| Answer::json(status, body)).await;

        // When the logs are followed
        let refused = engine.docker().follow_logs("r1", 200).await.err().expect("refused");

        // Then the error is the line the text ends with
        assert_eq!(refused.to_string(), format!("docker API returned {status}: {body}"));
        assert!(matches!(refused, ContainersError::Refused { status: got, .. } if got == status), "{refused:?}");
    }
}

#[tokio::test]
async fn given_a_stream_the_engine_drops_when_it_is_read_then_the_text_that_arrived_comes_first_and_then_an_error() {
    // Given a stream that sends one frame and drops the connection with the body unfinished
    let engine = start(|_| Answer::stream(vec![frame(1, b"partial\n")], StreamEnd::Cut)).await;
    let mut follow = engine.docker().follow_logs("r4", 200).await.expect("the stream opens");

    // When it is read
    let first = follow.next_text().await;
    let second = follow.next_text().await;
    let third = follow.next_text().await;

    // Then the frame arrives, then the failure Node calls aborted, then the stream is over
    assert_eq!(first, Ok(Some("partial\n".to_string())));
    assert_eq!(second, Err(ContainersError::failed("aborted")));
    assert_eq!(third, Ok(None));
}

#[tokio::test]
async fn given_an_open_follow_when_it_is_dropped_then_the_connection_to_the_engine_is_closed() {
    // Given a stream that stays open after its first frame
    let engine = start(|_| Answer::stream(vec![frame(1, b"one\n")], StreamEnd::HoldOpen)).await;
    let mut follow = engine.docker().follow_logs("l1", 200).await.expect("the stream opens");
    assert_eq!(follow.next_text().await, Ok(Some("one\n".to_string())));

    // When the caller drops it, as a client's disconnect does
    drop(follow);

    // Then the Engine sees its client close
    assert_eq!(engine.closed_by_client().await, 1);
}

#[tokio::test]
async fn given_a_containers_stats_when_they_are_sampled_then_the_one_shot_request_is_sent_and_the_fields_the_service_reads_come_back() {
    // Given an Engine with a sample of a container that used 3 MiB of which 1 MiB is cache, and twice the CPU time of before
    let sample = r#"{"memory_stats":{"usage":3145728,"limit":1073741824,"stats":{"inactive_file":1048576}},
        "cpu_stats":{"cpu_usage":{"total_usage":200},"system_cpu_usage":1000,"online_cpus":2},
        "precpu_stats":{"cpu_usage":{"total_usage":100},"system_cpu_usage":500}}"#;
    let engine = start(move |_| Answer::json(200, sample)).await;

    // When it is sampled
    let stats = engine.docker().stats("s1").await.expect("a sample");

    // Then the working set and the CPU share are those of the sample
    assert_eq!(working_set_bytes(&stats), 2_097_152.0);
    assert_eq!(cpu_percent(&stats), 40.0);
    // And the request is the one-shot one, with nothing else in its query
    assert_eq!(engine.calls(), ["GET /containers/s1/stats?stream=false"]);
}

#[tokio::test]
async fn given_an_engine_that_refuses_the_stats_when_they_are_sampled_then_the_error_carries_the_status() {
    // Given an Engine with no such container
    let engine = start(|_| Answer::json(404, r#"{"message":"No such container: s9"}"#)).await;

    // When it is sampled
    let error = engine.docker().stats("s9").await.expect_err("refused");

    // Then it is the Engine's 404, which a caller skips
    assert!(matches!(error, ContainersError::Refused { status: 404, .. }), "{error:?}");
    assert_eq!(error.to_string(), r#"docker API returned 404: {"message":"No such container: s9"}"#);
}

#[tokio::test]
async fn given_no_engine_on_the_socket_when_a_call_is_made_then_it_fails_with_the_words_node_uses_for_the_connection() {
    // Given a socket path with nothing there, and a socket file whose listener is gone
    let dir = tempfile::tempdir().expect("a temp dir");
    let missing = dir.path().join("missing.sock");
    let refused = dir.path().join("refused.sock");
    drop(UnixListener::bind(&refused).expect("a socket file"));

    // When a list is read from each
    let from_missing = DockerEngine::new(&missing).expect("a client").list_running().await.expect_err("unreachable");
    let from_refused = DockerEngine::new(&refused).expect("a client").list_running().await.expect_err("unreachable");

    // Then each says what Node says: connect, the code and the path
    assert_eq!(from_missing.to_string(), format!("connect ENOENT {}", missing.display()));
    assert_eq!(from_refused.to_string(), format!("connect ECONNREFUSED {}", refused.display()));
    assert!(matches!(from_missing, ContainersError::Failed { .. }));
}

#[tokio::test]
async fn given_an_engine_that_closes_without_answering_when_a_call_is_made_then_it_fails_as_a_socket_hang_up() {
    // Given an Engine that reads the request and hangs up
    let engine = start(|_| Answer::Hangup).await;

    // When a list is read
    let error = engine.docker().list_running().await.expect_err("hung up");

    // Then the message is Node's
    assert_eq!(error, ContainersError::failed("socket hang up"));
    assert_eq!(engine.calls(), ["GET /containers/json"]);
}

#[tokio::test(start_paused = true)]
async fn given_an_exec_that_ends_after_31_seconds_when_it_runs_then_it_is_waited_for_with_no_time_limit() {
    // Given an Engine whose exec start answers only after 31 s, one more than the reporting service waits
    let engine = start(|seen| match seen.call().as_str() {
        "POST /containers/as1/exec" => Answer::json(201, r#"{"Id":"exec-1"}"#),
        "POST /exec/exec-1/start" => Answer::After(Duration::from_secs(31), Box::new(Answer::frames(&frame(1, b"done\n")))),
        _ => Answer::json(200, r#"{"ExitCode":0}"#),
    })
    .await;
    let began = tokio::time::Instant::now();

    // When the command runs attached
    let result = engine.docker().exec("as1", &words(&["sleep", "31"]), &ExecOptions::default()).await;

    // Then it returns the output after the whole wait
    assert_eq!(result, Ok(ExecResult::attached(Some(0), "done\n".to_string())));
    assert!(began.elapsed() >= Duration::from_secs(31), "{:?}", began.elapsed());
}

#[tokio::test(start_paused = true)]
async fn given_a_follow_that_starts_to_answer_after_31_seconds_when_it_is_opened_then_it_is_waited_for_with_no_time_limit() {
    // Given an Engine whose follow stream begins only after 31 s
    let stream = Answer::stream(vec![frame(1, b"late\n")], StreamEnd::Finish);
    let engine = start(move |_| Answer::After(Duration::from_secs(31), Box::new(stream.clone()))).await;
    let began = tokio::time::Instant::now();

    // When the logs are followed
    let mut follow = engine.docker().follow_logs("l1", 1).await.expect("the stream opens");

    // Then the text arrives after the whole wait
    assert_eq!(follow.next_text().await, Ok(Some("late\n".to_string())));
    assert!(began.elapsed() >= Duration::from_secs(31), "{:?}", began.elapsed());
}
