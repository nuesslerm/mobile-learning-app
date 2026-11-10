//! The phone's recurrence (a `FREQ=DAILY` or `FREQ=WEEKLY` rrule, or a five-field cron of one time of day) and timezone as the schedule rca keeps: a time of
//! day on some weekdays at a fixed UTC offset. What rca cannot express is refused by name, never run as something else.
use agent_api_schedules::Schedule;
use orca_core::wire::Failure;

const DAY_CODES: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];
const DAY_MS: i64 = 86_400_000;

/// A timezone with no daylight saving that rca's fixed offset states: UTC, GMT and `Etc/GMT+n` (which is n hours *behind* UTC, as POSIX has it).
fn offset_minutes(timezone: &str) -> Result<i32, Failure> {
    if matches!(timezone, "UTC" | "Etc/UTC" | "Etc/UCT" | "UCT" | "Zulu" | "Etc/Zulu" | "GMT" | "Etc/GMT" | "Etc/GMT0" | "Etc/GMT+0" | "Etc/GMT-0" | "Greenwich" | "Etc/Greenwich") {
        return Ok(0);
    }
    let hours = timezone.strip_prefix("Etc/GMT").and_then(|rest| {
        let (sign, digits) = rest.split_at(1);
        let n: i32 = digits.parse().ok().filter(|n| (1..=14).contains(n) && digits.bytes().all(|b| b.is_ascii_digit()))?;
        match sign {
            "+" => Some(-n),
            "-" => Some(n),
            _ => None,
        }
    });
    hours.map(|h| h * 60).ok_or_else(|| Failure::invalid(format!("The timezone {timezone} is not offered: rca runs a schedule at a fixed UTC offset and has no timezone database, so only UTC, GMT and Etc/GMT+n zones are")))
}

fn number(text: &str, what: &str, max: u32) -> Result<u32, Failure> {
    text.parse::<u32>().ok().filter(|n| *n <= max && !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).ok_or_else(|| Failure::invalid(format!("Invalid recurrence {what}")))
}

fn weekdays_of_cron(field: &str) -> Result<Option<Vec<u8>>, Failure> {
    if field == "*" {
        return Ok(None);
    }
    let mut days = Vec::new();
    for part in field.split(',') {
        let (from, to) = match part.split_once('-') {
            Some((a, b)) => (number(a, "day", 7)?, number(b, "day", 7)?),
            None => (number(part, "day", 7)?, number(part, "day", 7)?),
        };
        if from > to {
            return Err(Failure::invalid("Invalid recurrence day"));
        }
        days.extend((from..=to).map(|d| (d % 7) as u8));
    }
    days.sort_unstable();
    days.dedup();
    Ok(if days.len() == 7 { None } else { Some(days) })
}

fn fixed(hour: u32, minute: u32, weekdays: Option<Vec<u8>>, offset: i32) -> Schedule {
    Schedule::FixedTime { time_of_day: format!("{hour:02}:{minute:02}"), weekdays, utc_offset_minutes: (offset != 0).then_some(offset) }
}

fn from_cron(expression: &str, offset: i32) -> Result<Schedule, Failure> {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    let [minute, hour, day_of_month, month, day_of_week] = fields[..] else { return Err(Failure::invalid("Cron schedule must have five fields.")) };
    if day_of_month != "*" || month != "*" {
        return Err(Failure::invalid("A cron with a day of the month or a month is not offered: rca runs a schedule at one time of day on some weekdays"));
    }
    let (minute, hour) = (number(minute, "minute", 59).map_err(|_| unsupported_time(expression))?, number(hour, "hour", 23).map_err(|_| unsupported_time(expression))?);
    Ok(fixed(hour, minute, weekdays_of_cron(day_of_week)?, offset))
}

fn unsupported_time(expression: &str) -> Failure {
    Failure::invalid(format!("The recurrence {expression} is not offered: rca runs a schedule at one time of day, not at several minutes or hours"))
}

fn from_rrule(rrule: &str, offset: i32) -> Result<Schedule, Failure> {
    let entries: Vec<(&str, &str)> = rrule.split(';').filter_map(|part| part.split_once('=')).collect();
    let get = |key: &str| entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| *v);
    let (hour, minute) = (number(get("BYHOUR").unwrap_or("9"), "hour", 23)?, number(get("BYMINUTE").unwrap_or("0"), "minute", 59)?);
    match get("FREQ") {
        Some("DAILY") => Ok(fixed(hour, minute, None, offset)),
        Some("WEEKLY") => {
            let codes: Vec<&str> = get("BYDAY").unwrap_or("").split(',').filter(|c| !c.is_empty()).collect();
            let mut days: Vec<u8> = codes.iter().map(|c| DAY_CODES.iter().position(|d| d == c).map(|p| p as u8).ok_or_else(|| Failure::invalid("Invalid recurrence day"))).collect::<Result<_, _>>()?;
            if days.is_empty() {
                return Err(Failure::invalid("Invalid recurrence day"));
            }
            days.sort_unstable();
            days.dedup();
            Ok(fixed(hour, minute, (days.len() < 7).then_some(days), offset))
        }
        Some("HOURLY") => Err(Failure::invalid("An hourly recurrence is not offered: rca has no schedule aligned to the hour, only an interval from when it was saved")),
        _ => Err(Failure::invalid("Unsupported automation recurrence.")),
    }
}

