use orca_core::wire::{self, Failure};
use orca_core::{Area, Ctx, Method, NotOffered};
use serde_json::Value;

const REASON_ENDPOINTS: &str = "proposed default awaiting owner confirmation (decision 1): a product choice, not something the chain cannot prove (the pinned host lets a directly paired phone call it): a phone is paired with this one host by its own offer, which already carries the endpoint; there is no desktop whose other addresses it could list";
const REASON_RELAY: &str = "proposed default awaiting owner confirmation (decision 1): provisioning a relay credential needs a relay service, which this host does not run";

const NOT_OFFERED: &[NotOffered] = &[
    NotOffered { name: "pairing.getEndpoints", reason: REASON_ENDPOINTS, decided_by: "M5" },
    NotOffered { name: "pairing.provisionRelay", reason: REASON_RELAY, decided_by: "M5" },
];

/// The pairing methods the host would answer for a phone's own pairing: none are served, and each says why with `not_offered`.
pub struct PairingArea;

impl Area for PairingArea {
    fn name(&self) -> &'static str {
        "pairing"
    }

    fn methods(&self) -> &'static [Method] {
        &[]
    }

    fn not_offered(&self) -> &'static [NotOffered] {
        NOT_OFFERED
    }

    fn call(&self, _ctx: &mut Ctx, method: &str, _params: &Value) -> Result<Value, Failure> {
        Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {method}")))
    }
}
