//! The clock the catalog group stamps a detection with.
use remote_chat_agent_core::application::ports::SpeechToText;
use rca_service_core::application::agent_groups::{CatalogClock, CatalogSpeech};
use std::sync::Arc;
use std::time::{SystemTime as Wall, UNIX_EPOCH};

pub struct SystemClock;

impl CatalogClock for SystemClock {
    fn now_ms(&self) -> u64 {
        Wall::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
    }
}

/// The catalog group's speech engine: the service's own `SpeechToText`, which reads a recording from a file, so the bytes are written to one for the call.
pub struct SpeechOver {
    stt: Arc<dyn SpeechToText>,
    engine: &'static str,
}

impl SpeechOver {
    pub fn new(stt: Arc<dyn SpeechToText>, engine: &'static str) -> SpeechOver {
        SpeechOver { stt, engine }
    }
}

#[async_trait::async_trait]
impl CatalogSpeech for SpeechOver {
    fn engine(&self) -> Option<String> {
        Some(self.engine.to_string())
    }

    async fn transcribe(&self, audio: Vec<u8>, format: &str) -> Result<String, String> {
        let dir = std::env::temp_dir().join(format!("rca-speech-{}-{}", std::process::id(), Wall::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
        tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
        let file = dir.join(format!("recording.{format}"));
        let heard = match tokio::fs::write(&file, &audio).await {
            Ok(()) => self.stt.transcribe(&file).await.map(|t| t.transcript).map_err(|e| e.message),
            Err(e) => Err(e.to_string()),
        };
        let _ = tokio::fs::remove_dir_all(&dir).await;
        heard
    }
}
