//! The one Bot API call the preview and plan routes make: their annotations go to Telegram.
use remote_chat_agent_core::application::ports::{Outbound, Payload, Post};
use serde_json::Value;

/// `sendMessage` through the Bot API; Err carries the error text the route answers 502 with.
pub async fn send_telegram(outbound: &dyn Outbound, token: &str, params: &Value) -> Result<(), String> {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let post = Post { url, headers: Vec::new(), basic_auth: None, payload: Payload::Json(params.clone()) };
    // The error is shown to the caller, and the client's text carries the URL, which holds the bot token.
    let reply = outbound.post(post).await.map_err(|e| format!("fetch failed: {}", e.without_url))?;
    let parsed: Value = serde_json::from_slice(&reply.body).map_err(|_| "error decoding response body".to_string())?;
    if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(parsed.get("description").and_then(Value::as_str).unwrap_or("Telegram error").to_string())
    }
}
