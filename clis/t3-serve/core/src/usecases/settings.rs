//! The M10 rows: the server's settings, the provider accounts and the host. Settings are device state (the agent API's `state` group keeps them
//! under the `t3` scope), so a restart of t3 keeps them. A provider's login is the agent API's `accounts` group: the CLI signs itself in and writes
//! its tokens in its own directory, so this side only ever holds a session's id and the prompt the person acts on, never a token.
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Follow, Op};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use t3_contracts::time::iso;
use t3_contracts::wire::{self, Model};

const SETTINGS_SCOPE: &str = "t3";
const SETTINGS_KEY: &str = "server.settings";
/// How long `provider.auth.complete` waits for a login to end before it answers where the login stands.
const COMPLETE_WAIT_MS: u64 = 15_000;
/// The rest between two looks of a stream at the login the phone has started.
const LOOK: Duration = Duration::from_millis(150);
/// How often the database is looked at for the storage stream.
const STORAGE_LOOK: Duration = Duration::from_secs(5);
const USAGE_CONTRACT_VERSION: u64 = 6;

/// What the settings and the accounts rows keep between calls: the login session each provider instance has (its id and method), and a lock that
/// makes a settings update one read and one write.
#[derive(Default)]
pub struct Held {
    sessions: Mutex<HashMap<String, (String, String)>>,
    settings: Mutex<()>,
    /// Makes the first signing key of attachment and asset URLs one read and one write.
    pub(super) signing: Mutex<()>,
    /// Makes a project's lookup by its folder and its making one step, so two calls for the same folder make one project.
    pub(super) projects: Mutex<()>,
    /// The review guides being written or that failed, by pull request (`Running` and `Failed` as T3's run union has them).
    pub(super) guide_runs: Mutex<HashMap<String, Value>>,
}

impl Held {
    fn session(&self, instance: &str) -> Option<(String, String)> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).get(instance).cloned()
    }

    fn keep(&self, instance: &str, auth: &str, method: &str) {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(instance.into(), (auth.into(), method.into()));
    }

    fn forget(&self, instance: &str) {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(instance);
    }
}

/// What the storage stream last told the phone.
pub struct StorageFold {
    told: Value,
}

/// What a login stream has told the phone: the provider instance it is for.
pub struct AuthFold {
    instance: String,
}

fn setup_error(instance: &str, operation: &str, e: &AgentError) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "ProviderSetupError", "instanceId": instance, "operation": operation, "detail": e.message()}))
}

fn refusal(instance: &str, operation: &str, detail: &str) -> QueryFailure {
    setup_error(instance, operation, &AgentError::Failed(detail.into()))
}

fn settings_error(path: &str, operation: &str, detail: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": "ServerSettingsError", "settingsPath": path, "operation": operation, "cause": detail}))
}

fn method_name(id: &str) -> &str {
    match id {
        "device_code" => "Sign in with a device code",
        "paste" => "Sign in by pasting a code",
        other => other,
    }
}

/// `ProviderAuthState` of a login session (the accounts group's `AuthView`), or the idle state of a provider with none.
fn auth_state(instance: &str, view: Option<&Value>, methods: &[Value]) -> Value {
    let mut state = json!({"instanceId": instance, "phase": "idle", "flowId": null, "authorizationUrl": null, "expiresAt": null, "message": null, "interaction": null, "credentialOwner": "provider"});
    if !methods.is_empty() {
        state["methods"] = json!(methods);
    }
    let Some(view) = view.filter(|v| v.is_object()) else { return state };
    let (auth, url, code) = (view["auth"].as_str().unwrap_or_default(), view["url"].as_str(), view["code"].as_str());
    let pending = view["status"] == "pending";
    state["phase"] = json!(match view["status"].as_str() {
        Some("succeeded") => "succeeded",
        Some("failed") => "failed",
        Some("cancelled") => "cancelled",
        _ if url.is_some() || code.is_some() => "waiting",
        _ => "starting",
    });
    state["flowId"] = json!(auth);
    state["authorizationUrl"] = url.map_or(Value::Null, Value::from);
    state["message"] = view["error"].clone();
    if let (true, Some(url)) = (pending, url) {
        state["interaction"] = match (code, view["method"].as_str()) {
            (Some(code), _) => json!({"type": "deviceCode", "id": auth, "url": url, "userCode": code}),
            (None, Some("paste")) => json!({"type": "terminal", "id": auth, "output": format!("Open {url}, sign in and paste the code it shows."), "outputOffset": 0}),
            _ => json!({"type": "browser", "id": auth, "url": url, "requiresConsent": false}),
        };
    }
    state
}

