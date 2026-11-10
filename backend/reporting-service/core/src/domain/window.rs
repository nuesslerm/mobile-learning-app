/// The series windows the API accepts, in the order it lists them.
pub const WINDOWS: [(&str, i64); 4] = [("1h", 3_600_000), ("24h", 86_400_000), ("7d", 604_800_000), ("30d", 2_592_000_000)];

pub const DEFAULT_WINDOW: &str = "1h";

pub fn window_ms(name: &str) -> Option<i64> {
    WINDOWS.iter().find(|(window, _)| *window == name).map(|(_, ms)| *ms)
}

pub fn window_names() -> Vec<&'static str> {
    WINDOWS.iter().map(|(name, _)| *name).collect()
}

/// How a window is cut into points: the 1 h window as stored, the day by the minute, the longer ones by the hour.
pub fn granularity_for(window_ms: i64) -> (crate::domain::sample::Granularity, i64) {
    use crate::domain::sample::{Granularity, HOUR_MS, MINUTE_MS};
    if window_ms <= HOUR_MS {
        (Granularity::Raw, 0)
    } else if window_ms <= 24 * HOUR_MS {
        (Granularity::Minute, MINUTE_MS)
    } else {
        (Granularity::Hour, HOUR_MS)
    }
}
