//! The server-administration reads a desktop makes on connect, and the access list (docs/solutions/workflow/t3-desktop-bearer-profile.md).
//! Process and trace rows are the empty answers the schema allows; history rows are host readings from the ring below.
use super::queries::QueryFailure;
use super::service::{Principal, Service};
use crate::domain::auth::token_id;
use crate::ports::Millis;
use serde_json::{json, Value};
use std::time::Instant;
use t3_contracts::scopes::legacy_subset;
use t3_contracts::time::iso;
use t3_contracts::wire::{self, BEARER_TTL_SECONDS, PAIRING_TTL_SECONDS};

/// How many host readings the ring keeps.
const RING: usize = 360;
/// The shortest window and bucket the pin answers.
const MIN_SPAN_MS: i64 = 1000;
/// The most buckets a history lists.
const MAX_BUCKETS: i64 = 120;

/// One bucket of a history: from, to, mean and highest cpu percent, and the most memory in use.
type Bucket = (Millis, Millis, f64, f64, u64);

/// One look at the host: when, how busy its processors were (percent) and how much memory was in use.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    at: Millis,
    cpu_percent: f64,
    used_bytes: u64,
}

pub enum Admin {
    Answer(Value),
    /// The frames a stream opens with; it stays open until the client closes it.
    Open(Vec<Value>),
}

pub fn handles(tag: &str) -> bool {
    matches!(
        tag,
        "subscribeAuthAccess" | "subscribeDiscoveredLocalServers" | "server.getProcessDiagnostics" | "server.getProcessResourceHistory" | "server.getResourceTelemetryHistory" | "server.getTraceDiagnostics" | "server.retryResourceTelemetry"
    )
}

fn none() -> Value {
    json!({"_tag": "None"})
}

fn some(value: Value) -> Value {
    json!({"_tag": "Some", "value": value})
}

fn zero_group() -> Value {
    json!({"cpuTimeMs": 0, "currentCpuPercent": 0, "currentRssBytes": 0, "ioReadBytes": 0, "ioReadBytesPerSecond": 0, "ioWriteBytes": 0, "ioWriteBytesPerSecond": 0, "peakRssBytes": 0, "processCount": 0, "processExits": 0, "processStarts": 0})
}

impl Service {
    pub fn desk_admin(&self, tag: &str, payload: &Value, principal: &Principal) -> Result<Admin, QueryFailure> {
        let now = self.0.clock.now_ms();
        Ok(match tag {
            "subscribeAuthAccess" => Admin::Open(vec![json!({"version": 1, "revision": 1, "type": "snapshot", "payload": self.access_snapshot(principal, now)?})]),
            "subscribeDiscoveredLocalServers" => Admin::Open(vec![json!({"configuredUrlProbing": false, "scannedAt": iso(now), "servers": []})]),
            "server.getProcessDiagnostics" => Admin::Answer(json!({"error": none(), "processCount": 0, "processes": [], "readAt": iso(now), "serverPid": std::process::id(), "totalCpuPercent": 0, "totalRssBytes": 0})),
            "server.getProcessResourceHistory" => Admin::Answer(self.process_history(payload, now)),
            "server.getResourceTelemetryHistory" => Admin::Answer(self.telemetry_history(payload, now)),
            "server.getTraceDiagnostics" => Admin::Answer(trace_diagnostics(now)),
            _ => Admin::Answer(self.retry_telemetry(now)),
        })
    }

