//! `Date`-compatible millisecond helpers for the ISO strings the ledger stores.

use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime, PrimitiveDateTime};

fn at(ms: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

/// `new Date(ms).toISOString()`.
pub fn iso(ms: i64) -> String {
    at(ms).format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")).unwrap_or_default()
}

/// `YYYY-MM-DD` in UTC for a Unix time in milliseconds.
pub fn utc_date(ms: i64) -> String {
    at(ms).format(format_description!("[year]-[month]-[day]")).unwrap_or_default()
}

/// Unix milliseconds of an RFC 3339 timestamp, or of a date-time written without a zone (read as UTC, seconds optional); `None`
/// for anything else.
pub fn parse(s: &str) -> Option<i64> {
    let at = OffsetDateTime::parse(s, &Rfc3339).ok().or_else(|| {
        PrimitiveDateTime::parse(s, format_description!("[year]-[month]-[day]T[hour]:[minute]:[second][optional [.[subsecond]]]"))
            .or_else(|_| PrimitiveDateTime::parse(s, format_description!("[year]-[month]-[day]T[hour]:[minute]")))
            .ok()
            .map(PrimitiveDateTime::assume_utc)
    })?;
    i64::try_from(at.unix_timestamp_nanos().div_euclid(1_000_000)).ok()
}

/// Unix milliseconds of the UTC midnight that starts a `YYYY-MM-DD` calendar date, or `None` for a day that does not exist.
pub fn date_ms(s: &str) -> Option<i64> {
    let date = Date::parse(s, format_description!("[year]-[month]-[day]")).ok()?;
    i64::try_from(date.midnight().assume_utc().unix_timestamp_nanos().div_euclid(1_000_000)).ok()
}

/// Milliseconds of the next UTC midnight after `now`.
pub fn next_utc_midnight(now: i64) -> i64 {
    (now.div_euclid(86_400_000) + 1) * 86_400_000
}
