//! The lifecycle of an agent API socket: requests in, one answer frame for each, and the events of the streams a request opened pushed as they
//! happen. The frames of both leave through one queue, so the order they were made in is the order the peer reads them, and a peer that stops
//! reading is closed (it resumes from the last `seq` it saw) instead of being waited on. A replay the peer asked for is the exception: it
//! waits for room, up to a share of the queue, so a long backlog is no stall and an answer still finds a place.
use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use rca_agent_core::{AgentApi, Build, Connection, EventSink};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// How long the frames still queued when a socket ends may take to be written.
const CLOSE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The frames a socket may have waiting to be written before its peer counts as not reading.
pub const QUEUE: usize = 4096;

/// A frame waiting to be written. A replayed one holds a place of the replay's share until the writer has taken it.
struct Frame {
    text: String,
    _place: Option<OwnedSemaphorePermit>,
}

/// The queue of a socket's frames as the agent API's `EventSink`.
struct Queue {
    frames: mpsc::Sender<Frame>,
    stalled: CancellationToken,
    replay: Arc<Semaphore>,
}

impl Queue {
    fn new(capacity: usize) -> (Queue, mpsc::Receiver<Frame>) {
        let (frames, queued) = mpsc::channel(capacity);
        // A quarter of the queue is never taken by a replay: that is where the answers and the live events of a socket mid-replay go
        let replay = Arc::new(Semaphore::new(capacity - capacity / 4));
        (Queue { frames, stalled: CancellationToken::new(), replay }, queued)
    }

    /// Queues a frame, or marks the socket stalled when the queue is full or closed: false then, which stops whoever sends.
    fn push(&self, text: String) -> bool {
        self.enqueue(Frame { text, _place: None })
    }

    fn enqueue(&self, frame: Frame) -> bool {
        let queued = self.frames.try_send(frame).is_ok();
        if !queued {
            self.stalled.cancel();
        }
        queued
    }
}

#[async_trait]
impl EventSink for Queue {
    fn send(&self, event: &agent_api_proto::Event) -> bool {
        self.push(event.encode())
    }

    async fn send_replay(&self, event: &agent_api_proto::Event) -> bool {
        let Ok(place) = Arc::clone(&self.replay).acquire_owned().await else { return false };
        self.enqueue(Frame { text: event.encode(), _place: Some(place) })
    }
}

/// The agent API socket of the service.
pub struct AgentSockets;

#[async_trait]
impl crate::ports::AgentSocketPort for AgentSockets {
    async fn serve(&self, api: &AgentApi, build: Build<'_>, socket: WebSocket) {
        serve(api, build, socket).await;
    }
}

/// Serves `socket` until it closes: each text frame is a request answered by `api`, and each stream a request opens runs until the socket
/// ends or a newer stream of the same topic replaces it.
pub async fn serve(api: &AgentApi, build: Build<'_>, socket: WebSocket) {
    serve_with(api, build, socket, QUEUE).await;
}

