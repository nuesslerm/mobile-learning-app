//! The last error a command printed, kept for its usage row (`usage_row.rs`).
use std::sync::Mutex;

static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// `eprintln!` that also keeps the line as the command's error for its usage row.
#[macro_export]
macro_rules! wf_err {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        $crate::usage::note_error(&line);
        eprintln!("{line}");
    }};
}

/// What `wf_err!` calls in the crate that expands it.
pub fn note_error(line: &str) {
    if let Ok(mut last) = LAST_ERROR.lock() {
        *last = Some(line.trim().to_string());
    }
}

pub fn last_error() -> Option<String> {
    LAST_ERROR.lock().ok().and_then(|e| e.clone())
}
