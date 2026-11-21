//! What a provider's account answers, read into the contract's `Limits`: Codex's app-server (`account/rateLimits/read`) and Claude's usage
//! service. Pure: it takes the answers' JSON and the time, and keeps only windows, a plan and the credit counts, never a token.
use agent_api_accounts::{Limits, ResetCredits, Window};
use serde_json::Value;

const SESSION_MINUTES: u64 = 300;
const WEEKLY_MINUTES: u64 = 10_080;

/// A time as milliseconds since the epoch: a number (seconds below 1e10, else milliseconds), a numeric string, or an RFC 3339 text.
pub fn time_ms(value: &Value) -> Option<u64> {
    let from_number = |n: f64| (n.is_finite() && n > 0.0).then_some(if n < 1e10 { (n * 1000.0) as u64 } else { n as u64 });
    match value {
        Value::Number(n) => n.as_f64().and_then(from_number),
        Value::String(text) => text.trim().parse::<f64>().ok().and_then(from_number).or_else(|| rfc3339_ms(text.trim())),
        _ => None,
    }
}

/// `2030-01-02T03:04:05[.fff](Z|+hh:mm)`; none for anything else.
fn rfc3339_ms(text: &str) -> Option<u64> {
    let (date, rest) = text.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let (year, month, day): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let (clock, offset_minutes) = match rest.find(['Z', 'z', '+', '-']) {
        Some(at) if rest[at..].eq_ignore_ascii_case("z") => (&rest[..at], 0),
        Some(at) => {
            let sign = if rest.as_bytes()[at] == b'-' { -1 } else { 1 };
            let (h, m) = rest[at + 1..].split_once(':')?;
            (&rest[..at], sign * (h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?))
        }
        None => (rest, 0),
    };
    let mut t = clock.split(':');
    let (hour, minute): (i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let seconds: f64 = t.next().map_or(Some(0.0), |s| s.parse().ok())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm)
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let ms = ((days * 24 + hour) * 60 + minute - offset_minutes) * 60_000 + (seconds * 1000.0) as i64;
    u64::try_from(ms).ok()
}

fn window(minutes: u64, used: Option<f64>, resets: Option<u64>) -> Option<Window> {
    Some(Window { used_percent: used?.clamp(0.0, 100.0), window_minutes: minutes, resets_at_ms: resets })
}

/// Codex's windows by their length (a minute of drift is the same window), an unnamed length keeping the primary for the session and the
/// secondary for the week, as Orca reads them.
fn codex_windows(limits: &Value) -> (Option<Window>, Option<Window>) {
    fn read(raw: &Value) -> Option<(&Value, f64, Option<f64>)> {
        Some((raw, raw["usedPercent"].as_f64()?, raw["windowDurationMins"].as_f64()))
    }
    let (primary, secondary) = (read(&limits["primary"]), read(&limits["secondary"]));
    let kind = |minutes: Option<f64>| match minutes {
        Some(m) if (m - SESSION_MINUTES as f64).abs() <= 1.0 => Some(SESSION_MINUTES),
        Some(m) if (m - WEEKLY_MINUTES as f64).abs() <= 1.0 => Some(WEEKLY_MINUTES),
        _ => None,
    };
    let (mut session, mut weekly) = (None, None);
    for (raw, used, minutes) in [primary, secondary].into_iter().flatten() {
        match kind(minutes) {
            Some(SESSION_MINUTES) if session.is_none() => session = Some((raw, used)),
            Some(WEEKLY_MINUTES) if weekly.is_none() => weekly = Some((raw, used)),
            _ => {}
        }
    }
    if session.is_none() {
        session = primary.filter(|(_, _, m)| kind(*m).is_none()).map(|(r, u, _)| (r, u));
    }
    if weekly.is_none() {
        weekly = secondary.filter(|(_, _, m)| kind(*m).is_none()).map(|(r, u, _)| (r, u));
    }
    let build = |found: Option<(&Value, f64)>, minutes| found.and_then(|(raw, used)| window(minutes, Some(used), time_ms(&raw["resetsAt"])));
    (build(session, SESSION_MINUTES), build(weekly, WEEKLY_MINUTES))
}

fn codex_credits(raw: &Value) -> Option<ResetCredits> {
    let available = raw["availableCount"].as_f64().filter(|n| n.is_finite())?.max(0.0) as u64;
    let rows = raw["credits"].as_array();
    let next = time_ms(&raw["nextExpiresAt"]).or_else(|| rows?.iter().filter(|c| c["status"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("available"))).filter_map(|c| time_ms(&c["expiresAt"])).min());
    Some(ResetCredits { available, total_earned: raw["totalEarnedCount"].as_f64().filter(|n| n.is_finite()).map(|n| n.max(0.0) as u64), next_expires_ms: next })
}

/// The limits in a `account/rateLimits/read` answer.
pub fn codex_limits(result: &Value, now_ms: u64) -> Limits {
    let (session, weekly) = codex_windows(&result["rateLimits"]);
    Limits { session, weekly, plan: result["planType"].as_str().map(str::to_string), credits: codex_credits(&result["rateLimitResetCredits"]), updated_ms: now_ms }
}

