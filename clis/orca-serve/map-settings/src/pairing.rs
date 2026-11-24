use orca_core::wire::{self, string_field, Failure, RUNTIME_ERROR};
use orca_core::{Area, Ctx, Kind, Method, NotOffered};
use serde_json::Value;

const REASON_ENDPOINTS: &str = "owner-confirmed (decision 7): a product choice, not something the chain cannot prove (the pinned host lets a directly paired phone call it): a phone is paired with this one host by its own offer, which already carries the endpoint; there is no desktop whose other addresses it could list";
const REASON_RELAY: &str = "owner-confirmed (decision 7): provisioning a relay credential needs a relay service, which this host does not run";

const NOT_OFFERED: &[NotOffered] = &[
    NotOffered { name: "pairing.getEndpoints", reason: REASON_ENDPOINTS, decided_by: "M5" },
    NotOffered { name: "pairing.provisionRelay", reason: REASON_RELAY, decided_by: "M5" },
];

const EXCHANGE: &str = "pairing.exchangeTicket";
const METHODS: &[Method] = &[Method { name: EXCHANGE, kind: Kind::Unary }];

/// The pairing methods a phone calls on its own pairing. `pairing.exchangeTicket` swaps a single-use pairing code for the phone's own token; the
/// devices of this host are minted tokens, never single-use codes, so a well-formed proposal is answered the way the pinned host answers a device
/// that is no ticket, and the capability is not advertised (a phone then keeps its token). The two methods that need a relay or another desktop say why with `not_offered`.
pub struct PairingArea;

/// `{deviceToken: 48 to 128 lowercase hex characters}`, nothing else; the first issue is the message, as the pinned host reports it.
fn check_exchange_params(params: &Value) -> Result<(), Failure> {
    let token = string_field(params, "deviceToken")?.ok_or_else(|| Failure::invalid("Invalid input: expected string, received undefined"))?;
    if !(48..=128).contains(&token.len()) || !token.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(Failure::invalid("expected 48-128 lowercase hex characters"));
    }
    match params.as_object().and_then(|o| o.keys().find(|k| k.as_str() != "deviceToken")) {
        Some(key) => Err(Failure::invalid(format!("Unrecognized key: \"{key}\""))),
        None => Ok(()),
    }
}

impl Area for PairingArea {
    fn name(&self) -> &'static str {
        "pairing"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn not_offered(&self) -> &'static [NotOffered] {
        NOT_OFFERED
    }

    fn call(&self, _ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            EXCHANGE => check_exchange_params(params).and_then(|()| Err(Failure::new(RUNTIME_ERROR, "not_a_ticket"))),
            other => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}
