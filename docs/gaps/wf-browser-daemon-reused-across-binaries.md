---
status: open
area: wf-browser
paths:
  - "clis/harness-browser/src/browser.rs"
  - "clis/harness-browser/src/browser_daemon.rs"
opened: 2026-10-12
last_verified: 2026-10-20
links: []
---
# `wf browser start` reuses a healthy daemon even when it was started by an older wf binary

## What is broken

A daemon outlives its run and a newer `wf` attaches to it, so a rebuilt binary silently runs the old daemon code (a trace fix looked ineffective for an hour until the stale pid was found).

## Why blocked

Not blocked.

## What unblocks it

Have the daemon report its build id on `/health` and have `start` restart it on a mismatch.

## Re-checked 2026-10-20

Reproduced live: I started a daemon from a copy of the wf binary, then ran `wf browser start` with the installed binary. It attached to the old daemon, whose /proc exe was still the copy. /health still returns only {"ok":true} (browser_daemon.rs:33), and both `start` (browser.rs:1089-1094) and every call through ensure_daemon (browser.rs:212-221) reuse any healthy daemon. Since this gap was opened, an idle daemon stops itself after 15 minutes (browser_daemon.rs:982-990, 70dbacd14). That narrows the window but does not close it. A fix cannot key on WF_BUILD_SHA alone, because local builds report it as "unknown" (`wf build-sha`, routes.rs:13-16); the id must change on a local rebuild (exe mtime, size or hash).