pub async fn serve_with(api: &AgentApi, build: Build<'_>, socket: WebSocket, capacity: usize) {
    let (mut write, mut read) = socket.split();
    let (queue, mut queued) = Queue::new(capacity);
    let stalled = queue.stalled.clone();
    let queue = Arc::new(queue);
    let connection = Connection::new(queue.clone());
    let writer = tokio::spawn(async move {
        while let Some(frame) = queued.recv().await {
            if write.send(Message::Text(frame.text.into())).await.is_err() {
                break;
            }
        }
    });
    let mut streams: HashMap<String, JoinHandle<()>> = HashMap::new();
    loop {
        tokio::select! {
            message = read.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let answer = api.dispatch_on(build, &text, &connection).await;
                    if !queue.push(answer.to_string()) {
                        break;
                    }
                    // Started after the answer is queued, so the peer reads the answer to a subscribe before the first event of it
                    for stream in connection.take_following() {
                        if let Some(replaced) = streams.insert(stream.topic, tokio::spawn(stream.job)) {
                            replaced.abort();
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            () = stalled.cancelled() => {
                tracing::warn!(event = "agent_socket.stalled", queued = capacity);
                break;
            }
        }
    }
    for (_, stream) in streams {
        stream.abort();
    }
    drop(connection);
    drop(queue);
    // The frames still queued are written before the socket closes, unless the peer is not reading them
    let mut writer = writer;
    if tokio::time::timeout(CLOSE_WAIT, &mut writer).await.is_err() {
        writer.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn event(seq: u64) -> agent_api_proto::Event {
        agent_api_proto::Event { seq, thread: Some("t".into()), turn: None, kind: "turn.started".into(), data: Default::default() }
    }

    #[test]
    fn given_a_peer_that_does_not_read_when_its_queue_is_full_then_the_next_frame_is_refused_and_the_socket_is_marked_stalled_and_one_that_reads_is_not() {
        // Given a socket whose queue holds two frames, and its writer that has not taken any
        let (queue, mut writer) = Queue::new(2);
        // When three events are sent
        let sent: Vec<bool> = (1..=3).map(|seq| queue.send(&event(seq))).collect();
        // Then the third is refused and the socket is stalled, so it is closed and the peer resumes from the last seq it read
        assert_eq!(sent, [true, true, false]);
        assert!(queue.stalled.is_cancelled());
        // And a peer that keeps reading is never stalled: the frames leave in the order they were queued
        let (healthy, mut reading) = Queue::new(2);
        for seq in 1..=10 {
            assert!(healthy.send(&event(seq)));
            assert_eq!(reading.try_recv().unwrap().text, event(seq).encode());
        }
        assert!(!healthy.stalled.is_cancelled());
        assert_eq!(writer.try_recv().unwrap().text, event(1).encode());
    }

    #[tokio::test]
    async fn given_a_replay_longer_than_the_queue_when_the_peer_reads_then_every_event_arrives_in_order_and_the_socket_is_not_stalled() {
        // Given a queue of eight frames and a replay of fifty events, sent while the writer reads one frame every millisecond
        let (queue, mut writer) = Queue::new(8);
        let queue = Arc::new(queue);
        let sink = queue.clone();
        let replay = tokio::spawn(async move {
            for seq in 1..=50 {
                assert!(sink.send_replay(&event(seq)).await);
            }
        });
        let mut read = Vec::new();
        // When the peer reads them all
        while read.len() < 50 {
            let frame = tokio::time::timeout(Duration::from_secs(5), writer.recv()).await.expect("the replay goes on while the peer reads").unwrap();
            read.push(frame.text);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        replay.await.unwrap();
        // Then they came in order, and the socket was never marked stalled
        assert_eq!(read, (1..=50).map(|seq| event(seq).encode()).collect::<Vec<_>>());
        assert!(!queue.stalled.is_cancelled());
    }

    #[tokio::test]
    async fn given_a_replay_that_has_taken_its_share_of_the_queue_when_an_answer_is_queued_then_it_finds_a_place_and_the_replay_waits_for_the_peer() {
        // Given a queue of eight frames, a replay that has taken all the places it may, and a peer that has not read any
        let (queue, mut writer) = Queue::new(8);
        let queue = Arc::new(queue);
        let sink = queue.clone();
        let replay = tokio::spawn(async move {
            for seq in 1..=20 {
                if !sink.send_replay(&event(seq)).await {
                    return seq;
                }
            }
            0
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!replay.is_finished(), "the replay waits for the peer instead of giving up");
        // When an answer is queued meanwhile
        let answered = queue.push("answer".into());
        // Then it is queued, the socket is not stalled, and once the peer reads the replay goes on to its end
        assert!(answered && !queue.stalled.is_cancelled());
        let mut read = Vec::new();
        while read.len() < 21 {
            read.push(tokio::time::timeout(Duration::from_secs(5), writer.recv()).await.expect("a frame").unwrap().text);
        }
        assert_eq!(replay.await.unwrap(), 0);
        assert_eq!(read.iter().filter(|text| text.as_str() == "answer").count(), 1);
        assert_eq!(read.iter().filter(|text| text.as_str() != "answer").cloned().collect::<Vec<_>>(), (1..=20).map(|seq| event(seq).encode()).collect::<Vec<_>>());
    }
}