/// The code a pasted address carries (`?code=`), or the text itself when it is not an address.
fn pasted_code(text: &str) -> String {
    let text = text.trim();
    let query = text.split_once('?').map(|(_, q)| q.split('#').next().unwrap_or(q));
    query.and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("code="))).filter(|c| !c.is_empty()).unwrap_or(text).to_string()
}

fn deep_merge(into: &mut Value, patch: &Value) {
    match (into.as_object_mut(), patch.as_object()) {
        (Some(into), Some(patch)) => {
            for (key, value) in patch {
                deep_merge(into.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        _ => *into = patch.clone(),
    }
}

/// Patch keys T3 accepts that the defaults template has no key of: `defaultThreadEnvMode` is kept like a setting, the others are operations on the model lists.
const EXTRA_SETTING_KEYS: [&str; 1] = ["defaultThreadEnvMode"];
const MODEL_OPERATIONS: [&str; 4] = ["setModelFavorites", "setModelsHidden", "setProviderModelOrder", "migrateModelPreferences"];

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value.get(key).and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or_else(|| format!("{key} must be a non-empty string"))
}

fn list_of<'a>(value: &'a Value, what: &str) -> Result<&'a Vec<Value>, String> {
    value.as_array().ok_or_else(|| format!("{what} must be a list"))
}

/// The preferences of one provider with both lists present.
fn provider_preferences<'a>(all: &'a mut Map<String, Value>, provider: &str) -> &'a mut Map<String, Value> {
    let entry = all.entry(provider.to_string()).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    let prefs = entry.as_object_mut().expect("made an object above");
    for list in ["hiddenModels", "modelOrder"] {
        if !prefs.get(list).is_some_and(Value::is_array) {
            prefs.insert(list.into(), json!([]));
        }
    }
    prefs
}

/// Applies the model list operations of a settings patch onto the stored settings, as T3 does: favorites and hidden models change by entry, the
/// order is replaced for one provider, and `migrateModelPreferences` fills only what nothing has set.
fn apply_model_operations(stored: &mut Value, patch: &Map<String, Value>) -> Result<(), String> {
    let stored = stored.as_object_mut().ok_or("the stored settings are not an object")?;
    let migrate = patch.get("migrateModelPreferences").filter(|m| m.is_object());
    let migrated = |key: &str| migrate.and_then(|m| m.get(key));
    let mut favorites = stored.get("favorites").filter(|f| f.is_array()).or_else(|| migrated("favorites").filter(|f| f.is_array())).and_then(Value::as_array).cloned();
    if let Some(changes) = patch.get("setModelFavorites") {
        let list = favorites.get_or_insert_with(Vec::new);
        for change in list_of(changes, "setModelFavorites")? {
            let (provider, model) = (text(change, "provider")?, text(change, "model")?);
            let at = list.iter().position(|f| f["provider"] == provider && f["model"] == model);
            match (change["favorite"].as_bool().ok_or("favorite must be a boolean")?, at) {
                (true, None) => list.push(json!({"provider": provider, "model": model})),
                (false, Some(at)) => drop(list.remove(at)),
                _ => {}
            }
        }
    }
    if let Some(favorites) = favorites {
        stored.insert("favorites".into(), Value::Array(favorites));
    }
    let mut preferences = stored.get("providerModelPreferences").filter(|p| p.is_object()).or_else(|| migrated("providerModelPreferences").filter(|p| p.is_object())).and_then(Value::as_object).cloned();
    if let Some(changes) = patch.get("setModelsHidden") {
        let all = preferences.get_or_insert_with(Map::new);
        for change in list_of(changes, "setModelsHidden")? {
            let (provider, model) = (text(change, "provider")?, text(change, "model")?);
            let hide = change["hidden"].as_bool().ok_or("hidden must be a boolean")?;
            let hidden = provider_preferences(all, provider).get_mut("hiddenModels").and_then(Value::as_array_mut).expect("made a list above");
            hidden.retain(|m| m != model);
            if hide {
                hidden.push(json!(model));
            }
        }
    }
    if let Some(order) = patch.get("setProviderModelOrder") {
        let provider = text(order, "provider")?;
        let models = list_of(&order["modelOrder"], "modelOrder")?;
        if !models.iter().all(Value::is_string) {
            return Err("modelOrder must list model names".into());
        }
        provider_preferences(preferences.get_or_insert_with(Map::new), provider).insert("modelOrder".into(), Value::Array(models.clone()));
    }
    if let Some(preferences) = preferences {
        stored.insert("providerModelPreferences".into(), Value::Object(preferences));
    }
    Ok(())
}

