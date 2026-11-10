use super::state_io::{make_dir, node_message, object_of, read_text, replace_file};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use vm_ops_api_core::application::ports::{RunLog, RunMeta, StateError};
use vm_ops_api_core::domain::js::stringify;
use vm_ops_api_core::domain::run_log::{last_lines, newest_first};

const META_FILE: &str = "meta.json";
const LINES_FILE: &str = "lines.jsonl";

/// The run log in a state directory: `<dir>/<runId>/meta.json` (compact JSON) and `<dir>/<runId>/lines.jsonl` (one line per line).
pub struct FileRunLog {
    dir: PathBuf,
}

impl FileRunLog {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn run_dir(&self, run_id: &str) -> PathBuf {
        self.dir.join(run_id)
    }

    async fn meta_at(&self, run_id: &str) -> Option<RunMeta> {
        let text = read_text(&self.run_dir(run_id).join(META_FILE)).await?;
        object_of(&text)
    }
}

async fn append(file: &Path, text: &str) -> Result<(), String> {
    let mut handle = tokio::fs::OpenOptions::new().append(true).create(true).open(file).await.map_err(|error| node_message(&error, "open", Some(file)))?;
    handle.write_all(text.as_bytes()).await.map_err(|error| node_message(&error, "write", None))?;
    handle.flush().await.map_err(|error| node_message(&error, "write", None))
}

#[async_trait]
impl RunLog for FileRunLog {
    async fn read_meta(&self, run_id: &str) -> Option<RunMeta> {
        self.meta_at(run_id).await
    }

    async fn write_meta(&self, run_id: &str, meta: RunMeta) -> Result<(), StateError> {
        let dir = self.run_dir(run_id);
        make_dir(&dir).await.map_err(StateError)?;
        replace_file(&dir.join(META_FILE), stringify(&Value::Object(meta)).as_bytes()).await.map_err(StateError)
    }

    async fn append_lines(&self, run_id: &str, lines: &[String]) -> Result<(), StateError> {
        if lines.is_empty() {
            return Ok(());
        }
        let dir = self.run_dir(run_id);
        make_dir(&dir).await.map_err(StateError)?;
        let text: String = lines.iter().flat_map(|line| [line.as_str(), "\n"]).collect();
        append(&dir.join(LINES_FILE), &text).await.map_err(StateError)
    }

    async fn read_lines(&self, run_id: &str) -> Vec<String> {
        match read_text(&self.run_dir(run_id).join(LINES_FILE)).await {
            Some(text) => last_lines(&text),
            None => Vec::new(),
        }
    }

    async fn list_runs(&self) -> Vec<RunMeta> {
        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else { return Vec::new() };
        let mut names = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                names.push(name);
            }
        }
        names.sort();
        let mut runs = Vec::new();
        for name in names {
            runs.extend(self.meta_at(&name).await);
        }
        newest_first(&mut runs);
        runs
    }
}
