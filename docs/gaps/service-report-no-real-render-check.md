---
status: open
area: mla-tamagui-e2e
paths:
  - "frontend/mla-tamagui/features/service-report/src/ui/ServiceReportScreen.tsx"
  - "frontend/mla-tamagui/e2e/run-all.mjs"
opened: 2026-10-22
last_verified: 2026-10-22
links: []
---
# No real-browser check that the service-report dashboard renders themed

## What is broken

`verify-service-report.mjs` was the only real-browser assertion that the dashboard's three cards and
its charts carry themed, non-transparent backgrounds (Tamagui resolving theme tokens into elements).
The Metro retirement deleted it on 22 Oct, so nothing asserts that on a real render any more.
`ServiceReportScreen.flow.test.tsx` and `tests/flow/serviceReport.flow.test.tsx` still cover the
screen's states and data in jsdom, which cannot read a computed Tamagui style.

## Why blocked

The check needs the dashboard in its data state, and that comes from a live health-report endpoint
(`EXPO_PUBLIC_HEALTH_REPORTS_ORDS_BASE_URL` baked into the export, plus a password). A local web
export has neither, and no check may call a live host from a sandbox. Without them the screen shows its
not-configured admonition, not the cards.

## What unblocks it

Bake a loopback stub URL for the endpoint into the export the check serves and answer it with a fixed
report (the harness already stubs network over CDP: `e2e/stubbed-run-local.lua`, `e2e/stubs/`), then
assert the cards and charts as the deleted check did. Land it as an `e2e/*.lua` browser test with
`focus: true`, or a `verify-*.mjs` check in `run-all.mjs`.

## Re-checked 2026-10-24

No script under `e2e/` or `frontend/mla-tamagui/e2e/` asserts the themed dashboard backgrounds (the only mention is `regression-spine-waivers.json`), so the gap stands.
