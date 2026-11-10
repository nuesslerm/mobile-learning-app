//! The rules, with no clock and no store: when a schedule is next due, what a webhook request makes of a prompt, whether a token is the one kept.
use agent_api_schedules::{time_of_day_minutes, Schedule};
use serde_json::{json, Value};

const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

/// When `schedule` is next due strictly after `after_ms`. An interval counts on from `after_ms`; a fixed time is the next such time of day on one of
/// its weekdays, at its offset from UTC. A webhook is never due by the clock.
pub fn next_run(schedule: &Schedule, after_ms: u64) -> Option<u64> {
    match schedule {
        Schedule::Interval { every_ms } => after_ms.checked_add(*every_ms),
        Schedule::Webhook {} => None,
        Schedule::FixedTime { time_of_day, weekdays, utc_offset_minutes } => {
            let minutes = i64::from(time_of_day_minutes(time_of_day)?);
            let offset = i64::from(utc_offset_minutes.unwrap_or(0)) * MINUTE_MS;
            let local_after = after_ms as i64 + offset;
            let first_day = local_after.div_euclid(DAY_MS);
            (0..=7).find_map(|ahead| {
                let day = first_day + ahead;
                // 1970-01-01 was a Thursday: day 0 is weekday 4, with Sunday as 0
                let weekday = (day + 4).rem_euclid(7) as u8;
                let candidate = day * DAY_MS + minutes * MINUTE_MS;
                (candidate > local_after && weekdays.as_ref().is_none_or(|days| days.contains(&weekday))).then(|| (candidate - offset) as u64)
            })
        }
    }
}

/// Whether two digests are equal, looking at every byte whichever way the first difference falls.
pub fn digests_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut difference = (a.len() ^ b.len()) as u8;
    for at in 0..a.len().max(b.len()) {
        difference |= a.get(at).copied().unwrap_or(0) ^ b.get(at).copied().unwrap_or(0);
    }
    difference == 0
}

/// What a webhook request carried, for the prompt's placeholders.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Request {
    pub body: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn lookup(request: &Request, path: &str) -> Option<String> {
    let (root, rest) = path.split_once('.').map_or((path, None), |(r, rest)| (r, Some(rest)));
    match (root, rest) {
        ("request", None) => {
            let headers: serde_json::Map<String, Value> = request.headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            Some(json!({"body": request.body, "query": request.query, "headers": headers}).to_string())
        }
        ("body", None) => Some(request.body.clone()),
        ("body", Some(rest)) => {
            let mut value: Value = serde_json::from_str(&request.body).ok()?;
            for part in rest.split('.') {
                value = match value {
                    Value::Array(items) => items.into_iter().nth(part.parse::<usize>().ok()?)?,
                    Value::Object(mut fields) => fields.remove(part)?,
                    _ => return None,
                };
            }
            Some(scalar(&value))
        }
        ("query", Some(name)) => url_pairs(&request.query).into_iter().find(|(k, _)| k == name).map(|(_, v)| v),
        ("headers", Some(name)) => request.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()),
        _ => None,
    }
}

fn url_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (k.to_string(), v.to_string())
        })
        .collect()
}

/// `prompt` with each `{{body}}`, `{{body.a.b}}`, `{{query.name}}`, `{{headers.name}}` and `{{request}}` replaced by what the request carried (nothing
/// for a path it does not have). Any other `{{...}}` stays as written.
pub fn render(prompt: &str, request: &Request) -> String {
    let mut out = String::with_capacity(prompt.len());
    let mut rest = prompt;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("}}") {
            Some(close) => {
                let path = after[..close].trim();
                match lookup(request, path) {
                    Some(text) => out.push_str(&text),
                    None if is_placeholder(path) => {}
                    None => out.push_str(&rest[open..open + 2 + close + 2]),
                }
                rest = &after[close + 2..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_placeholder(path: &str) -> bool {
    path == "request" || ["body", "query", "headers"].iter().any(|root| path == *root || path.starts_with(&format!("{root}.")))
}
