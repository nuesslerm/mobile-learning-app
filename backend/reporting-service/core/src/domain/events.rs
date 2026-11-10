//! Docker's `oom` and `die` events: what killed a container, and with which exit code.
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerEvent {
    pub time_ms: i64,
    pub container: String,
    pub action: String,
    pub exit_code: Option<i64>,
}

/// What an event means for the history: the kernel killed the container for memory, or it died with an exit code that is not a clean stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notable {
    OutOfMemory,
    Died(i64),
}

/// The events a `GET /events` reply holds, one JSON object per line; a line that is not an event is skipped.
pub fn parse_events(lines: &str) -> Vec<DockerEvent> {
    lines
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|event| {
            let action = event["Action"].as_str()?.to_string();
            let attributes = &event["Actor"]["Attributes"];
            let container = attributes["name"].as_str()?.trim_start_matches('/').to_string();
            let time_ms = event["timeNano"].as_i64().map(|ns| ns / 1_000_000).or_else(|| event["time"].as_i64().map(|s| s * 1000))?;
            let exit_code = attributes["exitCode"].as_str().and_then(|code| code.parse().ok());
            Some(DockerEvent { time_ms, container, action, exit_code })
        })
        .collect()
}

pub fn notable(event: &DockerEvent) -> Option<Notable> {
    match (event.action.as_str(), event.exit_code) {
        ("oom", _) => Some(Notable::OutOfMemory),
        ("die", Some(code)) if code != 0 => Some(Notable::Died(code)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = concat!(
        "{\"Type\":\"container\",\"Action\":\"oom\",\"Actor\":{\"Attributes\":{\"name\":\"web\"}},\"timeNano\":1700000000123000000}\n",
        "{\"Type\":\"container\",\"Action\":\"die\",\"Actor\":{\"Attributes\":{\"name\":\"/web\",\"exitCode\":\"137\"}},\"time\":1700000001}\n",
        "{\"Type\":\"container\",\"Action\":\"die\",\"Actor\":{\"Attributes\":{\"name\":\"job\",\"exitCode\":\"0\"}},\"time\":1700000002}\n",
        "not json\n",
        "{\"Action\":\"die\"}\n",
    );

    #[test]
    fn given_a_reply_when_parsed_then_events_carry_their_container_time_and_exit_code_and_junk_is_skipped() {
        let events = parse_events(REPLY);
        assert_eq!(events.len(), 3);
        assert_eq!((events[0].container.as_str(), events[0].time_ms, events[0].exit_code), ("web", 1_700_000_000_123, None));
        assert_eq!((events[1].container.as_str(), events[1].time_ms, events[1].exit_code), ("web", 1_700_000_001_000, Some(137)));
    }

    #[test]
    fn given_events_when_classified_then_only_oom_and_failed_exits_are_notable() {
        let notables: Vec<Option<Notable>> = parse_events(REPLY).iter().map(notable).collect();
        assert_eq!(notables, [Some(Notable::OutOfMemory), Some(Notable::Died(137)), None]);
    }
}
