//! Text out of documents and pictures (`lib/pdfParser.ts`, `lib/docxParser.ts`, `lib/ocrExtractor.ts`): a PDF through `pdftotext`
//! (poppler), a DOCX read by `remote_chat_agent_render::docx`, a picture through the `tesseract` binary. `PDFTOTEXT_PATH` and `TESSERACT_PATH`
//! name the binaries when they are not on `PATH`.

use rca_service_core::application::settings::Settings;
use async_trait::async_trait;
use remote_chat_agent_core::application::ports::{Commands, DocumentText};
use rca_service_core::application::log;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Tesseract can run out of memory on a large picture, so one bigger than this (about a megapixel encoded) is not read.
const MAX_OCR_BYTES: usize = 300_000;
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs `body` with `bytes` written to a file in a fresh temporary directory that is gone afterwards.
async fn with_file<T, Fut: Future<Output = Result<T, String>>>(prefix: &str, bytes: &[u8], body: impl FnOnce(PathBuf) -> Fut) -> Result<T, String> {
    let dir = tempfile::Builder::new().prefix(prefix).tempdir().map_err(|e| e.to_string())?;
    let file = dir.path().join("input");
    tokio::fs::write(&file, bytes).await.map_err(|e| e.to_string())?;
    body(file).await
}

/// The text of every page, pages separated by a blank line.
async fn pdf_text(commands: &dyn Commands, tool: &str, bytes: &[u8]) -> Result<String, String> {
    with_file("pdf-", bytes, |file| async move {
        let out = commands.run(tool, &["-enc", "UTF-8", &file.to_string_lossy(), "-"], None, Some(TOOL_TIMEOUT)).await.map_err(|e| {
            let first = e.stderr.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
            first.unwrap_or(e.message)
        })?;
        // pdftotext ends every page with a form feed.
        let mut pages: Vec<&str> = out.stdout.split('\u{c}').map(|page| page.trim_end_matches('\n')).collect();
        if pages.last().is_some_and(|page| page.is_empty()) {
            pages.pop();
        }
        Ok(pages.join("\n\n"))
    })
    .await
}

/// The words in a picture; nothing when it is too large or tesseract cannot read it, never an error.
async fn ocr_text(commands: &dyn Commands, tool: &str, bytes: &[u8]) -> String {
    if bytes.len() > MAX_OCR_BYTES {
        log::warn!("ocrExtractor.skipped_too_large", bytes = bytes.len(), maxBytes = MAX_OCR_BYTES);
        return String::new();
    }
    let read = with_file("ocr-", bytes, |file| async move {
        commands.run(tool, &[&file.to_string_lossy(), "stdout", "-l", "eng"], None, Some(TOOL_TIMEOUT)).await.map(|out| out.stdout.trim().to_string()).map_err(|e| e.message)
    })
    .await;
    read.unwrap_or_else(|e| {
        log::warn!("ocrExtractor.failed", err = e);
        String::new()
    })
}

/// `DocumentText` over `pdftotext`, `tesseract` (through `Commands`) and the render crate's DOCX reader.
pub struct Documents {
    commands: Arc<dyn Commands>,
    pdftotext: String,
    tesseract: String,
}

impl Documents {
    pub fn new(commands: Arc<dyn Commands>, settings: &Settings) -> Self {
        Self { commands, pdftotext: settings.pdftotext_path.clone(), tesseract: settings.tesseract_path.clone() }
    }
}

#[async_trait]
impl DocumentText for Documents {
    async fn pdf_text(&self, bytes: &[u8]) -> Result<String, String> {
        pdf_text(self.commands.as_ref(), &self.pdftotext, bytes).await
    }

    fn docx_text(&self, bytes: &[u8]) -> Result<String, String> {
        remote_chat_agent_render::docx::docx_text(bytes)
    }

    async fn ocr_text(&self, bytes: &[u8]) -> String {
        ocr_text(self.commands.as_ref(), &self.tesseract, bytes).await
    }
}
