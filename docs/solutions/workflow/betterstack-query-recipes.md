---
title: BetterStack ClickHouse query recipes (the remote() trap)
date: 2026-07-13
category: workflow
tags: [betterstack, clickhouse, logs, querying, remote-table-function]
problem_type: knowledge
module: docs
---

## Context

App logs forward to BetterStack (Telemetry). To read them back, query the ClickHouse connect host — but the only correct query shape goes through the `remote(<collection>_logs)` table function. A direct `FROM <table>` always returns `UNKNOWN_TABLE` even with valid creds. That error does **not** mean logs are missing. Full text: [`docs/archive/solutions/tooling-decisions/betterstack-querying.md`](../../archive/solutions/tooling-decisions/betterstack-querying.md).

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