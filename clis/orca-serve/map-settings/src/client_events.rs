use orca_core::wire::Failure;
use orca_core::{Ctx, Subscription};
use serde_json::{json, Value};

const PREFIX: &str = "runtime-client-events-";
const STATE_SCOPE: &str = "orca";
const QUEUE_FOLLOW_UPS_KEY: &str = "nativeChatQueueFollowUps";

struct ClientEvents {
    id: String,
    ready: Option<Value>,
}

/// The stream opens with `ready` and its snapshot. The host's later events (repos, worktrees, terminals) come with the areas that own them.
pub(crate) fn open(ctx: &mut Ctx) -> Box<dyn Subscription> {
    let id = ctx.subscription_id(PREFIX);
    // Queued follow-ups are the default; only an explicit `false` kept in rca's state turns them off.
    let queue = !matches!(ctx.agent.state_get(STATE_SCOPE, QUEUE_FOLLOW_UPS_KEY), Ok(Some(Value::Bool(false))));
    let ready = json!({"type": "ready", "subscriptionId": id, "snapshot": {"sshStates": [], "nativeChatQueueFollowUps": queue}});
    Box::new(ClientEvents { id, ready: Some(ready) })
}

impl Subscription for ClientEvents {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        self.ready.take().into_iter().collect()
    }

    fn end(&mut self) -> Vec<Value> {
        vec![json!({"type": "end"})]
    }
}

pub(crate) fn unsubscribe(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    let id = params.get("subscriptionId").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing subscriptionId"))?;
    // Only this connection's own streams: another socket's id is no stream here.
    let own = id.starts_with(&format!("{PREFIX}{}-", ctx.connection_id));
    if own {
        ctx.subscriptions.cancel(id);
    }
    Ok(json!({ "unsubscribed": own }))
}
