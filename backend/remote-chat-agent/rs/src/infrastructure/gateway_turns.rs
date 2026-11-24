//! What a turn's launch asks of the gateway (`TurnGateway`): a token of its own for the provider its CLI calls, and the address of the gateway as a
//! CLI at the workspace host reaches it (the service's tool server's address, with `/gateway/<provider>` after it).
use super::mcp_address::LoopbackMcp;
use rca_cli_driver::domain::GatewayRoute;
use rca_group_agent::TurnGateway;
use rca_group_gateway::{provider_for_driver, Gateway};
use std::sync::Arc;

pub struct GatewayTurns {
    gateway: Arc<Gateway>,
    address: Arc<LoopbackMcp>,
}

impl GatewayTurns {
    pub fn new(gateway: Arc<Gateway>, address: Arc<LoopbackMcp>) -> Self {
        Self { gateway, address }
    }
}

impl TurnGateway for GatewayTurns {
    fn route(&self, thread: &str, driver: &str, model: Option<&str>) -> Option<GatewayRoute> {
        let provider = provider_for_driver(driver, model)?;
        let minted = self.gateway.mint(thread, provider.id)?;
        // opencode names the provider block by the prefix of the model id, which is not always the gateway's id for the provider
        let block = if driver == "opencode" { model.and_then(|m| m.split_once('/')).map_or(provider.id, |(prefix, _)| prefix) } else { provider.id };
        Some(GatewayRoute { provider: block.to_string(), base_url: format!("{}/gateway/{}", self.address.base_url(), provider.id), token: minted.token })
    }

    fn release(&self, thread: &str) {
        self.gateway.release(thread);
    }
}
