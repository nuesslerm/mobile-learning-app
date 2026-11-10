//! RFC 3339 timestamps (`2026-03-01T12:30:00Z`, a fraction and a numeric offset allowed) as milliseconds since the epoch; both CLIs print them.

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn parse_ms(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = text.get(from..from + len)?;
        part.bytes().all(|c| c.is_ascii_digit()).then(|| part.parse().ok()).flatten()
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 2)?, num(8, 2)?, num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let mut i = 19;
    let mut millis = 0i64;
    if b.get(i) == Some(&b'.') {
        let digits: String = text[i + 1..].chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        millis = format!("{:0<3}", &digits[..digits.len().min(3)]).parse().ok()?;
        i += 1 + digits.len();
    }
    let offset_min = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        sign @ (b'+' | b'-') if b.len() == i + 6 && b[i + 3] == b':' => {
            let minutes = num(i + 1, 2)? * 60 + num(i + 4, 2)?;
            if *sign == b'+' { minutes } else { -minutes }
        }
        _ => return None,
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s - offset_min * 60;
    u64::try_from(secs * 1000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_timestamps_the_cli_prints_when_parsed_then_they_are_epoch_milliseconds_and_malformed_text_is_none() {
        // Given known instants
        let cases = [("1970-01-01T00:00:00Z", Some(0)), ("2026-11-04T12:00:00Z", Some(1_793_793_600_000)), ("2026-11-04T12:00:00.250Z", Some(1_793_793_600_250)), ("2026-11-04T14:00:00+02:00", Some(1_793_793_600_000)), ("2026-11-04T07:30:00-04:30", Some(1_793_793_600_000)), ("2024-02-29T00:00:00Z", Some(1_709_164_800_000))];
        for (text, expected) in cases {
            // When parsed, Then each is its instant
            assert_eq!(parse_ms(text), expected, "{text}");
        }
        // And text that is no timestamp is none
        for text in ["", "yesterday", "2026-13-01T00:00:00Z", "2026-11-04T12:00:00", "2026-11-04T25:00:00Z", "2026-11-04T12:00:00+0200"] {
            assert_eq!(parse_ms(text), None, "{text}");
        }
    }
}
