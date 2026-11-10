//! Graceful stop for a long VM wait: the first SIGINT or SIGTERM is recorded so the command can cancel its VM
//! job and leave with 143; a second one leaves at once with 128 plus the signal, as the TypeScript does.
use signal_hook::consts::{SIGINT, SIGTERM};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Once;

static FIRST: AtomicI32 = AtomicI32::new(0);
static CLAIMED: AtomicBool = AtomicBool::new(false);
static INSTALLED: Once = Once::new();

/// Starts recording the stop request. The handlers are installed once; while nobody has claimed the stop they do what the
/// signals do by default.
pub fn claim() {
    FIRST.store(0, Ordering::SeqCst);
    CLAIMED.store(true, Ordering::SeqCst);
    INSTALLED.call_once(|| {
        for signal in [SIGINT, SIGTERM] {
            // SAFETY: the action touches atomics, calls `_exit` or emulates the default action, all async-signal-safe.
            let _ = unsafe {
                signal_hook::low_level::register(signal, move || {
                    if !CLAIMED.load(Ordering::SeqCst) {
                        let _ = signal_hook::low_level::emulate_default_handler(signal);
                    } else if FIRST.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst).is_err() {
                        signal_hook::low_level::exit(128 + signal);
                    }
                })
            };
        }
    });
}

pub fn requested() -> bool {
    FIRST.load(Ordering::SeqCst) != 0
}

pub fn release() {
    CLAIMED.store(false, Ordering::SeqCst);
}
