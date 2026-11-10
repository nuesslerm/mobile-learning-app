/// `new Date(ms).toISOString()`, without a date crate.
pub fn iso(ms: i64) -> String {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, month) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

/// The milliseconds of an ISO date such as `iso` writes (`Z`, with or without the fraction); none for any other text.
pub fn parse_iso(text: &str) -> Option<i64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, fraction) = time.split_once('.').map_or((time, "0"), |(c, f)| (c, f));
    let mut t = clock.split(':').map(|p| p.parse::<i64>().ok());
    let (h, m, s) = (t.next()??, t.next()??, t.next()??);
    let millis: i64 = format!("{:0<3}", &fraction[..fraction.len().min(3)]).parse().ok()?;
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400_000 + h * 3_600_000 + m * 60_000 + s * 1000 + millis)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_reads_back_what_iso_writes() {
        // Given instants around a leap day, the epoch and a date before it
        for ms in [0, 951_782_400_000, 1_792_351_234_567, -86_399_001] {
            // When written and read back, Then the instant is the same
            assert_eq!(super::parse_iso(&super::iso(ms)), Some(ms));
        }
        assert_eq!(super::parse_iso("2000-02-29T00:00:00Z"), Some(951_782_400_000));
        assert_eq!(super::parse_iso("soon"), None);
    }

    #[test]
    fn iso_writes_what_a_date_writes() {
        // Given instants around a leap day and the epoch
        for (ms, date) in [(0, "1970-01-01T00:00:00.000Z"), (951_782_400_000, "2000-02-29T00:00:00.000Z"), (1_792_351_234_567, "2026-10-18T19:20:34.567Z")] {
            // When they are written, Then they read as ISO dates
            assert_eq!(super::iso(ms), date);
        }
    }
}
