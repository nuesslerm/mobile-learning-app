---
title: device-lab G1 native — voice edit on redroid (Android)
verifies: On a headless Android 14 (redroid) the app is driven by touch input and spoken clips only. Three leetcode problems are solved by spoken edits through the app's own recorder, upload, review sheet and planner, and Run Local passes every test.
tier: agent-driven
maxAgeDays: 30
e2e: e2e/voice-edit.lua
---

# device-lab G1 native: voice edit on redroid

The native twin of [`device-lab-voice-edit.md`](device-lab-voice-edit.md) (turn tables in
[`e2e/voice-edit.lua`](../../e2e/voice-edit.lua)). Result:
`docs/runbooks/.results/device-lab-voice-edit-android.json` (`platform: android-redroid`).

## Rules: the human input model

- Act only through input: `wf android tap-on` (ids, or `--point x,y`), `swipe`, `press-key`, `back`. `get-element`, `get-device-state`, `take-screenshot` only read. Nothing is typed into the editor or the transcript.
- **Speech.** Redroid has no microphone, so each row's `say` text is synthesised offline (piper, `en_US-lessac-medium`, 1.0, or 1.15 for a re-record) and injected at file level: a root loop on the device replaces every new `Audio/recording-*.m4a` the app's recorder creates with the clip (a new inode, so the recorder's writes go nowhere). The app then reads, uploads and reviews that clip as its own recording. Declared deviation.
- Touch dots on; each turn recorded with `wf android start-recording <path>` and `stop-recording`; input log via `WF_ANDROID_INPUT_LOG`.

## Setup

Persistent lab redroid on the VM (`.agents/tmp/lab-up.sh`: 720x1280, gesture navigation, DNS `100.100.100.100` so MagicDNS names resolve, Expo Go, port 5555 published on the agent-shell node), reached from the sandbox over the tailnet SOCKS proxy (`socks_forward.py`) with local `wf android`. The app is Expo Go on the EAS Update CI published (`exp://u.expo.dev/<projectId>/group/<group>/--`), which bakes the backend URLs and the leetcode token; the 2026-10-11 run used the shared dev Metro, since retired. Driver: `.agents/tmp/voice-run.py` (scratch).

## One spoken turn

1. Synthesise the clip, push it to `/data/local/tmp/clip.wav`.
2. Dismiss any LogBox toast; tap `ProblemEditorVoiceStartButton` (allow the microphone prompt); wait the clip length; tap Send on the recording toolbar (fixed coordinates, retried until the toolbar is gone).
3. Read `ProblemEditorVoiceReviewSheet-TranscriptInput`. If word overlap with the `say` text is below 0.6, tap Re-record and say it once more, slower.
4. Send from the review sheet. The edit applies, or the alternative picker opens: close it when every row is invalid (the whole-solution ask), else take the first valid row.
5. Put the keyboard away with Back (only when it is showing), tap `ProblemEditorRunLocalButton` after the last turn, read `ProblemEditorRunOutcomeTotals`.

## Gates

1. The source runbook's gates hold: voice only, buffer always parses, whole-solution asks refused or first-step only, renames local.
2. All-pass: Run Local passes every test (14 / 14, 12 / 12, 11 / 11 on 2026-10-12).
3. Input visible: touch dots and a recording per turn.
4. Input only: the input log holds taps, swipes and keys, no `eval`.

## Native differences and findings

- Two app and setup bugs surfaced and are fixed: the voice upload failed on Android with "Unsupported FormDataPart implementation" (expo/fetch; now XHR), and Android in redroid could not resolve MagicDNS names (now `androidboot.redroid_net_dns1=100.100.100.100`). See the decision log, 2026-10-11.
- On 2026-10-11 the dev Metro (retired 22 Oct) baked an empty leetcode token, so Run Local said "unauthorized". The EAS Update CI publishes bakes the token when it is published: a rotated `VM_GATEWAY_AUTH_SEED` with the app unchanged is not seen until `wf ship --changed --force` publishes again.
- With 3-button navigation the review sheet's Send and Re-record sit under the system bar; the lab uses gesture navigation. Also an app finding: on a phone with 3-button navigation the sheet's actions are covered.
- Container-with-most-water turn 1 ("write the optimal two pointer solution") applied a three-line first step rather than refusing; the runbook allows "refused or first-step only".
- Latency (n=20 turns, one run): transcribe p50 about 15 s, plan p50 about 10 s; both on the shared VM.

## Revision 2026-10-12: small edits only

- No whole-solution turn. Every turn is one small edit (declare a variable, add a loop header, add one condition, move a pointer, rename). The driver refuses an instruction over 18 words or containing "solution" or "solve".
- Warnings are errors: logcat is cleared before and checked after every turn; one video per turn.
- The Send button is tapped at its position scaled from the 540x960 screen to the device size while the waveform keeps uiautomator from going idle.
- The voice upload no longer reads the clip as a Blob (that raised `Response.blob()` warnings); the stats upload retries once before it warns.
