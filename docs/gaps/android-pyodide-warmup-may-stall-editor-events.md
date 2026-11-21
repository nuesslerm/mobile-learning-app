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
