use async_trait::async_trait;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

/// What reading one object can answer. `Unavailable` carries no detail on purpose: the adapter logs it, the client never sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadResult {
    Found(Object),
    NotModified,
    Missing,
    Unavailable,
}

#[async_trait]
pub trait ObjectRead: Send + Sync {
    async fn get(&self, key: &str, if_none_match: Option<&str>) -> ReadResult;
}
