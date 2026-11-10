//! What `/health` says: the build this process runs, when it started and what time it is.
use crate::application::ports::Clock;
use crate::domain::js;
use serde::Serialize;
use std::sync::Arc;

/// What the process knows about its own build and place, read from its environment at start.
#[derive(Debug, Clone)]
pub struct Build {
    pub commit: String,
    pub host: String,
    pub debug_mode: bool,
    pub server_script_sha: String,
    pub image_tag: String,
}

/// The reply body; the fields are written in the order Node wrote them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ok: bool,
    pub commit: String,
    pub started_at: String,
    pub host: String,
    pub debug_mode: bool,
    pub time: String,
    pub server_script_sha: String,
    pub image_tag: String,
}

pub struct Health {
    build: Build,
    started_at: String,
    clock: Arc<dyn Clock>,
}

impl Health {
    /// The moment it is made is the moment the service started.
    pub fn new(build: Build, clock: Arc<dyn Clock>) -> Self {
        let started_at = js::iso_string(clock.now_ms());
        Self { build, started_at, clock }
    }

    pub fn report(&self) -> Report {
        Report {
            ok: true,
            commit: self.build.commit.clone(),
            started_at: self.started_at.clone(),
            host: self.build.host.clone(),
            debug_mode: self.build.debug_mode,
            time: js::iso_string(self.clock.now_ms()),
            server_script_sha: self.build.server_script_sha.clone(),
            image_tag: self.build.image_tag.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeClock;

    fn build() -> Build {
        Build { commit: "0123abc".into(), host: "main".into(), debug_mode: false, server_script_sha: "0123456789ab".into(), image_tag: "sha-1234567".into() }
    }

    #[test]
    fn given_a_service_started_at_noon_when_health_is_read_an_hour_later_then_it_reports_both_times_and_the_build() {
        // Given a service that started at noon on 4 Oct 2026
        let clock = Arc::new(FakeClock::at(1_791_115_200_000));
        let health = Health::new(build(), clock.clone());
        // When an hour passes and /health is read
        clock.set(1_791_115_200_000 + 3_600_000);
        let report = health.report();
        // Then it carries the build, the start-up time and the time of the read
        assert_eq!(
            report,
            Report {
                ok: true,
                commit: "0123abc".into(),
                started_at: "2026-10-04T12:00:00.000Z".into(),
                host: "main".into(),
                debug_mode: false,
                time: "2026-10-04T13:00:00.000Z".into(),
                server_script_sha: "0123456789ab".into(),
                image_tag: "sha-1234567".into(),
            }
        );
    }

    #[test]
    fn given_a_report_when_serialised_then_its_keys_are_camel_case_in_the_order_node_wrote_them() {
        let health = Health::new(build(), Arc::new(FakeClock::at(0)));
        let json = serde_json::to_string(&health.report()).expect("a report serialises");
        assert_eq!(
            json,
            r#"{"ok":true,"commit":"0123abc","startedAt":"1970-01-01T00:00:00.000Z","host":"main","debugMode":false,"time":"1970-01-01T00:00:00.000Z","serverScriptSha":"0123456789ab","imageTag":"sha-1234567"}"#
        );
    }
}
