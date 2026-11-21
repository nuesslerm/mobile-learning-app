//! `voice.*`: the phone's dictation routes (`/api/voice/status`, `models`, `models/select`, `models/download`, `models/cancel`, `transcribe`)
//! over the one speech engine the agent API's catalog group offers (`speech.status`, `speech.transcribe`; the service answers, the executor is
//! never asked). The pin's own model shape and its PCM request body are kept: the engine is the one model, installed and active whenever the
//! service has one, so a download has nothing to fetch and none runs to cancel. The recognition options of a recording are read and checked but
//! not applied (the engine takes a recording and nothing else), and the rows that change the engine itself (`removeModel`, `linkModels`,
//! `updateAcceleration`, `updateModelUnloadTimeout`, `prepareModel`, `postProcess`, the cloud rows) stay not offered.
use super::service::{Refusal, Service};
use crate::ports::Op;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use t3_contracts::environment::EnvironmentError;

pub const MODEL_ID: &str = "rca-whisper";
const OPERATE_SCOPE: &str = "orchestration:operate";
const SETTINGS_SCOPE: &str = "settings:write";
const SAMPLE_RATE: usize = 16_000;
/// Five minutes of 16 kHz mono Float32 PCM: the most one recording holds.
const MAX_PCM_BYTES: usize = SAMPLE_RATE * 4 * 300;
const MAX_OPTIONS_BYTES: usize = 128_000;
/// The most one transcribe request carries: its length, its options and its audio.
pub const MAX_BODY_BYTES: usize = MAX_PCM_BYTES + MAX_OPTIONS_BYTES + 4;
const UNAVAILABLE: &str = "rca has no speech engine configured";

fn model() -> Value {
    json!({
        "id": MODEL_ID,
        "name": "Whisper (rca)",
        "description": "The speech engine of the rca service, which needs no download",
        "size": 0,
        "languages": [],
        "accuracy": 0,
        "speed": 0,
        "recommended": true,
        "supportsStreaming": false,
        "supportsLanguageDetection": true,
        "supportsTranslation": false,
        "active": true,
        "state": "installed",
    })
}

fn bounded(text: &Value, min: usize, max: usize) -> bool {
    text.as_str().is_some_and(|s| (min..=max).contains(&s.chars().count()))
}

fn spelling(text: &str) -> String {
    text.trim().to_lowercase()
}

/// The recognition options the pin's schema accepts: the language, the two switches, a dictionary of unique terms and aliases, an optional cue.
fn valid_options(options: &Value) -> bool {
    let Some(options) = options.as_object() else { return false };
    let flag = |key: &str, default: Option<bool>| options.get(key).map_or(default.is_some(), Value::is_boolean);
    let words = options.get("speechCustomWords").and_then(Value::as_array);
    let dictionary = words.is_some_and(|words| {
        words.len() <= 100
            && words.iter().all(|w| bounded(&w["term"], 1, 50) && w["aliases"].as_array().is_some_and(|a| a.len() <= 8 && a.iter().all(|alias| bounded(alias, 1, 50))))
            && {
                let spellings: Vec<String> = words.iter().flat_map(|w| std::iter::once(&w["term"]).chain(w["aliases"].as_array().into_iter().flatten()).filter_map(Value::as_str).map(spelling).collect::<Vec<_>>()).collect();
                spellings.iter().all(|s| !s.is_empty()) && spellings.iter().collect::<std::collections::BTreeSet<_>>().len() == spellings.len()
            }
    });
    bounded(&options["speechLanguage"], 2, 16)
        && flag("speechTranslateToEnglish", None)
        && dictionary
        && flag("speechRemoveFillerWords", None)
        && options.get("speechCorrectionWord").is_none_or(|w| bounded(w, 0, 50))
        && flag("speechSilenceFiltering", Some(true))
}

/// The recording of a transcribe request: four bytes of options length (little endian), the options as JSON, then the audio as Float32 samples
/// between -1 and 1. None when the pin would refuse it as invalid audio.
fn decode(body: &[u8]) -> Option<Vec<f32>> {
    let length = u32::from_le_bytes(body.get(..4)?.try_into().ok()?) as usize;
    if length > MAX_OPTIONS_BYTES || 4 + length >= body.len() {
        return None;
    }
    let options: Value = serde_json::from_str(std::str::from_utf8(&body[4..4 + length]).ok()?).ok()?;
    if !valid_options(&options) {
        return None;
    }
    let pcm = &body[4 + length..];
    if pcm.len() > MAX_PCM_BYTES || !pcm.len().is_multiple_of(4) {
        return None;
    }
    let samples: Vec<f32> = pcm.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    samples.iter().all(|s| s.is_finite() && (-1.0..=1.0).contains(s)).then_some(samples)
}

