# app-bench: the initial baselines (2026-10-06)

Owner priorities, in this order: **1. build speed, 2. memory efficiency, 3. runtime speed.** These are the numbers the refactor is measured against. `wf bench list` shows the suites; `wf bench <suite> --runs 3` re-measures; `wf bench compare --suite all [--budgets]` checks a baseline file. The `initial` block of each baseline is written once and never overwritten; `current` is ratcheted as wins land.

Every number is labelled where it can be trusted: **build** numbers are the real Metro and Babel work (the same for web and native); **lab-only** numbers are Chromium V8 in headless Chromium at iPhone 15 Pro size, not Hermes and not native memory, and rank suspects and catch regressions rather than predict device RSS. Nothing here is device-confirmed yet.

## Build speed (priority 1)

`wf bench build`: `expo export --platform web` from `frontend/mla-tamagui`, lab flag on, three runs, each a cold export (Metro and Babel caches cleared) then a warm one. Commit `5a8a89c3f`, this VM (the workspace's sandbox).

| Metric | Median | Min | Max | Spread | Scope |
|---|---|---|---|---|---|
| Cold export | 120.6 s | 114.0 s | 122.4 s | 6.9% | build |
| Warm export | 18.6 s | 17.7 s | 18.7 s | 5.4% | build |
| Metro bundle time (server + web bundles, summed) | 218.5 s | 208.3 s | 219.4 s | 5.1% | build |
| Tamagui plugin, logged files only | 1.44 s | 1.35 s | 1.52 s | 11.7% | build |
| JS emitted, raw | 9,196,176 B | | | 0.0% | build |
| JS emitted, gzip | 1,940,745 B | | | 0.0% | build |

What this says:
- The Tamagui plugin's visible time is about 1% of a cold export. Build speed is a question about the Metro module graph (5,912 modules in the server bundle, 5,682 in the web bundle), not about the Tamagui plugin. The plugin's non-logged per-file overhead is not captured by `logTimings`; measuring it needs a with/without-plugin pair (a refactor step, below).
- Expo stays alive for about 2.5 minutes after it prints `Exported:` (an open handle); the suite stops its clock at that line. A plain `pnpm run export:web` also includes the workspace postinstall, which made the first probe take 306 s end to end.
- Cold-to-warm is 6.5x, so the Metro cache is working; a cold build is dominated by transform work.

## Memory (priority 2)

`wf bench memory`: the lab export served at its base URL, a phone-size Chromium, JS heap and DOM after a forced GC; in-app route changes use `history.pushState` + `popstate`; three runs, a fresh browser each. Commit `ae07f096d`. **Lab-only.**

| Metric | Median | Spread |
|---|---|---|
| Cold-start heap (home) | 24,657,344 B | 0.0% |
| Cold-start DOM nodes / listeners | 1,308 / 326 | 0.0% |
| Heap, /tamil | 25,502,036 B | 0.0% |
| Heap, /tamil/uyir-1 | 23,112,240 B | 0.0% |
| Heap, /tamil/write/uyir-a | 23,020,600 B | 0.0% |
| Heap, /leetcode/valid-palindrome | 28,853,712 B | 0.0% |
| Heap, /settings | 23,248,428 B | 0.1% |
| Heap, /dashboards | 23,643,208 B | 0.1% |
| 30-loop growth (leetcode <-> Tamil write) | 1,189,856 B | 2.3% |
| 30-loop growth, DOM nodes / listeners | 0 / 0 | |

What this says:
- Spread is at most 2.3%, so budgets can be tight: a 5% change is a real change.
- The floor is high: every route sits at 23-29 MB of JS heap and the first route already holds 24.7 MB, so the cost is mostly the app shell and runtime (React Native Web, Tamagui, expo-router, the design system and everything the root layout pulls in), not any one screen. Halving it (target 12.3 MB cold start) has to come from loading less at start, which is also what build speed rewards.
- Loop growth is JS-heap retention only (about 20 KB per round trip; no DOM or listener growth). That is the first leak to chase with `wf browser memory --snapshot` diffs.
- The editor route is the heaviest (+4-5 MB over the others; its CodeMirror runs in an embedded frame, so its own memory is only partly in this number).

Budgets (`wf bench compare --budgets`): each memory metric must end at or below 50% of its initial median. Currently failing by design.

## Runtime speed (priority 3)

`wf bench runtime`: three runs; per run 20 taps on a toggle (Tamil write), 5 mounts of /showcase, 20 alternating Chip taps on /showcase (a Tamagui variant update across the tree), and the long tasks of the whole run (buffered, so the load counts). Commit `16f242a1d`. **Lab-only**, headless Chromium without CPU throttling.

| Metric | Median | Spread |
|---|---|---|
| Tap to second frame, toggle | 32.6 ms | 0.3% |
| Variant update (chip select) | 27.5 ms | 8.0% |
| Deep tree mount (/showcase) | 91.0 ms | 12.7% |
| Long tasks | 12 | 16.7% |
| Long-task time | 1,310 ms | 16.6% |

What this says: tap latency sits at two frames (about 33 ms), the floor of this probe, so it can only regress. The costly thing is start-up: about 1.3 s of long tasks, nearly all in the first load. That points at the same lever as memory and build: ship and run less at start.

## Device-confirmed

Not run. The recipe is `docs/runbooks/app-bench-on-device.md` (`adb shell dumpsys meminfo`, Instruments); it needs a desktop with a phone or simulator, so it stays a manual runbook until someone runs it. Every memory number in this audit is lab-only.

## Plan for the refactor, in priority order

1. Build speed: measure the Tamagui plugin with and without (and `disableExtraction` per platform), then cut the module graph (dependency and import trimming, lazy routes).
2. Memory: lazy-load routes and heavy libraries so the cold start holds less; chase the 20 KB per round trip.
3. Runtime: only after 1 and 2, guided by the long-task list.

## Refactor results (same day, commit `d20d2c732` plus the iOS fix)

Three runs per suite, `wf bench <suite> --runs 3 --record both`; spread in brackets. Initial values are above; "now" is `current` in the baseline files. Differences between sessions on this VM were about 8% on cold exports (96 s and 104 s for the same web graph), so read build deltas under that as noise.

### Build speed (priority 1)

| Metric | Initial | Now | Change |
|---|---|---|---|
| Web cold export | 120.6 s | 103.7 s [4.5%] | -14% |
| Web warm export | 18.6 s | 13.4 s [5.3%] | -28% |
| Web modules | 5,682 | 4,033 | -29% |
| Web JS raw / gzip | 9.20 MB / 1.94 MB | 7.48 MB / 1.74 MB | -19% / -10% |
| iOS production export | **failed** (Unable to resolve `fs` from canvaskit-wasm) | 62.7 s cold [9.4%], 34.3 s warm, 4,228 modules | now builds |
| iOS Hermes bytecode | none to measure | 11.08 MB (`.hbc`) | first value; recorded as its initial |

What did it:
1. **One screen imported the lucide barrel** (`features/service-report/src/ui/ServiceReportScreen.tsx`), which pulled 1,720 icon modules (30% of the graph) into every bundle even though the design system imports icons one file at a time. It now uses the per-icon exports, and the restricted-import guard (which existed only for the app's own folders) now applies to every feature package, as an error.
2. **The iOS export was broken** by two web-only Skia imports (`@shopify/react-native-skia/lib/module/web` in the Tamil Skia canvas and in `charts-spike`) that Metro resolved on iOS and failed on (`fs`). They now go through `lib/skiaWeb.ts` (a throwing native stub) and `lib/skiaWeb.web.ts` (the real loader); `test:flow:skia` and the default flow tests pass.
3. `inlineRequires: true` (see memory) did not change build time: within noise.

Not the Tamagui plugin: its logged time is 1.1 s of a 104 s cold export.

### Memory (priority 2), lab-only

| Metric | Initial | Now | Change |
|---|---|---|---|
| Cold-start JS heap | 24.66 MB | 19.67 MB | -20% |
| /tamil | 25.50 MB | 17.48 MB | -31% |
| /tamil/uyir-1 | 23.11 MB | 17.22 MB | -25% |
| /tamil/write/uyir-a | 23.02 MB | 17.21 MB | -25% |
| /leetcode/valid-palindrome | 28.85 MB | 24.59 MB | -15% |
| /settings | 23.25 MB | 19.04 MB | -18% |
| /dashboards | 23.64 MB | 19.17 MB | -19% |
| 30-loop growth | 1.19 MB | 1.36 MB | +15% (not fixed) |

What did it: **`inlineRequires: true`** in `metro.config.js`. Expo's default is `false`, so every module in the bundle was evaluated at start-up. The lucide fix on its own moved cold-start heap only 24.66 to 24.06 MB (the 1,720 icon modules were in the graph but never evaluated), which is why build speed and memory had to be measured separately.

Where the remaining heap is (V8 snapshot of the cold start, 34 MB total: 8.3 MB native including script source, 6.9 MB code, 5.9 MB objects, 3.8 MB strings, 2.7 MB arrays, 2.4 MB closures, 1.8 MB shapes) and what start-up still runs (`wf browser coverage`, 6.6 MB script, 1.74 MB executed): react-native-reanimated 262 KB, expo-router 160 KB, react-native-web 111 KB, @tamagui/web 83 KB, zod 71 KB, react-dom 67 KB, gesture-handler 64 KB, bottom-sheet 33 KB. Halving needs most of that not to run at start.

Not yet reached: the target is 12.3 MB cold start; the app is at 19.7 MB. Open levers, in order: (a) make reanimated, gesture-handler and bottom-sheet load on first use, (b) async routes on web (`asyncRoutes: { web: true }`; the Expo docs say it is on by default in SDK 58 and gives Hermes apps little), (c) zod schema construction at import, (d) the 1.36 MB loop growth.

### Runtime speed (priority 3)

Within noise: tap-to-frame 32.5 ms, variant update 29.6 ms (spread 13%), /showcase mount 100.5 ms (spread 20%), long tasks 11 totalling 1,208 ms (-8%, within spread).

### On device

Not run. What carries to a device: the module count (-29%), the Hermes bytecode size (now measurable), and the fact that start-up no longer evaluates every module (`inlineRequires`); what does not: the V8 heap numbers.

### Correction: the harness shell is not the app

Every lab heap reading includes the device-frame page that hosts the app (the harness shell): 713,096 B, measured on an app-less page in its own browser (`harnessShellHeapBytes`). The memory suite now reports each heap with it taken off (`coldStartAppHeapBytes`, `route.<id>.appHeapBytes`) and the 50% budget applies to those. The initial app-only values are the initial raw heaps minus that constant (recorded in the baseline's `env.appOnlyDerived`), because the shell was first measured after the refactor began and is not touched by it.

| Metric | Initial | Now | Budget (50%) |
|---|---|---|---|
| Cold-start app heap | 23.94 MB | 18.96 MB (-21%) | 11.97 MB |

A first attempt that loaded the app-less page in the same browser before the app moved cold-start heap by 1.8 MB (17.87 MB), so the shell is now read in a browser of its own and the app's first navigation stays the one every baseline took.

### Loop growth is noise, not a leak

The 30-loop growth (about 1.2-1.4 MB) looked like a leak. It is not: five windows of 30 round trips between the editor route and the Tamil write route, each read after a forced GC, gave 22.76, 23.17, 22.63, 22.79 and 23.01 MB with DOM nodes (504) and listeners (599) constant. A V8 snapshot diff of one window found 9,258 new objects but no app-level container holding them: V8 code and feedback (about 190 KB of 460 KB), React fiber alternates on live DOM, and 60 DevTools `DOMException`s from the probe itself. The suite now also records a second, steady window (`loopSteadyGrowthBytes`, 884 KB, spread 3%); the planted-leak e2e still separates a leaking route from a clean one. The noise floor of this reading is about 1 MB, so the memory goal's loop clause is "steady growth under 1 MB".

### What was tried for memory, and the floor

| Lever | Cold-start app heap | Verdict |
|---|---|---|
| Remove the lucide barrel (1,720 modules) | -0.6 MB (24.66 to 24.06 raw) | kept; mostly a build win |
| `inlineRequires: true` | -4.4 MB (to 19.67 raw, 18.96 app-only) | kept |
| Lazy `require` of the Skia canvas | -1.1 MB on the Tamil routes | superseded: the plain static import is deferred the same way by `inlineRequires` |
| Async routes on web (`asyncRoutes: { web: true }`, SDK 57) | 0 (same modules, same bytes) | dropped; no effect in the static export |

Measured floor: 18.95 MB app-only at cold start against a 11.97 MB target (50% of 23.94 MB). What start-up still executes (6.6 MB script, 1.28 MB of it runs; first-party code is about 70 KB of that): reanimated 262 KB, expo-router 160 KB, react-native-web 111 KB, @tamagui/web 83 KB, zod 71 KB, react-dom 67 KB, gesture-handler 64 KB, bottom-sheet 33 KB, @tamagui/themes 33 KB. These are the framework stack the root layout needs to draw anything (providers, gestures, drawer, toasts). Cutting the heap in half would mean replacing or not loading several of them, which is a product decision (dropping reanimated-based UI, or swapping the router), not a refactor. Lab-only: on Hermes the bytecode is memory-mapped and these figures do not transfer one to one; what does is that start-up now evaluates only what the first screen uses.

## Device-lab gates after the refactor (2026-10-06)

The refactor touched `frontend/mla-tamagui/features` and `app` (the Skia and icon imports, Metro `inlineRequires`), so G1-G3 were re-run through `wf browser` on the final tree, recorded with tap dots, and their records rewritten at `31fd48795`.

| Gate | Result | Evidence |
| --- | --- | --- |
| G1 voice edit | valid-palindrome 14/14, two-sum 12/12, container-with-most-water 11/11; 20 spoken turns; 100 of 100 taps dotted | `g1-final2-2026-10-06` |
| G2 Tamil writing | seven traced letters 97-100% on the line; live marker on the deployed VM passes அ, அம்மா, ஆடு, இலை (two of them on the second attempt); 17 of 17 taps dotted | `g2-final2-2026-10-06` |
| G3 CodeMirror typing | valid-palindrome 14/14, two-sum 12/12; 287 typed keys, no multi-character insert, inter-key p50 224 ms; 22 of 22 and 12 of 12 taps dotted | `g3-final2-2026-10-06` |

G1 ran against the remote-chat-agent from the checkout, not the deployed one: the deployed planner sent a retired model id from VM-side config (`docs/gaps/vm-voice-planner-empty-response.md`). That was true on 2026-10-06; by 2026-10-13 the deployed planner resolved the right model and answered (see the correction under the 2026-10-13 gates). G2's live marker was the deployed one, after the scratch-directory fix (`0b81b91be`).

Memory, stated plainly against the owner's goal: the lab-only cold-start app heap fell 21% (23.94 to 18.95 MB), not 50%; the remaining heap is the framework stack the root layout needs, and loop growth is within noise.

## Android (2026-10-13)

Lab-only, on a slim redroid 14 (540x960@240, 1.5 CPU, no GPU) on the shared VM; nothing here is device-confirmed.

- Slim device (`wf android up`, catalog in `clis/wf/src/android_features.json`): a fresh stock device idles at 792 MiB, 13% CPU, 85 processes; with the 61 optional packages disabled it idles at 517 MiB, 0.1% CPU, 67 processes (-35% memory). The set provably survives a restart (`wf android features --json`: `restartProven`, `disabledTotal` 61). Core also holds the packages the system refuses to boot without: `com.android.sdksandbox` (package manager throws without exactly one), the launcher, Settings (`FallbackHome`), `android.ext.services`.
- The lab APK (EAS build `b8e1fb0e`, `android-apk-lab`): 67.2 MB; after a cold start the app's PSS was about 198 MB (native heap 77, code 60, Java 13) and RSS 315 MB, against 18 MB of JS heap in the web lab. Most native memory is outside our JS.
- The earlier "cold start 11.8 s" is not a measurement: `am start -W` reported `Status: timeout`, so 11.8 s was its timeout. The Android suite now measures from the clock stamp before `am start` to logcat's first `ReactNativeJS: Running "main"` line (the app's JS is running), and records `initial` and `current` baselines at `03f3f3fc3` (n=3, on the shared slim device under the lease, nothing torn down): cold start 4.97 s median (3.98 to 5.65 s, spread 34%), PSS 216 MB (spread 20%), RSS 328 MB, native heap 80 MB, Java heap 13 MB, code 66 MB, device used RAM 1.44 GB, 0 crashes, APK 67.2 MB. The first run is the lightest in memory (PSS 178 MB, native heap 45 MB); later runs settle near 220 MB, so the tolerances (cold start 40%, PSS 25%, native heap 50%) sit above that noise floor. All lab-only.
- Why the first slim attempts "did not boot": my rollback deleted `package-restrictions.xml` inside a live container, and `system_server` rewrote it while shutting down, so packages disabled by earlier attempts (including `sdksandbox`, the launcher and Settings) stayed disabled after the catalog was fixed. The rollback now removes the container and its volume and boots a clean device.
- Web re-baseline at `3e9bdb9e8` (n=3, the Rust suites): app-only cold-start heap 18.13 MB (initial 23.94, -24%), leetcode route 23.2 MB, tap latency 32.2 ms, startup executed code 1.615 MB (initial 1.752). Reanimated's executed bytes read 342 KB against 262 KB in the earlier TS tool on a bundle whose size is identical within 0.02%, so the difference is the measurement (the tool was ported to Rust), not the code. Build wall times in this baseline were taken while other work shared the machine (cold export 114 s against 91 s in a quiet run) and are re-recorded in a quiet window.

## Web baselines re-recorded in a quiet window (2026-10-13, `6535ce447`)

`wf bench build --record current`, n=3, nothing else running on the sandbox (a first attempt overlapped a repository-wide grep and read 183 s, 106 s, 104 s; it was thrown away). Cold export 116.5 s [2.1%], warm export 18.2 s, web graph 3,539 modules (initial 5,682), raw 6.87 MB (initial 9.20 MB), gzip 1.61 MB (initial 1.94 MB); iOS cold export 69.0 s, warm 36.9 s, 3,731 modules, Hermes 10.26 MB (initial 11.08 MB).

Plainly: the size and module wins hold (-25% raw, -17% gzip, -12% iOS bytecode). The wall-clock wins shrink in this window: cold export is only 3.4% under the initial 120.6 s, and the 103.7 s and 13.4 s quoted above were taken on a faster VM state. The same tree measured 91 to 99 s and 116 s in different sessions, so a cold-export figure from a different session cannot show a change smaller than about 15%; a build-time claim needs an A/B in one session (`wf bench ab`).

Two metrics are reported but no longer gate a comparison (`"gate": false` with a `gateNote` in the scenario): the harness shell heap (the tool's own page, which was a smaller shell when `initial` was taken) and the per-package executed bytes of Reanimated (the first TS tool attributed that package differently: 262,602 B then, 342,357 B now, while "other" fell by 149 KB and the total, which does gate, fell 7.8%). I did not re-measure the initial commit with the Rust tool; that would need 18 commits of dependency changes installed, so the attribution explanation is an inference from those numbers, not a proof.

## Device-lab gates at the final tree (2026-10-13)

Web G1, G2 and G3 and Android G3 pass at `e21671842` (the last change under `features/` and `app/` is `2a8be30b2`); the evidence page links every recording, read back after upload: https://claude.ai/artifact/EtiP1ECtVZftaHznFFd64E. Android G2 fails and Android G1 was not run. Details and the failures on the way are in `docs/plans/device-test-process/progress.md` and the two new gap files (`android-editor-react-state-warning`, `android-tamil-gate-flaky`).

Correction (2026-10-14): web G1 used the checkout planner because I took the deployed one for broken. It works. From the VM, `/api/models/preferences` resolves `kimi-code-plan-global/kimi-for-coding-highspeed`, and `/api/voice-plan` answers 200 in 22 s cold and 7 s warm. Browser G1 passed against the deployed agent in 22 turns. The earlier failures came from the runbook script, which moved on before planning finished; it now waits for `ProblemEditorVoiceProcessing` to disappear.

One tool finding matters beyond the gates: the Android UI server injected a stroke with one step per segment in about 100 ms, so the Tamil pad scored every letter "Too short". With two steps per segment on a thinned stroke (`android_u2::pace`) strokes score. Probed on the device: one step fails, four steps (13 s) score 97.
