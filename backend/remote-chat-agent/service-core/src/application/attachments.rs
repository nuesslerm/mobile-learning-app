//! What a file attached to a chat turn becomes (`lib/attachmentProcessor.ts`, `lib/minimaxVision.ts`): a picture goes to the model as it
//! is when the model can see, else it is read twice, by OCR and by a vision model that describes it, and the model gets both as text;
//! a PDF or Word document, and any text or source file, reach the model as text.

use remote_chat_agent_core::application::ports::DocumentText;
use super::opencode_client::Client;
use crate::domain::util::encode_uri_component;
use base64::Engine as _;
use crate::application::one_shot::{self, Turn};
use super::log;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Attachment {
    /// Base64 of the bytes.
    pub data: String,
    pub mime: String,
    pub filename: String,
}

#[derive(Debug, PartialEq)]
pub enum Processed {
    /// A file part to send to the model beside the prompt.
    Native(Value),
    /// Text to put ahead of the prompt.
    Text(String),
    Skip,
}

const IMAGE_MIMES: [&str; 5] = ["image/jpeg", "image/png", "image/gif", "image/webp", "image/bmp"];
const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const TEXT_EXTENSIONS: [&str; 24] = [
    ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".rs", ".rb", ".java", ".kt", ".swift", ".md", ".json", ".yaml", ".yml", ".toml", ".csv", ".xml", ".html", ".css", ".sh", ".txt", ".env", ".lock",
];
const CAPABILITY_TTL: Duration = Duration::from_secs(10 * 60);
const CAPABILITY_ERROR_TTL: Duration = Duration::from_secs(60);
const VISION_PROMPT: &str = "Describe the visual elements, layout, UI components, diagrams, charts, screenshots, and any non-text content in this image. Do not transcribe text — OCR handles that separately. Focus only on what you see visually.";

/// The vision model (`OPENCODE_VISION_MODEL`, `minimax/MiniMax-M3` by default) as `(providerID, modelID)`.
fn vision_model(client: &Client) -> (String, String) {
    let raw = client.env().settings().opencode_vision_model;
    match raw.find('/').filter(|&i| i >= 1) {
        Some(i) => (raw[..i].to_string(), raw[i + 1..].to_string()),
        None => ("minimax".into(), "MiniMax-M3".into()),
    }
}

/// The first 8000 UTF-16 units, and a note that the rest was cut.
fn truncate(text: &str) -> String {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        units += c.len_utf16();
        if units > 8000 {
            return format!("{}\n… (truncated)", &text[..at]);
        }
    }
    text.to_string()
}

fn extension(filename: &str) -> String {
    filename.rsplit('.').next().unwrap_or_default().to_lowercase()
}

fn is_text_file(mime: &str, filename: &str) -> bool {
    mime.starts_with("text/") || TEXT_EXTENSIONS.contains(&format!(".{}", extension(filename)).as_str())
}

pub struct Processor {
    client: Client,
    /// OCR and the PDF reader.
    documents: Arc<dyn DocumentText>,
    /// `providerID/modelID` to whether it takes pictures, and until when that is believed.
    capabilities: Mutex<HashMap<String, (bool, Instant)>>,
}

impl Processor {
    pub fn new(client: Client, documents: Arc<dyn DocumentText>) -> Self {
        Self { client, documents, capabilities: Mutex::new(HashMap::new()) }
    }

    fn cached(&self, key: &str) -> Option<bool> {
        self.capabilities.lock().unwrap_or_else(|e| e.into_inner()).get(key).filter(|(_, until)| *until > Instant::now()).map(|(image, _)| *image)
    }

    /// Whether the model takes pictures, from opencode's model list. A list that cannot be had means no, for a minute.
    async fn supports_image(&self, provider_id: &str, model_id: &str) -> bool {
        let key = format!("{provider_id}/{model_id}");
        if let Some(known) = self.cached(&key) {
            return known;
        }
        let path = format!("/api/model?directory={}", encode_uri_component(&self.client.env().opencode_cwd()));
        let listed = self.client.fetch_json("GET", &path, None).await;
        let mut caps = self.capabilities.lock().unwrap_or_else(|e| e.into_inner());
        match listed {
            Ok(list) => {
                let until = Instant::now() + CAPABILITY_TTL;
                for model in list.get("data").and_then(Value::as_array).into_iter().flatten() {
                    let text = |name: &str| model.get(name).and_then(Value::as_str).unwrap_or("undefined");
                    let input = model.get("capabilities").and_then(|c| c.get("input"));
                    let image = match input {
                        Some(Value::Array(kinds)) => kinds.iter().any(|k| k.as_str() == Some("image")),
                        Some(other) => other.get("image") == Some(&Value::Bool(true)),
                        None => false,
                    };
                    caps.insert(format!("{}/{}", text("providerID"), text("id")), (image, until));
                }
            }
            Err(e) => {
                log::warn!("attachmentProcessor.model_caps_fetch_failed", err = e.to_string());
                // Default to the path that works without sight.
                caps.insert(key.clone(), (false, Instant::now() + CAPABILITY_ERROR_TTL));
            }
        }
        caps.get(&key).map(|(image, _)| *image).unwrap_or(false)
    }

    pub async fn process(&self, attachment: &Attachment, model: (&str, &str)) -> Processed {
        let Attachment { data, mime, filename } = attachment;
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default();

        if IMAGE_MIMES.contains(&mime.as_str()) {
            if self.supports_image(model.0, model.1).await {
                return Processed::Native(json!({ "type": "file", "mime": mime, "filename": filename, "url": format!("data:{mime};base64,{data}") }));
            }
            // Not a model that sees: OCR and the vision model run side by side.
            let (ocr, vision) = tokio::join!(self.documents.ocr_text(&bytes), describe_image(&self.client, data, mime, filename));
            let words = ocr.trim();
            return Processed::Text(format!("[Image: {filename}]\nVisual: {vision}\nText: {}", if words.is_empty() { "(none)" } else { words }));
        }
        if mime == "application/pdf" {
            return Processed::Text(match self.documents.pdf_text(&bytes).await {
                Ok(text) => format!("[PDF: {filename}]\n{}", truncate(&text)),
                Err(_) => format!("[PDF: {filename}]\n(could not extract text)"),
            });
        }
        if mime == DOCX_MIME {
            return Processed::Text(match self.documents.docx_text(&bytes) {
                Ok(text) => format!("[Document: {filename}]\n{}", truncate(&text)),
                Err(_) => format!("[Document: {filename}]\n(could not extract text)"),
            });
        }
        if is_text_file(mime, filename) {
            let ext = extension(filename);
            let fence = if ext.is_empty() { "text" } else { ext.as_str() };
            return Processed::Text(format!("[File: {filename}]\n```{fence}\n{}\n```", truncate(&String::from_utf8_lossy(&bytes))));
        }
        Processed::Skip
    }
}

/// Asks the vision model to say what the picture looks like; a placeholder when it cannot.
async fn describe_image(client: &Client, base64: &str, mime: &str, filename: &str) -> String {
    let turn = Turn {
        cwd: client.env().opencode_cwd(),
        model: Some(vision_model(client)),
        file_part: Some(json!({ "type": "file", "mime": mime, "filename": filename, "url": format!("data:{mime};base64,{base64}") })),
        ..Turn::default()
    };
    match one_shot::run(client, VISION_PROMPT, &turn, None).await {
        Ok(reply) => Some(reply.text.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| "(no description returned)".into()),
        Err(failure) => {
            log::warn!("minimaxVision.failed", err = failure.message());
            "(image could not be described)".into()
        }
    }
}
