//! The sleeping workspaces of the terminal group (`rca_group_terminal::SleepStore`) as one JSON file on the state volume, written beside the target
//! and renamed over it, so a sleep survives a restart of the service.
use rca_service_core::application::agent_groups::{SleepStore, SleepingWorkspace};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct SleepFile {
    path: PathBuf,
    lock: Mutex<()>,
}

impl SleepFile {
    pub fn at(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }
}

impl SleepStore for SleepFile {
    fn load(&self) -> Result<Vec<SleepingWorkspace>, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("the sleeping terminals file is not readable: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self, all: &[SleepingWorkspace]) -> Result<(), String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.path.parent().ok_or("the sleeping terminals file has no directory")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string(all).map_err(|e| e.to_string())?;
        let durable = || -> std::io::Result<()> {
            let mut file = std::fs::File::create(&tmp)?;
            std::io::Write::write_all(&mut file, text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&tmp, &self.path)?;
            std::fs::File::open(dir)?.sync_all()
        };
        durable().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_service_core::application::agent_groups::SleptTerminal;

    #[test]
    fn given_a_sleep_saved_when_the_file_is_read_again_then_it_is_the_same_and_a_missing_file_is_nothing_sleeping() {
        // Given a file in a new directory that nothing was saved to
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-api").join("sleeping-terminals.json");
        assert_eq!(SleepFile::at(path.clone()).load().unwrap(), Vec::new());
        // When a sleeping workspace is saved and a second reader opens the same file
        let kept = vec![SleepingWorkspace { workspace: "w1".into(), slept_at_ms: 5, terminals: vec![SleptTerminal { terminal: "t1".into(), title: "build".into(), cwd: "/r".into(), cols: 100, rows: 30 }] }];
        SleepFile::at(path.clone()).save(&kept).unwrap();
        // Then it reads back what was saved, and a file that is no list is an error, not an empty answer
        assert_eq!(SleepFile::at(path.clone()).load().unwrap(), kept);
        std::fs::write(&path, "nope").unwrap();
        assert!(SleepFile::at(path).load().unwrap_err().starts_with("the sleeping terminals file is not readable"));
    }
}
