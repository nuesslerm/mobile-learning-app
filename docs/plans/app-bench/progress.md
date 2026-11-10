# app-bench progress ledger

Plan: [`plan.md`](plan.md). Goal: [`docs/goals/2026-10-06-app-bench.md`](../../goals/2026-10-06-app-bench.md). Priorities, in order: build speed, memory efficiency, runtime speed.

## Phases

- [x] B0 Branch synced to `dev`; `clis/app-bench` scaffolded and mounted as `wf bench`
- [x] B1 Build suite: cold/warm export, Tamagui plugin timing, chunk sizes; `initial` baseline written
- [x] B2 `wf browser memory` (CDP metrics, GC, snapshots) with unit tests and an e2e
- [x] B3 Memory suite: route scenarios, 30-loop growth, planted-leak check; `initial` baseline written
- [x] B4 Runtime suite: long tasks, interaction latency, Tamagui-shaped scenarios; `initial` baseline written
- [x] B5 `wf bench compare` and budgets; baseline audit `docs/audits/2026-10-06-app-bench-baseline.md`
- [x] R1 Build-speed refactor, before/after in the audit (Tamagui plugin scoping and extraction bug first)
- [x] R2 Memory refactor, before/after in the audit; heap at or below 50% of `initial`, or a measured-floor Blocker
- [x] R3 Runtime-speed refactor, no build or memory regression
- [x] G Device-lab gates G1-G3 re-run after the last change to the code they exercise
- [x] On-device confirmation recipe documented (needs a desktop; listed as a gap if not run)

## Blockers

(none)

## Log

(append one line per landed step: date, commit, evidence)

