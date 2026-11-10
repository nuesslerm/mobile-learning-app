//! `GET /services/:app/ps`: the containers of one managed app, as the Engine lists them.
use crate::application::ports::{Containers, ContainersError};
use crate::domain::apps;
use crate::domain::containers::{container_name, DockerContainer};
use serde::Serialize;
use std::sync::Arc;

/// One container; a field the Engine did not list is left out of the reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub app: String,
    pub containers: Vec<Row>,
}

impl From<&DockerContainer> for Row {
    fn from(c: &DockerContainer) -> Self {
        Row { name: container_name(c).to_string(), state: c.state.clone(), status: c.status.clone(), image: c.image.clone() }
    }
}

pub struct Ps {
    containers: Arc<dyn Containers>,
}

impl Ps {
    pub fn new(containers: Arc<dyn Containers>) -> Self {
        Self { containers }
    }

    /// The app's containers in any state, in the Engine's order; none (not an empty report) when `app` is no managed app, in
    /// which case the Engine is not asked.
    pub async fn list(&self, app: &str) -> Result<Option<Report>, ContainersError> {
        if !apps::is_known_app(app) {
            return Ok(None);
        }
        let listed = self.containers.list_by_app(app).await?;
        Ok(Some(Report { app: app.to_string(), containers: listed.iter().map(Row::from).collect() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ContainersCall, FakeContainers};

    fn container(name: &str, state: Option<&str>, status: Option<&str>) -> DockerContainer {
        DockerContainer {
            id: name.to_string(),
            names: Some(vec![format!("/{name}")]),
            state: state.map(str::to_string),
            status: status.map(str::to_string),
            image: Some("ns/app:sha-1".to_string()),
        }
    }

    #[tokio::test]
    async fn given_an_app_with_a_stopped_container_and_one_without_a_status_when_listed_then_each_is_a_row_and_a_missing_field_stays_out() {
        // Given the Engine's containers of remote-chat-agent
        let fake = Arc::new(FakeContainers::new().with_app_containers(
            "remote-chat-agent",
            vec![container("remote-chat-agent", Some("running"), Some("Up 2 hours")), container("remote-chat-agent-opencode", Some("created"), None)],
        ));
        // When ps is read for it
        let report = Ps::new(fake.clone()).list("remote-chat-agent").await.expect("listed").expect("a managed app");
        // Then the rows keep the Engine's order and names lose the slash, and the reply leaves out what was not listed
        assert_eq!(
            serde_json::to_string(&report).expect("json"),
            r#"{"app":"remote-chat-agent","containers":[{"name":"remote-chat-agent","state":"running","status":"Up 2 hours","image":"ns/app:sha-1"},{"name":"remote-chat-agent-opencode","state":"created","image":"ns/app:sha-1"}]}"#
        );
        assert_eq!(fake.calls(), [ContainersCall::ListByApp("remote-chat-agent".to_string())]);
    }

    #[tokio::test]
    async fn given_an_app_that_is_not_managed_when_listed_then_nothing_is_returned_and_the_engine_is_not_asked() {
        let fake = Arc::new(FakeContainers::new());
        let ps = Ps::new(fake.clone());

        for app in ["vm-ops-api", "metro-runner", "toString", "nope"] {
            assert_eq!(ps.list(app).await, Ok(None), "{app}");
        }
        assert_eq!(fake.calls(), []);
    }

    #[tokio::test]
    async fn given_an_engine_that_refuses_the_list_when_listed_then_the_refusal_is_the_error() {
        let error = ContainersError::refused("docker API", 500, "down", 200);
        let ps = Ps::new(Arc::new(FakeContainers::new().with_listing_error(error.clone())));

        assert_eq!(ps.list("agent-shell").await, Err(error));
    }
}
