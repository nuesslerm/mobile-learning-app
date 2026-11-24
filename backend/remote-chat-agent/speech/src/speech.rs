//! Speech to text (`lib/whisper.ts`, `lib/sttEngine.ts`). The engine is chosen per call from the environment: the faster-whisper
//! sidecar at `STT_URL`, Groq's hosted whisper with `STT_ENGINE=groq`, else whisper.cpp behind ffmpeg. whisper.cpp and ffmpeg are the
//! binaries `WHISPER_CPP_BINARY` and `FFMPEG_PATH` name.

use rca_service_core::application::environment::Environment;
use rca_service_core::application::settings::SttSettings;
use async_trait::async_trait;
use remote_chat_agent_core::application::ports::{Commands, FormPart, Outbound, Payload, Post, SpeechToText, SttError, Transcription};
use std::sync::Arc;
use rca_service_core::application::log;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn whisper_error(message: impl Into<String>) -> SttError {
    SttError { name: "WhisperError", message: message.into() }
}

fn error(message: impl Into<String>) -> SttError {
    SttError { name: "Error", message: message.into() }
}

#[derive(Debug, PartialEq)]
pub enum Engine {
    Sidecar(String),
    Groq { api_key: String, model: String },
    WhisperCpp,
}

pub fn resolve_engine(stt: &SttSettings) -> Engine {
    if let Some(url) = stt.url.clone().map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
        return Engine::Sidecar(url);
    }
    if stt.engine.as_deref() == Some("groq") {
        match stt.groq_api_key.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
            Some(api_key) => return Engine::Groq { api_key, model: stt.groq_model.clone().map(|m| m.trim().to_string()).unwrap_or_else(|| "whisper-large-v3-turbo".into()) },
            None => log::warn!("stt.groq_key_missing", fallback = "whisper-cpp"),
        }
    }
    Engine::WhisperCpp
}

fn threads(stt: &SttSettings) -> u64 {
    match stt.whisper_threads.clone().and_then(|v| v.trim().parse::<u64>().ok()).filter(|&n| n > 0) {
        Some(n) => n,
        None => std::thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(1),
    }
}

fn millis(since: Instant) -> u64 {
    since.elapsed().as_millis() as u64
}

/// Where and how a recording is posted to an HTTP engine.
struct Target {
    kind: &'static str,
    url: String,
    headers: Vec<(&'static str, String)>,
    field: &'static str,
    extra: Vec<(&'static str, String)>,
}

async fn remote(outbound: &dyn Outbound, engine: &Engine, audio: &Path) -> Result<Transcription, SttError> {
    let filename = audio.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let target = match engine {
        Engine::Groq { api_key, model } => Target {
            kind: "groq",
            url: "https://api.groq.com/openai/v1/audio/transcriptions".into(),
            headers: vec![("Authorization", format!("Bearer {api_key}"))],
            field: "file",
            extra: vec![("model", model.clone()), ("language", "en".into()), ("response_format", "json".into())],
        },
        Engine::Sidecar(url) => Target { kind: "sidecar", url: url.clone(), headers: Vec::new(), field: "audio", extra: Vec::new() },
        Engine::WhisperCpp => return Err(error("whisper.cpp is not a remote engine")),
    };
    let kind = target.kind;
    let bytes = tokio::fs::read(audio).await.map_err(|e| error(e.to_string()))?;
    let mut parts = vec![FormPart { name: target.field.into(), file_name: Some(filename), mime: Some("application/octet-stream".into()), bytes }];
    parts.extend(target.extra.into_iter().map(|(name, value)| FormPart { name: name.into(), file_name: None, mime: None, bytes: value.into_bytes() }));
    let headers = target.headers.into_iter().map(|(name, value)| (name.to_string(), value)).collect();
    let post = Post { url: target.url, headers, basic_auth: None, payload: Payload::Form(parts) };
    let started = Instant::now();
    let answer = outbound.post(post).await.map_err(|e| SttError { name: "TypeError", message: format!("fetch failed: {}", e.message) })?;
    let status = answer.status;
    let body = String::from_utf8_lossy(&answer.body).into_owned();
    if !(200..300).contains(&status) {
        return Err(error(format!("{kind} STT {status}: {}", body.chars().take(300).collect::<String>())));
    }
    let parsed: Value = serde_json::from_str(&body).map_err(|e| SttError { name: "SyntaxError", message: e.to_string() })?;
    let said = |key: &str| parsed.get(key).and_then(Value::as_str);
    let transcript = said("transcript").or_else(|| said("text")).unwrap_or_default().trim().to_string();
    if transcript.is_empty() {
        return Err(error(format!("{kind} STT returned no transcript")));
    }
    let number = |key: &str| parsed.get(key).and_then(Value::as_f64);
    let model_name = match (said("model"), engine) {
        (Some(model), _) => model.to_string(),
        (None, Engine::Groq { model, .. }) => format!("groq:{model}"),
        (None, _) => "sidecar".into(),
    };
    Ok(Transcription {
        transcript,
        // The sidecar decodes the container itself; there is no ffmpeg leg.
        ffmpeg_ms: 0,
        whisper_ms: number("ms").map(|n| n as u64).unwrap_or_else(|| millis(started)),
        model_name,
        threads: number("threads").map(|n| n as u64).unwrap_or(0),
    })
}

async fn convert_to_wav(commands: &dyn Commands, stt: &SttSettings, input: &Path, dir: &Path) -> Result<PathBuf, SttError> {
    let ffmpeg = stt.ffmpeg_path.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| "ffmpeg".into());
    let size = tokio::fs::metadata(input).await.map(|m| m.len()).unwrap_or(0);
    log::info!("whisper.convert_to_wav.input", inputPath = input.to_string_lossy().as_ref(), inputByteSize = size);
    let output = dir.join("input.wav");
    let result = commands.run(&ffmpeg, &["-y", "-loglevel", "error", "-i", &input.to_string_lossy(), "-ar", "16000", "-ac", "1", "-f", "wav", &output.to_string_lossy()], None, Some(Duration::from_secs(60))).await;
    match result {
        Ok(_) if output.exists() => Ok(output),
        Ok(_) => Err(whisper_error("ffmpeg failed to produce WAV output")),
        Err(e) => Err(error(e.message)),
    }
}

async fn whisper_cpp(commands: &dyn Commands, stt: &SttSettings, audio: &Path) -> Result<Transcription, SttError> {
    let started = Instant::now();
    let dir = tempfile::Builder::new().prefix("whisper-").tempdir().map_err(|e| error(e.to_string()))?;
    let wav = convert_to_wav(commands, stt, audio, dir.path()).await?;
    let ffmpeg_ms = millis(started);
    let (transcript, whisper_ms, model_name, threads) = run_whisper(commands, stt, &wav).await?;
    log::info!("whisper.timing", ffmpegMs = ffmpeg_ms, whisperMs = whisper_ms, threads = threads, model = model_name.as_str());
    Ok(Transcription { transcript, ffmpeg_ms, whisper_ms, model_name, threads })
}

async fn run_whisper(commands: &dyn Commands, stt: &SttSettings, wav: &Path) -> Result<(String, u64, String, u64), SttError> {
    let binary = stt.whisper_cpp_binary.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| "whisper-cli".into());
    let binary_path = Path::new(&binary);
    if binary_path.components().count() > 1 && !binary_path.exists() {
        return Err(whisper_error(format!("whisper.cpp is not built: no binary at {binary}. Set WHISPER_CPP_BINARY to the whisper.cpp binary.")));
    }
    let model_name = stt.whisper_model_name.clone().unwrap_or_else(|| "base.en".into());
    let model_path = match stt.whisper_model_path.clone().filter(|p| !p.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => binary_path.parent().unwrap_or_else(|| Path::new("")).join("models").join(format!("ggml-{model_name}.bin")),
    };
    if !model_path.exists() {
        return Err(whisper_error(format!("whisper model not found at {}. Set WHISPER_MODEL_PATH to a ggml model.", model_path.display())));
    }
    let threads = threads(stt);
    let started = Instant::now();
    // `-nt` only: `-np` suppresses the transcript itself, not just the banner.
    let out = commands.run(&binary, &["-m", &model_path.to_string_lossy(), "-f", &wav.to_string_lossy(), "-l", "en", "-t", &threads.to_string(), "-nt"], None, None).await.map_err(|e| error(e.message))?;
    let whisper_ms = millis(started);
    let transcript = out.stdout.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('[')).collect::<Vec<_>>().join(" ").trim().to_string();
    if transcript.is_empty() {
        return Err(whisper_error("No transcript produced"));
    }
    Ok((transcript, whisper_ms, model_name, threads))
}

