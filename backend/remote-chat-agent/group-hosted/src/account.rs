//! Routing by account: a request with no workspace goes to an attached executor whose `gh` (or `glab`) is signed in to the provider, the one that
//! holds the scope the method needs, else a refusal naming it; a repository request with several candidates must name its workspace.
use crate::refusal::{refuse, Refused};
use agent_api_hosted::{Scope, Spec};
use agent_api_proto::{code, text};
use futures_util::future::join_all;
use exec_proto::Code;
use rca_exec_channel::remote::ExecutorChannel;
use rca_exec_channel::Executors;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

/// A method needs a sign-in scope that no signed-in executor holds.
pub(crate) const SCOPE_MISSING: &str = "PROVIDER_SCOPE_MISSING";

/// Whether the machine's sign-in lists `scope`, the one a method prefers an executor for.
fn holds(scopes: &Value, scope: &str) -> bool {
    scopes.as_array().into_iter().flatten().filter_map(Value::as_str).any(|s| s == scope)
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn account_channel(executors: &Arc<dyn Executors>, spec: &Spec, params: &Value) -> Result<Arc<dyn ExecutorChannel>, Refused> {
    let provider = text(params, "provider").or(spec.default_provider).unwrap_or("github");
    let needs = match spec.scope {
        Scope::Account { needs } => needs,
        _ => None,
    };
    let mut args = Map::new();
    args.insert("provider".into(), json!(provider));
    if let Some(host) = text(params, "host") {
        args.insert("host".into(), json!(host));
    }
    let probes = executors.executors().into_iter().filter_map(|name| executors.route_new(Some(&name)).ok()).map(|route| {
        let args = Value::Object(args.clone());
        async move {
            // An executor that cannot answer in time is not one to route to
            let answer = tokio::time::timeout(PROBE_TIMEOUT, route.channel.call(exec_hosted::AUTH_STATUS, args)).await.ok()?.ok()?;
            let status = answer.result;
            (status["installed"] == true && status["signed_in"] == true).then(|| (route.channel, needs.is_none_or(|n| holds(&status["scopes"], n))))
        }
    });
    let mut signed_in: Vec<(Arc<dyn ExecutorChannel>, bool)> = join_all(probes).await.into_iter().flatten().collect();
    if signed_in.is_empty() {
        return Err(refuse(Code::NoExecutor.as_str(), format!("no attached executor is signed in to {provider}")));
    }
    // Which account a repository belongs to is not known here, so a repository request with several candidates must name its workspace
    if matches!(spec.scope, Scope::Repo) && signed_in.len() > 1 {
        let n = signed_in.len();
        return Err(refuse(code::INVALID_PARAMS, format!("workspace is required: {n} executors are signed in to {provider} and the repository's account is not known")));
    }
    match signed_in.iter().position(|(_, scoped)| *scoped) {
        Some(i) => Ok(signed_in.swap_remove(i).0),
        None => Err(refuse(SCOPE_MISSING, format!("missing required scope {}: no signed-in executor holds it", needs.unwrap_or_default()))),
    }
}
