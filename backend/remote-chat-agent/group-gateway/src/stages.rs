//! The request stages. Each is run on the requests its rule matches, in the order they are registered; credential injection is always last.
use crate::domain::{Auth, Grant, Outbound, Refusal};
use crate::ports::{Compressor, KeySource};
use async_trait::async_trait;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::sync::Arc;

/// What a stage may read besides the request.
pub struct Context<'a> {
    pub grant: &'a Grant,
}

#[async_trait]
pub trait Stage: Send + Sync {
    fn name(&self) -> &'static str;
    async fn apply(&self, request: &mut Outbound, context: &Context<'_>) -> Result<(), Refusal>;
}

/// Sends a request to the model its turn's pinned reroute names for the model the request asks for: the provider changes with the reroute's target
/// (same protocol), and the request's `model` is rewritten. A request for a model no reroute matches (the CLI's own small background model) goes as it is.
pub struct ApplyReroute;

#[async_trait]
impl Stage for ApplyReroute {
    fn name(&self) -> &'static str {
        "reroute"
    }

    async fn apply(&self, request: &mut Outbound, context: &Context<'_>) -> Result<(), Refusal> {
        let Some(asked) = request.model() else { return Ok(()) };
        let from = request.provider;
        let local = asked.split_once('/').filter(|(prefix, _)| from.model_prefixes.contains(prefix)).map_or(asked.as_str(), |(_, rest)| rest);
        let Some(rule) = context.grant.reroutes.iter().find(|r| r.applies(&context.grant.provider, local)) else { return Ok(()) };
        let Some(provider) = crate::domain::provider(&rule.to_provider) else { return Err(Refusal::new(502, "GATEWAY_REROUTE_INVALID", format!("the reroute names the unknown provider {}", rule.to_provider))) };
        if provider.protocol != from.protocol {
            return Err(Refusal::new(502, "GATEWAY_REROUTE_INVALID", "a reroute keeps the protocol"));
        }
        request.provider = provider;
        if let Ok(mut body) = serde_json::from_slice::<serde_json::Value>(&request.body) {
            body["model"] = serde_json::Value::String(rule.to_model.clone());
            request.body = serde_json::to_vec(&body).map_err(|e| Refusal::new(502, "GATEWAY_REROUTE_INVALID", e.to_string()))?;
        }
        Ok(())
    }
}

/// Removes whatever credential the CLI sent (its token) and the headers that belong to one hop, and sets the provider's own from the key the
/// service holds. Without a key the request is refused: nothing is sent unauthenticated, and nothing is sent with the CLI's own header.
pub struct InjectCredential {
    keys: Arc<dyn KeySource>,
}

impl InjectCredential {
    pub fn new(keys: Arc<dyn KeySource>) -> Self {
        Self { keys }
    }
}

const HOP_BY_HOP: [&str; 10] = ["authorization", "x-api-key", "host", "content-length", "connection", "keep-alive", "transfer-encoding", "upgrade", "te", "proxy-authorization"];

#[async_trait]
impl Stage for InjectCredential {
    fn name(&self) -> &'static str {
        "credential"
    }

    async fn apply(&self, request: &mut Outbound, _: &Context<'_>) -> Result<(), Refusal> {
        let Some(key) = self.keys.key(request.provider) else { return Err(Refusal::new(503, "GATEWAY_NO_KEY", format!("the gateway holds no key for {}", request.provider.id))) };
        request.headers.retain(|(name, _)| !HOP_BY_HOP.contains(&name.to_ascii_lowercase().as_str()));
        match request.provider.auth {
            Auth::XApiKey => request.headers.push(("x-api-key".into(), key)),
            Auth::Bearer => request.headers.push(("authorization".into(), format!("Bearer {key}"))),
        }
        request.credentialed = true;
        Ok(())
    }
}

/// Shortens the newest message of a request through the compression sidecar, and nothing before it: the bytes up to that message (`system`,
/// `tools`, every earlier message) and after it are the ones the CLI sent, so the provider's prompt cache keeps hitting on them. A sidecar that fails
/// or does not shorten leaves the request as it was.
pub struct Headroom {
    compressor: Arc<dyn Compressor>,
}

impl Headroom {
    pub fn new(compressor: Arc<dyn Compressor>) -> Self {
        Self { compressor }
    }
}

const TEXT_KEYS: [&str; 3] = ["text", "content", "output"];

fn items_field(request: &Outbound) -> &'static str {
    if request.provider.protocol == crate::domain::Protocol::Responses { "input" } else { "messages" }
}

/// The byte range of the last element of the request's message list, in `body`.
fn last_message(body: &[u8], field: &str) -> Option<(usize, usize)> {
    let top: HashMap<&str, &RawValue> = serde_json::from_slice(body).ok()?;
    let items: Vec<&RawValue> = serde_json::from_str(top.get(field)?.get()).ok()?;
    let last = items.last()?.get();
    let start = (last.as_ptr() as usize).checked_sub(body.as_ptr() as usize)?;
    (start + last.len() <= body.len()).then_some((start, start + last.len()))
}

async fn shorten(compressor: &dyn Compressor, value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                Box::pin(shorten(compressor, item)).await;
            }
        }
        serde_json::Value::Object(map) => {
            if map.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                return;
            }
            for key in TEXT_KEYS {
                match map.get_mut(key) {
                    Some(serde_json::Value::String(text)) => {
                        if let Ok(shorter) = compressor.compress(text).await {
                            if shorter.len() < text.len() {
                                *text = shorter;
                            }
                        }
                    }
                    Some(nested @ (serde_json::Value::Array(_) | serde_json::Value::Object(_))) => Box::pin(shorten(compressor, nested)).await,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

#[async_trait]
impl Stage for Headroom {
    fn name(&self) -> &'static str {
        "headroom"
    }

    async fn apply(&self, request: &mut Outbound, _: &Context<'_>) -> Result<(), Refusal> {
        let Some((start, end)) = last_message(&request.body, items_field(request)) else { return Ok(()) };
        let Ok(mut message) = serde_json::from_slice::<serde_json::Value>(&request.body[start..end]) else { return Ok(()) };
        if message.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            return Ok(());
        }
        shorten(self.compressor.as_ref(), &mut message).await;
        let Ok(replaced) = serde_json::to_vec(&message) else { return Ok(()) };
        if replaced.len() < end - start {
            request.body.splice(start..end, replaced);
        }
        Ok(())
    }
}