fn usage_kind(instance: &str) -> Option<&'static str> {
    match instance {
        "claudeAgent" => Some("claude"),
        "codex" => Some("codex"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

fn is_day(day: &str) -> bool {
    let b = day.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

impl Service {
    fn held(&self) -> &Held {
        &self.2
    }

    /// The accounts group's answer to `op`, or the setup failure the phone shows for `operation` on `instance`.
    fn account(&self, instance: &str, operation: &str, op: &Op) -> Result<Value, QueryFailure> {
        self.agent().run(op).map_err(|e| setup_error(instance, operation, &e))
    }

    /// The methods a provider's CLI signs in by, as `ProviderAuthMethod`s: none when the accounts group does not list the provider.
    fn auth_methods(&self, instance: &str) -> Vec<Value> {
        let listed = self.agent().run(&Op::AccountsList).unwrap_or_default();
        let methods = listed["providers"].as_array().into_iter().flatten().find(|p| p["provider"] == instance).map(|p| p["auth_methods"].clone());
        methods.and_then(|m| m.as_array().cloned()).into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).map(|id| json!({"id": id, "name": method_name(&id), "description": null, "type": "agent"})).collect()
    }

    /// The state the login of `instance` stands in: its session as the accounts group has it, else the idle state with the methods on offer.
    fn current_auth_state(&self, instance: &str) -> Value {
        if let Some((auth, _)) = self.held().session(instance) {
            let read = self.agent().run(&Op::AuthComplete { auth, timeout_ms: 0 }).ok().map(|r| r["auth"].clone());
            if let Some(view) = read.filter(|v| v.is_object()) {
                return auth_state(instance, Some(&view), &[]);
            }
        }
        auth_state(instance, None, &self.auth_methods(instance))
    }

    fn state_of(&self, instance: &str, result: &Value) -> Value {
        auth_state(instance, Some(&result["auth"]), &[])
    }

    /// `provider.auth.start`: the CLI's login begins and the state names the address (and the code) the person opens.
    pub fn provider_auth_start(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let instance = field(payload, "instanceId")?;
        let method = payload.get("methodId").and_then(Value::as_str).filter(|m| !m.is_empty()).map(str::to_string);
        let started = self.account(instance, "start", &Op::AuthStart { provider: instance.into(), method })?;
        let (auth, method) = (started["auth"]["auth"].as_str().unwrap_or_default(), started["auth"]["method"].as_str().unwrap_or_default());
        self.held().keep(instance, auth, method);
        self.log("provider.auth.started", &[("instance", instance.to_string()), ("method", method.to_string())]);
        Ok(self.state_of(instance, &started))
    }

    /// `provider.auth.respond`: the text a person pasted goes to the CLI that asked for it. A credential never passes through here: the CLI is
    /// signed in by its own flow.
    pub fn provider_auth_respond(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (instance, flow) = (field(payload, "instanceId")?, field(payload, "flowId")?);
        let response = payload.get("response").filter(|r| r.is_object()).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"response\"]".into()))?;
        let op = match response["type"].as_str() {
            Some("terminal") => Op::AuthRespond { auth: flow.into(), text: response["data"].as_str().unwrap_or_default().trim_end_matches(['\r', '\n']).to_string() },
            Some("browser") if response["action"] == "decline" => Op::AuthCancel { auth: flow.into() },
            Some("browser") => Op::AuthComplete { auth: flow.into(), timeout_ms: 0 },
            Some("credentials") => return Err(refusal(instance, "respond", "A provider is signed in by its own flow: t3 never takes a credential, so open the address the login shows")),
            _ => return Err(QueryFailure::Defect("Expected a response of type browser, terminal or credentials\n  at [\"response\", \"type\"]".into())),
        };
        let answered = self.account(instance, "respond", &op)?;
        Ok(self.state_of(instance, &answered))
    }

    /// `provider.auth.complete`: a login that is finished by the address the person was sent back to hands the CLI its code, then waits for the
    /// login to end. A device-code login has no address to hand over, so it only waits.
    pub fn provider_auth_complete(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (instance, flow, callback) = (field(payload, "instanceId")?, field(payload, "flowId")?, field(payload, "callbackUrl")?);
        let paste = self.held().session(instance).is_some_and(|(auth, method)| auth == flow && method == "paste");
        if paste {
            self.account(instance, "complete", &Op::AuthRespond { auth: flow.into(), text: pasted_code(callback) })?;
        }
        let done = self.account(instance, "complete", &Op::AuthComplete { auth: flow.into(), timeout_ms: COMPLETE_WAIT_MS })?;
        Ok(self.state_of(instance, &done))
    }

    /// `provider.auth.cancel`: the login stops.
    pub fn provider_auth_cancel(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (instance, flow) = (field(payload, "instanceId")?, field(payload, "flowId")?);
        let cancelled = self.account(instance, "cancel", &Op::AuthCancel { auth: flow.into() })?;
        Ok(self.state_of(instance, &cancelled))
    }

    /// `provider.auth.logout`: the provider's directory is emptied of its login, and the provider is idle again.
    pub fn provider_auth_logout(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let instance = field(payload, "instanceId")?;
        self.account(instance, "logout", &Op::AccountsLogout { provider: instance.into() })?;
        self.held().forget(instance);
        self.log("provider.auth.logout", &[("instance", instance.to_string())]);
        Ok(auth_state(instance, None, &self.auth_methods(instance)))
    }

    /// `provider.auth.subscribe`: the login's state now, and then each state the login passes through, including those of a login started after
    /// the stream opened.
    pub fn open_auth_stream(&self, payload: &Value) -> Result<(AuthFold, Value), QueryFailure> {
        let instance = field(payload, "instanceId")?;
        Ok((AuthFold { instance: instance.into() }, self.current_auth_state(instance)))
    }

    /// Follows the instance's login on a thread of its own until the app's stream is gone or `stop` is set.
    pub fn follow_auth(&self, fold: AuthFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let mut followed: Option<String> = None;
            while !stop.load(Ordering::SeqCst) {
                let Some((auth, _)) = svc.held().session(&fold.instance).filter(|(auth, _)| followed.as_ref() != Some(auth)) else {
                    std::thread::sleep(LOOK);
                    continue;
                };
                followed = Some(auth.clone());
                let result = svc.agent().auth_follow(&auth, &mut |view| {
                    if stop.load(Ordering::SeqCst) {
                        return Follow::Stop;
                    }
                    if view.is_null() {
                        return Follow::Continue;
                    }
                    let frame = auth_state(&fold.instance, Some(&view), &[]);
                    if to.send((id.clone(), frame)).is_err() {
                        return Follow::Stop;
                    }
                    Follow::Continue
                });
                if let Err(e) = result {
                    svc.log("provider.auth.stopped", &[("instance", fold.instance.clone()), ("message", e.message())]);
                }
            }
        });
    }

    /// `provider.chatgpt.reconnect-profile`: a ChatGPT profile is something T3's own desktop keeps; the Codex CLI behind this server keeps its
    /// login in its own directory and offers none to reconnect.
    pub fn chatgpt_reconnect(&self, payload: &Value) -> Result<Value, QueryFailure> {
        field(payload, "instanceId")?;
        Ok(Value::Null)
    }

    /// `provider.chatgpt.import-profile` and `provider.chatgpt.handoff.subscribe`: they move a ChatGPT login's tokens through the mapper, which
    /// this server never holds.
    pub fn chatgpt_refused(&self, payload: &Value, operation: &str) -> QueryFailure {
        let instance = payload.get("instanceId").and_then(Value::as_str).unwrap_or_default();
        refusal(instance, operation, "A ChatGPT login is a credential, and this server never holds one: sign Codex in with provider.auth.start")
    }

    /// `provider.consumeResetCredit`: the credit the model API reset, when it has one.
    pub fn consume_reset_credit(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let Some(instance) = payload.get("instanceId").and_then(Value::as_str).filter(|i| !i.is_empty()) else {
            return Err(QueryFailure::Error(json!({"_tag": "UsageLimitSourceError", "detail": "This server has no usage limit sources, only providers"})));
        };
        self.agent().run(&Op::ResetCredit { provider: instance.into() }).map_err(|e| QueryFailure::Error(json!({"_tag": "UsageLimitSourceError", "detail": e.message()})))?;
        Ok(json!({"outcome": "reset"}))
    }

    fn catalog_models(&self) -> Vec<(String, String)> {
        self.0.catalog.models().into_iter().map(|m| (m.slug, m.name)).collect()
    }

    /// `server.refreshProviders`: the accounts group looks again at the providers (one, or all), and the answer is the providers as signed in or
    /// out.
    pub fn refresh_providers(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let instance = payload.get("instanceId").and_then(Value::as_str).filter(|i| !i.is_empty());
        self.account(instance.unwrap_or_default(), "refresh", &Op::AccountsRefresh { provider: instance.map(str::to_string) })?;
        let listed = self.account(instance.unwrap_or_default(), "refresh", &Op::AccountsList)?;
        let logins: Vec<(String, String)> = listed["providers"].as_array().into_iter().flatten().filter_map(|p| Some((p["provider"].as_str()?.to_string(), p["status"].as_str()?.to_string()))).collect();
        let models = self.catalog_models();
        let models: Vec<Model> = models.iter().map(|(slug, name)| Model { slug, name }).collect();
        Ok(json!({"providers": wire::providers_with_logins(&models, &iso(self.now_ms()), &logins)}))
    }

    /// `server.updateProvider`: a provider's CLI is part of the service's image, so a version is updated by shipping the image.
    pub fn update_provider(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let provider = field(payload, "provider")?;
        Err(QueryFailure::Error(json!({"_tag": "ServerProviderUpdateError", "provider": provider, "reason": "The provider CLIs are part of the service's image; ship a new image to update one"})))
    }

    /// `server.getUsageSummary`: one source per provider, from what the accounts group knows of each. No token or cost bucket is kept: the CLIs
    /// meter their own transcripts on the service's volume, which a mapper does not read.
    pub fn usage_summary(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (since, until, zone) = (field(payload, "sinceDay")?, field(payload, "untilDay")?, field(payload, "timeZone")?);
        for (key, day) in [("sinceDay", since), ("untilDay", until)] {
            if !is_day(day) {
                return Err(QueryFailure::Defect(format!("Expected a string matching the pattern ^\\d{{4}}-\\d{{2}}-\\d{{2}}$\n  at [\"{key}\"]")));
            }
        }
        if since > until {
            return Err(QueryFailure::Error(json!({"_tag": "UsageReadError", "reason": "invalidWindow", "detail": format!("{since} is after {until}")})));
        }
        let summary = self.agent().run(&Op::UsageSummary).map_err(|e| QueryFailure::Error(json!({"_tag": "UsageReadError", "reason": "scanFailed", "detail": e.message()})))?;
        let sources: Vec<Value> = summary["providers"].as_array().into_iter().flatten().filter_map(|row| {
            let instance = row["provider"].as_str()?;
            let kind = usage_kind(instance)?;
            let message = match (&row["usage"], row["status"].as_str()) {
                (Value::Object(usage), _) => Some(format!("{} of {} {}", usage["used"], usage["limit"].as_u64().map_or("an unknown limit".to_string(), |l| l.to_string()), usage["unit"].as_str().unwrap_or_default())),
                (_, Some("signed_in")) => Some("The provider reports no usage".to_string()),
                _ => Some("The provider is signed out".to_string()),
            };
            Some(json!({
                "fingerprint": {"hostId": "rca", "provider": kind, "resolvedHomePath": format!("rca:{instance}"), "volumeId": ""},
                "status": if row["status"] == "signed_in" { "ok" } else { "missing" },
                "scannedFiles": 0, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 0, "message": message,
            }))
        }).collect();
        Ok(json!({
            "contractVersion": USAGE_CONTRACT_VERSION, "readAt": iso(self.now_ms()), "timeZone": zone, "sinceDay": since, "untilDay": until,
            "buckets": [], "sources": sources,
            "pricing": {"status": "unavailable", "source": "rca", "fetchedAt": null, "knownModels": 0},
            "scanDurationMs": 0,
        }))
    }

    /// `server.refreshUsageRates`: rca keeps no price table, so the rates are never fresh.
    pub fn refresh_usage_rates(&self) -> Value {
        json!({"status": "unavailable", "source": "rca", "fetchedAt": null, "knownModels": 0})
    }

    /// `server.getHostResources`: the machine t3 serve runs on.
    pub fn host_resources(&self) -> Result<Value, QueryFailure> {
        let sample = self.0.host.sample().map_err(|detail| QueryFailure::Error(wire::host_error(&detail)))?;
        Ok(json!({
            "sampledAt": self.now_ms().max(0), "cpuUtilization": sample.cpu_utilization, "cpuCount": sample.cpu_count,
            "availableMemoryBytes": sample.available_memory_bytes, "totalMemoryBytes": sample.total_memory_bytes,
        }))
    }

    /// `server.getDatabaseStorage`: how big t3 serve's own database is and how much of it a vacuum would give back; null when it cannot be read.
    fn storage_status(&self) -> Value {
        match self.0.storage.sample() {
            Ok(s) => json!({"databaseBytes": s.database_bytes, "reclaimableBytes": s.reclaimable_bytes, "incrementalVacuumEnabled": s.incremental_vacuum, "compactCommand": format!("sqlite3 {} VACUUM", s.path)}),
            Err(e) => {
                self.log("storage.unreadable", &[("message", e)]);
                Value::Null
            }
        }
    }

    pub fn open_storage_stream(&self) -> (StorageFold, Value) {
        let first = self.storage_status();
        (StorageFold { told: first.clone() }, first)
    }

    /// Looks at the database every `STORAGE_LOOK` and sends its status when it changed, until the app's stream is gone or `stop` is set.
    pub fn follow_storage(&self, mut fold: StorageFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let mut waited = Duration::ZERO;
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(LOOK);
                waited += LOOK;
                if waited < STORAGE_LOOK {
                    continue;
                }
                waited = Duration::ZERO;
                let now = svc.storage_status();
                if now != fold.told {
                    if to.send((id.clone(), now.clone())).is_err() {
                        return;
                    }
                    fold.told = now;
                }
            }
        });
    }

    /// The settings the phone changed, as kept in device state: none when nothing was stored or the state cannot be read.
    fn stored_settings(&self) -> Result<Option<Value>, AgentError> {
        let read = self.agent().run(&Op::StateGet { scope: SETTINGS_SCOPE.into(), key: SETTINGS_KEY.into() })?;
        Ok(read.get("value").filter(|v| v.is_object()).cloned())
    }

    /// The settings as they are now: the defaults with what the phone changed over them.
    pub(super) fn effective_settings(&self) -> Value {
        Self::settings_with(self.stored_settings().ok().flatten().as_ref())
    }

    /// `ServerSettings`: the defaults with what the phone changed over them.
    fn settings_with(stored: Option<&Value>) -> Value {
        let mut settings = Value::Object(wire::default_settings());
        if let Some(stored) = stored {
            deep_merge(&mut settings, stored);
        }
        settings
    }

    /// The settings `ServerConfig` shows: those stored when there are any.
    pub(crate) fn config_settings(&self) -> Option<Value> {
        let stored = self.stored_settings().ok().flatten()?;
        Some(Self::settings_with(Some(&stored)))
    }

    /// `server.updateSettings`: the patch is merged into what is stored and the whole settings answer. A key that is no setting here is refused
    /// instead of dropped.
    pub fn update_settings(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let patch = payload.get("patch").and_then(Value::as_object).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"patch\"]".into()))?;
        if payload.get("providerInstanceMutation").is_some_and(|m| !m.is_null()) {
            return Err(settings_error("providerInstanceMutation", "create-provider-instance", "this server has the providers rca runs and no instances to add or remove"));
        }
        let known = wire::default_settings();
        if let Some(key) = patch.keys().find(|k| !known.contains_key(*k) && !EXTRA_SETTING_KEYS.contains(&k.as_str()) && !MODEL_OPERATIONS.contains(&k.as_str())) {
            return Err(settings_error(&format!("patch.{key}"), "normalize", &format!("{key} is not a setting this server stores")));
        }
        let _one_at_a_time = self.held().settings.lock().unwrap_or_else(|e| e.into_inner());
        let mut stored = self.stored_settings().map_err(|e| settings_error(SETTINGS_KEY, "read-file", &e.message()))?.unwrap_or_else(|| Value::Object(Map::new()));
        let settings: Map<String, Value> = patch.iter().filter(|(k, _)| !MODEL_OPERATIONS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
        deep_merge(&mut stored, &Value::Object(settings));
        if let Some(replaced) = patch.get("providerModelPreferences").filter(|p| p.is_object()) {
            stored["providerModelPreferences"] = replaced.clone();
        }
        apply_model_operations(&mut stored, patch).map_err(|why| settings_error("patch", "normalize", &why))?;
        self.agent().run(&Op::StateSet { scope: SETTINGS_SCOPE.into(), key: SETTINGS_KEY.into(), value: stored.clone() }).map_err(|e| settings_error(SETTINGS_KEY, "write-file", &e.message()))?;
        self.log("settings.updated", &[("keys", patch.keys().cloned().collect::<Vec<_>>().join(","))]);
        Ok(Self::settings_with(Some(&stored)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_model_list_operations_when_applied_then_favorites_hidden_models_and_order_change_by_entry_and_a_migration_fills_only_what_is_unset() {
        // Given settings holding one favorite, and a patch that favorites one model and drops the first, hides one, and orders another provider
        let mut stored = json!({"favorites": [{"provider": "codex", "model": "a"}]});
        let patch = json!({
            "setModelFavorites": [{"provider": "codex", "model": "b", "favorite": true}, {"provider": "codex", "model": "a", "favorite": false}],
            "setModelsHidden": [{"provider": "codex", "model": "c", "hidden": true}, {"provider": "codex", "model": "c", "hidden": true}],
            "setProviderModelOrder": {"provider": "claudeAgent", "modelOrder": ["x", "y"]},
            "migrateModelPreferences": {"favorites": [{"provider": "codex", "model": "old"}]},
        });
        // When it is applied
        apply_model_operations(&mut stored, patch.as_object().unwrap()).unwrap();
        // Then favorites changed by entry, the model is hidden once, the order is set, and the migration did not overwrite the favorites
        assert_eq!(stored["favorites"], json!([{"provider": "codex", "model": "b"}]));
        assert_eq!(stored["providerModelPreferences"], json!({"codex": {"hiddenModels": ["c"], "modelOrder": []}, "claudeAgent": {"hiddenModels": [], "modelOrder": ["x", "y"]}}));
        // And a migration over unset settings is taken, while a malformed operation is refused
        let mut fresh = json!({});
        apply_model_operations(&mut fresh, json!({"migrateModelPreferences": {"favorites": [{"provider": "codex", "model": "old"}]}}).as_object().unwrap()).unwrap();
        assert_eq!(fresh["favorites"], json!([{"provider": "codex", "model": "old"}]));
        assert!(apply_model_operations(&mut json!({}), json!({"setModelsHidden": [{"provider": "codex"}]}).as_object().unwrap()).is_err());
    }

    #[test]
    fn given_text_a_person_pasted_when_the_code_is_read_then_an_address_gives_its_code_parameter_and_anything_else_is_the_text() {
        // Given an address with the code among other parameters and a fragment, an address without one, and a bare code
        // Then the first gives the code, and the others are what was pasted, trimmed
        assert_eq!(pasted_code("https://x.test/cb?state=s&code=abc123&y=1#frag"), "abc123");
        assert_eq!(pasted_code("  https://x.test/cb?state=s \n"), "https://x.test/cb?state=s");
        assert_eq!(pasted_code("abc123\n"), "abc123");
    }

    #[test]
    fn given_a_patch_when_merged_then_objects_merge_by_key_and_any_other_value_is_replaced() {
        // Given settings with a nested object and a list, and a patch that changes one nested key, the list and a scalar
        let mut settings = json!({"a": {"b": 1, "c": 2}, "list": [1, 2], "flag": false});
        deep_merge(&mut settings, &json!({"a": {"c": 3}, "list": [9], "flag": true}));
        // Then the nested key beside the changed one stays, and the list and the scalar are the patch's
        assert_eq!(settings, json!({"a": {"b": 1, "c": 3}, "list": [9], "flag": true}));
    }

    #[test]
    fn given_each_status_of_a_login_when_its_state_is_built_then_the_phase_and_interaction_follow_what_the_cli_printed() {
        // Given a login that printed nothing yet, one with an address and a code, a paste one with an address, and the three ends
        let view = |status: &str, url: Option<&str>, code: Option<&str>, method: &str| json!({"auth": "a1", "provider": "codex", "method": method, "status": status, "url": url, "code": code, "error": null});
        let phase = |v: Value| auth_state("codex", Some(&v), &[]);
        // Then the phase is starting until a prompt shows, waiting with it, and the end's own after
        assert_eq!(phase(view("pending", None, None, "device_code"))["phase"], json!("starting"));
        let device = phase(view("pending", Some("https://x.test/d"), Some("AB-1"), "device_code"));
        assert_eq!((device["phase"].clone(), device["interaction"]["type"].clone(), device["interaction"]["userCode"].clone()), (json!("waiting"), json!("deviceCode"), json!("AB-1")));
        assert_eq!(phase(view("pending", Some("https://x.test/p"), None, "paste"))["interaction"]["type"], json!("terminal"));
        assert_eq!(phase(view("pending", Some("https://x.test/p"), None, "browser"))["interaction"]["type"], json!("browser"));
        // And an ended login has no interaction and a failed one carries the CLI's error as the message
        let failed = phase(json!({"auth": "a1", "method": "paste", "status": "failed", "url": "https://x.test/p", "code": null, "error": "bad code"}));
        assert_eq!((failed["phase"].clone(), failed["message"].clone(), failed["interaction"].clone()), (json!("failed"), json!("bad code"), Value::Null));
        assert_eq!((phase(view("succeeded", None, None, "paste"))["phase"].clone(), phase(view("cancelled", None, None, "paste"))["phase"].clone()), (json!("succeeded"), json!("cancelled")));
        // And no session is the idle state with the methods on offer
        assert_eq!(auth_state("codex", None, &[json!({"id": "paste"})])["methods"], json!([{"id": "paste"}]));
    }

    #[test]
    fn given_strings_when_checked_as_days_then_only_a_year_month_day_is_one() {
        // Given a date, a date with a time, a word and a date with other separators
        // Then only the first is a day
        assert!(is_day("2026-11-04"));
        assert!(!is_day("2026-11-04T00:00") && !is_day("yesterday") && !is_day("2026/11/04"));
    }
}