/// The samples as a mono 16-bit WAV file at 16 kHz, which the engine reads.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((SAMPLE_RATE as u32).to_le_bytes());
    out.extend((SAMPLE_RATE as u32 * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for s in samples {
        out.extend(((s * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// The `modelId` of a model request: a string of 1 to 160 characters, or none (the route's schema refuses the body).
fn model_id(body: &[u8]) -> Option<String> {
    let body: Value = serde_json::from_slice(body).ok()?;
    body["modelId"].as_str().filter(|id| (1..=160).contains(&id.chars().count())).map(str::to_string)
}

impl Service {
    fn speech_ready(&self) -> bool {
        self.agent().run(&Op::Speech { method: "speech.status", params: json!({}) }).is_ok_and(|status| status["available"] == json!(true))
    }

    fn voice_invalid(&self, route: &'static str, reason: &'static str) -> Refusal {
        self.environment_refusal(route, EnvironmentError::RequestInvalid(reason))
    }

    fn voice_models_body(&self, ready: bool) -> Value {
        json!({"models": if ready { vec![model()] } else { Vec::new() }})
    }

    /// `GET /api/voice/status`: whether the service can turn a recording into text, with the one model.
    pub fn voice_status(&self, authorization: Option<&str>) -> Result<Value, Refusal> {
        self.scope_gate("voice.status", authorization, OPERATE_SCOPE)?;
        if !self.speech_ready() {
            return Ok(json!({"supported": false, "reason": UNAVAILABLE}));
        }
        Ok(json!({
            "supported": true,
            "state": "ready",
            "modelId": MODEL_ID,
            "model": "Whisper (rca)",
            "size": 0,
            "supportsStreaming": false,
            "supportsBufferedStreaming": false,
            "supportsModelLinking": false,
            "supportsTranslation": false,
            "supportsLanguageDetection": true,
            "languages": [],
            "acceleration": "auto",
            "benchmarkTier": "cpu",
            "modelUnloadTimeout": "never",
            "gpuDevices": [],
        }))
    }

    /// `GET /api/voice/models`: the engine as the one model, none when the service has no engine. A `benchmarks` other than `1` is a body the schema refuses.
    pub fn voice_models(&self, authorization: Option<&str>, query: &[(String, String)]) -> Result<Value, Refusal> {
        self.scope_gate("voice.models", authorization, OPERATE_SCOPE)?;
        if query.iter().any(|(k, v)| k == "benchmarks" && v != "1") {
            return Err(Refusal::bare(400));
        }
        Ok(self.voice_models_body(self.speech_ready()))
    }

    /// `POST /api/voice/models/select`: the engine is the only model there is, so choosing it changes nothing; any other id is no model.
    pub fn voice_select_model(&self, authorization: Option<&str>, body: &[u8]) -> Result<Value, Refusal> {
        self.scope_gate("voice.selectModel", authorization, SETTINGS_SCOPE)?;
        let id = model_id(body).ok_or(Refusal::bare(400))?;
        let ready = self.speech_ready();
        if !ready || id != MODEL_ID {
            return Err(self.voice_invalid("voice.selectModel", "invalid_command"));
        }
        Ok(self.voice_models_body(ready))
    }

    /// `POST /api/voice/models/download`: the engine is installed with the service, so the download of it is already done; without an engine nothing downloads.
    pub fn voice_download_model(&self, authorization: Option<&str>, body: &[u8]) -> Result<Value, Refusal> {
        self.scope_gate("voice.downloadModel", authorization, SETTINGS_SCOPE)?;
        let id = model_id(body).ok_or(Refusal::bare(400))?;
        let ready = self.speech_ready();
        if !ready {
            return Err(self.voice_invalid("voice.downloadModel", "speech_unavailable"));
        }
        if id != MODEL_ID {
            return Err(self.voice_invalid("voice.downloadModel", "invalid_command"));
        }
        Ok(self.voice_models_body(ready))
    }

    /// `POST /api/voice/models/cancel`: no download ever runs, so there is none to stop, whatever the id (the pin answers the same for an id it does not know).
    pub fn voice_cancel_model_download(&self, authorization: Option<&str>, body: &[u8]) -> Result<Value, Refusal> {
        self.scope_gate("voice.cancelModelDownload", authorization, SETTINGS_SCOPE)?;
        model_id(body).ok_or(Refusal::bare(400))?;
        Ok(self.voice_models_body(self.speech_ready()))
    }

    /// A transcribe body over the limit: the pin's body limit says invalid audio, behind the bearer.
    pub fn voice_body_too_large(&self, authorization: Option<&str>) -> Refusal {
        match self.scope_gate("voice.transcribe", authorization, OPERATE_SCOPE) {
            Err(refused) => refused,
            Ok(()) => self.voice_invalid("voice.transcribe", "invalid_audio"),
        }
    }

    /// `POST /api/voice/transcribe`: the recording as one WAV to the engine, its text back.
    pub fn voice_transcribe(&self, authorization: Option<&str>, body: &[u8]) -> Result<Value, Refusal> {
        self.scope_gate("voice.transcribe", authorization, OPERATE_SCOPE)?;
        let samples = decode(body).ok_or_else(|| self.voice_invalid("voice.transcribe", "invalid_audio"))?;
        if !self.speech_ready() {
            return Err(self.voice_invalid("voice.transcribe", "speech_unavailable"));
        }
        match self.agent().run(&Op::Speech { method: "speech.transcribe", params: json!({"audio": STANDARD.encode(wav(&samples)), "format": "wav"}) }) {
            Ok(heard) => Ok(json!({"text": heard["text"].as_str().unwrap_or_default().trim()})),
            Err(e) => {
                self.log("voice.transcribe.failed", &[("error", e.message())]);
                Err(self.environment_refusal("voice.transcribe", EnvironmentError::Internal))
            }
        }
    }
}
