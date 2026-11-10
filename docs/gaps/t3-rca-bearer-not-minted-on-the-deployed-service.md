---
status: open
area: rca-parity
paths:
  - "clis/t3-serve/src/wiring.rs"
  - "clis/secrets/src/data/registry.json"
  - "backend/remote-chat-agent/rs/src/api/routes/bearers.rs"
opened: 2026-11-04
last_verified: 2026-11-04
links:
  - "docs/plans/rca-parity/plan.md"
---
# `wf t3 serve` has no device bearer for the deployed rca yet

## What is broken

`wf t3 serve --rca-url` now opens rca's agent API socket with a device bearer (`T3_RCA_BEARER`), because the service secret opens that socket no
longer. The registry names the key, but the vault holds no value for it (`wf secret doctor` lists it as not provisioned), so a `t3 serve`
started against the deployed rca stops at startup naming the variable until one exists.

## Why blocked

A bearer is minted by the service that will check it, and the deployed rca still runs the build before device bearers: it has no
`/agent/v1/bearers`. The first push of this milestone redeploys it; only then can `wf offload bearers mint --label t3` answer.

## What unblocks it

After the deploy that ships the bearer endpoints: run `wf offload bearers mint --label t3` (over the tailnet, with the service secret), store
the printed token in the vault under `T3_RCA_BEARER`, and start `t3 serve` with it. `wf secret doctor` then no longer lists the key.
