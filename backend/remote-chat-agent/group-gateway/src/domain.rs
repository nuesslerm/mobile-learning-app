//! What the gateway decides without I/O: the providers it fronts, the rule that says which requests a stage runs on, what a token is scoped to and
//! the same-protocol reroute table.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Protocol {
    Messages,
    Responses,
    ChatCompletions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// `x-api-key: <key>`
    XApiKey,
    /// `authorization: Bearer <key>`
    Bearer,
}

/// A model provider the gateway fronts. `key` is the registry name of the secret the service holds; it is not a name any CLI's own environment
/// prefix inherits, so a CLI never sees the key by way of its environment.
#[derive(Debug, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub protocol: Protocol,
    /// Where requests go, without a trailing slash; the path of the request the CLI made is appended.
    pub upstream: &'static str,
    pub auth: Auth,
    pub key: &'static str,
    /// The provider prefixes opencode names models with (`opencode-go/<model>`).
    pub model_prefixes: &'static [&'static str],
}

pub const PROVIDERS: [Provider; 5] = [
    Provider { id: "anthropic", protocol: Protocol::Messages, upstream: "https://api.anthropic.com", auth: Auth::XApiKey, key: "GATEWAY_ANTHROPIC_API_KEY", model_prefixes: &["anthropic"] },
    Provider { id: "openai", protocol: Protocol::Responses, upstream: "https://api.openai.com/v1", auth: Auth::Bearer, key: "GATEWAY_OPENAI_API_KEY", model_prefixes: &["openai"] },
    Provider { id: "opencode-go", protocol: Protocol::ChatCompletions, upstream: "https://opencode.ai/zen/go/v1", auth: Auth::Bearer, key: "GATEWAY_OPENCODE_GO_API_KEY", model_prefixes: &["opencode-go"] },
    Provider { id: "kimi", protocol: Protocol::ChatCompletions, upstream: "https://api.kimi.com/coding/v1", auth: Auth::Bearer, key: "GATEWAY_KIMI_API_KEY", model_prefixes: &["kimi-for-coding", "kimi"] },
    Provider { id: "openrouter", protocol: Protocol::ChatCompletions, upstream: "https://openrouter.ai/api/v1", auth: Auth::Bearer, key: "GATEWAY_OPENROUTER_API_KEY", model_prefixes: &["openrouter"] },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// The provider a turn of `driver` with `model` calls: Claude Code calls Anthropic, Codex OpenAI, opencode the provider its model id is prefixed with.
/// A subscription login is not a provider here: whether a turn is routed at all is the key's presence, never the login's.
pub fn provider_for_driver(driver: &str, model: Option<&str>) -> Option<&'static Provider> {
    match driver {
        "claudeAgent" => provider("anthropic"),
        "codex" => provider("openai"),
        "opencode" => {
            let prefix = model?.split_once('/')?.0;
            PROVIDERS.iter().find(|p| p.model_prefixes.contains(&prefix))
        }
        _ => None,
    }
}

/// What a token is scoped to: one thread and one provider. `reroutes` is the table as it stood when the token was minted (a turn's start), matched
/// against the model each request names; a table changed afterwards does not move a turn that began.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub thread: String,
    pub provider: String,
    pub reroutes: Vec<Reroute>,
}

/// A request the gateway received, after the path of the route: `path` is what follows `/gateway/<provider>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Inbound {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The request the gateway sends on, which the stages shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Outbound {
    pub provider: &'static Provider,
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Whether the credential stage ran: a request that did not is never sent.
    pub credentialed: bool,
}

impl Outbound {
    pub fn url(&self) -> String {
        let query = self.query.as_deref().filter(|q| !q.is_empty()).map(|q| format!("?{q}")).unwrap_or_default();
        format!("{}{}{}", self.provider.upstream, self.path, query)
    }

    pub fn model(&self) -> Option<String> {
        serde_json::from_slice::<serde_json::Value>(&self.body).ok()?.get("model")?.as_str().map(str::to_string)
    }
}

/// Why the gateway answers a request itself instead of passing it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl Refusal {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }
}

/// What a rule is matched against.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub provider: &'a str,
    pub route: &'a str,
    pub model: Option<&'a str>,
    pub thread: &'a str,
}

/// Which requests a stage runs on: every field that is set must match. `route` is a path prefix, `model` is exact or ends in `*`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub provider: Option<String>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub thread: Option<String>,
}

impl Rule {
    pub fn any() -> Self {
        Self::default()
    }

    pub fn provider(mut self, id: &str) -> Self {
        self.provider = Some(id.into());
        self
    }

    pub fn route(mut self, prefix: &str) -> Self {
        self.route = Some(prefix.into());
        self
    }

    pub fn model(mut self, pattern: &str) -> Self {
        self.model = Some(pattern.into());
        self
    }

    pub fn thread(mut self, id: &str) -> Self {
        self.thread = Some(id.into());
        self
    }

    pub fn matches(&self, facts: &Facts) -> bool {
        let model_ok = match (&self.model, facts.model) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(pattern), Some(model)) => match pattern.strip_suffix('*') {
                Some(prefix) => model.starts_with(prefix),
                None => pattern == model,
            },
        };
        model_ok
            && self.provider.as_deref().is_none_or(|p| p == facts.provider)
            && self.route.as_deref().is_none_or(|r| facts.route.starts_with(r))
            && self.thread.as_deref().is_none_or(|t| t == facts.thread)
    }
}

/// A model that is served by another one, on the same protocol: a Messages model by a Messages model, a Responses model by a Responses one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reroute {
    pub provider: String,
    pub from: String,
    pub to_provider: String,
    pub to_model: String,
}

impl Reroute {
    /// `Err` names why the pair is no reroute: an unknown provider, or two protocols (a reroute never translates one protocol into another).
    pub fn new(provider: &str, from: &str, to_provider: &str, to_model: &str) -> Result<Reroute, String> {
        let (a, b) = (self::provider(provider).ok_or(format!("unknown provider {provider}"))?, self::provider(to_provider).ok_or(format!("unknown provider {to_provider}"))?);
        if a.protocol != b.protocol {
            return Err(format!("{} speaks {:?} and {} speaks {:?}: a reroute keeps the protocol", a.id, a.protocol, b.id, b.protocol));
        }
        Ok(Reroute { provider: provider.into(), from: from.into(), to_provider: to_provider.into(), to_model: to_model.into() })
    }

    pub fn applies(&self, provider: &str, model: &str) -> bool {
        self.provider == provider && Rule::any().model(&self.from).matches(&Facts { provider, route: "", model: Some(model), thread: "" })
    }
}
