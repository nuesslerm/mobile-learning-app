//! The adapters of the schedules groups' ports: the schedules as one JSON file on the state volume (written beside the target and renamed over it,
//! readable by the service's user only: it holds the digests of webhook tokens), the system clock, and ids and tokens from the system's randomness.
use rca_service_core::application::agent_groups::{ScheduleClock, ScheduleIds, ScheduleStore, StoredSchedule};
use rca_service_core::domain::util::{now_ms, random_hex};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct ScheduleFile {
    path: PathBuf,
    lock: Mutex<()>,
}

impl ScheduleFile {
    pub fn at(path: PathBuf) -> Self {
        Self { path, lock: Mutex::new(()) }
    }
}

impl ScheduleStore for ScheduleFile {
    fn load(&self) -> Result<Vec<StoredSchedule>, String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("the schedules file is not readable: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self, all: &[StoredSchedule]) -> Result<(), String> {
        let _held = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.path.parent().ok_or("the schedules file has no directory")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("schedules-{}.json.tmp", random_hex(4)));
        let written = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp).and_then(|mut file| file.write_all(serde_json::to_string(all).unwrap_or_default().as_bytes()));
        written.and_then(|()| std::fs::rename(&tmp, &self.path)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

pub struct SystemTime;

impl ScheduleClock for SystemTime {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

pub struct RandomIds;

impl ScheduleIds for RandomIds {
    fn id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", random_hex(8))
    }

    /// Two random UUIDs' worth: 244 bits.
    fn token(&self) -> String {
        format!("{}{}", random_hex(16), random_hex(16))
    }
}
