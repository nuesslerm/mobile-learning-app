//! What the service checks before it takes a request (`startupGate.ts`): the agent CLI it is told to use must be one it can serve, so a
//! deploy that believes it runs claude cannot quietly run opencode. A failure names what is wrong and ends the process.

use crate::domain::clis;

/// The CLI the service will use, or the message it refuses to start with. `AGENT_CLI` names it; unset or empty means opencode.
pub fn check(agent_cli: Option<&str>) -> Result<&'static str, String> {
    let raw = agent_cli.map(str::trim).filter(|v| !v.is_empty());
    let available = clis::available().join(", ");
    let Some(raw) = raw else { return Ok(clis::selected(None)) };
    if !clis::is_cli(raw) {
        let supported = clis::ALL.iter().map(|(id, _)| *id).collect::<Vec<_>>().join(", ");
        return Err(format!("AGENT_CLI=\"{raw}\" is not a supported agent CLI. Supported: {supported}. Available in this image: {available}."));
    }
    if !clis::is_available(raw) {
        return Err(format!(
            "AGENT_CLI={raw} was selected, but this build does not serve that backend: the claude and codex gateways are not ported to the Rust service, and the production image carries opencode alone. Available: {available}. Set AGENT_CLI to one of the available backends."
        ));
    }
    Ok(clis::selected(Some(raw)))
}
