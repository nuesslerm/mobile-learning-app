//! `speech.*`: dictation over rca's one speech engine (`speech.status`, `speech.transcribe` of the catalog group). The pinned host streams the phone's
//! 16 kHz PCM through a local recognizer and answers partial text as it goes; rca transcribes a whole recording, so the chunks are kept here, in order and
//! once each, until the phone finishes, and then sent as one WAV. The model list names that one engine, which needs no download; the dictation settings
//! are the `voice` object of the host's settings, kept where `settings.set` keeps them. The cloud rows (`openAiKey.*`) stay not offered.
use crate::agent::{ask, state_set, SCOPE};
use agent_api_catalog::{SPEECH_STATUS, SPEECH_TRANSCRIBE};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
use orca_core::AgentApi;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The one model: rca's own engine.
pub const MODEL_ID: &str = "rca-whisper";
const MODEL_LABEL: &str = "Whisper (rca)";
const DEFAULT_SILENCE_SECONDS: u64 = 60;
const MAX_CHUNK_BASE64: usize = (16_000 * 2 * 5usize).div_ceil(3) * 4;
/// Ten minutes of 16 kHz mono PCM16: the most one dictation holds.
const MAX_PCM_BYTES: usize = 16_000 * 2 * 600;
/// The host drops a phone dictation it has not heard from for this long.
const GRACE: Duration = Duration::from_secs(30);

struct Session {
    id: String,
    sample_rate: u64,
    pcm: Vec<u8>,
    pending: BTreeMap<u64, Vec<u8>>,
    next: u64,
    limit_reached: bool,
    touched: Instant,
}

/// The dictations in flight, one per device; one past its grace of quiet is gone.
#[derive(Default)]
pub struct Dictations(Mutex<BTreeMap<String, Session>>);

impl Dictations {
    fn locked(&self) -> MutexGuard<'_, BTreeMap<String, Session>> {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|_, s| s.touched.elapsed() < GRACE);
        all
    }
}

fn plain(message: &str) -> Failure {
    Failure::new(RUNTIME_ERROR, message)
}

/// The params as the host reads them: a missing object is an empty one.
fn object(params: &Value) -> Result<Map<String, Value>, Failure> {
    match params {
        Value::Null => Ok(Map::new()),
        Value::Object(map) => Ok(map.clone()),
        other => Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(other)))),
    }
}

/// A required string: anything else, a number too, is the one message the schema gives.
fn required(params: &Value, key: &str, missing: &str) -> Result<String, Failure> {
    match object(params)?.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(Failure::invalid(missing)),
    }
}

fn optional(params: &Value, key: &str) -> Result<Option<String>, Failure> {
    match object(params)?.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
    }
}

