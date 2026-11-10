use async_trait::async_trait;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reporting_core::application::{DockerEngine, PortError};
use reporting_core::domain::container::{ContainerStats, ContainerSummary, DiskContainer};
use reporting_core::domain::events::{parse_events, DockerEvent};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// The Docker Engine API over the mounted Unix socket: no `docker` CLI in the image, no subprocess.
pub struct DockerSocket {
    client: reqwest::Client,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Df {
    #[serde(default)]
    containers: Option<Vec<DiskContainer>>,
}

impl DockerSocket {
    pub fn new(socket: &Path) -> Result<Self, reqwest::Error> {
        Ok(Self { client: reqwest::Client::builder().unix_socket(socket).timeout(Duration::from_secs(30)).build()? })
    }

    async fn get_text(&self, path: &str) -> Result<String, PortError> {
        let response = self.client.get(format!("http://docker{path}")).send().await.map_err(PortError::new)?;
        let status = response.status();
        let body = response.text().await.map_err(PortError::new)?;
        if !status.is_success() {
            return Err(PortError::new(format!("docker request failed with status {}: {body}", status.as_u16())));
        }
        Ok(body)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, PortError> {
        let response = self.client.get(format!("http://docker{path}")).send().await.map_err(PortError::new)?;
        let status = response.status();
        let body = response.text().await.map_err(PortError::new)?;
        if !status.is_success() {
            return Err(PortError::new(format!("docker request failed with status {}: {body}", status.as_u16())));
        }
        serde_json::from_str(&body).map_err(PortError::new)
    }
}

#[async_trait]
impl DockerEngine for DockerSocket {
    async fn list_containers(&self) -> Result<Vec<ContainerSummary>, PortError> {
        self.get("/containers/json").await
    }

    async fn container_stats(&self, id: &str) -> Result<ContainerStats, PortError> {
        self.get(&format!("/containers/{}/stats?stream=false", utf8_percent_encode(id, NON_ALPHANUMERIC))).await
    }

    /// `verbose=1` is what makes the response carry per-container `SizeRw`; the plain call returns only aggregate totals.
    async fn system_df(&self) -> Result<Vec<DiskContainer>, PortError> {
        Ok(self.get::<Df>("/system/df?verbose=1").await?.containers.unwrap_or_default())
    }

    /// With `until` in the past the engine answers what happened since `since` and closes the reply.
    async fn events(&self, since_s: i64, until_s: i64) -> Result<Vec<DockerEvent>, PortError> {
        let filters = utf8_percent_encode(r#"{"type":["container"],"event":["oom","die"]}"#, NON_ALPHANUMERIC);
        Ok(parse_events(&self.get_text(&format!("/events?since={since_s}&until={until_s}&filters={filters}")).await?))
    }
}
