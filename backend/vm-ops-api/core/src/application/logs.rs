//! `GET /services/:app/logs`: the last lines of each of an app's containers, or one container's log followed as it grows.
use crate::application::ports::{Containers, ContainersError, LogFollow};
use crate::domain::apps;
use crate::domain::containers::container_name_or;
use crate::domain::js;
use serde::Serialize;
use serde_json::{Map, Value};
use std::sync::Arc;

const DEFAULT_TAIL: f64 = 200.0;

/// The `tail`, `follow` and `container` query parameters as the request carried them, none when absent.
#[derive(Debug, Clone, Copy, Default)]
pub struct Query<'a> {
    pub tail: Option<&'a str>,
    pub follow: Option<&'a str>,
    pub container: Option<&'a str>,
}

/// Each container's text by name, in the order listed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub app: String,
    pub tail: u32,
    pub logs: Map<String, Value>,
}

/// The container whose log is followed. It is opened when the reply starts to be written, as Node writes its headers first.
pub struct Follow {
    pub container_name: String,
    container_id: String,
    tail: u32,
    containers: Arc<dyn Containers>,
}

impl Follow {
    pub async fn open(&self) -> Result<Box<dyn LogFollow>, ContainersError> {
        self.containers.follow_logs(&self.container_id, self.tail).await
    }
}

/// The text that closes a follow whose stream failed after the reply began, the only place its error can go.
pub fn stream_ended(error: &ContainersError) -> String {
    format!("\n<log stream ended: {error}>\n")
}

pub enum Outcome {
    UnknownApp,
    NoContainers,
    Snapshot(Snapshot),
    Follow(Follow),
    /// Following needs one container: the app has several and none was named.
    Ambiguous(Vec<String>),
    NoSuchContainer(Vec<String>),
}

pub struct Logs {
    containers: Arc<dyn Containers>,
    debug_mode: bool,
}

/// `parseInt(tail || '200', 10)` when it is a positive number, else 200, capped at 1000 (10,000 in debug mode).
fn parse_tail(raw: Option<&str>, debug_mode: bool) -> u32 {
    let requested = js::parse_int(raw.filter(|text| !text.is_empty()).unwrap_or("200"));
    let tail = if requested.is_finite() && requested > 0.0 { requested } else { DEFAULT_TAIL };
    tail.min(if debug_mode { 10_000.0 } else { 1000.0 }) as u32
}

fn wants_follow(raw: Option<&str>) -> bool {
    matches!(raw.unwrap_or_default().to_lowercase().as_str(), "true" | "1")
}

impl Logs {
    pub fn new(containers: Arc<dyn Containers>, debug_mode: bool) -> Self {
        Self { containers, debug_mode }
    }

    /// A container whose logs the Engine refuses reads `<docker API returned N>`; only an Engine that cannot be reached is an error.
    pub async fn read(&self, app: &str, query: &Query<'_>) -> Result<Outcome, ContainersError> {
        if !apps::LOG_APPS.contains(&app) {
            return Ok(Outcome::UnknownApp);
        }
        let listed = self.containers.list_by_app(app).await?;
        if listed.is_empty() {
            return Ok(Outcome::NoContainers);
        }
        let tail = parse_tail(query.tail, self.debug_mode);
        let named: Vec<(&str, String)> = listed.iter().map(|c| (c.id.as_str(), container_name_or(c, &c.id).to_string())).collect();
        if wants_follow(query.follow) {
            return Ok(self.follow(&named, query.container, tail));
        }
        let mut logs = Map::new();
        for (id, name) in &named {
            let text = match self.containers.logs(id, tail).await {
                Ok(text) => text,
                Err(ContainersError::Refused { status, .. }) => format!("<docker API returned {status}>"),
                Err(unreachable) => return Err(unreachable),
            };
            logs.insert(name.clone(), Value::String(text));
        }
        Ok(Outcome::Snapshot(Snapshot { app: app.to_string(), tail, logs }))
    }

