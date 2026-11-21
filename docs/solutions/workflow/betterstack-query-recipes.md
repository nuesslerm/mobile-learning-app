---
title: BetterStack ClickHouse query recipes (the remote() trap)
date: 2026-07-13
category: workflow
tags: [betterstack, clickhouse, logs, querying, remote-table-function]
problem_type: knowledge
module: docs
---

## Context

App logs forward to BetterStack (Telemetry). To read them back, query the ClickHouse connect host — but the only correct query shape goes through the `remote(<collection>_logs)` table function. A direct `FROM <table>` always returns `UNKNOWN_TABLE` even with valid creds. That error does **not** mean logs are missing. Full text: [`docs/archive/README.md`](../../archive/README.md).

## Guidance

**Two gotchas**:
1. **Always `remote(<collection>_logs)`** — never `FROM <table>` directly.
2. **HTTP basic auth** (`-u user:password`). Headers connect but hit same `remote()` requirement.

```bash
# ❌ WRONG — always UNKNOWN_TABLE even with correct creds

> **TWO SOURCES.** BetterStack holds a **Backend & Infra (OTLP)** source (every
> `backend/*` service + the `wf` CLI) and a **Mobile Learning App** source (the
> app's REST forwarder). Each has its OWN credential quadruple — `CH_BACKEND_*`
> and `CH_MOBILE_*` — and querying the wrong one returns zero rows, which looks
> exactly like "nothing was logged". Backend is the one that normally has data
> and is what `wf vm logs` defaults to; the mobile source only fills after an EAS
> publish. `cluster not found` means either "no such collection" OR "your
> credential cannot see it" — the two are indistinguishable, so check the source
> before guessing names.
>
> Renamed from `BETTERSTACK_CH_*`, which matched nothing: the vault delivered
> `CH_*` while the code read `BETTERSTACK_CH_*`.

curl -H "X-ClickHouse-User: $USER" -H "X-ClickHouse-Key: $PASS" \
  -X POST "https://$CH_BACKEND_HOST" \
  -d "SELECT * FROM $CH_BACKEND_TABLE LIMIT 5"

# ✅ RIGHT — basic auth + remote()
curl -sS -u "$CH_BACKEND_USER:$CH_BACKEND_PASSWORD" \
  -H 'Content-Type: plain/text' \
  -X POST "https://$CH_BACKEND_HOST" \
  -d "SELECT dt, raw FROM remote($CH_BACKEND_TABLE) ORDER BY dt DESC LIMIT 5 FORMAT JSONEachRow"
```

**Env vars** (already in env): `CH_BACKEND_HOST` (connect host, e.g. `eu-fsn-3-connect.betterstackdata.com`), `CH_BACKEND_USER`, `CH_BACKEND_PASSWORD`, `CH_BACKEND_TABLE` (incl. `_logs`, e.g. `t550358_mobile_learning_app_logs`). Ingest host (`s<id>...` ingest subdomain) ≠ query host (`t<team>_<source>`). Don't convert between them — they're different strings for the same source.

## Why this matters

The default `CH_BACKEND_* / CH_MOBILE_*` values work as-is; if you see `UNKNOWN_TABLE`, you forgot `remote(...)`. It's never a creds/team-mismatch problem. Headers vs basic auth both connect — only the `remote()` shape is wrong.

## When to apply

- Querying any device log captured by `@lib/logging`'s `RemoteForwarder`.
- Debugging a published EAS bundle (local `expo start` doesn't forward unless `EXPO_PUBLIC_LOG_BETTERSTACK=true`).
- Building an alert / dashboard query for a feature's logs (`WHERE JSONExtractString(raw,'module') = '<source>'`).

## Examples

**Recent logs (last 30m)**:
```bash
curl -sS -u "$CH_BACKEND_USER:$CH_BACKEND_PASSWORD" \
  -H 'Content-Type: plain/text' -X POST "https://$CH_BACKEND_HOST" \
  -d "SELECT dt,
             JSONExtractString(raw,'level')   AS level,
             JSONExtractString(raw,'module')  AS module,
             JSONExtractString(raw,'message') AS message
      FROM remote($CH_BACKEND_TABLE)
      WHERE dt > now() - INTERVAL 30 MINUTE
      ORDER BY dt DESC LIMIT 200 FORMAT TabSeparated"
```

**Errors only (last 24h)**:
```bash
... -d "SELECT dt, JSONExtractString(raw,'message') AS message
        FROM remote($CH_BACKEND_TABLE)
        WHERE JSONExtractString(raw,'level') = 'error'
          AND dt > now() - INTERVAL 24 HOUR
        ORDER BY dt DESC LIMIT 100 FORMAT TabSeparated"
```

`raw` columns: `dt`, `level`, `message`, `module` (logger source), `app`, `platform`, plus structured `withMetadata` keys spread at top level. Other collections: `remote(<collection>_metrics)`, `remote(<collection>_spans)`. Transient TLS flake (`certificate is not yet valid`): retry — clock skew.
## Reading the CI gauges (the `vm_health_metrics` source)

The `CH_BACKEND_*` pair cannot see this source (`CLUSTER_DOESNT_EXIST`). Mint a short-lived read connection with `BETTERSTACK_API_TOKEN` (`wf secret get`): `POST https://telemetry.betterstack.com/api/v1/connections` with `{"client_type":"clickhouse","note":"agent: read the CI gauges (expires in 2 h)","data_region":"us-west-2a","team_names":["Your team"],"valid_until":"<now+2h>"}`; the answer carries `host`, `username` and `password` once, so keep them in a scratch file, never in a log. Then `SELECT name, count() FROM remote(t550358_vm_health_metrics_metrics) WHERE _row_type='metric' AND name LIKE 'ci.%' GROUP BY name`; labels are the columns `_tags_<label>` and `tags`, the sample is in `value_avg` (binary) beside `name`.

### The parallelism gauges

Labels are `scope` and `name`. Per workflow and phase, the account's runner slots: `ci.runners.busy`, `ci.runners.queued`, `ci.runners.headroom` (scope = workflow, `all` for the account; name = `start` or `end`; the cap is 20). Per job: `ci.job.cpu_avg`, `cpu_p95`, `cpu_max`, `iowait_avg`, `mem_peak_mb`, `swap_peak_mb`, `psi_cpu_max`, `psi_mem_max`, `psi_io_max`, `idle_s`, `saturated_s` (scope = job, name = workflow). Per run: `ci.run.runner_s`, `peak_jobs`, `avg_jobs`, `queue_s_total`, `queue_s_max`, `critical_path_s`, `parallel_efficiency` (scope = run id, name = workflow). Account headroom by label: `SELECT _tags_scope, min(value_avg) AS lowest, avg(value_avg) AS mean FROM remote(t550358_vm_health_metrics_metrics) WHERE _row_type='metric' AND name='ci.runners.headroom' GROUP BY _tags_scope`. The tables behind them and the SQL recipes: [parallelism-telemetry](../ci/parallelism-telemetry.md).
