use super::breaker::Degraded;
use serde_json::{json, Value};

/// How long the calls of one kind took, as plain numbers: the caller measures, the domain only adds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    pub n: u64,
    pub sum_us: u64,
    pub max_us: u64,
}

impl Timing {
    pub fn record(&mut self, micros: u64) {
        self.n += 1;
        self.sum_us = self.sum_us.saturating_add(micros);
        self.max_us = self.max_us.max(micros);
    }

    pub fn to_json(&self) -> Value {
        let ms = |us: u64| (us as f64 / 100.0).round() / 10.0;
        json!({ "n": self.n, "sumMs": ms(self.sum_us), "maxMs": ms(self.max_us) })
    }
}

/// The last store call that failed, as a class and an HTTP status only: never the store's own words, which can carry a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failure {
    pub class: &'static str,
    pub status: Option<u16>,
}

/// What the run's own writes look like when asked for again: how many were looked up and how many the store holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seeded {
    pub checked: u64,
    pub present: u64,
}

/// What one run did against the store, as it is reported on stderr and in `bundle.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub hits: u64,
    pub misses: u64,
    pub puts: u64,
    /// Calls that sent nothing: cold, read-only, too large, or an entry that already existed.
    pub skipped: u64,
    /// Writes the tenancy's Object Storage quota refused (`StorageQuotaExceeded`): every cache write fails until space is freed.
    pub quota_exceeded: u64,
    /// Writes that were wanted and did not land, whatever the reason but a read-only run (the guard's refusal has its own report).
    pub dropped_puts: u64,
    pub last_failure: Option<Failure>,
    /// Set by `Cache::verify_written`; `None` when the run never asked.
    pub seeded: Option<Seeded>,
    pub requests: u64,
    pub bytes: u64,
    pub degraded: Option<Degraded>,
    /// The store calls that went out, timed: a put is the whole call, with its existence check.
    pub get: Timing,
    pub put: Timing,
    pub exists: Timing,
}

impl Stats {
    pub fn to_json(&self) -> Value {
        json!({
            "hits": self.hits,
            "misses": self.misses,
            "puts": self.puts,
            "skipped": self.skipped,
            "quotaExceeded": self.quota_exceeded,
            "droppedPuts": self.dropped_puts,
            "lastFailure": self.last_failure.map(|f| json!({ "class": f.class, "status": f.status })),
            "seeded": self.seeded.map(|s| json!({ "checked": s.checked, "present": s.present })),
            "requests": self.requests,
            "bytes": self.bytes,
            "degraded": self.degraded.map(Degraded::as_str),
            "latencyMs": { "get": self.get.to_json(), "put": self.put.to_json(), "exists": self.exists.to_json() },
        })
    }
}
