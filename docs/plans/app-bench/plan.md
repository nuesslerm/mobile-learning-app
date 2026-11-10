# Reusable benchmark suite for the app, then refactor for build speed, memory, runtime speed (in that order)

## Context

Priorities, fixed by the owner: **1. build speed, 2. memory efficiency, 3. runtime speed.** Ideal outcome: memory cut in half and kept low. Every refactor decision is ranked by those three, in that order, and a change that helps a lower priority at the cost of a higher one is rejected.

The benchmark must be repeatable at any time: a permanent `wf` capability with fixed scenarios, a stored baseline and a comparison, not a one-off script (repo rule: capability lives in a `clis/*` package surfaced through `wf`, never `scripts/`; benchmarks are logged in `docs/audits/`).

State today:
- Lab (`wf browser`, iPhone preset, touch input, device-lab export) renders the app at phone size, but `clis/multi-browser/src` has no memory or performance instrumentation; it only holds per-target CDP sessions (`data/browser/cdp-sessions.ts`).
- `frontend/mla-tamagui/babel.config.js` runs `@tamagui/babel-plugin` with `disableExtraction: true` (a workaround for theme-token elements like `bg="$color2"` vanishing from the static export), `logTimings: true`. Tamagui 2.7.7; 27 files import `tamagui`, 41 use `@lib/tamagui-design-system`.
- Heavy dependencies to suspect: CodeMirror in a WebView, `@shopify/react-native-skia`, `react-native-reanimated`, Tamagui. `app/` has almost no `<Image>` or `FlatList` use, so the bitmap and list pitfalls in the pasted analysis are low risk here.

## What transfers from browser to device (checked against this repo)

- Transfers 1:1 and is therefore gateable in the lab: JS-heap leaks (listeners, intervals, subscriptions), unbounded stores, component retention, bundle size and build time (Metro and Babel are the same for web and native).
- Does not transfer: native view and bitmap memory, split Hermes/ART/ARC GC, native screen stacks, cell recycling. These need native-side proxies (view-tree size from flow tests) and an on-device confirmation pass.
- The report must always state which numbers are lab-only and which are device-confirmed.

## What Tamagui benchmarks, and how we lean on it

Tamagui's own benchmarks (tamagui.dev/docs/intro/benchmarks) measure render time, not memory or build: native list of 28 items, 5-run average on an M2 Air (108 ms vs 106 ms plain RN; about 12% slower without the compiler); web with five scenarios (simple render, variant update, inline style change, fully dynamic style, deep tree mount) against react-native-web, Dripsy, Stitches, Emotion, styled-components. Credited: compiler flattening, tree flattening, CSS extraction; gains are larger on web, and the docs call the plugin optional on native.

Leaning in, by owner priority:
1. **Build speed**: the Tamagui plugin runs on every transformed file even with extraction off; measure its share of cold and warm Metro transform time (`logTimings` already exists). Options to test: restrict the plugin's `components`/`config` scope, `disableExtraction` per platform, drop the plugin on native (docs say little native gain) and keep it for web only, and `outputCSS` (tree-shakes about 6 KB gzipped of runtime CSS generation on web).
2. **Memory**: extraction and flattening cut runtime style objects and wrapper nodes on web; import from `@tamagui/core` where the full `tamagui` set is not needed; trim unused themes and fonts. Report lab-only vs device split.
3. **Runtime speed**: Tamagui's scenario shapes (variant update, inline style, deep tree mount) reused as our render scenarios.
4. Prerequisite: diagnose the theme-token extraction bug (the plugin is pointed at `./tamagui.config.ts`, which only re-exports the config that lives in `@lib/tamagui-design-system`; a likely cause), then decide on and off per platform from numbers, never by default.

## Plan

### A. Build the reusable suite (`wf bench`)

1. **New package `clis/app-bench`**, surfaced as `wf bench <suite>` (`build`, `memory`, `runtime`, `all`), with:
   - fixed scenario definitions as data (`scenarios/*.json`), a stored baseline (`baselines/<suite>.json`, committed), and `wf bench compare` that prints before/after per metric and exits non-zero on a budget breach;
   - machine-readable JSON per run plus a short markdown summary; N runs (default 5, median and spread) like Tamagui's method.
