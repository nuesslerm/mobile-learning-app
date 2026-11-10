//! `Host` is the process-wide part (runtime id, the areas, the table, the ports); a `Connection` is one authenticated socket's dispatcher.
use crate::area::{Area, Ctx, Kind, Subscriptions};
use crate::ports::{AgentApi, HostInfo, Principals};
use crate::table::{Entry, Table};
use crate::wire::{self, Failure};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct Host {
    pub runtime_id: String,
    areas: Vec<Box<dyn Area>>,
    table: Table,
    agent: Arc<dyn AgentApi>,
    principals: Arc<dyn Principals>,
    info: HostInfo,
    subscription_seq: AtomicU64,
    connections: AtomicU64,
}

impl Host {
    pub fn new(runtime_id: String, areas: Vec<Box<dyn Area>>, agent: Arc<dyn AgentApi>, principals: Arc<dyn Principals>, info: HostInfo) -> Result<Arc<Host>, String> {
        let table = Table::assemble(&areas)?;
        Ok(Arc::new(Host { runtime_id, areas, table, agent, principals, info, subscription_seq: AtomicU64::new(0), connections: AtomicU64::new(0) }))
    }

    pub fn api(&self) -> Value {
        self.table.api(&self.areas)
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    /// The dispatcher of a socket that proved `device_id` holds `device_token`.
    pub fn connection(self: &Arc<Self>, device_id: &str, device_token: &str, client_capabilities: Vec<String>) -> Connection {
        let n = self.connections.fetch_add(1, Ordering::SeqCst) + 1;
        Connection { host: self.clone(), id: format!("conn{n}"), device_id: device_id.to_string(), device_token: device_token.to_string(), client_capabilities, subscriptions: Subscriptions::default() }
    }
}

pub struct Connection {
    host: Arc<Host>,
    id: String,
    device_id: String,
    device_token: String,
    client_capabilities: Vec<String>,
    subscriptions: Subscriptions,
}

impl Connection {
    pub fn client_capabilities(&self) -> &[String] {
        &self.client_capabilities
    }

    pub fn open_streams(&self) -> usize {
        self.subscriptions.len()
    }

    /// Answers one request frame with the frames to send, in order (the answer, then what streams queued meanwhile).
    pub fn on_message(&mut self, text: &str) -> Vec<String> {
        let host = self.host.clone();
        let runtime_id = host.runtime_id.as_str();
        let mut frames = Vec::new();
        let request = match wire::parse(text) {
            Ok(r) => r,
            Err(unparsed) => return vec![wire::failure(&unparsed.0, runtime_id, &unparsed.1).to_string()],
        };
        if request.device_token.as_deref().is_some_and(|t| t != self.device_token) {
            return vec![wire::failure(&request.id, runtime_id, &Failure::new(wire::UNAUTHORIZED, "Device token mismatch")).to_string()];
        }
        if !host.principals.still_paired(&self.device_id, &self.device_token) {
            return vec![wire::failure(&request.id, runtime_id, &Failure::new(wire::UNAUTHORIZED, "Invalid device token")).to_string()];
        }
        let reply = match host.table.get(&request.method) {
            None => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {}", request.method))),
            Some(Entry::NotOffered { reason, decided_by }) => {
                let mut f = Failure::new(wire::NOT_OFFERED, format!("{} is not offered by this host: {reason}", request.method));
                f.data = Some(serde_json::json!({"decidedBy": decided_by}));
                Err(f)
            }
            Some(Entry::Served { area, kind }) => {
                let area = &host.areas[*area];
                let mut ctx = Ctx {
                    runtime_id,
                    connection_id: &self.id,
                    request_id: &request.id,
                    device_id: &self.device_id,
                    agent: host.agent.as_ref(),
                    agent_handle: host.agent.clone(),
                    host: &host.info,
                    capabilities: host.table.capabilities(),
                    client_capabilities: &mut self.client_capabilities,
                    subscriptions: &mut self.subscriptions,
                    seq: &host.subscription_seq,
                };
                match kind {
                    Kind::Unary => area.call(&mut ctx, &request.method, &request.params).map(Some),
                    Kind::Streaming => area.open(&mut ctx, &request.method, &request.params).map(|sub| {
                        ctx.subscriptions.add(&request.id, sub);
                        None
                    }),
                }
            }
        };
        match reply {
            Ok(Some(result)) => frames.push(wire::success(&request.id, runtime_id, result).to_string()),
            Ok(None) => {}
            Err(f) => frames.push(wire::failure(&request.id, runtime_id, &f).to_string()),
        }
        frames.extend(self.poll());
        frames
    }

    /// What the open streams have queued, as frames; the socket loop calls it between reads.
    pub fn poll(&mut self) -> Vec<String> {
        let runtime_id = self.host.runtime_id.clone();
        self.subscriptions.flush().into_iter().map(|(id, event)| wire::streaming(&id, &runtime_id, event).to_string()).collect()
    }

    /// The socket closed: every stream ends, with nobody left to hear it.
    pub fn close(&mut self) {
        self.subscriptions.end_all();
    }
}
