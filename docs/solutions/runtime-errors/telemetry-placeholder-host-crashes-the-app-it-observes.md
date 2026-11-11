---
title: "A documentation placeholder in `.env.defaults` crash-looped three backends — logging must never be able to kill its host process"
date: 2026-08-05
category: runtime-errors
module: frontend/libraries/logging
problem_type: runtime_error
component: backend
severity: high
symptoms:
  - "`docker compose ps` shows `restarting  Restarting (1) N seconds ago` for a backend that provisioned with `exitCode: 0`"
  - "`Error: Configuration: Could not parse user-provided export URL: 'https://<betterstack-otlp-host>.betterstackdata.com/v1/logs'`"
  - "Stack begins at `createNodeRootLogger` → `createBetterstackNodeTransport` → `getOrInitNodeOtelProviders` — i.e. before any application code runs"
  - "The same image is healthy on a host whose `.env` never picked up the placeholder"
---

## What happened

Every backend's committed `.env.defaults` documented the OTLP host by shipping a placeholder value:

```
BETTERSTACK_OTLP_HOST=<betterstack-otlp-host>.betterstackdata.com
```

`resolveNodeOtlpConfig()` guarded only the *absent* case (`if (!token || !host) return null`). A placeholder is not absent — it is a non-empty string, so it read as configured, `OTLPLogExporter` was handed `https://<betterstack-otlp-host>.betterstackdata.com/v1/logs`, and threw at construction. Since the exporter is built inside `createNodeRootLogger`, the throw happened at module init: `leetcode-backend`, `opencode-telegram-bot`, and `remote-chat-agent` all crash-looped before serving anything.

The comment directly above that line already promised the opposite behaviour — "degrades to console-only logging (never fails to boot)". The code did not implement it.

## The two lessons

**1. A placeholder is worse than an absent value.** `if (!host)` catches the unset case and nothing else. Any non-empty sentinel — `<fill-me>`, `TODO`, `changeme` — defeats the guard while being exactly as unconfigured. So a committed `.env.defaults` documents a value in a **comment**, never as a live assignment:

```
# Uncomment with the REAL host once the source exists:
# BETTERSTACK_OTLP_HOST=<your-source>.betterstackdata.com
```

**2. Observability must be strictly subordinate to the thing it observes.** A telemetry misconfiguration is a degradation, never a fatal error. Validate, warn, and return null — the caller already falls back to console-only logging.

## The fix

`isUsableOtlpHost()` in `frontend/libraries/logging/src/node/otelProviders.ts` validates with the URL parser rather than by matching `<`/`>`, so *any* unparseable host is rejected — not just this one placeholder's shape:

```ts
function isUsableOtlpHost(host: string): boolean {
  try {
    const url = new URL(`https://${host}`);
    return url.hostname === host.split('/')[0]?.split(':')[0];
  } catch {
    return false;
  }
}
```

The round-trip comparison is what makes it strict: `new URL()` alone accepts a surprising amount (it percent-encodes rather than rejecting), so the check is that the parsed hostname is *unchanged* from the input's host portion.

## Why it took a deploy to find

Nothing local exercises this path. `.env.defaults` is a deploy-time artifact composed by `wf secret sync-env`, so the placeholder only becomes a real environment variable on a VM. Tests now cover it directly (`frontend/libraries/logging/tests/otelProviders.test.ts`), including the exact production string.

## Related

- `docs/archive/README.md` — the same deploy where this surfaced
- `frontend/libraries/logging/src/node/otelProviders.ts` — `resolveNodeOtlpConfig`