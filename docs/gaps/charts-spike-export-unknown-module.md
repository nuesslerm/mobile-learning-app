---
status: open
area: mla-tamagui
paths:
  - "frontend/mla-tamagui/app/(app)/(content)/charts-spike.tsx"
  - "frontend/mla-tamagui/lib/skiaWeb.ts"
opened: 2026-10-21
last_verified: 2026-10-21
links: []
---
# The charts-spike probe crashes on load in the static web export

## What is broken

In a static web export, a cold load of `/charts-spike` logs a fatal `app.crash` with the message `Requiring unknown module "3107".` It arrives through React Native's `reportFatalError`. The route is the throwaway Skia probe; its chart component is loaded on demand through `WithSkiaWeb`'s `import()`, and the export does not register that module before the page requires it.

Found on 21 Oct, in an export built with `EXPO_PUBLIC_LEETCODE_FIXTURES=1` and cold-loaded with `wf browser test`, once the server-render error that had hidden it was fixed (Tamagui loaded twice in the static render, `metro.config.js`). The dev Metro does not show it. CI's browser e2e has run on the export since 22 Oct, but no e2e test opens this route, so it is still unseen there.

## What fixes it

Either delete the probe, since its question (whether victory-native renders on the web export) has been answered, or make its deferred import resolve in the export. If the probe stays, a cold-load test of the exported route should prove it renders.

## Re-checked 2026-10-24

`components/charts-spike/ChartSpike.tsx`, the route and `lib/skiaWeb.ts` are still in the tree and `frontend/mla-tamagui/e2e/verify-charts-spike.mjs` still exists, so the probe and its cold-load crash stand. Not fixed; deleting the probe is the cheaper fix (its question is answered).
