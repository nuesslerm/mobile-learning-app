//! `mobileRelay.hosts.list` and `.worktrees`: the servers the host relays a phone to. This host relays to none, which is what the pinned host
//! answers when no desktop has wired a relay (`{hosts: []}` and `{worktrees: null}`), so a phone shows only this host.
use crate::{HOSTS_LIST, HOSTS_WORKTREES};
use orca_core::wire::{self, Failure};
use orca_core::{Area, Ctx, Kind, Method};
use serde_json::{json, Value};

const METHODS: &[Method] = &[Method { name: HOSTS_LIST, kind: Kind::Unary }, Method { name: HOSTS_WORKTREES, kind: Kind::Unary }];

pub struct RelayArea;

/// `{hostId: 1..1024 chars, afterSnapshotId?: 1..128 chars | null, acceptRowDelta?: true}`, nothing else.
fn check_worktrees_params(params: &Value) -> Result<(), Failure> {
    let invalid = |m: &str| Failure::invalid(m.to_string());
    let o = params.as_object().ok_or_else(|| invalid("Invalid input: expected object"))?;
    if o.keys().any(|k| !matches!(k.as_str(), "hostId" | "afterSnapshotId" | "acceptRowDelta")) {
        return Err(invalid("Unrecognized key in worktrees params"));
    }
    if !o.get("hostId").and_then(Value::as_str).is_some_and(|h| (1..=1024).contains(&h.chars().count())) {
        return Err(invalid("Invalid hostId: expected a string of 1 to 1024 characters"));
    }
    match o.get("afterSnapshotId") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if (1..=128).contains(&s.chars().count()) => {}
        Some(_) => return Err(invalid("Invalid afterSnapshotId: expected null or a string of 1 to 128 characters")),
    }
    match o.get("acceptRowDelta") {
        None | Some(Value::Bool(true)) => Ok(()),
        Some(_) => Err(invalid("Invalid acceptRowDelta: expected true")),
    }
}

impl Area for RelayArea {
    fn name(&self) -> &'static str {
        "mobile-relay"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, _ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            HOSTS_LIST => Ok(json!({"hosts": []})),
            HOSTS_WORKTREES => check_worktrees_params(params).map(|()| json!({"worktrees": null})),
            other => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}
