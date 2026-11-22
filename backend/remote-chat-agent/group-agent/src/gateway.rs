//! The model gateway as a turn sees it: a turn that the service routes through it is launched with a gateway address and a token of its own,
//! and releases the token when it ends. The gateway itself is another group's; the composition root implements this port over it.
use rca_cli_driver::domain::GatewayRoute;

pub trait TurnGateway: Send + Sync {
    /// The route of a turn of `driver` on `thread` that starts now, minting its token; none when the turn is not routed (no key is held for its
    /// provider, so the CLI keeps its own login).
    fn route(&self, thread: &str, driver: &str, model: Option<&str>) -> Option<GatewayRoute>;
    /// The turn is over: its token opens nothing any more.
    fn release(&self, thread: &str);
}

/// No gateway: every CLI runs with its own credentials.
pub struct NoGateway;

impl TurnGateway for NoGateway {
    fn route(&self, _: &str, _: &str, _: Option<&str>) -> Option<GatewayRoute> {
        None
    }

    fn release(&self, _: &str) {}
}
