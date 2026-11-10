---
title: device-lab G2 native — Tamil writing on redroid (Android)
verifies: On a headless Android 14 (redroid, nested in agent-shell's dind on the VM) the installed lab APK is driven by touch input only. The seven traced letters score at least 80 percent, a freehand letter and three words are marked LOOKS RIGHT by the live marker.
tier: agent-driven
maxAgeDays: 30
e2e: e2e/tamil-writing.lua
---

# device-lab G2 native: Tamil writing on redroid

The native twin of [`device-lab-tamil-writing.md`](device-lab-tamil-writing.md): same steps and gates, same routes,
driven through `wf android` against the installed APK instead of `wf browser` against the web export.
Result: `docs/runbooks/.results/device-lab-tamil-writing-android.json` (`platform: android-redroid`).

## Rules: the human input model

- Act only through input: `wf android draw-strokes-on --id <pad> --source guide|template` (touch strokes read off the screen's pixels, with a hand's timing by default), `tap-on --id`, `swipe`. `get-element`, `assert-visible` and `take-screenshot` only read. Never glyph tables or injected SVG.
- Touch dots on (`wf android run-shell 'settings put system show_touches 1'`), recorded with `wf android start-recording <path>` and `stop-recording`, input log via `WF_ANDROID_INPUT_LOG`.

## Setup

1. APK: `eas build --platform android --profile android-apk-lab` (agent URL `http://100.91.40.14:8787` baked in, cleartext HTTP on for this profile only). Take the artifact URL.
2. From `wf vm exec`: pull the current `wf-cli` image for the `wf` binary, boot `redroid/redroid:14.0.0_64only-latest` privileged in agent-shell's dind (`--cpus 1.5`, 720x1280 at 320 dpi), `adb connect` its bridge IP, `wf android adb install -r <apk>`.
3. Probe the marker: `curl http://100.91.40.14:8787/health` returns 200 from the VM.
The driver script is `.agents/tmp/tamil9.sh` (scratch; its steps are the table below).

## Steps

| # | Do | Expect |
| --- | --- | --- |
| 1 | Force-stop the app, `wf android open-link mla-tamagui://tamil/uyir-1`, swipe the page down (swipe at x=12, outside the canvas) | The stroke coach on அ |
| 2 | Per stroke: `wf android draw-strokes-on --id TamilPracticeCanvasSurface --source guide`, swipe up, `assert-visible --id TamilPracticeStrokeScore` | The step advances; after the last stroke "NN% ON THE LINE", NN at least 80 |
| 3 | `tap-on --id TamilPracticeNext` (the › sits above the card; scroll to the top first), repeat for the six letters of `uyir-1` | Six letters scored |
| 4 | Force-stop, open `tamil/uyirmei-k`, steps 2 | Seventh letter scored |
| 5 | Force-stop, open `tamil/write/uyir-a`, `draw-strokes-on --id TamilWriteCanvasSurface --source template`, `tap-on --id TamilWriteCheck`, `assert-visible --id TamilWriteVerdictBadge` | "LOOKS RIGHT · NN" (on NOT YET: `TamilWriteClear` and once more) |
| 6 | Step 5 on `tamil/write/word-uyir-a`, `word-uyir-aa`, `word-uyir-i` | Each "LOOKS RIGHT" within two attempts |
| 7 | `wf android stop-recording`; publish videos, screenshots, input log | One page |

## Gates

1. Traced letters: all seven at least 80 percent ON THE LINE.
2. Freehand letter: அ marked LOOKS RIGHT.
3. Words: அம்மா, ஆடு, இலை each LOOKS RIGHT within two attempts.
4. Input only: the input log holds touch strokes, taps and swipes.

## Native differences from the browser run

- The pad is read from `uiautomator` bounds and a `screencap`, not the DOM; guide strokes come from the saturated-green mask (blue ink excluded), thinning, and a trail over the skeleton (`clis/harness-core/src/trace.rs`, `guide_trail`, `cover_tour`).
- Page scrolling is a swipe in the left margin; the canvas has to be on screen for `get-element` and `draw-strokes-on`.
- The leetcode token is not baked into the APK (the artifact URL is public); voice edit and editor typing need another way to hand the app its token.

## Revision 2026-10-12: clean runs

- Warnings are errors: logcat is cleared before and checked after every lesson and word (`wf android clear-logs` before; after, `wf android extended-wait-until --condition '#device:get_logs() == 0' --timeout 0`, which fails with the lines as evidence); any `ReactNativeJS` warning or error line fails the run.
- One recording per letter and per word (`wf android start-recording <path>`, then `stop-recording`), because the screen recorder stops a single file near 3 minutes. Every upload is read back; a missing object fails the run.
- Screen is 480x1040 at 213 dpi (`wf android start-device`, a Galaxy S25 in dp: 360 x 780); the script reads the size with `get-device-state screen` and scales the positions it measured on 540x960.
- Result and evidence: `.results/device-lab-tamil-writing-android.json`.
