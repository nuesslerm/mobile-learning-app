//! The `ModelApi` over an OpenAI-compatible chat endpoint (Kimi Code by default, `KIMI_API_KEY` from the secrets registry): the service's own
//! requests for a commit message. The key goes in the `Authorization` header of the one configured address and nowhere else, and no reply of
//! the agent API carries it. Usage and credit are not reported by this API, so they are not offered. Not run against the real endpoint.
use async_trait::async_trait;
use agent_api_accounts::{Credit, Usage};
use remote_chat_agent_core::application::ports::{Outbound, Payload, Post};
use rca_service_core::application::settings::Settings;
use rca_group_accounts::{Completed, Completion, ModelApi, ModelError};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct HttpModelApi {
    outbound: Arc<dyn Outbound>,
    base_url: String,
    model: String,
    key: Option<String>,
}

impl HttpModelApi {
    pub fn new(outbound: Arc<dyn Outbound>, base_url: &str, model: &str, key: Option<String>) -> HttpModelApi {
        HttpModelApi { outbound, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string(), key: key.filter(|k| !k.is_empty()) }
    }

    /// The API as the settings configure it: `KIMI_API_KEY`, and `RCA_MODEL_API_BASE_URL` and `RCA_MODEL_API_MODEL` to change where and what.
    pub fn from_settings(settings: &Settings, outbound: Arc<dyn Outbound>) -> HttpModelApi {
        HttpModelApi::new(outbound, &settings.model_api_base_url, &settings.model_api_model, settings.kimi_api_key.clone())
    }
}

/// The text of the first choice of a chat completion.
fn answer_text(body: &Value) -> Option<String> {
    body["choices"][0]["message"]["content"].as_str().map(str::to_string)
}

#[async_trait]
impl ModelApi for HttpModelApi {
    async fn complete(&self, request: Completion) -> Result<Completed, ModelError> {
        let Some(key) = &self.key else { return Err(ModelError::Unavailable("no model API key is configured (KIMI_API_KEY)".into())) };
        let payload = json!({"model": self.model, "max_tokens": request.max_tokens, "messages": [{"role": "system", "content": request.system}, {"role": "user", "content": request.prompt}]});
        let post = Post { url: format!("{}/chat/completions", self.base_url), headers: vec![("Authorization".into(), format!("Bearer {key}"))], basic_auth: None, payload: Payload::Json(payload) };
        let answer = self.outbound.post(post).await.map_err(|e| ModelError::Unavailable(format!("the model API could not be reached: {}", e.without_url)))?;
        if !(200..300).contains(&answer.status) {
            return Err(ModelError::Unavailable(format!("the model API answered {}", answer.status)));
        }
        let body: Value = serde_json::from_slice(&answer.body).map_err(|_| ModelError::Unavailable("the model API answered with no JSON".into()))?;
        let text = answer_text(&body).ok_or_else(|| ModelError::Unavailable("the model API answered with no message".into()))?;
        Ok(Completed { text, model: body["model"].as_str().unwrap_or(&self.model).to_string() })
    }

    async fn usage(&self, _provider: &str) -> Result<Option<Usage>, ModelError> {
        Ok(None)
    }

    async fn reset_credit(&self, provider: &str) -> Result<Credit, ModelError> {
        Err(ModelError::NotSupported(format!("{provider} has no credit the service can reset")))
    }
}