    /// The bearers and the unredeemed pairing codes, as the access pane lists them. Only this connection is known to be connected.
    fn access_snapshot(&self, principal: &Principal, now: Millis) -> Result<Value, QueryFailure> {
        let failed = |e: crate::ports::StoreError| {
            self.log("access.list.failed", &[("error", e.0.clone())]);
            QueryFailure::Error(wire::host_error(&e.0))
        };
        let sessions: Vec<Value> = self
            .0
            .auth
            .list_tokens(now)
            .map_err(failed)?
            .into_iter()
            .map(|t| {
                let current = t.id == principal.token_id;
                json!({
                    "client": {"deviceType": "unknown", "label": t.label},
                    "connected": current, "current": current,
                    "expiresAt": iso(t.expires_at), "issuedAt": iso(t.expires_at - BEARER_TTL_SECONDS * 1000),
                    "lastConnectedAt": if current { json!(iso(now)) } else { Value::Null },
                    "method": "bearer-access-token", "permissions": t.scopes, "scopes": legacy_subset(&t.scopes),
                    "sessionId": t.id, "subject": "one-time-token",
                })
            })
            .collect();
        let links: Vec<Value> = self
            .0
            .auth
            .list_grants(now)
            .map_err(failed)?
            .into_iter()
            .map(|(hash, grant, expires_at)| {
                // The store keeps no mint time, so a code minted for a custom lifetime shows the default one's start
                json!({"id": token_id(&hash), "label": grant.label, "permissions": grant.scopes, "scopes": legacy_subset(&grant.scopes), "subject": "one-time-token", "createdAt": iso(expires_at - PAIRING_TTL_SECONDS * 1000), "expiresAt": iso(expires_at)})
            })
            .collect();
        Ok(json!({"clientSessions": sessions, "pairingLinks": links}))
    }

    /// Takes one reading of the host into the ring; none when the host cannot be read.
    fn take_reading(&self, now: Millis) -> Result<(Reading, u128), String> {
        let started = Instant::now();
        let sample = self.0.host.sample()?;
        let reading = Reading { at: now, cpu_percent: sample.cpu_utilization.unwrap_or(0.0) * 100.0, used_bytes: sample.total_memory_bytes.saturating_sub(sample.available_memory_bytes) };
        let mut ring = self.2.telemetry.lock().unwrap_or_else(|e| e.into_inner());
        ring.push_back(reading);
        if ring.len() > RING {
            ring.pop_front();
        }
        Ok((reading, started.elapsed().as_micros()))
    }

    /// The buckets of the readings inside the window, and how many readings the ring holds.
    fn buckets(&self, payload: &Value, now: Millis) -> (i64, i64, Vec<Bucket>, usize) {
        let window = payload["windowMs"].as_i64().unwrap_or(0).max(MIN_SPAN_MS);
        let bucket = payload["bucketMs"].as_i64().unwrap_or(0).max(MIN_SPAN_MS).max(window / MAX_BUCKETS);
        let since = now - window;
        let ring = self.2.telemetry.lock().unwrap_or_else(|e| e.into_inner());
        let mut grouped: Vec<(i64, Vec<Reading>)> = Vec::new();
        for r in ring.iter().filter(|r| r.at >= since) {
            let index = (r.at - since) / bucket;
            match grouped.last_mut() {
                Some((i, group)) if *i == index => group.push(*r),
                _ => grouped.push((index, vec![*r])),
            }
        }
        let rows = grouped
            .into_iter()
            .map(|(index, group)| {
                let avg = group.iter().map(|r| r.cpu_percent).sum::<f64>() / group.len() as f64;
                let max_cpu = group.iter().map(|r| r.cpu_percent).fold(0.0, f64::max);
                let max_used = group.iter().map(|r| r.used_bytes).max().unwrap_or(0);
                (since + index * bucket, (since + (index + 1) * bucket).min(now), avg, max_cpu, max_used)
            })
            .collect();
        (window, bucket, rows, ring.len())
    }

    /// `server.getProcessResourceHistory`: no process is tracked (0 in every bucket, as in the process diagnostics); cpu and memory are host totals.
    fn process_history(&self, payload: &Value, now: Millis) -> Value {
        let error = match self.take_reading(now) {
            Ok(_) => none(),
            Err(message) => some(json!({"message": message})),
        };
        let (window, bucket, rows, retained) = self.buckets(payload, now);
        let buckets: Vec<Value> = rows.iter().map(|(from, to, avg, max_cpu, max_used)| json!({"avgCpuPercent": avg, "endedAt": iso(*to), "maxCpuPercent": max_cpu, "maxProcessCount": 0, "maxRssBytes": max_used, "startedAt": iso(*from)})).collect();
        json!({"bucketMs": bucket, "buckets": buckets, "error": error, "readAt": iso(now), "retainedSampleCount": retained, "sampleIntervalMs": 0, "topProcesses": [], "totalCpuSecondsApprox": 0, "windowMs": window})
    }

