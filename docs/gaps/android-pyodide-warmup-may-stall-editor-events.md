---
status: open
area: android-e2e
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/ProblemEditorScreen.tsx"
  - "frontend/mla-tamagui/features/leetcode/data/pyodide/pyodideRunner.ts"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# The on-device Python runner may warm up while the learner types, and the editor's changes reach React late

## What is broken

Unconfirmed. In hosted run 44932009867 three editor-typing units stalled for about 8.4 s at the same step (`press_key Enter` returned in 55 ms, the condition that the Enter reached the editor became true at 8354, 8373 and 8448 ms), starting about 10 s into each unit right after a cold start. `ProblemEditorScreen` calls `runner.warmUp()` once the editor reports hydrated, and the runner pulls and boots the 12.9 MiB runtime in the hidden WebView while the harness types. The accessibility-tree fix of the editor's text node (decision 46) explains the same lag by a stale reader snapshot, so the two causes have to be told apart.

## Why blocked

The hosted Android jobs cannot read the app's own log lines (`BetterStack config for source mobile missing: CH_MOBILE_HOST, ...` in every unit's report), so the `pyodide_runner.load` line (ms, transferMs, importMs, loadMs) is not in the bundle.

## What unblocks it

Give the hosted android-e2e-tests jobs the CH_MOBILE_* variables (CI secrets live in Bitwarden) or have the harness copy `adb logcat -d -s ReactNativeJS` into the bundle on a failed unit, then check whether the stall lines up with the load. If it does: warm the runner only after the editor has been quiet for a moment, or boot Pyodide off the renderer's main thread.

## Baseline (F2.13)

The editor now logs one `editor.cold_start` line per editor at its first hydrated (nth, htmlBytes, evalMs, mountToReadyMs, readyToHydratedMs, totalMs, and on a phone loadStartToReadyMs). Before numbers: bundle 662,693 chars (htmlBytes 659,028 in the page). Twin sample (one run, jsdom, not a phone number): evalMs 92, readyToHydratedMs 94.5, totalMs 331.6. `editor-typing` now prints this line and `pyodide_runner.load` in each unit after `open route`, so a hosted Android run gives cold samples when the log is readable; the existing blocker (no CH_MOBILE_* in hosted jobs) still hides them. Browser medians come from `wf browser test e2e/editor-cold-start.lua`, which has not been run.