    fn follow(&self, named: &[(&str, String)], requested: Option<&str>, tail: u32) -> Outcome {
        let requested = requested.filter(|name| !name.is_empty());
        let target = match requested {
            Some(wanted) => named.iter().find(|(_, name)| name == wanted),
            None if named.len() == 1 => named.first(),
            None => None,
        };
        let names = || named.iter().map(|(_, name)| name.clone()).collect();
        match (target, requested) {
            (Some((id, name)), _) => Outcome::Follow(Follow { container_name: name.clone(), container_id: (*id).to_string(), tail, containers: self.containers.clone() }),
            (None, Some(_)) => Outcome::NoSuchContainer(names()),
            (None, None) => Outcome::Ambiguous(names()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::containers::DockerContainer;
    use crate::testing::{ContainersCall, FakeContainers, FollowEnd};

    fn container(id: &str, name: &str) -> DockerContainer {
        DockerContainer { id: id.to_string(), names: Some(vec![format!("/{name}")]), ..Default::default() }
    }

    fn query<'a>(tail: Option<&'a str>, follow: Option<&'a str>, container: Option<&'a str>) -> Query<'a> {
        Query { tail, follow, container }
    }

    fn snapshot_of(outcome: Outcome) -> Snapshot {
        match outcome {
            Outcome::Snapshot(snapshot) => snapshot,
            _ => panic!("not a snapshot"),
        }
    }

    #[test]
    fn given_tails_in_every_form_when_parsed_then_they_read_as_parse_int_and_are_capped() {
        let cases = [("abc", 200), ("0", 200), ("-5", 200), ("5000", 1000), ("12abc", 12), ("", 200), ("1e3", 1), (" 7", 7), ("99999999999999999999", 1000)];
        for (raw, want) in cases {
            assert_eq!(parse_tail(Some(raw), false), want, "tail={raw:?}");
        }
        assert_eq!((parse_tail(None, false), parse_tail(Some("10000"), true), parse_tail(Some("20000"), true)), (200, 10_000, 10_000));
    }

    #[test]
    fn given_follow_values_when_read_then_only_true_and_1_in_any_case_follow() {
        assert!(["true", "TRUE", "True", "1"].iter().all(|raw| wants_follow(Some(raw))));
        assert!(![None, Some(""), Some("yes"), Some("0"), Some("true ")].iter().any(|raw| wants_follow(*raw)));
    }

    #[tokio::test]
    async fn given_an_app_with_two_containers_when_read_then_each_is_read_in_turn_and_a_refused_one_is_named_by_its_status() {
        // Given leetcode-backend's container and its sidecar, whose logs the Engine refuses with a 500
        let fake = Arc::new(
            FakeContainers::new()
                .with_app_containers("leetcode-backend", vec![container("l1", "leetcode-backend"), container("l2", "leetcode-backend-tailscale")])
                .with_logs("l1", Ok("started\nwarn: slow\n".to_string()))
                .with_logs("l2", Err(ContainersError::refused("docker API", 500, "logs failed", 300))),
        );
        // When its logs are read with a tail of 5000
        let snapshot = snapshot_of(Logs::new(fake.clone(), false).read("leetcode-backend", &query(Some("5000"), None, None)).await.expect("read"));
        // Then the tail is capped, the refused container reads <docker API returned 500>, and the Engine read them in order
        let body = js::stringify(&serde_json::to_value(&snapshot).expect("a value"));
        assert_eq!(
            body,
            r#"{"app":"leetcode-backend","tail":1000,"logs":{"leetcode-backend":"started\nwarn: slow\n","leetcode-backend-tailscale":"<docker API returned 500>"}}"#
        );
        let wanted = [
            ContainersCall::ListByApp("leetcode-backend".to_string()),
            ContainersCall::Logs { container_id: "l1".to_string(), tail: 1000 },
            ContainersCall::Logs { container_id: "l2".to_string(), tail: 1000 },
        ];
        assert_eq!(fake.calls(), wanted);
    }

    #[tokio::test]
    async fn given_the_control_plane_an_unknown_app_and_an_app_with_no_containers_when_read_then_each_gets_its_own_outcome() {
        // Given an Engine that lists vm-ops-api's container and nothing of leetcode-backend
        let fake = Arc::new(
            FakeContainers::new()
                .with_app_containers("vm-ops-api", vec![container("v1", "vm-ops-api-ops-api-1")])
                .with_logs("v1", Ok("listening\n".to_string())),
        );
        let logs = Logs::new(fake.clone(), false);
        // When the control plane's logs are read, Then they are served though vm-ops-api is no managed app
        assert_eq!(snapshot_of(logs.read("vm-ops-api", &Query::default()).await.expect("read")).tail, 200);
        // When an app with no containers is read, Then there are none
        assert!(matches!(logs.read("leetcode-backend", &Query::default()).await, Ok(Outcome::NoContainers)));
        // When an unknown app and the removed metro-runner are read, Then the Engine is never asked
        let before = fake.calls().len();
        for app in ["nope", "metro-runner"] {
            assert!(matches!(logs.read(app, &Query::default()).await, Ok(Outcome::UnknownApp)), "{app}");
        }
        assert_eq!(fake.calls().len(), before);
    }

    #[tokio::test]
    async fn given_an_app_with_two_containers_when_followed_then_one_must_be_named_and_the_names_are_listed_when_it_is_not_or_is_wrong() {
        // Given remote-chat-agent with an app container and an opencode container
        let fake = Arc::new(
            FakeContainers::new()
                .with_app_containers("remote-chat-agent", vec![container("r1", "remote-chat-agent"), container("r4", "remote-chat-agent-opencode")])
                .with_follow("r4", vec![Ok("one".to_string())], FollowEnd::Ends),
        );
        let logs = Logs::new(fake.clone(), false);
        let names = vec!["remote-chat-agent".to_string(), "remote-chat-agent-opencode".to_string()];
        // When it is followed with no container, Then it is ambiguous; with an unknown one, no such container
        assert!(matches!(logs.read("remote-chat-agent", &query(None, Some("true"), None)).await, Ok(Outcome::Ambiguous(listed)) if listed == names));
        assert!(matches!(logs.read("remote-chat-agent", &query(None, Some("1"), Some("nope"))).await, Ok(Outcome::NoSuchContainer(listed)) if listed == names));
        // When the opencode container is named, Then its stream opens with the tail, and nothing was opened before
        assert_eq!(fake.calls().iter().filter(|c| matches!(c, ContainersCall::FollowLogs { .. })).count(), 0);
        let Ok(Outcome::Follow(follow)) = logs.read("remote-chat-agent", &query(Some("5"), Some("TRUE"), Some("remote-chat-agent-opencode"))).await else {
            panic!("not a follow");
        };
        assert_eq!(follow.container_name, "remote-chat-agent-opencode");
        let mut stream = follow.open().await.expect("a stream");
        assert_eq!(stream.next_text().await, Ok(Some("one".to_string())));
        assert!(fake.calls().contains(&ContainersCall::FollowLogs { container_id: "r4".to_string(), tail: 5 }));
    }

    #[tokio::test]
    async fn given_an_app_with_one_container_when_followed_with_no_name_then_that_container_is_followed() {
        let fake = Arc::new(FakeContainers::new().with_app_containers("leetcode-backend", vec![container("l1", "leetcode-backend")]));

        let outcome = Logs::new(fake, false).read("leetcode-backend", &query(None, Some("true"), None)).await;

        assert!(matches!(outcome, Ok(Outcome::Follow(follow)) if follow.container_name == "leetcode-backend"));
    }

    #[test]
    fn given_an_engine_failure_when_a_follow_ends_then_the_text_names_it_on_lines_of_its_own() {
        let error = ContainersError::refused("docker API", 500, r#"{"message":"logs failed"}"#, 300);

        assert_eq!(stream_ended(&error), "\n<log stream ended: docker API returned 500: {\"message\":\"logs failed\"}>\n");
    }
}