/// The limits in the body of Claude's usage service: `five_hour` is the session and `seven_day` the week.
pub fn claude_limits(body: &Value, now_ms: u64) -> Limits {
    let read = |raw: &Value, minutes| {
        let used = raw["utilization"].as_f64().or_else(|| raw["used_percentage"].as_f64());
        window(minutes, used, time_ms(&raw["resets_at"]))
    };
    Limits { session: read(&body["five_hour"], SESSION_MINUTES), weekly: read(&body["seven_day"], WEEKLY_MINUTES), plan: None, credits: None, updated_ms: now_ms }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_times_in_each_spelling_when_read_then_seconds_milliseconds_numeric_text_and_rfc3339_agree() {
        // Given the same instant as seconds, milliseconds, numeric texts, RFC 3339 with Z and with an offset, and values that are no time
        let instant = 1_900_000_000_000u64;
        // Then each reading is that instant, and the others are none
        assert_eq!([json!(1_900_000_000), json!(1_900_000_000_000u64), json!("1900000000"), json!("2030-03-17T17:46:40Z"), json!("2030-03-17T19:46:40+02:00"), json!("2030-03-17T17:46:40.000Z")].iter().map(time_ms).collect::<Vec<_>>(), vec![Some(instant); 6]);
        assert_eq!([json!(null), json!(0), json!(-4), json!("soon"), json!("2030-13-01T00:00:00Z"), json!(true)].iter().map(time_ms).collect::<Vec<_>>(), vec![None; 6]);
    }

    #[test]
    fn given_codex_windows_in_either_order_or_of_unknown_length_when_read_then_each_is_the_session_or_the_week_by_its_length() {
        // Given the windows in the usual order, swapped, drifting by a minute, and of lengths Codex has not named
        let usual = codex_limits(&json!({"rateLimits": {"primary": {"usedPercent": 40, "windowDurationMins": 300, "resetsAt": 1_900_000_000}, "secondary": {"usedPercent": 10, "windowDurationMins": 10080, "resetsAt": 1_900_500_000}}}), 7);
        let swapped = codex_limits(&json!({"rateLimits": {"primary": {"usedPercent": 10, "windowDurationMins": 10079}, "secondary": {"usedPercent": 40, "windowDurationMins": 301}}}), 7);
        let unnamed = codex_limits(&json!({"rateLimits": {"primary": {"usedPercent": 5, "windowDurationMins": 77}, "secondary": {"usedPercent": 6}}}), 7);
        // Then the session and week follow the lengths, the minutes are normalised, and unnamed lengths keep primary and secondary
        assert_eq!((usual.session.clone().unwrap(), usual.weekly.clone().unwrap().resets_at_ms), (Window { used_percent: 40.0, window_minutes: 300, resets_at_ms: Some(1_900_000_000_000) }, Some(1_900_500_000_000)));
        assert_eq!((swapped.session.unwrap().used_percent, swapped.weekly.unwrap().window_minutes), (40.0, 10_080));
        assert_eq!((unnamed.session.unwrap().used_percent, unnamed.weekly.unwrap().used_percent, usual.updated_ms), (5.0, 6.0, 7));
    }

    #[test]
    fn given_reset_credits_when_read_then_the_count_the_total_and_the_next_expiry_of_an_available_credit_come_out_and_an_answer_with_no_count_is_none() {
        // Given credits with rows, a count alone, and an answer that has no count
        let rows = codex_limits(&json!({"planType": "plus", "rateLimitResetCredits": {"availableCount": 2, "totalEarnedCount": 5, "credits": [{"status": "spent", "expiresAt": 10}, {"status": "AVAILABLE", "expiresAt": "2031-01-01T00:00:00Z"}, {"status": "available", "expiresAt": "2030-01-01T00:00:00Z"}]}}), 1);
        let bare = codex_limits(&json!({"rateLimitResetCredits": {"availableCount": 0}}), 1);
        let none = codex_limits(&json!({"rateLimitResetCredits": {"credits": []}}), 1);
        // Then the plan, count, total and the earliest available expiry are read
        assert_eq!(rows.plan.as_deref(), Some("plus"));
        assert_eq!(rows.credits, Some(ResetCredits { available: 2, total_earned: Some(5), next_expires_ms: Some(1_893_456_000_000) }));
        assert_eq!(bare.credits, Some(ResetCredits { available: 0, total_earned: None, next_expires_ms: None }));
        assert_eq!(none.credits, None);
    }

    #[test]
    fn given_claude_usage_when_read_then_the_five_hour_window_is_the_session_the_seven_day_the_week_and_a_percent_is_held_to_the_range() {
        // Given a body with utilization above 100 in one window, used_percentage in the other, and a body with neither window
        let full = claude_limits(&json!({"five_hour": {"utilization": 120, "resets_at": "2030-03-17T17:46:40Z"}, "seven_day": {"used_percentage": 3.5, "resets_at": 1_900_500_000}}), 9);
        let empty = claude_limits(&json!({"spend": {}}), 9);
        // Then the first is clamped, the second read, and the empty one has no window
        assert_eq!(full.session.unwrap(), Window { used_percent: 100.0, window_minutes: 300, resets_at_ms: Some(1_900_000_000_000) });
        assert_eq!(full.weekly.unwrap(), Window { used_percent: 3.5, window_minutes: 10_080, resets_at_ms: Some(1_900_500_000_000) });
        assert_eq!((empty.session, empty.weekly, empty.plan), (None, None, None));
    }
}
