use super::breaker::Degraded;
use super::kind::Kind;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// The committed limits (`data/budget.json`): what the cache may store, send and keep.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    /// The kill switch: false sends nothing.
    pub enabled: bool,
    pub stored_bytes: u64,
    /// The tenancy-wide Object Storage quota every bucket shares (`always_free_object_storage_cap_gb` of `infra/oracle-cloud/always-free-quota.tf`).
    pub tenancy_stored_bytes: u64,
    /// What the VM state bucket may hold, every object version included (`vm_state_budget_bytes` of `infra/oracle-cloud/vm-state.tf`): four times the sum of the `maxBytes` of the state volumes in vm-apps.json, counted with the cache against the tenancy cap.
    #[serde(default)]
    pub vm_state_bytes: u64,
    /// What the live restic repository in that bucket may hold (`vm_state_repository_bytes` of `vm-state.tf`): twice the same sum.
    #[serde(default)]
    pub vm_state_repository_bytes: u64,
    pub requests_per_month: u64,
    pub requests_per_run: u64,
    pub bytes_per_run: u64,
    kinds: BTreeMap<String, KindBudget>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KindBudget {
    ttl_days: u32,
    max_entry_bytes: u64,
    /// A kind whose run is many small calls (a compile cache) sets its own per-run limits instead of an Nx run's.
    #[serde(default)]
    requests_per_run: Option<u64>,
    #[serde(default)]
    bytes_per_run: Option<u64>,
}

impl Budget {
    /// A budget with one row for every kind and none for a kind that does not exist.
    pub fn parse(text: &str) -> Result<Budget, String> {
        let budget: Budget = serde_json::from_str(text).map_err(|e| e.to_string())?;
        for kind in Kind::ALL {
            if !budget.kinds.contains_key(kind.prefix()) {
                return Err(format!("budget has no row for kind {}", kind.prefix()));
            }
        }
        if let Some(extra) = budget.kinds.keys().find(|k| Kind::from_prefix(k).is_none()) {
            return Err(format!("budget has a row for unknown kind {extra}"));
        }
        Ok(budget)
    }

    pub fn committed() -> &'static Budget {
        static COMMITTED: OnceLock<Budget> = OnceLock::new();
        COMMITTED.get_or_init(|| Budget::parse(include_str!("../../data/budget.json")).expect("clis/blob-cache/data/budget.json is a valid budget"))
    }

    pub fn ttl_days(&self, kind: Kind) -> u32 {
        self.kinds[kind.prefix()].ttl_days
    }

    pub fn max_entry_bytes(&self, kind: Kind) -> u64 {
        self.kinds[kind.prefix()].max_entry_bytes
    }

    /// The budget of a run of `kind`: the committed per-run limits, unless the kind's row sets its own.
    pub fn for_kind_run(&self, kind: Kind) -> Budget {
        let row = self.kinds[kind.prefix()];
        let mut budget = self.clone();
        budget.requests_per_run = row.requests_per_run.unwrap_or(budget.requests_per_run);
        budget.bytes_per_run = row.bytes_per_run.unwrap_or(budget.bytes_per_run);
        budget
    }

    /// The budget of a run whose one job is a single large entry of `kind` (the pnpm store): the byte limit that keeps an Nx run
    /// small would refuse it before its first byte, so a run may send at least that entry's cap and a sidecar beside it.
    pub fn for_one_entry_of(&self, kind: Kind) -> Budget {
        const SIDECAR: u64 = 1024 * 1024;
        let mut budget = self.clone();
        budget.bytes_per_run = budget.bytes_per_run.max(self.max_entry_bytes(kind).saturating_add(SIDECAR));
        budget
    }
}

/// What one run has spent against `requests_per_run` and `bytes_per_run`.
#[derive(Debug)]
pub struct Meter {
    requests: u64,
    bytes: u64,
    max_requests: u64,
    max_bytes: u64,
}

impl Meter {
    pub fn new(budget: &Budget) -> Meter {
        Meter { requests: 0, bytes: 0, max_requests: budget.requests_per_run, max_bytes: budget.bytes_per_run }
    }

    /// Charges `requests` that send `outgoing` bytes, or refuses without charging when the run would pass a limit.
    pub fn admit(&mut self, requests: u64, outgoing: u64) -> Result<(), Degraded> {
        if self.requests.saturating_add(requests) > self.max_requests || self.bytes.saturating_add(outgoing) > self.max_bytes {
            return Err(Degraded::Budget);
        }
        self.requests += requests;
        self.bytes += outgoing;
        Ok(())
    }

    /// Bytes that came back; the next `admit` refuses once they are over the limit.
    pub fn received(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
    }

    pub fn requests(&self) -> u64 {
        self.requests
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