/// The host's base64 check: the alphabet, at most two `=`, and a length no one can decode.
fn valid_base64(value: &str) -> bool {
    let body = value.trim_end_matches('=');
    value.len() - body.len() <= 2 && value.len() % 4 != 1 && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

fn voice(agent: &dyn AgentApi) -> Result<Map<String, Value>, Failure> {
    let kept = agent.state_get(SCOPE, "settings").map_err(crate::agent::refusal)?;
    let kept = kept.as_ref().and_then(|s| s.get("voice")).and_then(Value::as_object).cloned().unwrap_or_default();
    let mut voice = Map::new();
    voice.insert("enabled".into(), json!(false));
    voice.insert("sttModel".into(), json!(""));
    voice.extend(kept);
    Ok(voice)
}

fn available(agent: &dyn AgentApi) -> bool {
    ask(agent, SPEECH_STATUS.name(), json!({})).is_ok_and(|s| s["available"] == json!(true))
}

fn setup(agent: &dyn AgentApi) -> Result<Value, Failure> {
    let voice = voice(agent)?;
    let models = if available(agent) { vec![json!({"id": MODEL_ID, "label": MODEL_LABEL, "provider": "local", "sizeBytes": null, "recommended": true, "status": "ready", "progress": null})] } else { Vec::new() };
    let silence = match voice.get("stopAfterSilenceSeconds") {
        Some(Value::Null) => Value::Null,
        Some(v) if [15u64, 30, 60, 120, 300].iter().any(|c| v.as_u64() == Some(*c)) => v.clone(),
        _ => json!(DEFAULT_SILENCE_SECONDS),
    };
    Ok(json!({
        "enabled": voice.get("enabled") == Some(&json!(true)),
        "selectedModelId": voice.get("sttModel").and_then(Value::as_str).unwrap_or(""),
        "dictationMode": if voice.get("dictationMode") == Some(&json!("hold")) { "hold" } else { "toggle" },
        "dictationHoldToRecord": voice.get("dictationHoldToRecord") != Some(&json!(false)),
        "removeFillerWords": voice.get("removeFillerWords") != Some(&json!(false)),
        "stopAfterSilenceSeconds": silence,
        "dictationTerms": [],
        "models": models,
    }))
}

pub(crate) fn list(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    optional(params, "worktreeId")?;
    setup(agent)
}

pub(crate) fn download(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let model = required(params, "modelId", "Missing model ID")?;
    if model == MODEL_ID && available(agent) {
        return Ok(json!({"started": true}));
    }
    Err(plain("voice_model_not_downloadable"))
}

pub(crate) fn delete(params: &Value) -> Result<Value, Failure> {
    let model = required(params, "modelId", "Missing model ID")?;
    Err(plain(if model == MODEL_ID { "voice_model_delete_failed" } else { "voice_model_unknown" }))
}

pub(crate) fn configure(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let given = object(params)?;
    let model = optional(params, "modelId")?;
    if model.as_deref().is_some_and(|m| !m.is_empty() && m != MODEL_ID) {
        return Err(plain("voice_model_unknown"));
    }
    let mut next = voice(agent)?;
    if let Some(enabled) = given.get("enabled") {
        let Some(enabled) = enabled.as_bool() else { return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", received(enabled)))) };
        next.insert("enabled".into(), json!(enabled));
    }
    if let Some(model) = model {
        next.insert("sttModel".into(), json!(model));
    }
    if let Some(mode) = given.get("dictationMode") {
        match mode.as_str() {
            Some(m @ ("toggle" | "hold")) => {
                next.insert("dictationMode".into(), json!(m));
            }
            _ => return Err(Failure::invalid("Invalid option: expected one of \"toggle\"|\"hold\"")),
        }
    }
    if let Some(hold) = given.get("dictationHoldToRecord") {
        let Some(hold) = hold.as_bool() else { return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", received(hold)))) };
        next.insert("dictationHoldToRecord".into(), json!(hold));
    } else if given.get("dictationMode") == Some(&json!("hold")) {
        next.insert("dictationHoldToRecord".into(), json!(true));
    }
    let mut settings = agent.state_get(SCOPE, "settings").map_err(crate::agent::refusal)?.and_then(|s| s.as_object().cloned()).unwrap_or_default();
    settings.insert("voice".into(), Value::Object(next));
    state_set(agent, "settings", Value::Object(settings))?;
    setup(agent)
}

pub(crate) fn start(all: &Dictations, device: &str, agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let id = required(params, "dictationId", "Missing dictation ID")?;
    let asked = optional(params, "modelId")?.filter(|m| !m.is_empty());
    optional(params, "worktreeId")?;
    let voice = voice(agent)?;
    if voice.get("enabled") != Some(&json!(true)) {
        return Err(plain("voice_dictation_disabled"));
    }
    let model = match asked.or_else(|| voice.get("sttModel").and_then(Value::as_str).filter(|m| !m.is_empty()).map(str::to_string)) {
        Some(model) => model,
        None => return Err(plain("voice_model_not_selected")),
    };
    if model != MODEL_ID || !available(agent) {
        return Err(plain("voice_model_not_ready"));
    }
    all.locked().insert(device.to_string(), Session { id: id.to_string(), sample_rate: 16_000, pcm: Vec::new(), pending: BTreeMap::new(), next: 0, limit_reached: false, touched: Instant::now() });
    Ok(json!({"dictationId": id, "modelId": model, "dedupesResentChunks": true}))
}

pub(crate) fn chunk(all: &Dictations, device: &str, params: &Value) -> Result<Value, Failure> {
    let id = required(params, "dictationId", "Missing dictation ID")?;
    let audio = required(params, "audioBase64", "Missing audio chunk")?;
    if audio.len() > MAX_CHUNK_BASE64 {
        return Err(Failure::invalid("Audio chunk is too large"));
    }
    if !valid_base64(&audio) {
        return Err(Failure::invalid("Audio chunk must be base64"));
    }
    let rate = match object(params)?.get("sampleRate") {
        Some(r) if r.as_f64().is_some_and(|r| r.is_finite() && r > 0.0) => r.as_u64().unwrap_or(16_000),
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected number, received {}", received(other)))),
        None => return Err(Failure::invalid("Invalid input: expected number, received undefined")),
    };
    let seq = match object(params)?.get("seq") {
        None | Some(Value::Null) => None,
        Some(s) => Some(s.as_u64().ok_or_else(|| Failure::invalid("Invalid input: expected nonnegative integer"))?),
    };
    let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(audio.trim_end_matches('=')).map_err(|_| Failure::invalid("Audio chunk must be base64"))?;
    let mut sessions = all.locked();
    let Some(session) = sessions.get_mut(device).filter(|s| s.id == id) else { return Err(plain("dictation_stream_not_started")) };
    session.touched = Instant::now();
    session.sample_rate = rate;
    let mut ready = Vec::new();
    match seq {
        None => ready.push(bytes),
        Some(seq) if seq < session.next => {}
        Some(seq) => {
            session.pending.insert(seq, bytes);
            while let Some(next) = session.pending.remove(&session.next) {
                ready.push(next);
                session.next += 1;
            }
        }
    }
    for part in ready {
        if session.pcm.len() + part.len() > MAX_PCM_BYTES {
            session.limit_reached = true;
        } else if !session.limit_reached {
            session.pcm.extend(part);
        }
    }
    Ok(if session.limit_reached { json!({"dictationId": id, "audioLimitReached": true}) } else { json!({"dictationId": id}) })
}

/// A mono 16-bit PCM recording as a WAV file.
fn wav(pcm: &[u8], rate: u64) -> Vec<u8> {
    let rate = rate as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend(b"RIFF");
    out.extend((36 + pcm.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((pcm.len() as u32).to_le_bytes());
    out.extend(pcm);
    out
}

pub(crate) fn finish(all: &Dictations, device: &str, agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let id = required(params, "dictationId", "Missing dictation ID")?;
    let session = {
        let mut sessions = all.locked();
        if !sessions.get(device).is_some_and(|s| s.id == id) {
            return Err(plain("dictation_stream_not_started"));
        }
        sessions.remove(device).expect("checked above")
    };
    if session.pcm.is_empty() {
        return Ok(json!({"dictationId": id, "text": ""}));
    }
    let heard = ask(agent, SPEECH_TRANSCRIBE.name(), json!({"audio": STANDARD.encode(wav(&session.pcm, session.sample_rate)), "format": "wav"}))?;
    Ok(json!({"dictationId": id, "text": heard["text"].as_str().unwrap_or("").trim()}))
}

pub(crate) fn cancel(all: &Dictations, device: &str, params: &Value) -> Result<Value, Failure> {
    let id = required(params, "dictationId", "Missing dictation ID")?;
    let mut sessions = all.locked();
    if sessions.get(device).is_some_and(|s| s.id == id) {
        sessions.remove(device);
    }
    Ok(json!({"dictationId": id}))
}
