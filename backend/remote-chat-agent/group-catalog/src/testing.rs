//! A clock the test sets.
use crate::ports::{Clock, Speech};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct SetClock(pub AtomicU64);

impl SetClock {
    pub fn at(ms: u64) -> SetClock {
        SetClock(AtomicU64::new(ms))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for SetClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A speech engine that hears one thing: what it was told to say, or fails with the words it was given.
pub struct ScriptedSpeech {
    pub engine: Option<String>,
    pub said: Result<String, String>,
    pub heard: std::sync::Mutex<Vec<(usize, String)>>,
}

impl ScriptedSpeech {
    pub fn saying(text: &str) -> ScriptedSpeech {
        ScriptedSpeech { engine: Some("whisper-cpp".into()), said: Ok(text.into()), heard: Default::default() }
    }
}

#[async_trait::async_trait]
impl Speech for ScriptedSpeech {
    fn engine(&self) -> Option<String> {
        self.engine.clone()
    }

    async fn transcribe(&self, audio: Vec<u8>, format: &str) -> Result<String, String> {
        self.heard.lock().unwrap().push((audio.len(), format.to_string()));
        self.said.clone()
    }
}
