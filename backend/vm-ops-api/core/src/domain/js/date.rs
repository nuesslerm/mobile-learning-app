//! `new Date(value).getTime()` for what a stored time can be: an ISO 8601 text (V8 reads one without an offset as local time, and
//! the service runs in UTC), a number of milliseconds or a boolean. Other text forms V8 guesses at (`Oct 20 2026`) are not read.
use serde_json::Value;

const MS_PER_DAY: i64 = 86_400_000;
const MAX_TIME_MS: i64 = 8_640_000_000_000_000;

/// Days from 1970-01-01 to a civil date (Howard Hinnant's days_from_civil).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn eat(&mut self, expected: &[u8]) -> bool {
        let found = self.bytes.get(self.at).is_some_and(|b| expected.contains(b));
        if found {
            self.at += 1;
        }
        found
    }

    fn digits(&mut self, count: usize) -> Option<i64> {
        let slice = self.bytes.get(self.at..self.at + count)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.at += count;
        Some(slice.iter().fold(0, |value, digit| value * 10 + i64::from(digit - b'0')))
    }

    /// One or more digits, as the milliseconds they begin: `.1` is 100, `.123456` is 123.
    fn fraction_ms(&mut self) -> Option<i64> {
        let run = self.bytes[self.at..].iter().take_while(|b| b.is_ascii_digit()).count();
        if run == 0 {
            return None;
        }
        let ms = (0..3).fold(0, |value, place| value * 10 + self.bytes.get(self.at + place).filter(|_| place < run).map_or(0, |d| i64::from(d - b'0')));
        self.at += run;
        Some(ms)
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// `Date.parse(text)` for ISO 8601: `YYYY[-MM[-DD]]` (or a signed six-digit year), then `T`, `t` or a space, `HH:mm[:ss[.s…]]`
/// and `Z` or `±HH:mm`. A day up to 31 is taken whatever the month, as V8 does: `2026-02-31` is 3 March.
pub fn parse_date(text: &str) -> Option<i64> {
    let mut cursor = Cursor { bytes: text.as_bytes(), at: 0 };
    let year = match cursor.bytes.first()? {
        sign @ (b'+' | b'-') => {
            cursor.at = 1;
            let year = cursor.digits(6)?;
            if *sign == b'-' {
                if year == 0 {
                    return None;
                }
                -year
            } else {
                year
            }
        }
        _ => cursor.digits(4)?,
    };
    let (mut month, mut day) = (1, 1);
    if cursor.eat(b"-") {
        month = cursor.digits(2)?;
        if cursor.eat(b"-") {
            day = cursor.digits(2)?;
        }
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (mut in_day_ms, mut offset_minutes) = (0, 0);
    if cursor.eat(b"Tt ") {
        let hours = cursor.digits(2)?;
        cursor.eat(b":").then_some(())?;
        let minutes = cursor.digits(2)?;
        let (mut seconds, mut ms) = (0, 0);
        if cursor.eat(b":") {
            seconds = cursor.digits(2)?;
            if cursor.eat(b".") {
                ms = cursor.fraction_ms()?;
            }
        }
        if hours > 24 || minutes > 59 || seconds > 59 || (hours == 24 && minutes + seconds + ms > 0) {
            return None;
        }
        in_day_ms = ((hours * 60 + minutes) * 60 + seconds) * 1000 + ms;
        if cursor.eat(b"+-") {
            let sign = if cursor.bytes[cursor.at - 1] == b'-' { -1 } else { 1 };
            let zone_hours = cursor.digits(2)?;
            cursor.eat(b":");
            offset_minutes = sign * (zone_hours * 60 + cursor.digits(2)?);
        } else {
            cursor.eat(b"Zz");
        }
    }
    if !cursor.done() {
        return None;
    }
    let ms = (days_from_civil(year, month, 1) + day - 1) * MS_PER_DAY + in_day_ms - offset_minutes * 60_000;
    (ms.abs() <= MAX_TIME_MS).then_some(ms)
}

/// `new Date(value).getTime()`, `None` for an Invalid Date.
pub fn new_date_ms(value: &Value) -> Option<i64> {
    match value {
        Value::String(text) => parse_date(text),
        Value::Number(number) => {
            let ms = number.as_f64()?.trunc();
            (ms.abs() <= MAX_TIME_MS as f64).then_some(ms as i64)
        }
        Value::Bool(flag) => Some(i64::from(*flag)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_iso_texts_when_parsed_then_they_are_the_milliseconds_node_reads() {
        let cases = [
            ("2026-10-20T10:00:00.000Z", 1_792_490_400_000),
            ("2026-10-20", 1_792_454_400_000),
            ("2026-10", 1_790_812_800_000),
            ("2026", 1_767_225_600_000),
            ("2026-10-20T10:00", 1_792_490_400_000),
            ("2026-10-20T10:00:00", 1_792_490_400_000),
            ("2026-10-20 10:00:00", 1_792_490_400_000),
            ("2026-10-20T10:00:00z", 1_792_490_400_000),
            ("2026-10-20t10:00:00Z", 1_792_490_400_000),
            ("2026-10-20T10:00Z", 1_792_490_400_000),
            ("2026-10-20T10:00:00.1Z", 1_792_490_400_100),
            ("2026-10-20T10:00:00.123456Z", 1_792_490_400_123),
            ("2026-10-20T10:00:00+02:00", 1_792_483_200_000),
            ("2026-10-20T10:00:00+0200", 1_792_483_200_000),
            ("2026-10-20T10:00:00-02:30", 1_792_499_400_000),
            ("2026-02-31", 1_772_496_000_000),
            ("2026-10-20T24:00:00Z", 1_792_540_800_000),
            ("+002026-10-20T00:00:00Z", 1_792_454_400_000),
            ("-000001-01-01T00:00:00Z", -62_198_755_200_000),
            ("1969-12-31T23:59:59.999Z", -1),
        ];
        for (text, want) in cases {
            assert_eq!(parse_date(text), Some(want), "{text:?}");
        }
    }

    #[test]
    fn given_texts_that_are_no_iso_date_when_parsed_then_the_date_is_invalid() {
        let cases = [
            "", "abc", "2026-13-01", "2026-10-32", "2026-00-10", "2026-10-00", "2026-10-20T24:00:01Z", "2026-10-20T25:00:00Z",
            "2026-10-20T10:60:00Z", "2026-10-20T10:00:60Z", "2026-10-20T10:00:00.Z", "2026-10-20T1:00:00Z", "20261020", "2026-10-20T10:00:00 Z",
            "2026-10-20T10:00:00.000+02", "-000000-01-01T00:00:00Z", "2026-10-20T", "2026-10-20T10", "2026-10-20x", "275761-01-01",
            "+275760-09-13T00:00:00.001Z",
        ];
        for text in cases {
            assert_eq!(parse_date(text), None, "{text:?}");
        }
    }

    #[test]
    fn given_json_values_when_read_as_a_date_then_numbers_truncate_and_booleans_count_as_one_and_zero() {
        assert_eq!(new_date_ms(&json!("2026-10-20T10:00:00.000Z")), Some(1_792_490_400_000));
        assert_eq!(new_date_ms(&json!(5.9)), Some(5));
        assert_eq!(new_date_ms(&json!(-5.9)), Some(-5));
        assert_eq!(new_date_ms(&json!(8.64e15)), Some(8_640_000_000_000_000));
        assert_eq!(new_date_ms(&json!(8.64e15 + 2.0)), None);
        assert_eq!(new_date_ms(&json!(true)), Some(1));
        assert_eq!(new_date_ms(&json!(false)), Some(0));
        assert_eq!(new_date_ms(&json!([])), None);
        assert_eq!(new_date_ms(&json!({})), None);
    }
}
