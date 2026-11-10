---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/hooks/useVoiceEdit.ts"
  - "frontend/mla-tamagui/features/leetcode/data/ExpoAudioRecorderAdapter.tsx"
  - "frontend/mla-tamagui/features/leetcode/ui/ProblemEditorScreen.tsx"
opened: 2026-10-21
last_verified: 2026-10-21
links: []
---
# A second tap on the mic while a recording starts or runs rejects, unhandled

## What is broken

The mic button (`ProblemEditorVoiceStartButton`) stays drawn and enabled while recording. A second press runs `handleVoiceStart`, then `useVoiceEdit.start()`, which has no guard, then `useVoiceRecording.startRecording`, then the adapter's `start()`, which calls `recorder.prepareToRecordAsync()` again. expo-audio rejects with "AudioRecorder has already been prepared". The call site is `void handleVoiceStart()`, so the rejection is unhandled. `app-shell/crashHandler.ts` logs it as `app.crash`, and in a dev build LogBox's notification covers the voice toolbar's Send. `beginTurn` also overwrites the first turn's stats.

Secondary: `useVoiceEdit.stop` and `cancel` return early while `recording.isRecording` is still false during the 1 to 3 s start-up, so a Cancel pressed then leaves the recorder prepared.

Found by CI runs 39540461741 and 39543792431. There the Android e2e harness pressed the mic twice: an unacknowledged UI-server click was followed by an `input tap`. The harness is fixed (5c8081ff8), but a person can still double-tap.

## What fixes it

Guard `useVoiceEdit.start` with a ref that is set while a start is in flight or a recording runs, and cleared in `finally`. Do not guard on `state.phase`: `handleVoiceReRecord` calls `reRecord()` and then `handleVoiceStart()` in the same tick, so a phase guard would break Re-record. A flow test that taps the mic twice must see one start reach the audio port double and no `app.crash` warning. The guard has to live in the hook, or the double has to count starts, because the flow tests replace the adapter with `fakeAudioRecorder`.

## Re-checked 2026-10-24

`features/leetcode/hooks/useVoiceEdit.ts` still has no in-flight guard on `start` (no ref set around the start), and no flow test covers a double tap, so the gap stands.