    fn health(&self, now: Millis, reading: &Result<(Reading, u128), String>) -> Value {
        let (native, micros) = match reading {
            Ok((_, micros)) => (json!({"lastError": none(), "lastSampleAt": some(json!(iso(now))), "status": "healthy"}), *micros as u64),
            Err(message) => (json!({"lastError": some(json!(message)), "lastSampleAt": none(), "status": "unavailable"}), 0),
        };
        json!({
            "collectionDurationMicros": micros,
            "desktop": {"lastError": some(json!("Desktop telemetry descriptor is unavailable in 'web' mode.")), "lastSampleAt": none(), "status": "unavailable"},
            "inaccessibleProcessCount": 0, "native": native, "restartCount": 0, "retainedProcessCount": 0, "scannedProcessCount": 0,
            "sidecarPid": none(), "sidecarVersion": none(),
        })
    }

    /// `server.getResourceTelemetryHistory`: the same readings with the io columns at zero, the host being read by no sidecar.
    fn telemetry_history(&self, payload: &Value, now: Millis) -> Value {
        let reading = self.take_reading(now);
        let (window, bucket, rows, retained) = self.buckets(payload, now);
        let buckets: Vec<Value> = rows.iter().map(|(from, to, avg, max_cpu, max_used)| json!({"avgCpuPercent": avg, "endedAt": iso(*to), "ioReadBytes": 0, "ioWriteBytes": 0, "maxCpuPercent": max_cpu, "maxProcessCount": 0, "maxRssBytes": max_used, "startedAt": iso(*from)})).collect();
        json!({"bucketMs": bucket, "buckets": buckets, "health": self.health(now, &reading), "readAt": iso(now), "retainedSampleCount": retained, "sampleIntervalMs": 0, "topProcesses": [], "windowMs": window})
    }

    /// `server.retryResourceTelemetry`: there is no sidecar to restart, so nothing was accepted; the snapshot is the zero reading of no process.
    fn retry_telemetry(&self, now: Millis) -> Value {
        let reading = self.take_reading(now);
        json!({
            "accepted": false,
            "snapshot": {
                "attribution": {"entries": [], "readAt": iso(now)},
                "groups": {"allT3": zero_group(), "backend": zero_group(), "electron": zero_group(), "monitor": zero_group()},
                "health": self.health(now, &reading),
                "power": {"idle": "unknown", "idleSeconds": null, "locked": "unknown", "lowPowerMode": "unknown", "onBattery": "unknown", "source": "unknown", "stale": true, "suspended": false, "thermalState": "unknown", "updatedAt": iso(now)},
                "processes": [], "readAt": iso(now), "sampleIntervalMs": 0, "speedLimitPercent": none(),
            },
        })
    }
}

/// `server.getTraceDiagnostics`: t3 serve writes no trace file, which the schema's own error kind says (docs/gaps/t3-desktop-trace-diagnostics-no-trace-file.md).
fn trace_diagnostics(now: Millis) -> Value {
    json!({
        "commonFailures": [], "error": some(json!({"kind": "trace-file-not-found", "message": "t3 serve writes no trace file."})), "failureCount": 0, "firstSpanAt": none(),
        "interruptionCount": 0, "lastSpanAt": none(), "latestFailures": [], "latestWarningAndErrorLogs": [], "logLevelCounts": {}, "parseErrorCount": 0,
        "partialFailure": none(), "readAt": iso(now), "recordCount": 0, "scannedFilePaths": [], "slowSpanCount": 0, "slowSpanThresholdMs": 1000,
        "slowestSpans": [], "topSpansByCount": [], "traceFilePath": "none",
    })
}