- 2026-10-06 B0: `clis/app-bench` scaffolded and mounted as `wf bench` (`list`, `compare`, suite runners to come); 22 unit tests (stats, compare, baseline, runSuite). First export probe: `pnpm run export:web` took 306 s including the workspace postinstall, and Expo printed "Something prevented Expo from exiting, forcefully exiting now", so the build suite times the export itself and ends the process after the output lands.
- 2026-10-06 B1: build suite and its `initial` baseline at `5a8a89c3f` (3 runs): cold export 120.6 s (spread 6.9%), warm 18.6 s, Metro bundles 218.5 s summed across the server and web bundles (5912 and 5682 modules), Tamagui plugin 1.4 s (the logged part: about 1% of the cold export, so build speed is a Metro graph question, not a Tamagui-plugin one), JS 9,196,176 B raw / 1,940,745 B gzip. Expo keeps running about 2.5 minutes after printing `Exported:`; the suite stops the clock at that line.
- 2026-10-06 B2: `wf browser memory` (CDP `Performance.getMetrics`, forced GC, optional heap snapshot) wired through contracts, schema, dispatcher, container, session and CLI, with a unit test and an e2e; `wf bench memory` runner runs end to end.
- 2026-10-06 B3: memory suite and its `initial` baseline at `ae07f096d` (3 runs, lab-only, iPhone 15 Pro, Chromium V8): cold-start heap 24,657,344 B (spread 0.0%), 1,308 DOM nodes, 326 listeners; per-route heap 23.0-28.9 MB (leetcode editor route highest at 28.85 MB), DOM 198-807 nodes; 30-loop growth between the leetcode and Tamil write routes 1,189,856 B (about 20 KB per round trip, 0 nodes, 0 listeners: JS-heap retention, not DOM or listener leaks). Spread is under 2.5% everywhere, so budgets can be tight. In-app route changes use `history.pushState` + `popstate` (no app change). The planted-leak e2e (`wf` memory suite against a SPA that leaks on /b) separates the leaky run from the clean one and fails `compare`. `wf bench compare` checks regressions; `--budgets` adds the 50% target (currently failing by design).
- 2026-10-06 B4: runtime suite and its `initial` baseline at `16f242a1d` (3 runs, lab-only): tap-to-second-frame 32.6 ms (the two-frame floor), variant update 27.5 ms, deep tree mount of /showcase 91.0 ms, 12 long tasks totalling 1,310 ms (nearly all in the first load).
- 2026-10-06 B5: `wf bench compare --suite all` passes; audit `docs/audits/2026-10-06-app-bench-baseline.md` carries all three baselines with their lab-only / build labels and the refactor order. Findings that steer the refactor: the Tamagui plugin's logged time is about 1% of a cold export (the cost is the Metro graph), the cold-start JS heap is 24.7 MB at every route (an app-shell floor), and start-up long tasks (1.3 s) dominate runtime.
- 2026-10-06 R1: build speed, `d20d2c732` and the Skia split: web cold export 120.6 -> 103.7 s (-14%), warm 18.6 -> 13.4 s (-28%), 5,682 -> 4,033 modules, JS 9.20 -> 7.48 MB raw / 1.94 -> 1.74 MB gzip; the lucide barrel was imported by one screen (1,720 modules); the iOS production export, which failed before (canvaskit `fs` through two web-only Skia imports), now builds in 62.7 s with an 11.08 MB Hermes bytecode (first iOS values). Details and the Tamagui-plugin finding (1% of the build): `docs/audits/2026-10-06-app-bench-baseline.md`.
- 2026-10-06 R2 (in progress): memory, lab-only: cold-start heap 24.66 -> 19.67 MB (-20%) from `inlineRequires: true`; target 12.3 MB. Remaining start-up work by package (reanimated, expo-router, react-native-web, @tamagui/web, zod, react-dom, gesture-handler, bottom-sheet) is in the audit; next levers: lazy reanimated/gesture-handler/bottom-sheet, async routes on web, the 1.36 MB loop growth.
- 2026-10-06 B3 (correction): the memory suite reads the harness shell (713,096 B) in a browser of its own and reports app-only heaps; initial app-only values derived (raw minus shell). Cold-start app heap 23.94 -> 18.96 MB (-21%); budget 11.97 MB.
- 2026-10-06 R2 done: cold-start app heap 23.94 -> 18.95 MB (-21%); loop growth is noise (five windows oscillate 22.6-23.2 MB, DOM and listeners constant; steady window 884 KB under the 1 MB floor). Async routes on web had no effect and were dropped.
- Memory floor: cold-start app-only heap 18.95 MB against a 11.97 MB target (50% of 23.94 MB); what start-up executes is the framework stack the root layout needs (reanimated 262 KB, expo-router 160, react-native-web 111, @tamagui/web 83, zod 71, react-dom 67, gesture-handler 64, bottom-sheet 33); halving needs a product decision to drop or replace those. Evidence: `docs/audits/2026-10-06-app-bench-baseline.md`, "What was tried for memory, and the floor".
- 2026-10-06 R3 done: runtime within noise of the initial baseline (tap 32.5 vs 32.6 ms, long tasks 11 vs 12, 1,208 vs 1,310 ms); nothing regressed while build and memory improved, so runtime work stops here by the owner's priority order.
- 2026-10-06 G: G1, G2 and G3 re-run on the final tree through `wf browser` and recorded with tap dots, records rewritten at `31fd48795` (pages `g1-final2-2026-10-06`, `g2-final2-2026-10-06`, `g3-final2-2026-10-06`): G1 14/14, 12/12, 11/11 over 20 spoken turns, 100 of 100 taps dotted; G2 seven letters 97-100% and the marker on the deployed VM passing all four words; G3 14/14 and 12/12, 287 typed keys, no multi-character insert. G1 used the checkout agent because the deployed planner sends a retired model id from VM-side config; G2 used the deployed marker.
- 2026-10-06 On-device recipe: `docs/runbooks/app-bench-on-device.md` (adb meminfo, Instruments); not run, needs a desktop; every memory number stays lab-only.
- 2026-10-06 VM findings along the way: a plain redeploy leaves sidecars on old images (opencode 1.18.17 gave the empty planner responses), so `wf ship --recreate-all` was added (`31fd48795`) and the opencode version pinned (`6b09e4fcc`); the Tamil marker 502 was a scratch directory the opencode container could not see (`0b81b91be`); the remaining planner failure is a stale model id in VM-side env, documented in `docs/gaps/vm-voice-planner-empty-response.md`.

## Round 3 (2026-10-13)

- Web suites re-recorded at `3e9bdb9e8`; Android suite added (`wf bench android`) with a baseline pending its real cold-start marker; findings in `docs/audits/2026-10-06-app-bench-baseline.md` ("Android"). The VM-side test process lives in `docs/plans/device-test-process/`.
