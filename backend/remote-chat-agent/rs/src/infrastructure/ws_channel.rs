//! The executor channel over a WebSocket: the transport adapter of `rca_exec_channel`'s `FrameSink` and the loop that feeds the socket's frames
//! to the `Channel` until it closes. What a turn does when its executor's channel closes is the `agent.threads` group's.

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use rca_exec_channel::remote::{Channel, ExecutorChannel, FrameSink};
use rca_exec_channel::{Registry, WorkspaceError};
use std::sync::Arc;
use tokio::sync::mpsc;

enum Out {
    Text(String),
    Close(u16),
}

/// The WebSocket as the channel's transport: frames queue to a writer task that owns the socket's write half.
struct WsSink(mpsc::UnboundedSender<Out>);

impl FrameSink for WsSink {
    fn send(&self, frame: String) -> bool {
        self.0.send(Out::Text(frame)).is_ok()
    }

    fn close(&self, code: u16) {
        let _ = self.0.send(Out::Close(code));
    }
}

/// The executor sockets of the service: each upgraded socket becomes the channel of its executor in the registry, until the socket closes.
#[derive(Clone)]
pub struct ExecutorSockets {
    executors: Arc<Registry>,
}

impl crate::ports::ExecutorSocketPort for ExecutorSockets {
    fn accept(&self, upgrade: WebSocketUpgrade, executor: String, workspace: String) -> Result<Response, WorkspaceError> {
        ExecutorSockets::accept(self, upgrade, executor, workspace)
    }
}

impl ExecutorSockets {
    pub fn new(executors: Arc<Registry>) -> Self {
        Self { executors }
    }

    /// Registers the channel before the 101 goes out, so a caller that has seen the handshake finish finds it; frames queue until the socket is served.
    /// `executor` is the registry key, `workspace` the executor's root workspace. A workspace another executor owns is refused as `Taken`
    /// before the upgrade, so the older executor keeps serving it.
    pub fn accept(&self, upgrade: WebSocketUpgrade, executor: String, workspace: String) -> Result<Response, WorkspaceError> {
        let (out, queued) = mpsc::unbounded_channel::<Out>();
        let channel = Channel::new(Arc::new(WsSink(out)));
        let open: Arc<dyn ExecutorChannel> = channel.clone();
        self.executors.open_executor(&executor, &workspace, Arc::clone(&open))?;
        tracing::info!(event = "exec.channel.opened", executor = %executor, workspace = %workspace);
        let executors = Arc::clone(&self.executors);
        Ok(upgrade.on_upgrade(move |socket| serve_channel(executors, executor, socket, channel, open, queued)))
    }
}

async fn serve_channel(executors: Arc<Registry>, executor: String, socket: WebSocket, channel: Arc<Channel>, open: Arc<dyn ExecutorChannel>, mut queued: mpsc::UnboundedReceiver<Out>) {
    let (mut write, mut read) = socket.split();
    tokio::spawn(async move {
        while let Some(next) = queued.recv().await {
            let sent = match next {
                Out::Text(text) => write.send(Message::Text(text.into())).await.is_ok(),
                Out::Close(code) => {
                    let _ = write.send(Message::Close(Some(CloseFrame { code, reason: "replaced by a newer channel for this executor".into() }))).await;
                    false
                }
            };
            if !sent {
                break;
            }
        }
    });
    while let Some(Ok(message)) = read.next().await {
        match message {
            Message::Text(text) => channel.on_frame(&text),
            Message::Binary(bytes) => channel.on_binary(&bytes),
            Message::Close(_) => break,
            _ => {}
        }
    }
    channel.shutdown();
    executors.release(&executor, &open);
    tracing::info!(event = "exec.channel.closed", executor = %executor);
}
