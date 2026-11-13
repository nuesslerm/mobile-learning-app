//! The executor channel's workspace ledger (`rca_exec_channel::WorkspaceLedger`) on the state volume: one JSON file of `{workspace, executor, place}`
//! rows, written beside the target and renamed over it.
use rca_exec_channel::{Place, WorkspaceLedger};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Serialize, Deserialize)]
struct Row {
    workspace: String,
    executor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
}

pub struct WorkspaceFile {
    path: PathBuf,
    lock: Mutex<()>,
}

impl WorkspaceFile {
    pub fn at(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }

    fn write(&self, rows: &[Row]) -> Result<(), String> {
        let dir = self.path.parent().ok_or("the workspace ledger has no directory")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        let text = serde_json::to_string(rows).map_err(|e| e.to_string())?;
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

impl WorkspaceLedger for WorkspaceFile {
    fn load(&self) -> Vec<(String, String, Place)> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(e) => {
                tracing::warn!(event = "workspace_ledger.unreadable", path = %self.path.display(), reason = %e);
                return Vec::new();
            }
        };
        match serde_json::from_str::<Vec<Row>>(&text) {
            Ok(rows) => rows.into_iter().map(|r| (r.workspace, r.executor, Place { prefix: r.prefix, worktree: r.worktree })).collect(),
            Err(e) => {
                let kept = self.path.with_extension("json.bad");
                tracing::warn!(event = "workspace_ledger.malformed", path = %self.path.display(), kept = %kept.display(), reason = %e);
                if let Err(e) = std::fs::rename(&self.path, &kept) {
                    tracing::warn!(event = "workspace_ledger.keep_failed", path = %self.path.display(), reason = %e);
                }
                Vec::new()
            }
        }
    }

    fn save(&self, owners: &HashMap<String, String>, places: &HashMap<String, Place>) {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<Row> = owners
            .iter()
            .map(|(workspace, executor)| {
                let place = places.get(workspace).cloned().unwrap_or_default();
                Row { workspace: workspace.clone(), executor: executor.clone(), prefix: place.prefix, worktree: place.worktree }
            })
            .collect();
        rows.sort_by(|a, b| a.workspace.cmp(&b.workspace));
        if let Err(reason) = self.write(&rows) {
            tracing::warn!(event = "workspace_ledger.save_failed", path = %self.path.display(), reason = %reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn given_saved_ownership_when_it_is_loaded_again_then_owners_and_places_round_trip_and_no_temp_file_stays() {
        // Given a ledger file that saved two workspaces, one with a place
        let dir = tempfile::tempdir().unwrap();
        let ledger = WorkspaceFile::at(dir.path().join("agent-api/workspaces.json"));
        let place = Place { prefix: Some("/base/wt".into()), worktree: Some("wt".into()) };
        ledger.save(&map(&[("repo", "box"), ("wt", "box")]), &HashMap::from([("wt".to_string(), place.clone())]));
        // When a ledger over the same path loads
        let rows = WorkspaceFile::at(dir.path().join("agent-api/workspaces.json")).load();
        // Then both rows come back with their place, and nothing but the file is left beside it
        assert_eq!(rows, vec![("repo".to_string(), "box".to_string(), Place::default()), ("wt".to_string(), "box".to_string(), place)]);
        assert_eq!(std::fs::read_dir(dir.path().join("agent-api")).unwrap().count(), 1);
    }

    #[test]
    fn given_a_malformed_file_when_it_is_loaded_then_it_loads_empty_and_the_file_is_kept_aside_not_overwritten_by_the_next_save() {
        // Given a ledger file that is cut off
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspaces.json");
        std::fs::write(&path, "[{\"workspace\":\"repo\",").unwrap();
        let ledger = WorkspaceFile::at(path.clone());
        // When it loads and the next save runs
        let rows = ledger.load();
        ledger.save(&map(&[("repo", "box")]), &HashMap::new());
        // Then nothing was loaded, the broken text is still on disk next to the new file, and the new file reads back
        assert!(rows.is_empty());
        assert_eq!(std::fs::read_to_string(dir.path().join("workspaces.json.bad")).unwrap(), "[{\"workspace\":\"repo\",");
        assert_eq!(ledger.load().len(), 1);
    }

    #[test]
    fn given_a_directory_that_cannot_be_written_when_it_saves_then_it_does_not_panic_and_leaves_no_temp_file() {
        // Given a ledger whose parent is a file, so no directory can be made
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let ledger = WorkspaceFile::at(blocker.join("workspaces.json"));
        // When it saves
        ledger.save(&map(&[("repo", "box")]), &HashMap::new());
        // Then it returned, and loading finds nothing
        assert!(ledger.load().is_empty());
    }
}
