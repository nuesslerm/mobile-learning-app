//! The shared cooldown table (`infra/oracle-cloud/sql/pool_cooldowns.sql`) as the ladder's store: ORDS auto-REST over HTTPS as the
//! `health_rpt` user. Redirects are never followed, so the credential stays with the host it was meant for.

use rca_service_core::application::settings::Settings;
use remote_chat_agent_core::application::ports::Ledgers;
use rca_ladder::ords_store::{OrdsLedgerStore, OrdsReply, OrdsTransport};
use rca_ladder::store::CooldownStore;
use std::time::Duration;
use tokio::runtime::Handle;

pub struct OrdsLedgers {
    base_url: String,
    user: String,
    password: String,
    client: reqwest::Client,
}

impl OrdsLedgers {
    pub fn from_settings(settings: &Settings) -> Self {
        let user = Some(settings.health_reports_ords_user.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| "health_rpt".into());
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("a client with a fixed configuration");
        Self { base_url: settings.pool_cooldowns_ords_base_url.clone(), user, password: settings.health_reports_ords_password.trim().to_string(), client }
    }
}

struct Http {
    runtime: Handle,
    client: reqwest::Client,
    user: String,
    password: String,
}

impl OrdsTransport for Http {
    /// Called from the run's blocking thread, which drives the async client on the service's runtime.
    fn send(&self, method: &str, url: &str, body: Option<&str>) -> Result<OrdsReply, String> {
        let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
        let mut request = self.client.request(method, url).basic_auth(&self.user, Some(&self.password));
        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body.to_string());
        }
        self.runtime.block_on(async {
            let reply = request.send().await.map_err(|e| e.without_url().to_string())?;
            let status = reply.status().as_u16();
            let body = reply.text().await.map_err(|e| e.without_url().to_string())?;
            Ok(OrdsReply { status, body })
        })
    }
}

impl Ledgers for OrdsLedgers {
    fn open(&self, runtime: &Handle, _cwd: &str) -> Result<Box<dyn CooldownStore>, String> {
        if self.base_url.is_empty() || self.password.is_empty() {
            return Err("POOL_COOLDOWNS_ORDS_BASE_URL and HEALTH_REPORTS_ORDS_PASSWORD must be set: the pool cooldowns live in the shared database".into());
        }
        let http = Http { runtime: runtime.clone(), client: self.client.clone(), user: self.user.clone(), password: self.password.clone() };
        Ok(Box::new(OrdsLedgerStore::new(&self.base_url, http)))
    }
}
