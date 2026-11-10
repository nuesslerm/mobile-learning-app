---
title: device-lab G3 native — code typed into CodeMirror on redroid (Android)
verifies: On a headless Android 14 (redroid, nested in agent-shell's dind) the installed lab APK is driven by input only, with the soft keyboard on screen. Two leetcode solutions are typed key by key into the CodeMirror WebView and Run Local passes every test.
tier: agent-driven
maxAgeDays: 30
e2e: e2e/editor-typing.lua
---

# device-lab G3 native: code typed into CodeMirror on redroid

The native twin of [`device-lab-codemirror-typing.md`](device-lab-codemirror-typing.md): same two problems and gates.
Result: `docs/runbooks/.results/device-lab-codemirror-typing-android.json` (`platform: android-redroid`).

## Rules: the human input model

- Act only through input: `wf android tap-on`, `input-text` (key by key, 80 ms between keys), `press-key` (Enter, Backspace, the Dpad arrows), `swipe`. `get-element`, `get-device-state`, `take-screenshot` only read. No paste, no multi-line insert, no `wf android run-shell 'input text ...'` of whole lines.
- **The soft keyboard must be on screen while typing.** `wf android input-text` refuses when `dumpsys input_method` does not report `mInputShown=true`; the run records `wf android get-device-state keyboard` at the start and at the end of each problem, and a screenshot after every line shows the keyboard.
- Touch dots on, screen recorded with `wf android start-recording <path>` and `stop-recording`, input log via `WF_ANDROID_INPUT_LOG`.

## Setup

Lab APK (`eas build --profile android-apk-lab`: agent, leetcode-backend and vm-ops URLs baked in, the leetcode token from the EAS `preview` environment, cleartext on), redroid booted as in [`device-lab-tamil-writing-android.md`](device-lab-tamil-writing-android.md). Driver: `.agents/tmp/code1.sh` (scratch).

## Steps (per problem: `valid-palindrome`, then `two-sum`)

| # | Do | Expect |
| --- | --- | --- |
| 1 | Force-stop, `wf android open-link mla-tamagui://leetcode/<problem>`, `assert-visible --id CodeEditorInput`; tap `ProblemEditorResetButton` and `ResetCodeDialog-Confirm` | The Python stub |
| 2 | Tap into the editor below the stub's last line; `wf android extended-wait-until --condition "device:get_device_state('keyboard').visible" --timeout 0` | The soft keyboard is up |
| 3 | Per line: read the caret line's indent off `get-element --id CodeEditorInput` (result.text), fix it with `press-key Backspace` or spaces, `input-text` the line, screenshot; `press-key Enter` between lines | Each line lands as intended |
| 4 | The mistake: on the first line `input-text q`, screenshot, `press-key Backspace` | The typo is visible, then gone |
| 5 | Tap `EditorAction-Done` to put the keyboard away (Run Local sits under it; `back` refuses while the keyboard is up); tap `ProblemEditorRunLocalButton`; `assert-visible --id ProblemEditorRunOutcomeTotals` | "N / N test cases passed" |

## Gates

1. All-pass: Run Local passes every test on both problems (14 / 14 and 12 / 12 on 2026-10-11).
2. Input only: keys went through `type`/`press`; the input log holds taps and keys.
3. Keyboard: shown at the start of each problem and at the end of typing (`keyboard` result in the evidence).
4. The typed buffer matches the target lines.

## Native differences and findings

- **The syntax toolbar and action row are covered by the keyboard on redroid.** With the keyboard up, `EditorSyntaxToolbar` is at y 1076 to 1178 and `EditorActionToolbar` at 1178 to 1280, below the keyboard's top (about y 660). Taps on them land on keyboard keys (the first run typed stray letters). So symbols are typed on the keyboard and the keyboard is put away with Back instead of `EditorAction-Done`. Open question: whether the keyboard-controller toolbar rides above the keyboard on a real Android device; on redroid it does not.
- The editor's buffer is readable as one accessibility `EditText`; its indentation shows up shifted onto the previous line's trailing spaces, so the driver reads the caret line's indent from the buffer's trailing spaces.
- Keys are injected key events with the keyboard open, not touches on the keyboard's own keys.

## Revision 2026-10-12: toolbar above the keyboard

- Gate: with the keyboard up, `EditorHelperRails` must end at or above the keyboard top (`dumpsys window` IME frame); the run fails otherwise. The rail used to render under the keyboard on Android (edge-to-edge: the window does not resize, and the reported keyboard height leaves out the navigation bar).
- Symbols are tapped on the toolbar by id when the button is on screen and typed on the keyboard when it is scrolled out of view.
- After tapping into the editor press Ctrl+End (`input keycombination 113 123`) and open a new line when the last line has text, so typing never lands mid-document.
- Warnings are errors (logcat checked per problem); one recording per problem, segmented past 3 minutes; uploads are read back.
