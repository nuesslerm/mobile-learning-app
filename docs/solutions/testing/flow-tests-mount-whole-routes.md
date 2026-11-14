---
title: Flow tests mount whole routes — createFlowApp + useDependencyOverrides replace hand-injected screens
date: 2026-09-26
category: testing
tags: [testing, flow-test, expo-router, react-native-web, dependency-overrides, createFlowApp, app-di, colocated-routes]
problem_type: knowledge
module: frontend/libraries/flow-test-harness
---

## Context

The trunk-clean-arch review found `/leetcode` crashing on mount in flow tests: tests rendered screens with hand-injected ports (`renderScreen(<ProblemWorkspace ports=… />)`) and never ran the real composition root that lives in the route's layout. The layout chain (`app/leetcode/_layout.tsx`) mounts `ExpoAudioRecorderProvider` above `LeetCodeLiveContainer`, which reads the audio port from that provider — but `LeetCodeLiveContainer` is created in the layout's own render, *above* the provider it consumes. A component cannot read a context provided by its descendant, so `useContext(AudioRecordingContext)` is null and `LeetCodeContainer` throws `ExpoAudioRecorderProvider missing` on mount. Flow tests that injected ports directly masked the provider-order bug instead of catching it, so the fix is not one provider and one container — it is a generic way to mount the **real route tree** of a feature and swap individual dependencies at a shared runtime seam.

`createFlowApp` (`@lib/flow-test-harness`) mounts the real layout chain plus screen for a URL path; `DependencyOverridesProvider` / `useDependencyOverrides` (`@lib/app-di`) is the seam where test doubles drop in. Every screen flow test renders through `app.render(path, { overrides })`; `renderHook` gets the same wrapper.

## Solution

A package's flow test exports its app once, from a glob of its colocated routes:

```ts
// frontend/mla-tamagui/features/coding-agent/src/__tests__/flowApp.ts
export const app = createFlowApp({
  routes: import.meta.glob('../routes/**/*.tsx'),
  routesRoot: '../routes/',
  basePath: '/coding-agent',
});
```

Resolution strips `routesRoot` and `.tsx`, drops `(group)` segments for matching but keeps their `_layout.tsx` in the chain, matches `index` to the directory path and `[name]` to one segment (static wins over dynamic), merges `search` into params, and throws `createFlowApp: no route for <path>` for unknown paths. Rendering goes through `renderRoute` with `wrapper` = `DependencyOverridesProvider`, so the composition root reads overrides where they win over live adapters; without a provider, `useDependencyOverrides` returns `{}` and production is unchanged.

One example — leetcode's audio port:

```tsx
const { result } = await app.render('/leetcode/42', {
  overrides: {
    leetcode: { audio: createFakeAudioRecorder() },
  },
});
```

The layout chain still boots the real `ExpoAudioRecorderProvider > LeetCodeLiveContainer`; only the `audio` port it pulls from `useLeetCodePorts()` is the double. No `vi.mock` of project modules, no `transport="fixture"` prop.

## How a feature opts in

Composition roots read `useDependencyOverrides(key)`; the overrides win over live adapters. `vi.mock` of project modules is replaced by these overrides; `vi.mock` of third-party native modules that crash in jsdom moves into harness stubs.

| key | shape | composition root | fakes (exported for tests) |
|---|---|---|---|
| `leetcode` | `Partial<LeetCodePorts>` | `LeetCodeLiveContainer` (audio hook still always called) | `@lib/flow-test-harness` doubles + a fake gateway in the test |
| `coding-agent` | `{ gateway: Gateway }` | read in three places: `integration/useGateway.ts` (transport param and prop removed; default = ws gateway), `ui/ThreadListScreen.tsx:102` and `ui/DispatchRunScreen.tsx:56` | `@feature/coding-agent/testing` → `createFixtureGateway` |
| `service-report` | `{ fetchHealthReportHistory }` | `state/useHealthReportHistory.ts` | `@feature/service-report/testing` → fixture from `ui/mockReports.ts` |
| `dashboards` | `{ reportingGateway: ReportingGateway }` | `AgentsDashboardScreen` / `InfraDashboardScreen` effect | `createFixtureReportingGateway` from `features/dashboards/data/reportingGateway.ts` |

settings and tamil-writing have no injected dependencies: their tests use `createFlowApp` without overrides.

## Colocated routes

Route modules live with their feature, mirroring the URL below the feature's prefix: `features/coding-agent/src/routes/index.tsx`, `[threadId].tsx`, `runs/[runId].tsx`, `[threadId]/settings.tsx`, with `package.json` `exports` gaining `"./routes/*"`. Local features use `features/<f>/routes/**`; dashboards use `features/dashboards/routes/**`. Each `app/**` file of a feature becomes one line re-exporting that module (e.g. `export { default } from '@feature/coding-agent/routes/[threadId]'`), so Expo Router still resolves it while the feature owns its slice. App-shell files stay in `app/`.

## Prevention

- A screen flow test renders through `app.render(path, { overrides })` — never bare `renderScreen(<XScreen/>)` or a `transport="fixture"` prop, so the real layout chain (providers, containers) is always exercised.
- A new dependency of a feature goes in as an override key in the table above, read at the composition root; the fake ships from the feature's `testing` entry (leetcode's audio and voice fakes still live in `@lib/flow-test-harness`).
- Hooks are called unconditionally; no `container.useX()`.
- App route files under a feature prefix are one-line re-exports of that feature's route module. No lint checks this since the thin-route rule went; the target check and the composition-root rules are in `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.