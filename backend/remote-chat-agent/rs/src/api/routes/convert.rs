//! `POST /api/convert`: the text of an uploaded PDF or Word document (`routes/convert.ts`).

use super::{respond, upload};
use crate::api::app::App;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, State};
use axum::response::Response;
use serde_json::json;
use std::sync::Arc;

const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

pub async fn post(State(app): State<Arc<App>>, form: Result<Multipart, MultipartRejection>) -> Response {
    let form = match upload::read(form).await {
        Ok(form) => form,
        Err(message) => return respond(400, &json!({ "error": message })),
    };
    let Some(file) = form.file() else {
        return respond(400, &json!({ "error": "No file uploaded" }));
    };
    let text = match file.mime.as_str() {
        "application/pdf" => app.documents.pdf_text(&file.data).await,
        DOCX_MIME => app.documents.docx_text(&file.data),
        _ => return respond(415, &json!({ "error": "Unsupported file type" })),
    };
    match text {
        Ok(text) => respond(200, &json!({ "text": text })),
        Err(message) => respond(422, &json!({ "error": if message.is_empty() { "Failed to parse file".to_string() } else { message } })),
    }
}
