//! A run that is told to stop (SIGTERM from the queue's timeout, or Ctrl-C) still ends with its report: the signal only
//! raises a flag, and every wait, sleep and Lua instruction checks it, so the script fails with a plain reason and the harness run
//! finalises (recording, hits, report) as after any other failure.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static STOP: AtomicBool = AtomicBool::new(false);
static ARMED: OnceLock<Instant> = OnceLock::new();

/// Catches SIGTERM and SIGINT for the rest of the process; later signals keep the flag raised.
pub fn arm() {
    ARMED.get_or_init(Instant::now);
    REGISTERED.get_or_init(|| {
        for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
            // SAFETY: the handler only stores into an atomic, which is async-signal-safe.
            let _ = unsafe { signal_hook::low_level::register(sig, || STOP.store(true, Ordering::SeqCst)) };
        }
        true
    });
}

static REGISTERED: OnceLock<bool> = OnceLock::new();

pub fn requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// The reason a stopped run's steps fail with, for the report's message.
pub fn reason() -> String {
    let after = ARMED.get().map(|t| format!(" after {}s", t.elapsed().as_secs())).unwrap_or_default();
    format!("stopped: SIGTERM received{after} (the queue's timeout, or a cancel); the report holds what the run reached")
}

/// `Err` once a stop was asked for, so a polling loop ends at its next turn.
pub fn check() -> Result<(), String> {
    if requested() {
        Err(reason())
    } else {
        Ok(())
    }
}
