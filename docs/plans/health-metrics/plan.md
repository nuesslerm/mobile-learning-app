# Health metrics to Better Stack, history in ORDS

## Why

On 1 Nov the CI runner sat idle for about 45 minutes with jobs queued and nothing told us: its container was "up", it shares the tailscale container's network, and that network had been replaced under it. The signals existed (agent-shell's heartbeat stopped at 17:21) but only as logs, nothing alerted on a missing one, and the runner's own log was never shipped. Health data should be metrics: numbers sampled every 15 s, sent to Better Stack for monitoring and alerting, and kept in ORDS for history.

## What exists

- `reporting-service` samples every container and the host (cpu, memory, disk, `health`, docker list time) and stores each `Sample` in ORDS (`rs/src/infrastructure/ords_store.rs`). The container and host samples run every 15 s since 77301e0a0; the `health` sample (1 healthy, 0 unhealthy, none without a healthcheck) is new in the same commit.
- `watchdog` sweeps every 15 s and recreates a container that is unhealthy on two sweeps; `gh-runner` now has a healthcheck (listener alive, GitHub reachable from inside its network).
- `clis/telemetry` already sends logs to Better Stack over OTLP (`wf log emit`, the services' tracing layer); it has no metrics pipeline.

## Design

1. **A metrics sink in `clis/telemetry`.** A port `MetricsSink` in the crate's core (`record(name, scope, value, at)`), an OTLP/HTTP adapter that posts gauges to a Better Stack metrics source (OpenTelemetry metrics, the workspace's `opentelemetry` 0.30), batching one request per sample sweep, and an in-memory double behind its `testing` feature. A store that is unreachable never fails a sweep (the same rule the build cache follows): the failure is counted and logged once.
2. **reporting-service publishes what it stores.** After a sweep stores its `Sample`s, the same slice goes to the sink. One extra port call in the scheduler's composition (`api/app.rs`); the sampler and the ORDS store do not change. ORDS stays the history of record, Better Stack is the live and alerting copy.
3. **New metrics** (all gauges, names stable, labels `scope` and `name`): `container.health`, `host.docker.list_ms` (already sampled), `ci.runner.listener_up`, `ci.runner.github_reachable`, `ci.runner.queued_jobs_waiting` and `ci.runner.idle_seconds` (the last two from the GitHub API with the repo's read token: queued jobs while the runner is idle is the exact condition that went unnoticed). The runner metrics are sampled by reporting-service the same way as the host.
4. **Secret.** One new entry in `clis/secrets/src/data/registry.json` for the metrics source token and endpoint (no value invented; the owner mints the source in Better Stack and stores it in Bitwarden). Without it the sink is off and the service logs that once.
5. **Alerts** (defined in Better Stack, written down in `docs/runbooks/health-alerts.md` with the exact rules so they can be recreated): `container.health` = 0 for 1 minute; no sample at all from a host for 60 seconds (the missing-heartbeat case); `ci.runner.queued_jobs_waiting` > 0 while `ci.runner.idle_seconds` > 120.
6. **Polling is 15 s** for health data (done); storage and prune stay on their long intervals.

## Verification

- Flow test: given the reporting service with an in-memory sink and containers with and without healthchecks, when it samples, then the sink receives the same readings that were stored, and a failing sink does not stop the sweep or the ORDS write (a negative probe that proves the request arrived).
- Telemetry crate test: the OTLP adapter against a local collector double receives one request per batch with the expected metric names.
- Runbook step (after the owner stores the token): `wf vm logs`-style read-back from Better Stack shows a `container.health` point for `agent-shell-gh-runner` within 30 s of deploy; stopping tailscale on purpose shows the runner going unhealthy and the alert firing (the one drill, once).

## Not in this plan

Replacing ORDS, a metrics sidecar (Vector) for containers that run no code of ours, dashboards. The generic per-host log shipper stays a separate plan.
