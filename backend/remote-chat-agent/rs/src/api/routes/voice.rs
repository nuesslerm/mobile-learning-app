//! `/api/voice-transcribe` (audio in, transcript out), `/api/voice-plan` (a transcript in, edit alternatives out) and
//! `/api/voice-edit` (both in one request, kept for callers that have not moved to the two steps), `routes/voiceEdit.ts`.
//! Every call leaves a latency row, a failed one too.

use super::upload::{self, Form};
use super::{respond, truthy, Call};
use crate::api::app::App;
use rca_service_core::application::edit_stats::empty_row;
use rca_service_core::domain::util::{js_string, random_hex};
use axum::extract::multipart::MultipartRejection;
use axum::extract::Multipart;
use axum::extract::State;
use axum::response::Response;
use rca_service_core::domain::voice_dsl::{self as dsl, Alternative};
use rca_service_core::application::voice_planner::{PlanError, Planned};
use remote_chat_agent_core::application::ports::{SttError, Transcription};
use rca_service_core::application::log;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

type Row = Map<String, Value>;

/// What a failed call carries into its row: the error's class name or code, its message, and the status it answers with.
struct Failure {
    code: String,
    message: String,
    status: u16,
}

impl From<PlanError> for Failure {
    fn from(e: PlanError) -> Self {
        Self { status: if e.code == "timeout" { 504 } else { 500 }, code: e.code.into(), message: e.message }
    }
}

impl From<SttError> for Failure {
    fn from(e: SttError) -> Self {
        Self { code: e.name.into(), message: e.message, status: 500 }
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn base36(mut n: u64) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

fn server_edit_id() -> String {
    let hex = random_hex(6);
    let suffix: String = (0..6).map(|i| char::from_digit(u32::from(u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0)) % 36, 36).unwrap_or('0')).collect();
    format!("srv-{}-{suffix}", base36(log::now_ms()))
}

/// The recording's file name inside its temporary directory: the last segment of what the client sent, else `recording.wav`.
fn safe_name(filename: Option<&str>) -> String {
    filename.and_then(|f| Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "recording.wav".into())
}

/// The client's per-turn id when it sent a sensible one, else a server-made one; and the source when it is one we know.
fn identity(edit_id: Option<&str>, source: Option<&str>) -> (String, Option<&'static str>) {
    let id = edit_id.filter(|id| !id.is_empty() && utf16_len(id) <= 64).map(str::to_string).unwrap_or_else(server_edit_id);
    (id, dsl::edit_source(source))
}

/// `Number(text)` for the clip number of a live transcription, accepted when it is a whole number from 0 to 10,000.
fn chunk_index(text: Option<&str>) -> Option<u64> {
    let text = text?.trim();
    let number = if text.is_empty() {
        0.0
    } else if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u64::from_str_radix(digits, 16).ok()? as f64
    } else {
        text.parse::<f64>().ok()?
    };
    (number.fract() == 0.0 && (0.0..=10_000.0).contains(&number)).then_some(number as u64)
}

