---
status: open
area: rca-parity
paths:
  - "libraries/agent-api/exec-preview/**"
  - "backend/remote-chat-agent/group-preview/**"
  - "docs/plans/rca-parity/coverage/t3-decisions.json"
opened: 2026-11-05
last_verified: 2026-11-05
links:
  - "docs/plans/rca-parity/plan.md"
---
# `device_*` and `browser_*` executor actions are not built, and the preview tabs have no browser behind them

## What is broken

Plan section 2's executor table lists `preview_start/stop/list`, `device_*` and `browser_*`. Only the first three exist (`exec-preview`). There is
no executor action that opens a device (an Android emulator, an iOS simulator) or drives a browser, so T3's `device.*` rows and
`subscribeDeviceState` are marked not-offered (a proposed default for decision 1), and `preview.open` with the `server` runtime (a headless
Chromium on the server) is refused by name. The preview tabs the service keeps (`preview.sessions`) are a registry the phone draws: the service
renders no page, and nothing in the chain proves that a page loads, only that the tab, its viewport, zoom and the navigation the phone reports
are kept and told to every follower.

## Why blocked

A device or browser action needs a real emulator or Chromium at the executor, which the pipeline runs only in CI (the Android job) and which a
chain test cannot start: the device-driving verbs are forbidden by hand, and there is no recorded device or browser session to replay. Owner
decision 1 (not-offered sets) has not confirmed that device and preview are dropped for T3.

## What unblocks it

The owner confirming the not-offered rows (`docs/plans/rca-parity/coverage/t3-not-offered.md`), or a device-lab runbook that records a real
emulator session so an `exec-device` family can be written from it and replayed.
