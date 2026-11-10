//! `AgentApi` over rca's agent API (`GET /agent/v1/socket`, a device bearer), through `agent-api-client`. Each call opens a socket of its own.
use agent_api_client::{Client, Config, Error};
use agent_api_proto::{GroupName, Method, PROTOCOL};
use agent_api_service::{StatusResult, STATUS};
use orca_core::{AgentApi, AgentError, ServiceStatus};
use serde_json::{json, Value};

pub struct RcaAgent {
    authority: String,
    /// A device bearer: the one credential this server holds for rca (`wf offload bearers mint`).
    bearer: String,
}

/// The agent API groups this server's mapping crates need so far.
///
/// `workspace` is not here although `repo.list` is in it: the service lists a group only once every owner method of it is served
/// (`workspace.activate` and `repo.hooks` are not), and a method of an unlisted group still answers.
const BASE_GROUPS: &[&str] = &["service", "state"];

/// The groups of `BASE_GROUPS` and of every method a mapping crate declares in its `USES`, so a new method cannot drift from the startup check.
pub fn needed_groups() -> Vec<&'static str> {
    let mut groups: Vec<&'static str> = BASE_GROUPS.to_vec();
    for m in orca_map_workspace::USES.iter().chain(orca_map_agent::USES).chain(orca_map_terminal::USES).chain(orca_map_git::USES).chain(orca_map_accounts::USES).chain(orca_map_state::USES).chain(orca_map_hosted::USES).chain(orca_map_schedules::USES) {
        let g = m.group().as_str();
        if g != "workspace" && !groups.contains(&g) {
            groups.push(g);
        }
    }
    groups
}

fn failed(e: Error) -> AgentError {
    if e.from_service() {
        AgentError::Refused { code: e.code, message: e.message }
    } else {
        AgentError::Failed(e.to_string())
    }
}

impl RcaAgent {
    pub fn new(url: &str, bearer: &str) -> Self {
        Self { authority: url.trim_start_matches("ws://").trim_start_matches("http://").trim_end_matches('/').to_string(), bearer: bearer.to_string() }
    }

    fn open(&self) -> Result<Client, AgentError> {
        Client::connect(Config::new(format!("ws://{}", self.authority), self.bearer.clone())).map_err(failed)
    }

    /// Retries only an rca that does not answer (a tailnet node that is still joining resolves no names yet); a protocol or group mismatch is final.
    pub fn check_waiting(&self, attempts: u32, pause: std::time::Duration) -> Result<(), String> {
        let mut last = self.check();
        for _ in 1..attempts {
            match &last {
                Err(e) if e.contains("did not answer its agent API") => {
                    std::thread::sleep(pause);
                    last = self.check();
                }
                _ => break,
            }
        }
        last
    }

    /// Refuses at startup a service that speaks another protocol or lacks a group this server needs, instead of failing on the first request.
    pub fn check(&self) -> Result<(), String> {
        let status = self.status().map_err(|e| format!("rca at {} did not answer its agent API: {e}", self.authority))?;
        if status.protocol != PROTOCOL {
            return Err(format!("rca speaks agent API protocol {}, this orca-serve speaks {PROTOCOL}: update one of them", status.protocol));
        }
        let missing: Vec<&str> = needed_groups().into_iter().filter(|g| !status.groups.iter().any(|s| s == g)).collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("rca does not serve the agent API group(s) {} that orca-serve needs", missing.join(", ")))
        }
    }
}

impl AgentApi for RcaAgent {
    fn status(&self) -> Result<ServiceStatus, AgentError> {
        let result = self.open()?.call(STATUS, json!({})).map_err(failed)?;
        let s: StatusResult = serde_json::from_value(result).map_err(|e| AgentError::Failed(format!("service.status is unreadable: {e}")))?;
        Ok(ServiceStatus { protocol: s.protocol, version: s.version, commit: s.commit, groups: s.groups })
    }

    fn state_get(&self, scope: &str, key: &str) -> Result<Option<Value>, AgentError> {
        let result = self.open()?.call(agent_api_state::GET, json!({"scope": scope, "key": key})).map_err(failed)?;
        Ok(result.get("value").filter(|v| !v.is_null()).cloned())
    }

    fn request(&self, method: &'static str, params: Value) -> Result<Value, AgentError> {
        // The client sends the method's name only; the group of a `Method` is for the service's side.
        self.open()?.call(Method::new(GroupName::new("orca"), method), params).map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn closed_port_authority() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    }

    #[test]
    fn given_an_rca_that_does_not_answer_when_checked_waiting_then_it_retries_every_pause_and_then_reports_the_last_error() {
        let rca = RcaAgent::new(&closed_port_authority(), "bearer");
        let (attempts, pause) = (3_u32, Duration::from_millis(80));
        let started = Instant::now();
        let outcome = rca.check_waiting(attempts, pause);
        let waited = started.elapsed();
        assert!(outcome.unwrap_err().contains("did not answer its agent API"));
        assert!(waited >= pause * (attempts - 1), "waited only {waited:?}");
    }
}
