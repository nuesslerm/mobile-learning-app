//! Doubles of the service's ports for the tests of `rca-service` (feature `testing`).

use crate::application::environment::Environment;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// The environment as a map: what a test sets is all the service sees, and nothing is shared with another test of the process.
pub struct MapEnv {
    vars: Mutex<HashMap<String, String>>,
    home: PathBuf,
    cwd: Option<PathBuf>,
    temp: PathBuf,
}

impl MapEnv {
    pub fn new(home: PathBuf, cwd: Option<PathBuf>, temp: PathBuf) -> Self {
        Self { vars: Mutex::new(HashMap::new()), home, cwd, temp }
    }

    pub fn set(&self, name: &str, value: &str) {
        self.vars.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), value.to_string());
    }

    pub fn unset(&self, name: &str) {
        self.vars.lock().unwrap_or_else(|e| e.into_inner()).remove(name);
    }
}

impl Environment for MapEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.lock().unwrap_or_else(|e| e.into_inner()).get(name).cloned()
    }

    fn current_dir(&self) -> Option<PathBuf> {
        self.cwd.clone()
    }

    fn home_dir(&self) -> PathBuf {
        self.home.clone()
    }

    fn temp_dir(&self) -> PathBuf {
        self.temp.clone()
    }
}
