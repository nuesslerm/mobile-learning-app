use async_trait::async_trait;
use capture_core::application::ports::{BrowserLauncher, BrowserSession, LaunchOptions, PortError};
use capture_core::domain::cookies::Cookie;
use capture_core::domain::sign_in::is_signed_in;
use reqwest::Client;
use serde_json::{json, Value};

const SESSION_NAME: &str = "leetcode-auth-capture";

/// Runs inside the signed-in page itself (credentials: same-origin), so no cookie read is needed for the check.
const SIGN_IN_CHECK: &str = r#"(async () => {
          const response = await fetch('/graphql', {
            method: 'POST',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              query: 'query globalData { userStatus { isSignedIn username } }',
            }),
          });
          return response.json();
        })()"#;

/// The browser daemon of the Rust `wf` (`wf browser`): the one sanctioned browser-automation surface of the repository. The daemon is
/// found with `wf browser --json status` (or started with `start`), then driven with `POST <url>/call {method, args}`.
pub struct WfBrowser {
    client: Client,
    wf: String,
}

impl WfBrowser {
    /// The `wf` that `wf_bin` names, or the one on `PATH`.
    pub fn new(client: Client, wf_bin: Option<&str>) -> Self {
        Self::with_program(client, wf_bin.filter(|v| !v.is_empty()).unwrap_or("wf"))
    }

    pub fn with_program(client: Client, wf: impl Into<String>) -> Self {
        Self { client, wf: wf.into() }
    }

    async fn wf_browser(&self, args: &[&str]) -> Result<(bool, String, String), PortError> {
        let out = tokio::process::Command::new(&self.wf)
            .args(["browser", "--json"])
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| PortError::new(format!("cannot run \"{}\": {e}", self.wf)))?;
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned()))
    }

    /// The daemon's URL: the running one's, or a fresh daemon's.
    async fn daemon_url(&self) -> Result<String, PortError> {
        let (ok, stdout, _) = self.wf_browser(&["status"]).await?;
        if let Some(url) = ok.then(|| url_from(&stdout)).flatten() {
            return Ok(url);
        }
        let (ok, stdout, stderr) = self.wf_browser(&["start"]).await?;
        match ok.then(|| url_from(&stdout)).flatten() {
            Some(url) => Ok(url),
            None => {
                let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
                Err(PortError::new(format!("wf browser start failed: {detail}")))
            }
        }
    }
}

/// The last line of a `--json` reply that parses and carries `result.url`.
fn url_from(stdout: &str) -> Option<String> {
    stdout.trim().lines().rev().find_map(|line| serde_json::from_str::<Value>(line).ok()?.pointer("/result/url")?.as_str().map(str::to_string))
}

struct Session {
    client: Client,
    url: String,
}

impl Session {
    async fn call(&self, method: &str, args: Value) -> Result<Value, PortError> {
        let reply: Value = self
            .client
            .post(format!("{}/call", self.url))
            .json(&json!({ "method": method, "args": args }))
            .send()
            .await
            .map_err(|e| PortError::new(e.to_string()))?
            .json()
            .await
            .map_err(|e| PortError::new(e.to_string()))?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            let why = reply.get("error").and_then(Value::as_str).map_or_else(|| format!("browser command {method} failed"), str::to_string);
            return Err(PortError::new(why));
        }
        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }
}

#[async_trait]
impl BrowserLauncher for WfBrowser {
    async fn launch(&self, options: &LaunchOptions, login_url: &str) -> Result<Box<dyn BrowserSession>, PortError> {
        let session = Session { client: self.client.clone(), url: self.daemon_url().await? };
        let mut open = json!({ "name": SESSION_NAME, "renderMode": "plain", "keyboardMode": "none", "headless": false });
        if let Some(display) = &options.display {
            // Chromium's own sandbox wants a user namespace the container does not grant, and its window goes to the virtual display.
            let mut env: serde_json::Map<String, Value> = std::env::vars().map(|(k, v)| (k, Value::String(v))).collect();
            env.insert("DISPLAY".into(), Value::String(display.clone()));
            open["launchArgs"] = json!(["--no-sandbox"]);
            open["launchEnv"] = Value::Object(env);
        }
        session.call("open", open).await?;
        session.call("navigate", json!({ "name": SESSION_NAME, "url": login_url })).await?;
        Ok(Box::new(session))
    }
}

#[async_trait]
impl BrowserSession for Session {
    async fn check_signed_in(&self) -> Result<bool, PortError> {
        let result = self.call("eval", json!({ "name": SESSION_NAME, "expression": SIGN_IN_CHECK })).await?;
        let text = result.get("result").and_then(Value::as_str).ok_or_else(|| PortError::new("the sign-in check returned no result"))?;
        let reply: Value = serde_json::from_str(text).map_err(|e| PortError::new(format!("the sign-in check returned: {e}")))?;
        Ok(is_signed_in(&reply))
    }

    async fn cookies(&self) -> Result<Vec<Cookie>, PortError> {
        let result = self.call("cookies", json!({ "name": SESSION_NAME })).await?;
        serde_json::from_value(result.get("cookies").cloned().unwrap_or(Value::Null)).map_err(|e| PortError::new(format!("unreadable cookies: {e}")))
    }

    async fn close(&self) {
        let _ = self.call("close", json!({ "name": SESSION_NAME })).await;
    }
}
