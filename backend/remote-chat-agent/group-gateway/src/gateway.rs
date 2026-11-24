//! The gateway: mints the token of a turn, and answers each request a CLI makes with it by running the stages and calling the provider.
//! Every way it can fail ends the request: no token, a token of another provider, no key, a stage that refuses, a provider that does not answer.
use crate::domain::{Facts, Grant, Inbound, Outbound, Refusal, Reroute, Rule, provider, Provider};
use crate::ports::{Compressor, Ids, KeySource, TokenStore, Upstream, UpstreamReply};
use crate::stages::{ApplyReroute, Context, Headroom, InjectCredential, Stage};
use rca_agent_core::bearers::token_digest;
use std::sync::{Arc, RwLock};

/// What every token starts with, so a leaked one is recognised for what it is.
pub const TOKEN_PREFIX: &str = "rcag_";

pub struct Minted {
    pub token: String,
    pub provider: &'static Provider,
}

pub enum Reply {
    Answer(UpstreamReply),
    Refused(Refusal),
}

pub struct Gateway {
    store: Arc<dyn TokenStore>,
    keys: Arc<dyn KeySource>,
    upstream: Arc<dyn Upstream>,
    ids: Arc<dyn Ids>,
    stages: Vec<(Rule, Arc<dyn Stage>)>,
    credential: InjectCredential,
    reroutes: RwLock<Vec<Reroute>>,
}

impl Gateway {
    /// The standard pipeline: the reroute a token was pinned to, then credential injection. Compression is not in it until `with_headroom`.
    pub fn new(store: Arc<dyn TokenStore>, keys: Arc<dyn KeySource>, upstream: Arc<dyn Upstream>, ids: Arc<dyn Ids>) -> Gateway {
        Gateway { store, credential: InjectCredential::new(keys.clone()), keys, upstream, ids, stages: vec![(Rule::any(), Arc::new(ApplyReroute))], reroutes: RwLock::new(Vec::new()) }
    }

    /// Adds the compression stage for the requests `rule` matches. Off unless the composition root asks.
    pub fn with_headroom(mut self, rule: Rule, compressor: Arc<dyn Compressor>) -> Gateway {
        self.stages.push((rule, Arc::new(Headroom::new(compressor))));
        self
    }

    /// Replaces the reroute table. Turns that already began keep the target they were pinned to.
    pub fn set_reroutes(&self, table: Vec<Reroute>) {
        *self.reroutes.write().unwrap_or_else(|e| e.into_inner()) = table;
    }

    /// The token of a turn that starts now on `thread` for `provider_id`, with the reroute table of this moment fixed in it (each request is matched
    /// against it by the model it names). Any token the thread had is gone. None when the gateway holds no key for the provider the turn calls: that
    /// turn is not routed, and keeps its own login. A reroute whose target the gateway holds no key for is left out.
    pub fn mint(&self, thread: &str, provider_id: &str) -> Option<Minted> {
        let from = provider(provider_id)?;
        self.keys.key(from)?;
        let reroutes = {
            let table = self.reroutes.read().unwrap_or_else(|e| e.into_inner());
            table.iter().filter(|r| r.provider == provider_id && provider(&r.to_provider).is_some_and(|p| self.keys.key(p).is_some())).cloned().collect()
        };
        self.store.remove_thread(thread);
        let token = format!("{TOKEN_PREFIX}{}", self.ids.token());
        self.store.put(token_digest(&token), Grant { thread: thread.into(), provider: provider_id.into(), reroutes });
        Some(Minted { token, provider: from })
    }

    /// The turn is over: its token no longer opens anything.
    pub fn release(&self, thread: &str) {
        self.store.remove_thread(thread);
    }

    pub async fn handle(&self, provider_id: &str, token: Option<&str>, inbound: Inbound) -> Reply {
        match self.serve(provider_id, token, inbound).await {
            Ok(answer) => Reply::Answer(answer),
            Err(refusal) => Reply::Refused(refusal),
        }
    }

    async fn serve(&self, provider_id: &str, token: Option<&str>, inbound: Inbound) -> Result<UpstreamReply, Refusal> {
        let Some(provider) = provider(provider_id) else { return Err(Refusal::new(404, "GATEWAY_UNKNOWN_PROVIDER", format!("the gateway fronts no provider called {provider_id}"))) };
        let Some(token) = token.filter(|t| !t.is_empty()) else { return Err(Refusal::new(401, "GATEWAY_TOKEN_MISSING", "the request carries no gateway token")) };
        let Some(grant) = self.store.get(&token_digest(token)) else {
            return Err(Refusal::new(401, "GATEWAY_TOKEN_DEAD", "the gateway token is no longer valid (its turn ended or the service restarted); start the turn again"));
        };
        if grant.provider != provider_id {
            return Err(Refusal::new(401, "GATEWAY_WRONG_SCOPE", format!("the gateway token is not for {provider_id}")));
        }
        let mut request = Outbound { provider, method: inbound.method, path: inbound.path, query: inbound.query, headers: inbound.headers, body: inbound.body, credentialed: false };
        let context = Context { grant: &grant };
        for (rule, stage) in &self.stages {
            let model = request.model();
            let facts = Facts { provider: provider_id, route: &request.path, model: model.as_deref(), thread: &grant.thread };
            if rule.matches(&facts) {
                stage.apply(&mut request, &context).await?;
            }
        }
        self.credential.apply(&mut request, &context).await?;
        if !request.credentialed {
            return Err(Refusal::new(500, "GATEWAY_NOT_CREDENTIALED", "a request must carry the gateway's credential before it is sent"));
        }
        self.upstream.send(request).await.map_err(|why| Refusal::new(502, "GATEWAY_UPSTREAM_FAILED", format!("the provider did not answer: {why}")))
    }
}
