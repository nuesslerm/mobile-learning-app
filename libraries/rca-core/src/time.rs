//! `Date`-compatible millisecond helpers for the ISO strings the ledger stores.

const MS_DAY: i64 = 86_400_000;
const MIN_MS: i64 = -377_705_116_800_000;
const MAX_MS: i64 = 253_402_300_799_999;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn year_text(y: i64) -> String {
    if y < 0 {
        format!("-{:04}", -y)
    } else {
        format!("{y:04}")
    }
}

fn clamp(ms: i64) -> i64 {
    if (MIN_MS..=MAX_MS).contains(&ms) {
        ms
    } else {
        0
    }
}

/// `new Date(ms).toISOString()`.
pub fn iso(ms: i64) -> String {
    let ms = clamp(ms);
    let (y, m, d) = civil_from_days(ms.div_euclid(MS_DAY));
    let t = ms.rem_euclid(MS_DAY);
    format!("{}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", year_text(y), t / 3_600_000, t / 60_000 % 60, t / 1000 % 60, t % 1000)
}

/// `YYYY-MM-DD` in UTC for a Unix time in milliseconds.
pub fn utc_date(ms: i64) -> String {
    let (y, m, d) = civil_from_days(clamp(ms).div_euclid(MS_DAY));
    format!("{}-{m:02}-{d:02}", year_text(y))
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn eat(&mut self, c: u8) -> bool {
        let hit = self.0.first() == Some(&c);
        if hit {
            self.0 = &self.0[1..];
        }
        hit
    }

    fn skip(&mut self) -> Option<()> {
        self.0 = self.0.get(1..)?;
        Some(())
    }

    fn digits(&mut self, n: usize) -> Option<i64> {
        let (head, rest) = self.0.split_at_checked(n)?;
        if !head.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.0 = rest;
        Some(head.iter().fold(0, |acc, b| acc * 10 + i64::from(b - b'0')))
    }

    fn date(&mut self, signed: bool) -> Option<i64> {
        let neg = signed && self.eat(b'-');
        if signed && !neg {
            self.eat(b'+');
        }
        let y = self.digits(4)?;
        let y = if neg { -y } else { y };
        self.eat(b'-').then_some(())?;
        let m = self.digits(2)?;
        self.eat(b'-').then_some(())?;
        let d = self.digits(2)?;
        ((1..=12).contains(&m) && (1..=days_in_month(y, m)).contains(&d)).then(|| days_from_civil(y, m, d))
    }

    fn clock(&mut self, seconds: bool, leap: Option<&mut bool>) -> Option<i64> {
        let h = self.digits(2)?;
        self.eat(b':').then_some(())?;
        let mi = self.digits(2)?;
        let (s, ms) = if seconds {
            self.eat(b':').then_some(())?;
            (self.digits(2)?, self.fraction()?)
        } else {
            (0, 0)
        };
        if let (Some(flag), 60, true) = (leap, s, h < 24 && mi < 60) {
            *flag = true;
            return Some(((h * 60 + mi) * 60 + 59) * 1000 + 999);
        }
        (h < 24 && mi < 60 && s < 60).then_some(((h * 60 + mi) * 60 + s) * 1000 + ms)
    }

    fn fraction(&mut self) -> Option<i64> {
        if !self.eat(b'.') {
            return Some(0);
        }
        let n = self.0.iter().take_while(|b| b.is_ascii_digit()).count();
        if n == 0 {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head.iter().chain(&[b'0'; 3]).take(3).fold(0, |acc, b| acc * 10 + i64::from(b - b'0')))
    }

    fn offset(&mut self) -> Option<i64> {
        if self.eat(b'Z') || self.eat(b'z') {
            return Some(0);
        }
        let sign = if self.eat(b'+') {
            1
        } else if self.eat(b'-') {
            -1
        } else {
            return None;
        };
        let h = self.digits(2)?;
        self.eat(b':').then_some(())?;
        let m = self.digits(2)?;
        (h < 24 && m < 60).then_some(sign * (h * 60 + m) * 60_000)
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }
}

fn rfc3339(s: &str) -> Option<i64> {
    let mut c = Cursor(s.as_bytes());
    let days = c.date(false)?;
    c.skip()?;
    let mut leap = false;
    let t = c.clock(true, Some(&mut leap))?;
    let off = c.offset()?;
    let ms = days * MS_DAY + t - off;
    c.done().then_some(())?;
    let (y, m, d) = civil_from_days(ms.div_euclid(MS_DAY));
    (!leap || (ms.rem_euclid(MS_DAY) == MS_DAY - 1 && d == days_in_month(y, m))).then_some(ms)
}

fn zoneless(s: &str, seconds: bool) -> Option<i64> {
    let mut c = Cursor(s.as_bytes());
    let days = c.date(true)?;
    c.eat(b'T').then_some(())?;
    let t = c.clock(seconds, None)?;
    c.done().then_some(days * MS_DAY + t)
}

/// Unix milliseconds of an RFC 3339 timestamp, or of a date-time written without a zone (read as UTC, seconds optional); `None`
/// for anything else.
pub fn parse(s: &str) -> Option<i64> {
    rfc3339(s).or_else(|| zoneless(s, true)).or_else(|| zoneless(s, false))
}

/// Unix milliseconds of the UTC midnight that starts a `YYYY-MM-DD` calendar date, or `None` for a day that does not exist.
pub fn date_ms(s: &str) -> Option<i64> {
    let mut c = Cursor(s.as_bytes());
    let days = c.date(true)?;
    c.0.is_empty().then(|| days * MS_DAY)
}

/// Milliseconds of the next UTC midnight after `now`.
pub fn next_utc_midnight(now: i64) -> i64 {
    (now.div_euclid(MS_DAY) + 1) * MS_DAY
}