2. **Build suite** (priority 1): cold and warm `expo export --platform web` wall time, Metro transform time per phase, Babel/Tamagui plugin time (parse `logTimings`), bundle and chunk size (raw and gzipped), file counts, and the deploy-relevant numbers the greenfield ledger already tracks. Cold = cleared Metro/Babel caches (note from the repo: Metro's transform cache survives babel config edits, so cold runs clear `node_modules/.cache` and `/tmp/metro-*`).
3. **Memory suite** (priority 2): `wf browser memory` in `clis/multi-browser` (new `src/data/browser/memory.ts`, reusing `cdpSessionFor`): CDP `Performance.getMetrics` (`JSHeapUsedSize`, `Nodes`, `JSEventListeners`, `LayoutObjects`), `HeapProfiler.collectGarbage` before each sample, `--snapshot` heap snapshots. Agent-driven runbooks (`docs/runbooks/device-lab-memory-*.md`, same style as the G1-G3 gates): cold start, each main route open and back, a 30-loop open/close of the same screen (growth must be about zero), a voice-edit turn, a Run Local.
4. **Runtime suite** (priority 3): `Performance.getMetrics` (script/layout/recalc durations), long-task count via `PerformanceObserver`, interaction latency per tap from the lab's touch input log, and Tamagui-shaped render scenarios (variant update, inline style change, deep tree mount) on dedicated lab-only routes.
5. **Native proxies**: native view count per route from the flow-test harness (ceiling asserted), no unbounded `<Image>`, single editor instance; on-device confirmation recipe (`adb shell dumpsys meminfo`, Instruments) documented as a runbook, flagged as needing a desktop.
6. **Repeatability**: one command per suite, deterministic scenarios and fixtures, pinned device preset and throttling in the scenario file, results appended to `docs/audits/<date>-app-bench.md` with the commit sha; add the suites to the device-lab gates ledger so a significant change re-runs them. Unit tests for the metric parsers and comparison logic; an e2e that runs the memory suite on one route; a planted leak (un-cleared interval) must fail the loop scenario, proving the benchmark can see leaks.

### B. Baseline and budgets

7. Run all suites on current `dev`; commit the baseline; set budgets: memory at 50% of baseline steady state per route and zero net loop growth; build time and bundle size never above baseline (ratcheted down as wins land).

### C. Refactor, in priority order, each change re-measured and kept only if the ordering holds

8. **Build speed**: Tamagui plugin scoping and per-platform settings, dependency and import trimming (`@tamagui/core`, unused themes/fonts/icons), Metro config (cache keys, resolver, minification), lazy-loading heavy routes to cut the export graph.
9. **Memory**: fix leaks found by snapshot diffs; bound stores; `freezeOnBlur`/screen freezing and unmounting of blurred routes; CodeMirror/Skia teardown; Tamagui extraction where it helps web; flatten wrapper views.
10. **Runtime speed**: only after 8-9 are banked; targeted by the runtime suite (long tasks, interaction latency), never at the expense of build or memory budgets.

## Critical files

- New: `clis/app-bench/**` (+ `wf` wiring in `clis/workflow-cli`), `clis/multi-browser/src/data/browser/memory.ts`, `docs/runbooks/device-lab-memory-*.md`, `docs/audits/<date>-app-bench.md`, scenario and baseline JSON.
- Existing to reuse: `cdp-sessions.ts`, `frontend/mla-tamagui/lib/device-lab` (`__deviceLab` for in-page counters), `.agents/skills/browser-cli/SKILL.md` (document the commands), `babel.config.js`, `metro.config.js`, `app/_layout.tsx`, `app/(app)/(content)/_layout.tsx`, `features/leetcode/ui` (CodeMirror), `features/tamil-writing/ui/TracingCanvas.tsx` (Skia), `docs/plans/greenfield/progress.md` (deploy-speed ledger format), `docs/audits/2026-09-02-token-diet-decision-log.md`.

## Verification

- `wf bench all` twice in a row on an idle machine agrees within a few percent (reports spread); `wf bench compare` exits 0 against its own baseline and non-zero against a deliberately worse build.
- Planted-leak check fails the loop scenario; removing it passes.
- Per refactor: before/after table in the audit, with the priority order (build, memory, runtime) shown, plus lab-only vs device-confirmed labels.

## Decision needed

"Half the memory": measured as lab JS heap and DOM size (available now, gateable) with device RSS as a separate confirmation pass. Recommendation: lab heap for the gate, device RSS confirmation per release.

## Notes from the first probe (2026-10-06)

- `pnpm run export:web` from `frontend/mla-tamagui` first runs the workspace postinstall (verify-deps) and rebuilds libraries, so a naive timing measures that, not the export. The build suite must call `expo export --platform web --output-dir <tmp>` with `--config.verify-deps-before-run=false` on pnpm, and set the same `EXPO_PUBLIC_*` env as the `export:web` script.
- `export:web` sets `EXPO_PUBLIC_DEVICE_LAB=1` by default; the benchmark states which flag value it measures and records it in the result.
- Baselines store both `initial` (written once, never overwritten, the "half the memory" reference) and `current` (ratcheted).
