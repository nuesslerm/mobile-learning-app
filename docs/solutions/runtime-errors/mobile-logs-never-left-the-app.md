---
module: mobile-learning-app
date: 2026-08-13
problem_type: runtime_error
component: tooling
severity: high
symptoms:
  - "The mobile BetterStack collection has never received a single row"
  - "No app logs appear in the browser console either, despite the console transport being enabled in dev"
  - "No IndexedDB database is created, so persistence never ran"
  - "Zero network requests to the ingest host — not a failed request, no request at all"
root_cause: incomplete_setup
resolution_type: code_fix
related_components:
  - logging
tags:
  - betterstack
  - logging
  - expo
  - side-effect-import
  - silent-failure
  - non-enumerable
---

# The mobile app's logs never left the app

## Problem

The mobile BetterStack source had never received data. Two stacked defects each produced total silence — fixing the first only revealed the second, and neither raised an error anywhere.

## Symptoms

Instrumented in a real browser session against a running dev server:

```
APP_LOGS_COUNT    0      # nothing in the console either, though console is on in dev
BS_ATTEMPTS       0      # not a failed request — no request was ever made
IDB               []     # no database created, so persistence never ran
```

`0` attempts is the load-bearing observation. A blocked or failing request would still appear as a `requestfailed` event; nothing at all means the code that sends never ran.

## What Didn't Work

- **Blaming the sandbox's TLS restriction.** Headless Chromium here genuinely cannot TLS-handshake an external HTTPS host, so that was the natural suspect. It was not the cause: `curl` to the same ingest host returned `202`, and the browser never attempted a request to fail in the first place.
- **Clearing the Metro cache.** `EXPO_PUBLIC_*` values are inlined at transform time, so a warm cache holding pre-restart values was a plausible theory. Restarting with `--clear` changed nothing.
- **Checking the env at runtime.** `process.env.EXPO_PUBLIC_BETTERSTACK_TOKEN` and `_HOST` both read as present in the page, which looked like proof the configuration was fine. It was — the configuration was never the problem.
- **Reading `localStorage` for the outbox.** Empty, which seemed to confirm nothing was queued. Misleading: web persistence is IndexedDB, not `localStorage`, so this looked in the wrong place.
- **A `console.warn` inside the forwarder's `send`.** It never appeared, which read as "send is not called". Also misleading — console capture patches `console.*` to route into the in-app live tail, so the diagnostic was swallowed by the very system under investigation. Re-running it against the *unpatched* console was what made the next step trustworthy.

## Solution

**1. Nothing imported the module that configures the logger.** The app module that builds the root LogLayer — console transport, persistence, and the BetterStack forwarder — does so as a **side effect of being imported**, and no module imported it. Every `Logger.*` call in the app resolved against an unconfigured root and became a no-op. A comment in the root layout asserted this happened automatically, describing an import that did not exist.

```tsx
// app/_layout.tsx — must be first, and must be a side-effect import
import '@/lib/logging/logger';
```

After this, console output and IndexedDB persistence both started working, and the remote stage began receiving batches (19 entries on first load) — which exposed the second defect.

**2. The forwarder's `serialize` threw on every entry.** The remote pipeline stage is attached through a cast, and the two entry types disagreed about whether the timestamp was optional:

- the pipeline's entry type had `ts?: number`
- the forwarder's entry type has `ts: number` (required)

`new Date(undefined).toISOString()` throws `RangeError: Invalid time value`. The throw propagated out of the forwarder's `process()` and was caught by the pipeline's own "logging must never throw" guard, so **nothing was ever queued and no error was ever reported**. Fixed at the source by always stamping a timestamp, with a defensive guard at the serialize site:

```ts
// pipeline entry — ts is now required and always populated
ts: typeof ts === 'number' ? ts : Date.now(),

// forwarder serialize — belt and braces
dt: new Date(entry.ts ?? Date.now()).toISOString(),
```

**3. Verified with real traffic, not a probe.** An e2e suite navigates and interacts with the app and asserts on the lines the app itself chose to emit — 26 lines accepted by the ingest host in one batch, then queryable (`layout_module_loaded`, `RootLayout_called`, `leetcode.catalog.getAll`, `agent.message.error`, `sheet.root.render`).

Because headless Chromium cannot reach the ingest host directly, the suite routes that one hop through `context.route()`, the same CDP-layer interception the repo's browser CLI exposes as its network proxy: Playwright's Node process performs the fetch, the payload is forwarded byte-for-byte, and the log content remains entirely the app's own.

## Why This Works

Both defects sat on the same chain — configure → emit → stage → serialize → send — and the chain reports nothing when any link is missing. The first defect meant the chain was never built; the second meant it was built but severed at `serialize`, inside a `catch` whose explicit purpose is to never surface an error.

The timestamp fix belongs at the source rather than only at the serialize site because the cast is what allowed the mismatch to compile. Making the field required and always populated removes the class of bug; the `?? Date.now()` guard only limits the damage if a future caller reintroduces it.

## Prevention

**A side-effect import is invisible to every reader and every type-checker.** Nothing in the type system, no lint rule, and no test noticed that the module which configures logging was never imported. If a module's whole contract is a side effect, the import belongs somewhere structurally guaranteed — a root entry point — and the comment claiming it happens must name the file that does it.

**"Logging must never throw" hides bugs in logging.** The guard is correct and should stay, but it means any exception inside a stage, serializer, or transport disappears. Anything running inside that boundary deserves a test that calls it directly with realistic inputs — including the optional-field cases.

**A cast across two entry types is an unchecked contract.** `as Stage<T>` silenced exactly the required-vs-optional mismatch that caused the throw. Where a cast bridges two shapes, the fields it papers over are the ones to pin with a test.

**Distinguish "no request" from "failed request" before theorising.** Zero attempts and a failed attempt have completely different causes; conflating them sent the investigation at the sandbox's TLS restriction, which was real, documented, and irrelevant.

**Do not diagnose a logging system with its own console.** Console capture patches `console.*`, so a `console.warn` inside the pipeline routes into the system under test and vanishes. Use the captured original console for diagnostics in logging infrastructure.

The backend hit the same class of failure from the other direction — a transport-owned serialization detail silently dropping content — see `docs/archive/solutions/integration-issues/logs-that-never-arrive-four-silent-forwarding-failures.md`.