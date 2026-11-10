//! The host's filesystem behind `WorkspaceFiles`.
use remote_chat_agent_core::application::ports::WorkspaceFiles;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct HostFiles;

impl WorkspaceFiles for HostFiles {
    fn canonical(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn create_dir(&self, path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut f| f.write_all(bytes)).map_err(|e| e.to_string())
    }

    fn remove(&self, path: &Path) {
        let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    }

    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect()
    }

    fn files_under(&self, dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                match entry.file_type() {
                    Ok(t) if t.is_dir() => stack.push(entry.path()),
                    Ok(t) if t.is_file() => {
                        if let Ok(bytes) = std::fs::read(entry.path()) {
                            found.push((entry.path(), bytes));
                        }
                    }
                    _ => {}
                }
            }
        }
        found
    }

    fn age(&self, path: &Path) -> Option<Duration> {
        path.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok())
    }

    fn temp_dir(&self, base: &Path, prefix: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
        Ok(tempfile::Builder::new().prefix(prefix).tempdir_in(base).map_err(|e| e.to_string())?.keep())
    }
}
