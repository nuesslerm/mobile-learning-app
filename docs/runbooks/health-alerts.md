---
title: health-alerts - the three Better Stack alert rules on the VM's health gauges
verifies: A container whose healthcheck fails, a silent host and a runner idling with jobs queued each raise an alert within two minutes.
tier: runbook
---

# health-alerts

`reporting-service` samples every 15 s and publishes each sweep's samples as gauges to a Better Stack OTLP metrics source
(`clis/telemetry` `OtlpMetricsSink`, one request per batch). ORDS keeps the history; these rules read the live copy. Plan:
`docs/plans/health-metrics/plan.md`.

## Prerequisites (the owner, once)

1. Better Stack -> Telemetry -> Sources -> Connect source -> OpenTelemetry metrics, a source of its own. Name it `vm-health-metrics`.
2. Store its ingest host and token in Bitwarden: `wf secret set BETTERSTACK_METRICS_HOST` and `wf secret set BETTERSTACK_METRICS_TOKEN`.
3. The runner rules read GitHub with `VM_PROVISION_GITHUB_TOKEN` (Actions and Administration read are enough), and `REPORTING_GITHUB_REPO` and `REPORTING_RUNNER_NAME` (`vm-<HOST_LABEL>`) come from reporting-service's `.env.defaults`: nothing to store. If the runner rules stay silent, the service logs `runner.sampling.off` or `sample.runner.github_failed`.
4. Without the metrics host and token the service logs `metrics.publish.off` and publishes nothing.

## The gauges

All gauges, two labels: `scope` (`container`, `host` or `runner`) and `name` (the container, `host`, `docker` or the runner's name).

| Metric | Value | Source |
| --- | --- | --- |
| `container.health` | 1 healthy, 0 unhealthy; absent for a container without a healthcheck or still starting | Docker healthcheck |
| `host.cpu` (and the other `host.*`) | host figures, one per sweep | `/proc` |
| `host.docker.list_ms` | milliseconds the Docker daemon took to list containers | Docker Engine |
| `ci.runner.listener_up` | 1 when GitHub lists the runner online, else 0 | GitHub API |
| `ci.runner.github_reachable` | 1 when the GitHub API answered, 0 when it did not (the other `ci.runner.*` are absent then) | GitHub API |
| `ci.runner.queued_jobs_waiting` | queued workflow runs of the repository | GitHub API |
| `ci.runner.idle_seconds` | seconds since the runner was last busy, 0 while it is | GitHub API |

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

### 3. Runner idle with work waiting

- Metrics: `ci.runner.queued_jobs_waiting` and `ci.runner.idle_seconds`, grouped by `name` (the runner)
- Fire when: `ci.runner.queued_jobs_waiting` > 0 and `ci.runner.idle_seconds` > 120, both holding at the latest point of the same sweep
- Message: `{{name}} has been idle for over two minutes with jobs queued`

## Verifying (the one drill, once the token is stored)

1. Within 30 s of a deploy, a `container.health` point for `ci-runner-gh-runner` is in the source (Better Stack -> Live tail on the metrics source, or a
   query for the metric).
2. Stop tailscale on purpose; the runner goes unhealthy, rule 1 fires within about two minutes. Start it again and the alert resolves.
3. Record the outcome in the decision log with the time each alert fired.
