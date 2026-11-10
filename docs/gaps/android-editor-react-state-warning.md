---
status: open
area: device-lab
paths:
  - "frontend/mla-tamagui/features/leetcode/**"
opened: 2026-10-13
last_verified: 2026-10-20
links: []
---
# A React state-update warning toast appeared once while typing in the Android editor

## What is broken

In the first queued Android G3 run of the day (job `1a136f3166a-495a0`, commit `9f8421cff`, Expo Go on the slim redroid against the dev Metro), a red toast reading "Can't perform a React state update on ..." covered the editor's action row after the 11th line of the first problem. The next step (Done, then Run Local) could not find its button and the run failed. The rerun of the same gate (job `1a137006cfe-4a891`, commit `e21671842`) typed both problems with no warning at all (`warnings: {}` for each), so it does not reproduce on demand. The toast text is cut off in the screenshot and the log only had the tail, so the component is not identified.

## Why it is open

Not reproduced; the warning text was lost. `editor-typing.lua` now logs the first JS warning with its line number (`first JS warning, after <problem> line <n>: ...`), so the next occurrence names itself in the job log.

## What unblocks it

Re-run `wf test device device-lab-codemirror-typing-android` (the Device tests workflow on the VM) a few times on a fresh app start; when the log line appears, find the component it names.

## Re-checked 2026-10-20

I read CI evidence with `wf test results` and the run bundles. The warning recurred once after the gap opened: warm-app run-7bb20fda20f0acc6 (17 Oct, 7219d6f23) captured its full text. It is the 'hasn't mounted yet' variant, a side effect in render, and it happened at app warm-up, not only while typing. None of the 429 Android CI runs since 18 Oct 09:00 has a warning, and every editor-typing unit gates on zero warnings. No fix or component has been identified, and the old rate (about 1 in 285) is too low for that absence to prove it gone. It stays open as a rare, unidentified warning; the next sighting needs the logcat tail.

Out of date above:

- "Re-run `wf test device device-lab-codemirror-typing-android` (the Device tests workflow on the VM)": `wf test device` and the Device tests workflow are gone. `wf test` now has only plan|run|publish|results|goldens, and .github/workflows has e2e-tests.yml. editor-typing runs as units in CI's android-e2e-tests job, the emulator may not be driven by hand, and evidence comes from `wf test results --query "test=editor-typing platform=android"`.
- "the warning text was lost": warm-app run-7bb20fda20f0acc6 (17 Oct) has the full text, "Can't perform a React state update on a component that hasn't mounted yet ... Move this work to useEffect instead". It is the not-yet-mounted variant, and it was seen at app warm-up as well as in the editor.
- Job ids `1a136f3166a-495a0` / `1a137006cfe-4a891` and the 'slim redroid' belong to the removed test queue. Runs are now CI runIds on the rdl device at 480x1040.
- "`editor-typing.lua` now logs the first JS warning": the file is e2e/editor-typing.lua (lines 173-180, logcat tail at :178), and the zero-warnings gate is at :336.
