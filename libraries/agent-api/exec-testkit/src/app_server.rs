//! The scripted `codex app-server`'s conversation: the frames of a recording or of a `Run`, played as the client's messages arrive. The recordings
//! are those of `transcripts/codex-app-server` (Codex 0.156.1 and 0.157.0, as another client's run of the real server wrote them).
use crate::script::Frame;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// What the scripted server does after a message of the client's.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    Line(String),
    Sleep(u64),
    Exit(i32),
}

/// A conversation being played.
pub struct Player {
    frames: Vec<Frame>,
    at: usize,
    last_request: Option<Value>,
}

fn has_method(v: &Value) -> bool {
    v.get("method").is_some_and(|m| !m.is_null())
}

impl Player {
    pub fn new(frames: Vec<Frame>) -> Player {
        Player { frames, at: 0, last_request: None }
    }

    /// What the server says before the client has said anything (what precedes the first `Expect`).
    pub fn begin(&mut self) -> Vec<Output> {
        self.say()
    }

    /// The server's side of the next message of the client's: what it waited for is checked, then what the script says up to the next wait is returned.
    pub fn on_client(&mut self, message: &Value) -> Result<Vec<Output>, String> {
        match self.frames.get(self.at) {
            Some(Frame::Expect { method }) => {
                let got = message["method"].as_str().filter(|_| has_method(message));
                if got != method.as_deref() {
                    return Err(format!("expected {}, got {message}", method.as_deref().map_or("an answer to a request of the server's".to_string(), |m| format!("a {m}"))));
                }
                if got.is_some() && message.get("id").is_some() {
                    self.last_request = message.get("id").cloned();
                }
                self.at += 1;
            }
            Some(other) => return Err(format!("the script was to say {other:?} before the client sent {message}")),
            None => return Err(format!("the script is over, and the client sent {message}")),
        }
        Ok(self.say())
    }

    fn say(&mut self) -> Vec<Output> {
        let mut out = Vec::new();
        while let Some(frame) = self.frames.get(self.at) {
            match frame {
                Frame::Expect { .. } => break,
                Frame::Emit { frame } => out.push(Output::Line(self.addressed(frame).to_string())),
                Frame::Sleep { ms } => out.push(Output::Sleep(*ms)),
                Frame::Exit { code } => {
                    out.push(Output::Exit(*code));
                    self.at = self.frames.len();
                    return out;
                }
            }
            self.at += 1;
        }
        out
    }

    /// A response goes out under the id of the request it answers.
    fn addressed(&self, frame: &Value) -> Value {
        let mut frame = frame.clone();
        let response = !has_method(&frame) && (frame.get("result").is_some() || frame.get("error").is_some());
        if let (true, Some(id)) = (response, &self.last_request) {
            frame["id"] = id.clone();
        }
        frame
    }
}

/// The directory of the recordings of Codex's app-server.
pub fn transcripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("transcripts/codex-app-server")
}

/// The frames of a recording in the form `{type: expect_outbound | emit_inbound, frame}` per line, as the client's expectations and the server's
/// emissions; its header and its end carry nothing to play.
pub fn frames_of(ndjson: &str) -> Vec<Frame> {
    ndjson
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| match entry["type"].as_str()? {
            "expect_outbound" => Some(Frame::Expect { method: entry["frame"]["method"].as_str().map(str::to_string) }),
            "emit_inbound" => Some(Frame::Emit { frame: entry["frame"].clone() }),
            _ => None,
        })
        .collect()
}

/// The recording `name` (`message_steering`, `turn_interrupt`, ...), its frames.
pub fn transcript(name: &str) -> Vec<Frame> {
    let path = transcripts_dir().join(format!("{name}.ndjson"));
    frames_of(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// The recordings, by name.
pub fn transcript_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(transcripts_dir()).into_iter().flatten().flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned())).collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Run;
    use serde_json::json;

    #[test]
    fn given_a_run_when_the_client_sends_what_it_waits_for_then_the_server_says_the_next_frames_under_the_clients_request_ids_and_a_wrong_message_is_refused() {
        // Given a run that answers an initialize and waits for a thread/start
        let mut player = Player::new(Run::new().handshake().thread_start("th-1").0);
        assert!(player.begin().is_empty());
        // When the client numbers its requests 41 and 42
        let init = player.on_client(&json!({"id": 41, "method": "initialize", "params": {}})).unwrap();
        let none = player.on_client(&json!({"method": "initialized"})).unwrap();
        let start = player.on_client(&json!({"id": 42, "method": "thread/start", "params": {}})).unwrap();
        // Then each answer carries the id of the request it answers, and the notification of the client says nothing
        let id_of = |out: &[Output]| match &out[0] {
            Output::Line(line) => serde_json::from_str::<Value>(line).unwrap()["id"].clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!((id_of(&init), none.len(), id_of(&start)), (json!(41), 0, json!(42)));
        // And a message the script does not wait for is refused with what it waited for
        let wrong = Player::new(Run::new().expect("turn/start").0).on_client(&json!({"id": 1, "method": "thread/start"}));
        assert!(wrong.unwrap_err().contains("expected a turn/start"));
        let over = Player::new(vec![]).on_client(&json!({"id": 1, "method": "x"}));
        assert!(over.unwrap_err().contains("the script is over"));
    }

    #[test]
    fn given_a_recording_when_its_frames_are_read_then_the_clients_requests_are_what_the_server_waits_for_and_a_server_request_is_answered_by_an_answer() {
        // Given the recorded run in which Codex asks to approve a command
        let frames = transcript("tool_call_read_only_on_request");
        // Then it waits for the handshake and a thread and a turn, asks for the approval under its own id 0, and waits for the client's answer to it
        let waits: Vec<Option<&str>> = frames.iter().filter_map(|f| if let Frame::Expect { method } = f { Some(method.as_deref()) } else { None }).collect();
        assert_eq!(waits, [Some("initialize"), Some("initialized"), Some("thread/start"), Some("turn/start"), None]);
        let asked = frames.iter().position(|f| matches!(f, Frame::Emit { frame } if frame["method"] == "item/commandExecution/requestApproval")).unwrap();
        assert!(matches!(&frames[asked], Frame::Emit { frame } if frame["id"] == 0));
        assert_eq!(frames[asked + 1], Frame::Expect { method: None });
        // And every recording of the set has frames to play
        assert!(transcript_names().len() >= 4 && transcript_names().iter().all(|n| !transcript(n).is_empty()));
    }

    #[test]
    fn given_a_run_that_ends_in_a_crash_when_it_is_played_then_the_exit_comes_after_the_frames_before_it_and_nothing_after() {
        let mut player = Player::new(Run::new().expect("a").notify("x", json!({})).exiting(9).notify("never", json!({})).0);
        let out = player.on_client(&json!({"method": "a"})).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], Output::Exit(9));
        assert!(player.on_client(&json!({"method": "b"})).is_err());
    }
}
