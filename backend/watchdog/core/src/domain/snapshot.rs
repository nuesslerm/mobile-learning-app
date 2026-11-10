use serde_json::Value;

const LABEL_PROJECT: &str = "com.docker.compose.project";
const LABEL_SERVICE: &str = "com.docker.compose.service";
const LABEL_WORKING_DIR: &str = "com.docker.compose.project.working_dir";
const LABEL_CONFIG_FILES: &str = "com.docker.compose.project.config_files";

/// What one container looks like to the watchdog: its health, its restart counter, whose network it joins and the compose labels
/// that say how to recreate it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    pub running: bool,
    /// When it stopped, in Unix ms, for a container the engine reports as `exited`; none for any other state.
    pub exited_at: Option<i64>,
    /// When it was created, in Unix ms, for a container the engine reports as `created` (never started); none for any other state.
    pub created_at: Option<i64>,
    /// The container whose network namespace this one shares (`network_mode: service:<x>`), by id as the engine records it.
    pub network_of: Option<String>,
    pub health: Option<String>,
    pub restart_count: i64,
    pub project: Option<String>,
    pub service: Option<String>,
    pub working_dir: Option<String>,
    pub config_files: Vec<String>,
}

/// `docker inspect` output (a JSON array) as snapshots; anything that is not an array is no containers.
pub fn parse_snapshots(inspected: &Value) -> Vec<Snapshot> {
    let Some(containers) = inspected.as_array() else { return Vec::new() };
    containers
        .iter()
        .map(|container| {
            let label = |key: &str| container["Config"]["Labels"][key].as_str().filter(|value| !value.is_empty()).map(str::to_string);
            Snapshot {
                id: container["Id"].as_str().unwrap_or_default().to_string(),
                name: container["Name"].as_str().unwrap_or_default().strip_prefix('/').unwrap_or(container["Name"].as_str().unwrap_or_default()).to_string(),
                running: container["State"]["Running"].as_bool().unwrap_or(false),
                exited_at: container["State"]["Status"].as_str().filter(|status| *status == "exited").and_then(|_| container["State"]["FinishedAt"].as_str()).and_then(parse_time_ms),
                created_at: container["State"]["Status"].as_str().filter(|status| *status == "created").and_then(|_| container["Created"].as_str()).and_then(parse_time_ms),
                network_of: container["HostConfig"]["NetworkMode"].as_str().and_then(|mode| mode.strip_prefix("container:")).map(str::to_string),
                health: container["State"]["Health"]["Status"].as_str().map(str::to_string),
                restart_count: container["RestartCount"].as_i64().unwrap_or(0),
                project: label(LABEL_PROJECT),
                service: label(LABEL_SERVICE),
                working_dir: label(LABEL_WORKING_DIR),
                config_files: label(LABEL_CONFIG_FILES).map(|files| files.split(',').filter(|file| !file.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
            }
        })
        .collect()
}

/// An engine timestamp (`2026-10-23T08:41:07.123456789Z`) as Unix ms. Only the UTC form the engine writes is read; the zero time of a
/// container that never ran (`0001-01-01T00:00:00Z`) is none.
fn parse_time_ms(text: &str) -> Option<i64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let mut ymd = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (ymd.next()??, ymd.next()??, ymd.next()??);
    let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut hms = clock.splitn(3, ':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (hms.next()??, hms.next()??, hms.next()??);
    if year < 1970 || !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let millis = format!("{fraction:0<3}")[..3].parse::<i64>().ok()?;
    // Howard Hinnant's days_from_civil
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_engine_timestamps_when_parsed_then_utc_ms_and_the_zero_time_is_none() {
        assert_eq!(parse_time_ms("2023-11-14T22:13:20Z"), Some(1_700_000_000_000));
        assert_eq!(parse_time_ms("2023-11-14T22:13:20.5Z"), Some(1_700_000_000_500));
        assert_eq!(parse_time_ms("2023-11-14T22:13:20.123456789Z"), Some(1_700_000_000_123));
        assert_eq!(parse_time_ms("2024-02-29T00:00:00Z"), Some(1_709_164_800_000));
        assert_eq!([parse_time_ms("0001-01-01T00:00:00Z"), parse_time_ms("2023-11-14 22:13:20"), parse_time_ms("")], [None, None, None]);
    }

    #[test]
    fn given_containers_in_each_state_when_inspected_then_only_an_exited_one_has_a_stop_time_and_only_a_created_one_a_creation_time() {
        let at = |status: &str| json!({ "Id": "a", "Name": "/a", "State": { "Status": status, "Running": status == "running", "FinishedAt": "2023-11-14T22:13:20Z" }, "Created": "2023-11-14T22:13:20.5Z" });
        let states = ["exited", "running", "created", "dead", "paused"];
        let seen: Vec<Option<i64>> = states.map(|s| parse_snapshots(&json!([at(s)]))[0].exited_at).to_vec();
        assert_eq!(seen, [Some(1_700_000_000_000), None, None, None, None]);
        let seen: Vec<Option<i64>> = states.map(|s| parse_snapshots(&json!([at(s)]))[0].created_at).to_vec();
        assert_eq!(seen, [None, None, Some(1_700_000_000_500), None, None]);
    }
}
