//! Identifiers and timestamps in the shapes the thread view's wire format uses. The core does not read the clock or draw random
//! numbers: it asks the two ports below, which the service crate implements over the system and tests implement with fixed values.

use time::macros::format_description;
use time::OffsetDateTime;

/// The wall clock, in milliseconds since the Unix epoch.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;

    /// ISO-8601 with milliseconds and a Z, as `Date#toISOString`.
    fn now_iso(&self) -> String {
        iso_from_ms(self.now_ms())
    }
}

/// Fresh identifiers.
pub trait IdSource: Send + Sync {
    /// A random (version 4) UUID, as `crypto.randomUUID`.
    fn uuid(&self) -> String;

    /// `n` random bytes (at most 16) as lowercase hex.
    fn hex(&self, n: usize) -> String;
}

pub fn iso_from_ms(ms: u64) -> String {
    let at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    at.format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")).unwrap_or_default()
}

/// Milliseconds since the Unix epoch of an RFC 3339 time (as `iso_from_ms` writes them), or none for text that is not one.
pub fn ms_from_iso(iso: &str) -> Option<u64> {
    let at = OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339).ok()?;
    u64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_time_in_milliseconds_when_written_and_read_back_then_it_is_the_same_and_text_that_is_no_time_is_none() {
        // Given times with and without a fraction of a second, and text that is none
        // When each is written and read back, Then it is the same
        for ms in [0, 1, 1_700_000_000_123, 4_102_444_800_000] {
            assert_eq!(ms_from_iso(&iso_from_ms(ms)), Some(ms));
        }
        assert_eq!(ms_from_iso("2023-11-14T22:13:20Z"), Some(1_700_000_000_000));
        assert_eq!((ms_from_iso("yesterday"), ms_from_iso("1969-12-31T23:59:59Z")), (None, None));
    }
}
