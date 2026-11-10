---
title: cache-health-alerts - the seven Better Stack alert rules on the CI cache gauges
verifies: A refused cache write, a cold cache layer, a stage that replays nothing, a slow pnpm save, task drift and a silent cache each raise an alert, and the CI report job fails on the first four.
tier: runbook
---

# cache-health-alerts

Every cache operation of a CI run leaves one logfmt event (source `ci-cache`) and a few gauges on the Better Stack metrics source
`vm-health-metrics` (`BETTERSTACK_METRICS_HOST` / `_TOKEN`, see `docs/runbooks/health-alerts.md` for the source). The report job
runs `wf ci cache-health`, which reads the same events from the job logs and exits 1 on the problems below, so each rule here
is the second line behind a red report job: it catches the runs the report job never saw (a cancelled run, a drift audit, the
local pre-push gate) and the silence of a cache that stopped reporting. Cause and cure of each problem: `docs/solutions/ci/cache-layers.md`.

## The gauges

All gauges have two labels, `scope` and `name`; a metric with one label carries `all` in `name`.

| Metric (scope, name) | Value |
| --- | --- |
| `ci.cache.requests` (kind, result) | requests of one run by result: `hit`, `miss`, `put`, `skipped`, `dropped-put`, `quota-exceeded`, `not-asked` (kind `nx`, `sccache`, `sccache-compiler`, `pnpm`, `metro`) |
| `ci.cache.degraded` (kind, reason) | 1 when the kind went cold: `unreachable`, `credential`, `budget` |
| `ci.cache.seeded` (kind, `checked` or `present`) | writes of the run looked up again after the run, and how many the store holds |
| `ci.cache.step_seconds` (kind, verb) | seconds of a step; pnpm is `save` and `restore` |
| `ci.nx.tasks` (stage, `ran` or `hit`) and `ci.nx.replay_ratio` (stage, all) | Nx tasks of a stage and the share replayed |
| `ci.gate.seeded` (stage, all) | 1 when the pre-push gate's stage seeded the shared cache, 0 when it did not |
| `ci.drift.tasks_drifted` (all, all) | tasks the drift audit found hashed differently |
| `ci.vmstate.ok` (app, verb) and `ci.vmstate.age_seconds` (app, all) | 1 when the VM state backup (`backup`) or its verification (`verify`) of the app passed, 0 when it failed; the age of the app's latest backup (`wf vm state`, nightly) |

## Alert rules

Create each in Better Stack -> Telemetry -> Alerts on the source `vm-health-metrics`. Notify email and push, the same policy as
the rules of `health-alerts.md`.

### 1. Refused cache write

- Metric: `ci.cache.requests`, filter `name` is `quota-exceeded` or `dropped-put`
- Group by: `scope` (the kind)
- Aggregate: sum per 5 minutes
- Fire when: value > 0
- Message: `{{scope}} cache writes were refused or dropped: the tenancy's Object Storage is full or the store said no. Run wf cache guard`

### 2. Cache layer degraded

- Metric: `ci.cache.degraded`
- Group by: `scope`, `name`
- Aggregate: max per 5 minutes
- Fire when: value >= 1
- Message: `{{scope}} went cold ({{name}}): the store was unreachable, its link expired or the budget ran out`

### 3. Nx replays almost nothing

- Metric: `ci.nx.replay_ratio`, filter `scope` is `flow-tests`, `integration-tests` or `typecheck-lint-unit-tests`
- Group by: `scope` (the stage)
- Aggregate: min per 1 hour
- Fire when: value < 0.2 on two consecutive hours that had a sample (a stage with no sample is not a breach)
- Message: `{{scope}} replayed under 20 percent of its tasks: the pre-push gate's entries are not the ones CI looks for`

### 4. pnpm save over 60 seconds

- Metric: `ci.cache.step_seconds`, filter `scope` is `pnpm` and `name` is `save`
- Aggregate: max per 15 minutes
- Fire when: value > 60
- Message: `the pnpm save took {{value}} s: the store is packed for an entry the bucket may refuse (limit 60 s)`

### 5. Task drift

No CI job runs `wf test drift` yet, so only a manual or future run feeds this metric, and the report job judges drift only when a job log carries the event.

- Metric: `ci.drift.tasks_drifted`
- Aggregate: max per 1 day
- Fire when: value > 0
- Message: `{{value}} Nx tasks hash differently on two machines; run wf test drift`

### 6. No cache metric for 24 hours

- Metric: `ci.cache.requests` (any kind)
- Fire when: no data point for 24 hours (the "no data" condition, not a threshold; a push to dev a day makes this a silence, a weekend is not one: mute Saturday and Sunday)
- Message: `no cache metric for 24 h: the shim is disabled, the metrics source is unreachable or CI stopped`

### 7. VM state backup failed or stale

The nightly `vm-state-backup` workflow runs `wf vm state backup` and `verify` and reports through the same path (event `vm.state.backup` / `vm.state.verify`, source `ci-cache`).

- Metric: `ci.vmstate.ok` (alert when value = 0) and `ci.vmstate.age_seconds`
- Group by: `scope` (the app)
- Aggregate: min of `ok` and max of `age_seconds` per 1 hour
- Fire when: `ok` is 0, or `age_seconds` > 90000 (a night missed)
- Message: `the VM state backup of {{scope}} failed or is over 25 h old: wf vm state verify names the cause; a recreate is refused until it verifies`

## Creating them through the API

The Telemetry API (`https://telemetry.betterstack.com/api/v2`, `BETTERSTACK_API_TOKEN`) lists alerts (`GET /alerts`) and can create a
dashboard, a chart with a SQL query on it (`POST /dashboards/{id}/charts`) and an alert on that chart, because an alert there is a
threshold on a chart's series, not a rule on a metric name. These six rules therefore need a dashboard `CI cache health` with one
chart per rule, written against the metrics table of `vm-health-metrics`; the table's column names are Better Stack's and can only be
read off a source that holds the `ci.*` gauges, which the first push after this change creates. `BETTERSTACK_API_TOKEN` is a global
read-write token shared by every team of the account: use it for this and nothing else, and write the dashboard under the team name
`Your team` (`team_name`).

## Verify

1. Run `wf cache save pnpm` with a store while the tenancy is full (or `WF_BLOB_CACHE=off wf cache restore pnpm` for rule 6's silence): the event `cache.save` or `cache.shim.disabled` appears on stderr and the gauge on the source within a minute.
2. Rule 1 fires within 5 minutes of a refused write; resolve it with `wf cache guard`.
3. `wf ci cache-health` on that run exits 1 and names the problem; the report job is red.
4. Record the outcome in `docs/audits/2026-09-02-token-diet-decision-log.md` with the time each alert fired.
