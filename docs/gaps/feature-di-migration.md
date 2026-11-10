---
status: open
area: architecture
paths:
  - "frontend/mla-tamagui/features/tamil-writing/**"
  - "frontend/mla-tamagui/features/coding-agent/src/**"
  - "frontend/mla-tamagui/features/service-report/src/**"
  - "frontend/mla-tamagui/components/dashboards/**"
opened: 2026-10-04
last_verified: 2026-10-21
links:
  - solutions/architecture/hexagonal-architecture-by-package-type.md
  - plans/hexagonal-architecture/spec.md
---
# Features other than leetcode are not on ports + DI yet

## What is broken

trunk-clean-arch migrated only `frontend/mla-tamagui/features/leetcode` (contracts/data/di/ui). Still
importing `data/` straight from UI: tamil-writing (`TamilCourseScreen`, `TamilLessonScreen`,
`TamilReviewScreen`, `TamilWriteScreen`), coding-agent-impl (`ConnectionBanner`, `DispatchRunScreen`,
`ThreadSettingsSheet`), service-report-impl (flow test imports `../../data/ordsClient`); settings-impl
has no container (dev-tools dissolved into the app shell).

## Why blocked

Each migration is its own clean-arch pass (ports, adapter lift, provider mount, flow tests on harness
doubles), deliberately sequenced one feature at a time.

## What unblocks it

A plan per feature repeating the leetcode sequence, tamil-writing first; then widen
dependency-cruiser's `ui-never-imports-data` beyond `features/leetcode` (the oxlint rule that carried it went in D4b).

## Re-checked 2026-10-20

Re-checked against dev at 95e494827. tamil-writing still has no contracts/ or di/, and four of its screens import data/ directly (TamilCourseScreen.tsx:10, TamilLessonScreen.tsx:23, TamilReviewScreen.tsx:17, TamilWriteScreen.tsx:9). coding-agent's UI imports data/ in four files. dev-tools still has only a configure/get singleton in src/di/runtime.ts. The no-direct-io rule is still scoped to features/leetcode/ui/** in frontend/.oxlintrc.json:139-142, and its glob would not reach the src/ui folders of the package features.

Out of date above:

- The paths features/coding-agent-impl/src/**, features/service-report-impl/src/**, features/settings-impl/src/** and features/dev-tools-impl/src/** no longer exist: there is no top-level features/ and no -impl packages. They are now frontend/mla-tamagui/features/{coding-agent,service-report,settings,dev-tools}/src/**.
- "service-report-impl (flow test imports ../../data/ordsClient)" is no longer true. The service-report flow tests (src/ui/__tests__/*.flow.test.tsx) import no data module. The remaining references are src/testing/index.ts:6 (type-only) and src/state/useHealthReportHistory.ts:14 (state to data, an allowed direction).
- The coding-agent list misses ThreadListScreen.tsx:16, which imports a type from ../data/orchestrationGateway. coding-agent now has src/contracts and src/integration but still no container.
- "dev-tools has only a global di/runtime.ts": dev-tools dissolved into app-shell (its runtime was never read). settings has no data layer at all (src/routes and src/ui only), so it may not need a container.
- "widen mobile-learn/no-direct-io-in-feature-ui": ESLint is gone, and the rule now runs as an oxlint JS-plugin rule configured in frontend/.oxlintrc.json:139-151. Widening it also needs the rule's glob '**/features/*/ui/**' changed to reach features/<name>/src/ui/**.
- The links solutions/architecture/two-tier-di-containers.md and solutions/architecture/clean-architecture-standard.md are broken. Both files now live under docs/archive/solutions/architecture/.

## Re-checked 2026-10-21

The frontmatter paths and links above are fixed. The target is now type 8, the hexagonal feature, in docs/solutions/architecture/hexagonal-architecture-by-package-type.md: ports in `domain/ports.ts`, one `di/` container that alone reads the overrides, `ui` importing `state/` and never `data/`. That settles the conflict with the shared dependency-cruiser preset, which still allows `ui → data` (`frontend/feature-kit/depcruise.feature.cjs:7`): the preset changes, not the target. settings is a presentation feature (type 9) and needs no container; dev-tools is proposed for dissolving into the app shell; dashboards (`frontend/mla-tamagui/components/dashboards`) joins the list. The per-feature units are Phase 3 of docs/plans/hexagonal-architecture/spec.md. The debt counts are unchanged: tamil-writing's ui imports `data/` in 4 files and coding-agent's in 4.
