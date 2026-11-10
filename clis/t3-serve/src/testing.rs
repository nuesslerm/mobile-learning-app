//! The real router over a loopback socket with every port a double: the flow tests' server.
use crate::adapters::StaticCatalog;
use std::sync::{Arc, Mutex};
use t3_core::ports::{AgentApi, AuthStore, CatalogModel, Clock, EventStore, HostProbe, HostSample, Ids, Log, MetaStore, ModelCatalog, StorageProbe, StorageSample};
use t3_core::testing::{FixedClock, FixedHost, FixedStorage, MemLog, MemStore, ScriptedAgent, SeqIds};
use t3_core::usecases::engine::Engine;
use t3_core::usecases::pairing::{self, Pairing, PairingRequest};
use t3_core::{EnvironmentInfo, Parts, Service};

pub struct Harness {
    pub clock: Arc<FixedClock>,
    pub log: Arc<MemLog>,
    pub store: Arc<MemStore>,
    pub ids: Arc<SeqIds>,
    pub agent: Arc<ScriptedAgent>,
    pub host: Arc<FixedHost>,
    pub storage: Arc<FixedStorage>,
    pub service: Service,
    server: Option<t3_http::Running>,
    pub addr: std::net::SocketAddr,
}

/// 2026-11-01T00:00:00Z
pub const START_MS: i64 = 1_793_491_200_000;
pub const ENVIRONMENT_ID: &str = "00000000-0000-4000-8000-000000000001";

impl Harness {
    pub fn start() -> Harness {
        let clock = Arc::new(FixedClock::at(START_MS));
        let (log, store, ids) = (Arc::new(MemLog::default()), Arc::new(MemStore::default()), Arc::new(SeqIds::seeded(0x9E37_79B9_7F4A_7C15)));
        Harness::over(clock, log, store, ids, Arc::new(ScriptedAgent::default()))
    }

    /// A server started again on the store, the log and the agent API this one has: what a restart of `t3 serve` is. Nothing is resumed until
    /// `service.resume_runs()` is called, as `t3 serve` does at startup.
    pub fn restart(&self) -> Harness {
        Harness::over(self.clock.clone(), self.log.clone(), self.store.clone(), self.ids.clone(), self.agent.clone())
    }

    fn over(clock: Arc<FixedClock>, log: Arc<MemLog>, store: Arc<MemStore>, ids: Arc<SeqIds>, agent: Arc<ScriptedAgent>) -> Harness {
        let catalog = StaticCatalog(vec![CatalogModel { slug: "rca-default".into(), name: "rca default".into() }]);
        let engine = Arc::new(Engine::open(store.clone() as Arc<dyn EventStore>, clock.clone() as Arc<dyn Clock>, ids.clone() as Arc<dyn Ids>).expect("a readable log"));
        let env = EnvironmentInfo { id: ENVIRONMENT_ID.into(), label: "test rca".into(), os: "linux".into(), arch: "arm64".into(), server_version: "0.1.0-test".into(), cwd: "/workspace".into() };
        let host = Arc::new(FixedHost(Mutex::new(Ok(HostSample { cpu_count: 4, cpu_utilization: Some(0.25), available_memory_bytes: 6_000_000_000, total_memory_bytes: 8_000_000_000 }))));
        let storage = Arc::new(FixedStorage(Mutex::new(Ok(StorageSample { database_bytes: 4_096_000, reclaimable_bytes: 8_192, incremental_vacuum: true, path: "/state/state.sqlite".into() }))));
        let service = Service::new(Parts {
            env,
            auth: store.clone() as Arc<dyn AuthStore>,
            clock: clock.clone() as Arc<dyn Clock>,
            ids: ids.clone() as Arc<dyn Ids>,
            log: log.clone() as Arc<dyn Log>,
            catalog: Arc::new(catalog) as Arc<dyn ModelCatalog>,
            engine,
            agent: agent.clone() as Arc<dyn AgentApi>,
            meta: store.clone() as Arc<dyn MetaStore>,
            host: host.clone() as Arc<dyn HostProbe>,
            storage: storage.clone() as Arc<dyn StorageProbe>,
        });
        let server = t3_http::serve("127.0.0.1:0", service.clone()).expect("loopback");
        let addr = server.addr();
        Harness { clock, log, store, ids, agent, host, storage, service, server: Some(server), addr }
    }

    pub fn pair(&self, request: PairingRequest) -> Pairing {
        pairing::mint(self.store.as_ref(), self.clock.as_ref(), self.ids.as_ref(), request).expect("a pairing")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}
