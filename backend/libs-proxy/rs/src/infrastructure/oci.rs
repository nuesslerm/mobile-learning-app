//! Reads objects through a read pre-authenticated URL. Nothing of the link (URL, query, headers) is logged or returned.
use super::config::Secret;
use async_trait::async_trait;
use libs_proxy_core::{Object, ObjectRead, ReadResult};
use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::{redirect, Client, StatusCode};
use std::time::Duration;

pub struct OciObjectRead {
    client: Client,
    prefix: Secret,
}

impl OciObjectRead {
    pub fn new(par_url: &Secret) -> Result<OciObjectRead, reqwest::Error> {
        let client = Client::builder().redirect(redirect::Policy::none()).connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(60)).build()?;
        let mut prefix = par_url.expose().to_string();
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        Ok(OciObjectRead { client, prefix: Secret::new(prefix) })
    }
}

#[async_trait]
impl ObjectRead for OciObjectRead {
    async fn get(&self, key: &str, if_none_match: Option<&str>) -> ReadResult {
        let mut request = self.client.get(format!("{}{key}", self.prefix.expose()));
        if let Some(tag) = if_none_match {
            request = request.header(IF_NONE_MATCH, tag);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                let error = error.without_url();
                tracing::error!(timeout = error.is_timeout(), connect = error.is_connect(), "bucket.read.failed");
                return ReadResult::Unavailable;
            }
        };
        match response.status() {
            StatusCode::OK => {
                let etag = response.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_string);
                match response.bytes().await {
                    Ok(bytes) => ReadResult::Found(Object { bytes: bytes.to_vec(), etag }),
                    Err(error) => {
                        tracing::error!(timeout = error.is_timeout(), "bucket.read.body_failed");
                        ReadResult::Unavailable
                    }
                }
            }
            StatusCode::NOT_MODIFIED => ReadResult::NotModified,
            StatusCode::NOT_FOUND => ReadResult::Missing,
            status => {
                tracing::error!(status = status.as_u16(), "bucket.read.rejected");
                ReadResult::Unavailable
            }
        }
    }
}