/// The schedule rca keeps for the phone's recurrence and timezone.
pub fn schedule_of(rrule: &str, timezone: &str) -> Result<Schedule, Failure> {
    let offset = offset_minutes(timezone)?;
    if rrule.contains("FREQ=") {
        from_rrule(rrule, offset)
    } else {
        from_cron(rrule, offset)
    }
}

/// The first moment after `now_ms` a schedule is due, for an automation that is switched off (rca names none then).
pub fn next_after(schedule: &Schedule, now_ms: u64) -> Result<u64, Failure> {
    match schedule {
        Schedule::Interval { every_ms } => Ok(now_ms + every_ms),
        Schedule::FixedTime { time_of_day, weekdays, utc_offset_minutes } => {
            let minutes = i64::from(agent_api_schedules::time_of_day_minutes(time_of_day).ok_or_else(|| Failure::invalid("A schedule with a time that is no time of day"))?);
            let offset = i64::from(utc_offset_minutes.unwrap_or(0)) * 60_000;
            let local_now = now_ms as i64 + offset;
            let today = local_now.div_euclid(DAY_MS);
            (0..=7)
                .map(|d| today + d)
                .find(|day| day * DAY_MS + minutes * 60_000 > local_now && weekdays.as_ref().is_none_or(|w| w.contains(&((day + 4).rem_euclid(7) as u8))))
                .map(|day| (day * DAY_MS + minutes * 60_000 - offset) as u64)
                .ok_or_else(|| Failure::invalid("A schedule that never comes due"))
        }
        Schedule::Webhook {} => Err(Failure::invalid("A webhook schedule is no automation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_time(schedule: Schedule) -> (String, Option<Vec<u8>>, Option<i32>) {
        match schedule {
            Schedule::FixedTime { time_of_day, weekdays, utc_offset_minutes } => (time_of_day, weekdays, utc_offset_minutes),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn given_the_phones_recurrences_when_read_then_each_is_a_time_of_day_on_its_weekdays_at_the_zones_offset() {
        // Given the phone's default cron, a daily and a weekly rrule, and zones of UTC and of a fixed offset
        // When each is read
        // Then the time, the weekdays and the offset are the ones it names
        assert_eq!(fixed_time(schedule_of("0 9 * * 1-5", "UTC").unwrap()), ("09:00".into(), Some(vec![1, 2, 3, 4, 5]), None));
        assert_eq!(fixed_time(schedule_of("30 7 * * 0,7", "Etc/GMT+5").unwrap()), ("07:30".into(), Some(vec![0]), Some(-300)));
        assert_eq!(fixed_time(schedule_of("FREQ=DAILY;BYHOUR=18;BYMINUTE=5", "Etc/GMT-2").unwrap()), ("18:05".into(), None, Some(120)));
        assert_eq!(fixed_time(schedule_of("FREQ=WEEKLY;BYDAY=SA,MO", "GMT").unwrap()), ("09:00".into(), Some(vec![1, 6]), None));
        assert_eq!(fixed_time(schedule_of("0 6 * * *", "UTC").unwrap()).1, None);
    }

    #[test]
    fn given_a_recurrence_or_zone_rca_cannot_run_when_read_then_it_is_refused_with_the_reason() {
        for (rrule, zone, reason) in [
            ("FREQ=HOURLY;BYMINUTE=15", "UTC", "hourly"),
            ("*/15 * * * *", "UTC", "several minutes or hours"),
            ("0 9 1 * *", "UTC", "day of the month"),
            ("0 9 * * MON", "UTC", "Invalid recurrence day"),
            ("FREQ=WEEKLY", "UTC", "Invalid recurrence day"),
            ("0 9 * * 1-5", "America/Los_Angeles", "no timezone database"),
        ] {
            let failure = schedule_of(rrule, zone).unwrap_err();
            assert!(failure.message.contains(reason), "{rrule} {zone}: {}", failure.message);
        }
    }

    #[test]
    fn given_a_schedule_when_the_next_occurrence_is_asked_then_it_is_the_next_listed_weekday_at_the_time_in_the_zone() {
        // Given Friday 2026-10-02 12:00 UTC, and a weekday 09:00 schedule at UTC-5 (14:00 UTC)
        let friday_noon = 1_790_942_400_000u64;
        let schedule = schedule_of("0 9 * * 1-5", "Etc/GMT+5").unwrap();
        // When asked, Then it is the same day at 14:00 UTC, and after that time it is Monday 14:00 UTC
        assert_eq!(next_after(&schedule, friday_noon).unwrap(), friday_noon + 2 * 3_600_000);
        assert_eq!(next_after(&schedule, friday_noon + 3 * 3_600_000).unwrap(), friday_noon + 3 * DAY_MS as u64 + 2 * 3_600_000);
        assert_eq!(next_after(&Schedule::Interval { every_ms: 60_000 }, 5).unwrap(), 60_005);
    }
}
