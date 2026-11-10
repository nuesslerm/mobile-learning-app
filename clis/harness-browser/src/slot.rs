//! The slot registry's shared helper: the registry lock.
use crate::browser::{read_registry, registry_dir, registry_path};
use serde_json::Value;
use std::time::{Duration, Instant};

pub fn with_lock(mutate: impl FnOnce(&mut Value)) -> bool {
    let dir = registry_dir();
    let _ = std::fs::create_dir_all(&dir);
    let lock = dir.join("registry.lock");
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let done = read_registry().is_some_and(|mut reg| {
        mutate(&mut reg);
        let tmp = dir.join(format!("registry.json.tmp.{}", std::process::id()));
        let text = format!("{}\n", rca_core::stats::js_json_pretty(&reg));
        std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, registry_path()).is_ok()
    });
    let _ = std::fs::remove_file(&lock);
    done
}
