//! `POST /api/tamil/check`: marks a handwritten Tamil letter (`routes/tamilCheck.ts`). The app sends the ink as an SVG, black strokes on
//! white with no template and no guide, because a model shown the answer beside the attempt marks the answer; this rasterises it and
//! puts it to the vision model with the letter it was supposed to be. The verdict is advice, not a gate: `ok` and a score out of 100,
//! one line of what it sees, and one line of what to fix.

use super::{respond, truthy, Call};
use remote_chat_agent_render::tamil;
use rca_service_core::application::environment::Environment;
use rca_service_core::domain::util::js_string;
use remote_chat_agent_core::application::ports::WorkspaceFiles;
use rca_service_core::domain::voice_dsl::is_js_space;
use rca_service_core::application::one_shot::{self, Turn};
use rca_service_core::application::log;
use axum::response::Response;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Instant;

const MAX_SVG_CHARS: usize = 2_000_000;

fn marker_model(env: &dyn Environment) -> (String, String) {
    let raw = env.settings().opencode_tamil_marker_model;
    match raw.find('/').filter(|&i| i >= 1) {
        Some(i) => (raw[..i].to_string(), raw[i + 1..].to_string()),
        None => ("opencode-go".into(), "gpt-6-luna".into()),
    }
}

/// An empty directory for the marking turn to live in. Marking a picture needs no workspace, and the default (this process's own
/// directory) is a tree the gateway's directory probe has to walk. When opencode runs in its own container the directory has to exist at
/// the same path there, so production points `TAMIL_CHECK_SCRATCH_DIR` at the shared workspace volume.
fn marking_cwd(env: &dyn Environment, files: &dyn WorkspaceFiles) -> Result<String, String> {
    static CWD: Mutex<Option<String>> = Mutex::new(None);
    let mut held = CWD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dir) = held.as_ref() {
        return Ok(dir.clone());
    }
    let base = env.settings().tamil_check_scratch_dir.unwrap_or_else(|| env.temp_dir());
    let dir = files.temp_dir(&base, "tamil-check-")?.to_string_lossy().into_owned();
    *held = Some(dir.clone());
    Ok(dir)
}

pub async fn check(Call { app, body, .. }: Call) -> Response {
    let body = body.unwrap_or_else(|| json!({}));
    let field = |name: &str| body.get(name).filter(|v| !v.is_null());
    let glyph = field("glyph").filter(|v| truthy(v)).map(js_string);
    let svg = field("svg").and_then(Value::as_str).filter(|s| !s.is_empty());
    let (Some(glyph), Some(svg)) = (glyph, svg) else {
        return respond(400, &json!({ "error": "glyph and svg are required" }));
    };
    if svg.encode_utf16().count() > MAX_SVG_CHARS {
        return respond(413, &json!({ "error": "that is a lot of ink" }));
    }
    if !svg.trim_start_matches(is_js_space).starts_with("<svg") {
        return respond(400, &json!({ "error": "svg must be an <svg> document" }));
    }

    let character_id = field("characterId");
    let started = Instant::now();
    let marked = async {
        let cwd = marking_cwd(app.env.as_ref(), app.files.as_ref())?;
        let (vector, glyph_for_png) = (svg.to_string(), glyph.clone());
        let png = tokio::task::spawn_blocking(move || tamil::rasterise(&vector, &glyph_for_png)).await.map_err(|e| e.to_string())??;
        let transliteration = field("transliteration").map(js_string).unwrap_or_default();
        let question = tamil::prompt(&glyph, &transliteration, tamil::is_word(character_id));
        let turn = Turn {
            cwd,
            model: Some(marker_model(app.env.as_ref())),
            file_part: Some(json!({
                "type": "file",
                "mime": "image/png",
                "filename": format!("{}.png", character_id.map_or_else(|| "letter".to_string(), js_string)),
                "url": format!("data:image/png;base64,{}", STANDARD.encode(&png)),
            })),
            ..Turn::default()
        };
        let reply = one_shot::run(&app.opencode, &question, &turn, None).await.map_err(|f| f.message())?;
        tamil::parse_verdict(reply.text.trim())
    }
    .await;
    let elapsed = started.elapsed().as_millis() as u64;
    match marked {
        Ok(verdict) => {
            log::info!("tamil.check.marked", characterId = character_id, score = verdict["score"].clone(), ok = verdict["ok"].clone(), elapsedMs = elapsed);
            respond(200, &verdict)
        }
        Err(message) => {
            log::warn!("tamil.check.failed", characterId = character_id, err = message, elapsedMs = elapsed);
            respond(502, &json!({ "error": message }))
        }
    }
}
