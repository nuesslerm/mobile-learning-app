//! Authored doubles of the gateway's ports. None of them is a recording of a provider or of the sidecar: the upstream answers what a test scripts.
use crate::domain::{Grant, Outbound, Provider};
use crate::ports::{Compressor, Ids, KeySource, TokenStore, Upstream, UpstreamReply};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MemoryTokens {
    grants: Mutex<HashMap<String, Grant>>,
}

impl MemoryTokens {
    /// The digests held, which a test compares with the tokens it was given.
    pub fn digests(&self) -> Vec<String> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).keys().cloned().collect()
    }
}

impl TokenStore for MemoryTokens {
    fn put(&self, digest: String, grant: Grant) {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).insert(digest, grant);
    }

    fn get(&self, digest: &str) -> Option<Grant> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).get(digest).cloned()
    }

    fn remove_thread(&self, thread: &str) {
        self.grants.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, g| g.thread != thread);
    }
}

/// Keys by registry name.
#[derive(Default)]
pub struct FixedKeys(pub Vec<(&'static str, &'static str)>);

impl KeySource for FixedKeys {
    fn key(&self, provider: &Provider) -> Option<String> {
        self.0.iter().find(|(name, _)| *name == provider.key).map(|(_, value)| value.to_string())
    }
}

#[derive(Default)]
pub struct CountingIds(AtomicU64);

impl Ids for CountingIds {
    fn token(&self) -> String {
        format!("{:032}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

type Script = Box<dyn Fn(&Outbound) -> Result<UpstreamReply, String> + Send + Sync>;

/// An upstream that records what it was sent and answers by `script`.
pub struct ScriptedUpstream {
    seen: Mutex<Vec<Outbound>>,
    script: Script,
}

impl ScriptedUpstream {
    pub fn answering(script: impl Fn(&Outbound) -> Result<UpstreamReply, String> + Send + Sync + 'static) -> Arc<ScriptedUpstream> {
        Arc::new(ScriptedUpstream { seen: Mutex::default(), script: Box::new(script) })
    }

    /// Answers 200 with `chunks`, in order.
    pub fn streaming(chunks: &[&str]) -> Arc<ScriptedUpstream> {
        let chunks: Vec<Vec<u8>> = chunks.iter().map(|c| c.as_bytes().to_vec()).collect();
        Self::answering(move |_| Ok(reply(200, chunks.iter().cloned().map(Ok).collect())))
    }

    pub fn seen(&self) -> Vec<Outbound> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

pub fn reply(status: u16, chunks: Vec<Result<Vec<u8>, String>>) -> UpstreamReply {
    UpstreamReply { status, headers: vec![("content-type".into(), "text/event-stream".into())], body: Box::pin(futures_util::stream::iter(chunks)) }
}

#[async_trait]
impl Upstream for ScriptedUpstream {
    async fn send(&self, request: Outbound) -> Result<UpstreamReply, String> {
        let answer = (self.script)(&request);
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push(request);
        answer
    }
}

/// A sidecar that keeps the first `keep` characters of what it is given, or fails.
pub struct FakeSidecar {
    pub keep: usize,
    pub fails: bool,
    pub calls: Mutex<Vec<String>>,
}

impl FakeSidecar {
    pub fn keeping(keep: usize) -> Arc<FakeSidecar> {
        Arc::new(FakeSidecar { keep, fails: false, calls: Mutex::default() })
    }

    pub fn broken() -> Arc<FakeSidecar> {
        Arc::new(FakeSidecar { keep: 0, fails: true, calls: Mutex::default() })
    }
}

#[async_trait]
impl Compressor for FakeSidecar {
    async fn compress(&self, text: &str) -> Result<String, String> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).push(text.to_string());
        if self.fails {
            return Err("the sidecar is down".into());
        }
        Ok(text.chars().take(self.keep).collect())
    }
}
