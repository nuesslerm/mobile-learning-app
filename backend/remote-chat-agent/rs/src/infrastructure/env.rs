//! The process's environment as the service's `Environment` port: variables, working directory, home and temporary directories, read at each call.

use rca_service_core::application::environment::Environment;
use std::path::PathBuf;

#[derive(Clone, Copy, Default)]
pub struct ProcessEnv;

impl Environment for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn current_dir(&self) -> Option<PathBuf> {
        std::env::current_dir().ok()
    }

    fn home_dir(&self) -> PathBuf {
        dirs::home_dir().filter(|home| !home.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("/root"))
    }

    fn temp_dir(&self) -> PathBuf {
        std::env::temp_dir()
    }
}
