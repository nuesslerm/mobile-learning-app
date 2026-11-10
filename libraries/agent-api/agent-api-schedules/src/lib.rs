//! The `schedules` group of the agent API (`docs/plans/rca-parity/plan.md`, section 2): a schedule is a prompt the service sends to a CLI by itself,
//! on an interval, at a time of day, or when a request reaches its webhook. Neutral names: snake_case fields, times in milliseconds since the epoch.
//! `schedules.watch` (`schedule.subscribe`) is a separate sub-group; its events, and the webhook a `webhook` schedule is reached by, are in `watch`.
//!
//! A schedule starts its turns through the agent group (`thread.create` when it names no thread, then `turn.start`), so a turn it starts is
//! one any mapper can follow with `thread.subscribe`. Its runs are events on the `schedules` stream, numbered by a count of its own.
use agent_api_proto::{GroupName, Method, ParamError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod watch;

pub const GROUP: GroupName = GroupName::new("schedules");

pub const LIST: Method = Method::new(GROUP, "schedule.list");
pub const UPSERT: Method = Method::new(GROUP, "schedule.upsert");
pub const SET_ENABLED: Method = Method::new(GROUP, "schedule.setEnabled");
pub const DELETE: Method = Method::new(GROUP, "schedule.delete");
pub const RUN_NOW: Method = Method::new(GROUP, "schedule.runNow");
pub const ROTATE_WEBHOOK_TOKEN: Method = Method::new(GROUP, "schedule.rotateWebhookToken");

/// Every method of the group.
pub const METHODS: &[Method] = &[LIST, UPSERT, SET_ENABLED, DELETE, RUN_NOW, ROTATE_WEBHOOK_TOKEN];

/// The shortest interval an `upsert` accepts: a stored schedule with a shorter one (an older client) still runs.
pub const MIN_INTERVAL_MS: u64 = 60_000;

/// The most a schedule's `meta` holds, as JSON text.
pub const MAX_META_BYTES: usize = 8 * 1024;

/// The widest offset from UTC a fixed time may name, in minutes.
pub const MAX_UTC_OFFSET_MINUTES: i32 = 14 * 60;

/// The codes of the group. A malformed param is `INVALID_PARAMS`; a refusal of the turn a run starts keeps the agent group's code.
pub mod code {
    pub const SCHEDULE_NOT_FOUND: &str = "SCHEDULE_NOT_FOUND";
    /// `schedule.rotateWebhookToken` on a schedule that is not a webhook.
    pub const NOT_A_WEBHOOK: &str = "NOT_A_WEBHOOK";
    /// The service could not read or write its schedules; nothing changed.
    pub const SCHEDULES_UNAVAILABLE: &str = "SCHEDULES_UNAVAILABLE";

    pub const ALL: [&str; 3] = [SCHEDULE_NOT_FOUND, NOT_A_WEBHOOK, SCHEDULES_UNAVAILABLE];
}

pub mod message {
    pub fn not_found(id: &str) -> String {
        format!("no schedule has the id {id}")
    }

    pub fn not_a_webhook(id: &str) -> String {
        format!("schedule {id} is not a webhook schedule")
    }
}

fn read<T: DeserializeOwned>(params: &Value) -> Result<T, ParamError> {
    let params = if params.is_null() { json!({}) } else { params.clone() };
    serde_json::from_value(params).map_err(|e| ParamError(e.to_string()))
}

fn required(name: &str, value: &str) -> Result<(), ParamError> {
    if value.trim().is_empty() {
        return Err(ParamError(format!("{name} is required")));
    }
    Ok(())
}

/// Minutes after midnight of a `H:MM` or `HH:MM` time of day.
pub fn time_of_day_minutes(text: &str) -> Option<u32> {
    let (hours, minutes) = text.split_once(':')?;
    let digits = |s: &str, len: std::ops::RangeInclusive<usize>| len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(hours, 1..=2) || !digits(minutes, 2..=2) {
        return None;
    }
    let (h, m) = (hours.parse::<u32>().ok()?, minutes.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// When a schedule runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Schedule {
    /// Every `every_ms` after it was saved or enabled, and after each run.
    Interval { every_ms: u64 },
    /// At `time_of_day` (`HH:MM`) on each of `weekdays` (0 is Sunday, none is every day), at `utc_offset_minutes` from UTC (none is UTC).
    FixedTime {
        time_of_day: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weekdays: Option<Vec<u8>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utc_offset_minutes: Option<i32>,
    },
    /// On each request to its webhook with the schedule's token.
    Webhook {},
}

impl Schedule {
    /// What `upsert` accepts: the shapes the type allows, an interval of at least a minute, a time of day that exists.
    pub fn validate(&self) -> Result<(), ParamError> {
        match self {
            Schedule::Interval { every_ms } if *every_ms < MIN_INTERVAL_MS => Err(ParamError(format!("schedule.every_ms must be at least {MIN_INTERVAL_MS}"))),
            Schedule::FixedTime { time_of_day, weekdays, utc_offset_minutes } => {
                if time_of_day_minutes(time_of_day).is_none() {
                    return Err(ParamError("schedule.time_of_day must be HH:MM".into()));
                }
                if weekdays.as_ref().is_some_and(|days| days.is_empty() || days.iter().any(|d| *d > 6)) {
                    return Err(ParamError("schedule.weekdays are 0 (Sunday) to 6 (Saturday), and not empty".into()));
                }
                if utc_offset_minutes.is_some_and(|o| o.abs() > MAX_UTC_OFFSET_MINUTES) {
                    return Err(ParamError("schedule.utc_offset_minutes is within 14 hours of UTC".into()));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn is_webhook(&self) -> bool {
        matches!(self, Schedule::Webhook {})
    }
}

/// `schedule.list`: every schedule, oldest first. It takes no param.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {}

impl ListParams {
    pub fn parse(params: &Value) -> Result<ListParams, ParamError> {
        read(params)
    }
}

/// `schedule.upsert`: a schedule saved. `id` names the one to change (a new one gets an id when it is left out); `require_existing` refuses a
/// save of an id that no longer exists. `thread` is the thread every run goes on; none starts a new thread for each run. A webhook schedule
/// gets its token with its first save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_existing: Option<bool>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub prompt: String,
    pub enabled: bool,
    pub schedule: Schedule,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction_mode: Option<String>,
    /// What the mapper keeps with the schedule and reads back unread (a JSON object of at most `MAX_META_BYTES`): the service never looks inside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl UpsertParams {
    pub fn parse(params: &Value) -> Result<UpsertParams, ParamError> {
        let parsed: UpsertParams = read(params)?;
        for (name, value) in [("title", &parsed.title), ("prompt", &parsed.prompt), ("workspace", &parsed.workspace), ("driver", &parsed.driver)] {
            required(name, value)?;
        }
        for (name, value) in [("id", &parsed.id), ("model", &parsed.model), ("thread", &parsed.thread), ("runtime_mode", &parsed.runtime_mode), ("interaction_mode", &parsed.interaction_mode)] {
            if value.as_deref().is_some_and(str::is_empty) {
                return Err(ParamError(format!("{name} must not be empty")));
            }
        }
        parsed.schedule.validate()?;
        if let Some(meta) = &parsed.meta {
            if !meta.is_object() || meta.to_string().len() > MAX_META_BYTES {
                return Err(ParamError(format!("meta must be an object of at most {MAX_META_BYTES} bytes")));
            }
        }
        Ok(parsed)
    }
}

/// `schedule.setEnabled`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetEnabledParams {
    #[serde(default)]
    pub id: String,
    pub enabled: bool,
}

impl SetEnabledParams {
    pub fn parse(params: &Value) -> Result<SetEnabledParams, ParamError> {
        let parsed: SetEnabledParams = read(params)?;
        required("id", &parsed.id)?;
        Ok(parsed)
    }
}

/// `schedule.delete`, `schedule.runNow` and `schedule.rotateWebhookToken`: one schedule by id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    #[serde(default)]
    pub id: String,
}

impl IdParams {
    pub fn parse(params: &Value) -> Result<IdParams, ParamError> {
        let parsed: IdParams = read(params)?;
        required("id", &parsed.id)?;
        Ok(parsed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Never,
    Running,
    Succeeded,
    Failed,
}

/// The webhook of a `webhook` schedule: the path a request is posted to is `path` and the token, `{path}/{token}`. The token is shown once, by the
/// save or rotation that made it (`MutationResult::webhook_token`); only a digest is kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookView {
    pub path: String,
    pub has_token: bool,
}

/// A schedule as the service keeps and shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub enabled: bool,
    pub schedule: Schedule,
    pub workspace: String,
    pub driver: String,
    pub model: Option<String>,
    pub thread: Option<String>,
    pub runtime_mode: Option<String>,
    pub interaction_mode: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// When the next run is due; none for a disabled schedule and for a webhook.
    pub next_run_ms: Option<u64>,
    pub last_run_ms: Option<u64>,
    pub last_run_status: RunStatus,
    pub last_run_error: Option<String>,
    pub run_count: u64,
    pub webhook: Option<WebhookView>,
    /// The mapper's own data, as `upsert` gave it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

pub fn webhook_path(id: &str) -> String {
    format!("{}{id}", watch::WEBHOOK_PREFIX)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    pub tasks: Vec<TaskView>,
}

/// The answer to a save, an enable or a rotation. `webhook_token` is present only when the call made a token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MutationResult {
    pub task: TaskView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteResult {
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    Timer,
    Manual,
    Webhook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// The turn began.
    Started,
    /// The turn ended well.
    Succeeded,
    /// The turn could not begin or ended badly; `error` says why.
    Failed,
    /// The due run was not started because the schedule's previous turn still runs.
    Skipped,
}

/// One run of a schedule: what started it, and how far it got.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunView {
    pub run: String,
    pub schedule: String,
    pub source: RunSource,
    pub at_ms: u64,
    pub state: RunState,
    pub thread: Option<String>,
    pub turn: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunNowResult {
    pub task: TaskView,
    pub run: RunView,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixtures;
    use agent_api_proto::Request;

    fn parse_params(method: &str, params: &Value) -> Result<(), ParamError> {
        match method {
            "schedule.list" => ListParams::parse(params).map(drop),
            "schedule.upsert" => UpsertParams::parse(params).map(drop),
            "schedule.setEnabled" => SetEnabledParams::parse(params).map(drop),
            "schedule.delete" | "schedule.runNow" | "schedule.rotateWebhookToken" => IdParams::parse(params).map(drop),
            "schedule.subscribe" => watch::SubscribeParams::parse(params).map(drop),
            other => panic!("{other} is no method of the group"),
        }
    }

    fn parse_result(method: &str, result: &Value) -> Result<(), String> {
        let read = |r: Result<(), serde_json::Error>| r.map_err(|e| e.to_string());
        match method {
            "schedule.list" => read(serde_json::from_value::<ListResult>(result.clone()).map(drop)),
            "schedule.upsert" | "schedule.setEnabled" | "schedule.rotateWebhookToken" => read(serde_json::from_value::<MutationResult>(result.clone()).map(drop)),
            "schedule.delete" => read(serde_json::from_value::<DeleteResult>(result.clone()).map(drop)),
            "schedule.runNow" => read(serde_json::from_value::<RunNowResult>(result.clone()).map(drop)),
            "schedule.subscribe" => read(serde_json::from_value::<watch::SubscribeResult>(result.clone()).map(drop)),
            other => panic!("{other} is no method of the group"),
        }
    }

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_every_method_has_a_success_and_a_refusal_the_params_and_results_read() {
        // Given the recorded exchanges of the group
        let recorded = fixtures();
        // Then there is one file per method, and no more
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().chain(watch::METHODS).map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        for fixture in &recorded {
            let method = fixture.method.as_str();
            // And each has an exchange that succeeds and one that is refused
            assert!(fixture.exchanges.iter().any(|e| e.succeeds()) && fixture.exchanges.iter().any(|e| !e.succeeds()), "{method}");
            for exchange in &fixture.exchanges {
                let what = format!("{method}: {}", exchange.name);
                for request in exchange.before.iter().chain([&exchange.request]) {
                    let parsed = Request::from_value(request).unwrap_or_else(|_| panic!("{what}: not a request"));
                    assert!(METHODS.iter().chain(watch::METHODS).any(|m| m.name() == parsed.method), "{what}: {}", parsed.method);
                }
                // And the service calls no executor action to answer, but the checkpoint a turn it starts takes first (`git_checkpoint`)
                let expected: &[&str] = if method == "schedule.runNow" && exchange.succeeds() { &["git_checkpoint"] } else { &[] };
                assert_eq!((exchange.request["method"].as_str(), exchange.actions.iter().map(String::as_str).collect::<Vec<_>>()), (Some(method), expected.to_vec()), "{what}");
                if exchange.succeeds() {
                    assert_eq!(parse_params(method, &exchange.request["params"]), Ok(()), "{what}");
                    // A volatile number is read as a number: its recorded stand-in is text
                    let result = exchange.filled(|pointer| (pointer == "/result/seq").then(|| json!(0)))["result"].clone();
                    assert_eq!(parse_result(method, &result), Ok(()), "{what}");
                } else {
                    // And a refusal is one of the group's codes, or the params' own message, or a refusal of the turn it starts
                    let code = exchange.response["error"]["code"].as_str().unwrap();
                    assert!(code::ALL.contains(&code) || ["INVALID_PARAMS", "NO_EXECUTOR", "TURN_RUNNING", "THREAD_NOT_FOUND"].contains(&code), "{what}: {code}");
                    if code == "INVALID_PARAMS" {
                        assert_eq!(parse_params(method, &exchange.request["params"]).unwrap_err().0, exchange.response["error"]["message"].as_str().unwrap(), "{what}");
                    }
                }
            }
        }
    }

    #[test]
    fn given_params_that_are_not_a_schedule_when_read_then_each_is_refused_with_what_is_wrong() {
        let base = |schedule: Value| json!({"title": "t", "prompt": "p", "enabled": true, "schedule": schedule, "workspace": "w", "driver": "claudeAgent"});
        // Given an interval under a minute, a time that does not exist, weekdays out of range, an offset beyond 14 hours, an empty prompt and an unknown field
        let cases = [
            (base(json!({"type": "interval", "every_ms": 59_999})), "schedule.every_ms must be at least 60000"),
            (base(json!({"type": "fixed_time", "time_of_day": "24:00"})), "schedule.time_of_day must be HH:MM"),
            (base(json!({"type": "fixed_time", "time_of_day": "9:5"})), "schedule.time_of_day must be HH:MM"),
            (base(json!({"type": "fixed_time", "time_of_day": "09:30", "weekdays": [7]})), "schedule.weekdays are 0 (Sunday) to 6 (Saturday), and not empty"),
            (base(json!({"type": "fixed_time", "time_of_day": "09:30", "utc_offset_minutes": 900})), "schedule.utc_offset_minutes is within 14 hours of UTC"),
            (json!({"title": "t", "prompt": " ", "enabled": true, "schedule": {"type": "webhook"}, "workspace": "w", "driver": "d"}), "prompt is required"),
        ];
        for (params, message) in cases {
            // Then each is refused naming the field
            assert_eq!(UpsertParams::parse(&params).unwrap_err().0, message, "{params}");
        }
        let mut big = base(json!({"type": "webhook"}));
        big["meta"] = json!({"text": "x".repeat(MAX_META_BYTES)});
        assert_eq!(UpsertParams::parse(&big).unwrap_err().0, "meta must be an object of at most 8192 bytes");
        big["meta"] = json!("text");
        assert_eq!(UpsertParams::parse(&big).unwrap_err().0, "meta must be an object of at most 8192 bytes");
        big["meta"] = json!({"project": "p1"});
        assert_eq!(UpsertParams::parse(&big).map(|p| p.meta), Ok(Some(json!({"project": "p1"}))));
        let mut extra = base(json!({"type": "webhook"}));
        extra["token"] = json!("x");
        assert!(UpsertParams::parse(&extra).unwrap_err().0.contains("unknown field `token`"));
        // And a webhook, a nine o'clock time and a one minute interval are accepted
        for ok in [json!({"type": "webhook"}), json!({"type": "fixed_time", "time_of_day": "9:05", "weekdays": [0, 6]}), json!({"type": "interval", "every_ms": 60_000})] {
            assert_eq!(UpsertParams::parse(&base(ok.clone())).map(drop), Ok(()), "{ok}");
        }
    }

    #[test]
    fn given_the_methods_of_the_group_when_listed_then_they_are_of_this_group_with_the_owners_names() {
        let names: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        assert_eq!(names, ["schedule.list", "schedule.upsert", "schedule.setEnabled", "schedule.delete", "schedule.runNow", "schedule.rotateWebhookToken"]);
        assert!(METHODS.iter().all(|m| m.group() == GROUP) && GROUP.as_str() == "schedules");
        assert_eq!((time_of_day_minutes("09:30"), time_of_day_minutes("9:30"), time_of_day_minutes("23:59"), time_of_day_minutes("12:60")), (Some(570), Some(570), Some(1439), None));
        assert_eq!(webhook_path("s1"), "/hooks/v1/schedules/s1");
    }
}
