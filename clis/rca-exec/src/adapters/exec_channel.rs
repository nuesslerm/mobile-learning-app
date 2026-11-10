//! The executor channel client (`docs/plans/rca-client/protocol.md`): connects to rca `GET /exec/v2/executors?workspace=<id>` as a
//! WebSocket client (plain ws:// over loopback or the tailnet, Bearer header), runs each request frame on a thread pool against a
//! `ToolExecutor` and writes the stream and result frames back. One I/O thread owns the socket; workers hand it frames.
use serde_json::Value;
use std::collections::HashSet;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use crate::domain::{execute, execute_binary, ToolExecutor};
use exec_proto::{error_frame, output_frame, result_frame, subscribe_frame, Request, Stream, SubscribeFrame};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::CloseFrame;
use tungstenite::{Error as WsError, Message, WebSocket};

pub const POOL: usize = 8;
const POLL: Duration = Duration::from_millis(15);

/// A message toward rca: a JSON frame as text, or a pty output frame as bytes.
enum Out {
    Text(String),
    Binary(Vec<u8>),
}

struct Job {
    id: String,
    tool: String,
    args: Value,
}

/// An open channel. Dropping it closes the socket; `wait` blocks until rca (or `close`) ends it.
pub struct ExecChannel {
    stop: Arc<AtomicBool>,
    io: Option<JoinHandle<Result<Option<u16>, String>>>,
    live: Arc<AtomicBool>,
}

fn encode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// `url` is `http://host:port` or `ws://host:port`. The executor id is sent only when it differs from the workspace: rca reads a channel
/// that names none as the executor of that one workspace.
fn ws_url(url: &str, executor: Option<&str>, workspace: &str) -> Result<(String, String), String> {
    let authority = url.strip_prefix("http://").or_else(|| url.strip_prefix("ws://")).map(|r| r.trim_end_matches('/')).filter(|a| !a.is_empty() && !a.contains('/'));
    let authority = authority.ok_or_else(|| format!("rca url {url} must be http://host:port or ws://host:port"))?;
    let executor = executor.map_or(String::new(), |id| format!("&executor={}", encode(id)));
    Ok((authority.to_string(), format!("ws://{authority}/exec/v2/executors?workspace={}{executor}", encode(workspace))))
}

impl ExecChannel {
    pub fn open(url: &str, workspace: &str, bearer: &str, executor: Arc<dyn ToolExecutor>) -> Result<ExecChannel, String> {
        ExecChannel::open_as(url, None, workspace, bearer, executor)
    }

    /// Opens the channel of the executor `id` (or of the one a workspace names, with none), whose root workspace is `workspace`.
    pub fn open_as(url: &str, id: Option<&str>, workspace: &str, bearer: &str, executor: Arc<dyn ToolExecutor>) -> Result<ExecChannel, String> {
        let (authority, ws) = ws_url(url, id, workspace)?;
        let stream = TcpStream::connect(&authority).map_err(|e| format!("channel connect {authority}: {e}"))?;
        let mut request = ws.as_str().into_client_request().map_err(|e| e.to_string())?;
        request.headers_mut().insert("Authorization", HeaderValue::from_str(&format!("Bearer {bearer}")).map_err(|e| e.to_string())?);
        let (socket, _) = tungstenite::client(request, stream).map_err(|e| format!("channel handshake: {e}"))?;
        socket.get_ref().set_read_timeout(Some(POLL)).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let live = Arc::new(AtomicBool::new(true));
        let (stop2, live2) = (stop.clone(), live.clone());
        let io = std::thread::spawn(move || {
            let r = serve(socket, executor, &stop2);
            live2.store(false, Ordering::SeqCst);
            r
        });
        Ok(ExecChannel { stop, io: Some(io), live })
    }

    pub fn is_open(&self) -> bool {
        self.live.load(Ordering::SeqCst)
    }

    /// Blocks until the channel ends; the close code rca sent, if any.
    pub fn wait(mut self) -> Result<Option<u16>, String> {
        self.io.take().map_or(Ok(None), |h| h.join().map_err(|_| "channel thread panicked".to_string())?)
    }

    /// Closes from this side and waits.
    pub fn close(self) -> Result<Option<u16>, String> {
        self.stop.store(true, Ordering::SeqCst);
        self.wait()
    }
}

impl Drop for ExecChannel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.io.take() {
            let _ = h.join();
        }
    }
}