const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(500);

/// A failure of the remote engine that a second try can pass: the request never got an answer, or the engine answered 5xx or 429.
fn transient(failure: &SttError) -> bool {
    let message = failure.message.as_str();
    message.starts_with("fetch failed") || [" STT 5", " STT 429"].iter().any(|marker| message.contains(marker))
}

/// The recording's transcript through the engine the environment names: a remote one over `outbound`, else whisper.cpp and ffmpeg
/// through `commands`.
async fn transcribe(outbound: &dyn Outbound, commands: &dyn Commands, stt: &SttSettings, audio: &Path) -> Result<Transcription, SttError> {
    if !audio.exists() {
        return Err(whisper_error(format!("Audio file not found: {}", audio.display())));
    }
    match resolve_engine(stt) {
        Engine::WhisperCpp => whisper_cpp(commands, stt, audio).await,
        engine => {
            let result = match remote(outbound, &engine, audio).await {
                Err(first) if transient(&first) => {
                    log::warn!("stt.retry", reason = first.message.as_str());
                    tokio::time::sleep(RETRY_PAUSE).await;
                    remote(outbound, &engine, audio).await?
                }
                other => other?,
            };
            let kind = if matches!(engine, Engine::Groq { .. }) { "groq" } else { "sidecar" };
            log::info!("whisper.timing", engine = kind, whisperMs = result.whisper_ms, model = result.model_name.as_str());
            Ok(result)
        }
    }
}

/// `SpeechToText` over the engine the environment names, posting through `Outbound` and running programs through `Commands`.
pub struct Speech {
    outbound: Arc<dyn Outbound>,
    commands: Arc<dyn Commands>,
    env: Arc<dyn Environment>,
}

impl Speech {
    /// The engine is chosen at each call from `env`'s settings, so a variable changed after start shows at the next recording.
    pub fn new(outbound: Arc<dyn Outbound>, commands: Arc<dyn Commands>, env: Arc<dyn Environment>) -> Self {
        Self { outbound, commands, env }
    }
}

#[async_trait]
impl SpeechToText for Speech {
    async fn transcribe(&self, audio: &Path) -> Result<Transcription, SttError> {
        transcribe(self.outbound.as_ref(), self.commands.as_ref(), &self.env.settings().stt, audio).await
    }
}