fn num(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

fn with_transcription(row: &mut Row, result: &Transcription, audio_bytes: usize) {
    row.insert("ffmpeg_ms".into(), json!(result.ffmpeg_ms));
    row.insert("whisper_ms".into(), json!(result.whisper_ms));
    row.insert("whisper_model".into(), json!(result.model_name));
    row.insert("whisper_threads".into(), json!(result.threads));
    row.insert("audio_bytes".into(), json!(audio_bytes));
    row.insert("transcript_chars".into(), json!(utf16_len(&result.transcript)));
}

fn with_plan(row: &mut Row, planned: &Planned, invalid_count: usize, code_chars: usize, language: &str) {
    let meta = &planned.meta;
    let agent = meta.agent.as_deref().map(|a| format!("+{a}")).unwrap_or_default();
    let usage = |pick: fn(&rca_service_core::application::one_shot::Usage) -> Option<f64>| meta.usage.as_ref().and_then(pick).map(num).unwrap_or(Value::Null);
    row.insert("planner_model".into(), json!(format!("{}@{}{agent}", meta.model, meta.variant)));
    row.insert("planner_ms".into(), json!(meta.ms));
    row.insert("session_reused".into(), json!(i32::from(meta.session_reused)));
    row.insert("prompt_tokens".into(), usage(|u| u.input));
    row.insert("cache_read".into(), usage(|u| u.cache_read));
    row.insert("cache_write".into(), usage(|u| u.cache_write));
    row.insert("alternatives".into(), json!(planned.alternatives.len()));
    row.insert("invalid_count".into(), json!(invalid_count));
    row.insert("retried".into(), json!(i32::from(meta.retried)));
    row.insert("code_chars".into(), json!(code_chars));
    row.insert("language".into(), json!(language));
}

/// A failed call is a row too -- its latency is the one nobody measured before.
fn record_failure(app: &App, mut row: Row, failure: &Failure, ms: u64) {
    row.insert("ok".into(), json!(0));
    row.insert("status".into(), json!(failure.status));
    row.insert("ms".into(), json!(ms));
    row.insert("error_code".into(), json!(failure.code));
    row.insert("error_text".into(), json!(failure.message.chars().take(1000).collect::<String>()));
    app.edit_stats.record(row);
}

fn validated(planned: &Planned, code: &str, transcript: &str) -> (Vec<Value>, usize) {
    let limits = dsl::edit_size_limits_for(Some(transcript));
    let checked: Vec<Alternative> = planned.alternatives.iter().map(|a| dsl::validate_alternative(a, code, limits)).collect();
    let invalid = checked.iter().filter(|a| a.valid == Some(false)).count();
    (checked.iter().map(Alternative::to_json).collect(), invalid)
}

fn plan_log(event: &str, planned: &Planned, invalid: usize, extra: Vec<(&str, Value)>) {
    let usage = |pick: fn(&rca_service_core::application::one_shot::Usage) -> Option<f64>| planned.meta.usage.as_ref().and_then(pick).map(num).unwrap_or(Value::Null);
    let mut meta = extra;
    meta.extend([
        ("alternativeCount", json!(planned.alternatives.len())),
        ("model", json!(planned.meta.model)),
        ("sessionReused", json!(planned.meta.session_reused)),
        ("promptTokens", usage(|u| u.input)),
        ("cacheRead", usage(|u| u.cache_read)),
        ("cacheWrite", usage(|u| u.cache_write)),
        ("invalidCount", json!(invalid)),
    ]);
    log::info(event, &meta);
}

/// The uploaded recording and form fields, or the response that refuses them.
async fn collect_upload(form: Result<Multipart, MultipartRejection>) -> Result<(Vec<u8>, Form), Response> {
    let form = upload::read(form).await.map_err(|message| respond(400, &json!({ "error": message })))?;
    // Logged before anything touches ffmpeg: an empty or suspiciously small buffer here proves the upload itself was the problem.
    let file = form.file();
    log::info!("voice_edit.upload_received", byteSize = file.map_or(0, |f| f.data.len()), mimeType = file.map(|f| f.mime.as_str()).unwrap_or_default(), filename = form.field("filename"));
    match form.files.last() {
        Some(file) => Ok((file.data.clone(), form)),
        None => Err(respond(400, &json!({ "error": "No audio file uploaded" }))),
    }
}

/// The recording on disk in a fresh temporary directory, under the last segment of the name the client gave it.
async fn transcribe_upload(app: &App, data: &[u8], filename: Option<&str>) -> Result<Transcription, SttError> {
    let dir = app.files.temp_dir(&std::env::temp_dir(), "voice-edit-").map_err(|message| SttError { name: "Error", message })?;
    let audio: PathBuf = dir.join(safe_name(filename));
    let result = match app.files.write(&audio, data) {
        Ok(()) => app.speech.transcribe(&audio).await,
        Err(message) => Err(SttError { name: "Error", message }),
    };
    app.files.remove(&dir);
    result
}

fn ms_since(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

pub async fn transcribe(State(app): State<Arc<App>>, form: Result<Multipart, MultipartRejection>) -> Response {
    let (data, form) = match collect_upload(form).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    let (edit_id, source) = identity(form.field("editId"), form.field("source"));
    let chunk = chunk_index(form.field("chunkIndex"));
    let mut row = empty_row(&edit_id, "server", if chunk.is_none() { "transcribe" } else { "transcribe-chunk" });
    row.insert("source".into(), json!(source.unwrap_or("voice")));
    row.insert("chunk_index".into(), json!(chunk));
    let started = Instant::now();
    match transcribe_upload(&app, &data, form.field("filename")).await {
        Ok(transcription) => {
            with_transcription(&mut row, &transcription, data.len());
            let ms = ms_since(started);
            row.insert("ms".into(), json!(ms));
            row.insert("status".into(), json!(200));
            app.edit_stats.record(row);
            log::info!("voice_transcribe.timing", ms = ms, ffmpegMs = transcription.ffmpeg_ms, whisperMs = transcription.whisper_ms, threads = transcription.threads, model = transcription.model_name, editId = edit_id);
            respond(200, &json!({ "transcript": transcription.transcript }))
        }
        Err(e) => {
            let failure = Failure::from(e);
            let ms = ms_since(started);
            log::error!("voice_transcribe.failed", err = failure.message, ms = ms);
            let body = json!({ "error": "voice_transcribe_failed", "message": failure.message });
            record_failure(&app, row, &Failure { status: 500, ..failure }, ms);
            respond(500, &body)
        }
    }
}

fn text_field(body: &Value, name: &str) -> Option<String> {
    body.get(name).filter(|v| truthy(v)).map(js_string)
}

fn language_of(value: Option<String>) -> String {
    value.unwrap_or_else(|| "typescript".into())
}

pub async fn plan(Call { app, body, .. }: Call) -> Response {
    let body = body.unwrap_or_else(|| json!({}));
    let Some(transcript) = text_field(&body, "transcript") else {
        return respond(400, &json!({ "error": "Missing transcript" }));
    };
    let Some(code) = text_field(&body, "currentCode") else {
        return respond(400, &json!({ "error": "Missing currentCode" }));
    };
    let language = language_of(body.get("language").filter(|v| !v.is_null()).map(js_string));
    let (edit_id, source) = identity(body.get("editId").and_then(Value::as_str), body.get("source").and_then(Value::as_str));
    let mut row = empty_row(&edit_id, "server", "plan");
    row.insert("source".into(), json!(source));
    row.insert("transcript_chars".into(), json!(utf16_len(&transcript)));
    let started = Instant::now();

    match app.planner.plan(&transcript, &code, &language).await {
        Ok(planned) => {
            let (alternatives, invalid) = validated(&planned, &code, &transcript);
            with_plan(&mut row, &planned, invalid, utf16_len(&code), &language);
            let ms = ms_since(started);
            row.insert("ms".into(), json!(ms));
            row.insert("status".into(), json!(200));
            app.edit_stats.record(row);
            plan_log("voice_plan.timing", &planned, invalid, vec![("ms", json!(ms)), ("plannerMs", json!(planned.meta.ms)), ("retried", json!(planned.meta.retried)), ("editSource", json!(source)), ("editId", json!(edit_id))]);
            respond(200, &json!({ "transcript": planned.transcript, "alternatives": alternatives }))
        }
        Err(e) => {
            let failure = Failure::from(e);
            let ms = ms_since(started);
            log::error!("voice_plan.failed", err = failure.message, ms = ms);
            // A planner that ran out of its turn budget is a gateway timeout, not a server fault.
            let (status, body) = (failure.status, json!({ "error": "voice_plan_failed", "message": failure.message }));
            record_failure(&app, row, &failure, ms);
            respond(status, &body)
        }
    }
}

pub async fn edit(State(app): State<Arc<App>>, form: Result<Multipart, MultipartRejection>) -> Response {
    let (data, form) = match collect_upload(form).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    let Some(code) = form.field("currentCode").filter(|c| !c.is_empty()).map(str::to_string) else {
        return respond(400, &json!({ "error": "Missing currentCode" }));
    };
    let language = form.field("language").map(str::to_string).unwrap_or_else(|| "typescript".into());
    let (edit_id, source) = identity(form.field("editId"), form.field("source"));
    let mut row = empty_row(&edit_id, "server", "edit");
    row.insert("source".into(), json!(source.unwrap_or("voice")));
    let started = Instant::now();

    // Split timing so a slow request can be attributed to whisper or to the LLM; the plan needs the transcript, so they cannot overlap.
    let transcribed = Instant::now();
    let result = async {
        let transcription = transcribe_upload(&app, &data, form.field("filename")).await.map_err(Failure::from)?;
        let transcribe_ms = ms_since(transcribed);
        let planning = Instant::now();
        let planned = app.planner.plan(&transcription.transcript, &code, &language).await.map_err(Failure::from)?;
        Ok::<_, Failure>((transcription, planned, transcribe_ms, ms_since(planning)))
    }
    .await;
    match result {
        Ok((transcription, planned, transcribe_ms, plan_ms)) => {
            let (alternatives, invalid) = validated(&planned, &code, &transcription.transcript);
            with_transcription(&mut row, &transcription, data.len());
            with_plan(&mut row, &planned, invalid, utf16_len(&code), &language);
            row.insert("ms".into(), json!(ms_since(started)));
            row.insert("status".into(), json!(200));
            app.edit_stats.record(row);
            plan_log("voice_edit.timing", &planned, invalid, vec![("transcribeMs", json!(transcribe_ms)), ("planMs", json!(plan_ms)), ("totalMs", json!(transcribe_ms + plan_ms))]);
            respond(200, &json!({ "transcript": planned.transcript, "alternatives": alternatives }))
        }
        Err(failure) => {
            let ms = ms_since(started);
            log::error!("voice_edit.failed", err = failure.message, ms = ms);
            let body = json!({ "error": "voice_edit_failed", "message": failure.message });
            record_failure(&app, row, &Failure { status: 500, ..failure }, ms);
            respond(500, &body)
        }
    }
}