fn run_job(job: &Job, ex: &dyn ToolExecutor, out: &Sender<Out>, inflight: &Mutex<HashSet<String>>) {
    let mut stream = |s: Stream, data: &str| {
        let _ = out.send(Out::Text(output_frame(&job.id, s, data)));
    };
    let mut seq = 0;
    let mut events = |event: Value| {
        seq += 1;
        let _ = out.send(Out::Text(subscribe_frame(&SubscribeFrame { id: job.id.clone(), seq, event })));
    };
    let mut binary = |frame: Vec<u8>| {
        let _ = out.send(Out::Binary(frame));
    };
    let frame = match execute_binary(ex, &job.id, &job.tool, &job.args, &mut stream, &mut events, &mut binary) {
        Ok(result) => result_frame(&job.id, &result),
        Err(e) => error_frame(&job.id, e.code.as_str(), &e.message),
    };
    inflight.lock().unwrap().remove(&job.id);
    let _ = out.send(Out::Text(frame));
}

fn worker(jobs: Arc<Mutex<Receiver<Job>>>, ex: Arc<dyn ToolExecutor>, out: Sender<Out>, inflight: Arc<Mutex<HashSet<String>>>) {
    loop {
        let job = match jobs.lock().unwrap().recv() {
            Ok(j) => j,
            Err(_) => return,
        };
        if !inflight.lock().unwrap().contains(&job.id) {
            continue;
        }
        run_job(&job, ex.as_ref(), &out, &inflight);
    }
}

fn serve(mut socket: WebSocket<TcpStream>, ex: Arc<dyn ToolExecutor>, stop: &AtomicBool) -> Result<Option<u16>, String> {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (out_tx, out_rx) = mpsc::channel::<Out>();
    let mut longs: Vec<JoinHandle<()>> = Vec::new();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let inflight = Arc::new(Mutex::new(HashSet::<String>::new()));
    let workers: Vec<_> = (0..POOL).map(|_| std::thread::spawn({ let (j, e, o, i) = (job_rx.clone(), ex.clone(), out_tx.clone(), inflight.clone()); move || worker(j, e, o, i) })).collect();
    let mut close_code = None;
    let mut closing = false;
    let mut closing_since = Instant::now();
    let outcome: Result<(), String> = loop {
        while let Ok(frame) = out_rx.try_recv() {
            let message = match frame {
                Out::Text(text) => Message::text(text),
                Out::Binary(bytes) => Message::binary(bytes),
            };
            if let Err(e) = socket.send(message) {
                if !matches!(e, WsError::ConnectionClosed | WsError::AlreadyClosed) {
                    break;
                }
            }
        }
        if stop.load(Ordering::SeqCst) {
            if !closing {
                closing = true;
                closing_since = Instant::now();
                let _ = socket.close(Some(CloseFrame { code: CloseCode::Normal, reason: "".into() }));
            } else if closing_since.elapsed() > Duration::from_secs(2) {
                break Ok(());
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => match Request::parse(text.as_str()) {
                Ok(Request { id, tool, args }) => {
                    // Answered here, not by a worker: every worker may be busy with the command being cancelled.
                    if tool == exec_proc::CANCEL {
                        let _ = out_tx.send(Out::Text(match execute(ex.as_ref(), &id, &tool, &args, &mut |_, _| {}, &mut |_| {}) {
                            Ok(result) => result_frame(&id, &result),
                            Err(e) => error_frame(&id, e.code.as_str(), &e.message),
                        }));
                    } else if exec_pty::LONG.contains(&tool.as_str()) || exec_git::LONG.contains(&tool.as_str()) || exec_hosted::LONG.contains(&tool.as_str()) || exec_preview::LONG.contains(&tool.as_str()) {
                        // A stream that stays open takes a thread of its own: a pool worker held by each would starve the other requests.
                        inflight.lock().unwrap().insert(id.clone());
                        longs.push(std::thread::spawn({ let (ex, out, inflight) = (ex.clone(), out_tx.clone(), inflight.clone()); move || run_job(&Job { id, tool, args }, ex.as_ref(), &out, &inflight) }));
                    } else {
                        inflight.lock().unwrap().insert(id.clone());
                        let _ = job_tx.send(Job { id, tool, args });
                    }
                }
                Err(e) => {
                    let _ = out_tx.send(Out::Text(error_frame("", e.code.as_str(), &e.message)));
                }
            },
            Ok(Message::Close(frame)) => {
                close_code = frame.map(|f| u16::from(f.code));
                // The close reply is flushed and the socket dropped: a server closes the TCP connection first only if it chooses to.
                let _ = socket.flush();
                break Ok(());
            }
            Ok(_) => {}
            Err(WsError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => break Ok(()),
            Err(e) => break Err(e.to_string()),
        }
    };
    // The channel is gone: whatever still runs for it is stopped, then the workers drain and exit.
    let pending: Vec<String> = inflight.lock().unwrap().drain().collect();
    for id in &pending {
        ex.cancel(id);
    }
    drop(job_tx);
    for w in workers.into_iter().chain(longs) {
        let _ = w.join();
    }
    outcome.map(|()| close_code)
}
