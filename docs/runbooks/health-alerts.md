---
title: health-alerts - the two Better Stack alert rules on the VM's health gauges
verifies: A container whose healthcheck fails, and a silent host each raise an alert within two minutes.
tier: runbook
---

# health-alerts

`reporting-service` samples every 15 s and publishes each sweep's samples as gauges to a Better Stack OTLP metrics source
(`clis/telemetry` `OtlpMetricsSink`, one request per batch). ORDS keeps the history; these rules read the live copy. Plan:
`docs/plans/health-metrics/plan.md`.

## Prerequisites (the owner, once)

1. Better Stack -> Telemetry -> Sources -> Connect source -> OpenTelemetry metrics, a source of its own. Name it `vm-health-metrics`.
2. Store its ingest host and token in Bitwarden: `wf secret set BETTERSTACK_METRICS_HOST` and `wf secret set BETTERSTACK_METRICS_TOKEN`.
3. Without the metrics host and token the service logs `metrics.publish.off` and publishes nothing.

## The gauges

All gauges, two labels: `scope` (`container` or `host`) and `name` (the container, `host` or `docker`).

| Metric | Value | Source |
| --- | --- | --- |
| `container.health` | 1 healthy, 0 unhealthy; absent for a container without a healthcheck or still starting | Docker healthcheck |
| `host.cpu` (and the other `host.*`) | host figures, one per sweep | `/proc` |
| `host.docker.list_ms` | milliseconds the Docker daemon took to list containers | Docker Engine |

## Alert rules

Create each in Better Stack -> Telemetry -> Alerts on the source `vm-health-metrics`. Notify email and push, the same policy as the
existing heartbeat.

### 1. Container unhealthy

- Metric: `container.health`
- Filter: none (every container with a healthcheck)
- Group by: `name`
- Aggregate: minimum per 1 minute
- Fire when: value = 0 for 1 minute
- Message: `{{name}} has failed its healthcheck for a minute`

### 2. Host silent (the missing heartbeat)

- Metric: `host.cpu`
- Filter: `scope = host` and `name = host`
- Fire when: no data point for 60 seconds (the "no data" condition, not a threshold)
- Message: `no host sample for 60 s: reporting-service or the VM is down`

## Verifying (the one drill, once the token is stored)

1. Within 30 s of a deploy, a `container.health` point for an app's container (agent-shell's, say) is in the source (Better Stack -> Live tail on the metrics source, or a
   query for the metric).
2. Stop a tailscale sidecar on purpose; its app goes unhealthy, rule 1 fires within about two minutes. Start it again and the alert resolves.
3. Record the outcome in the decision log with the time each alert fired.
