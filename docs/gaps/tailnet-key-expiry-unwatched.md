---
status: open
area: ci
paths:
  - ".github/workflows/scheduled-triggers.yml"
  - "docs/audits/2026-11-11-scheduled-tasks-audit.md"
opened: 2026-11-11
last_verified: 2026-11-11
links: []
---
# Nothing watches the tailnet auth key's expiry

## What is broken

The weekly Tailscale key watch (the `[ts-key-watch]` tag and its workflows) was removed on purpose: its OAuth client is a `vm`-scope secret and must never reach public CI. Nothing else checks that the VM's tailnet auth key has more than 14 days left, so an expiring key is found by the VM dropping off the tailnet.

## What closes it

An owner-side or VM-side timer (not a public-CI workflow) that runs `wf vm net auth-key status` weekly and reports a key with 14 days or less left. It needs an owner: only a person holds the `vm`-scope credential. Decision: `docs/audits/2026-11-11-scheduled-tasks-audit.md`, constraint 7.
