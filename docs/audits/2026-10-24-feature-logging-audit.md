# Feature logging audit, 2026-10-24

Question: for each flow in `frontend/mla-tamagui/features/*`, can a failing Android test be diagnosed from the log lines
alone (user actions, network calls and failures, route loads, permission and recorder state, agent and voice calls)?
Method: every non-test, non-fixture source file over 25 lines was checked for a `Logger.*` call; the files with none were
read for the kind of boundary they own.

## Verdict per feature

| Feature | Verdict | Evidence |
| --- | --- | --- |
| coding-agent | Covered | Gateway, ws client, subscriptions, outbox, commands and stores log; flow tests `wsLogging`, `threadStreamLogging`, `commandLogging` assert it. Files without a call are pure views or types (cards, feed items, `orchestrationGateway.ts` types): their actions go through `useThreadCommands`, which logs. |
| service-report | Covered | `ordsClient`, golden comment client, history and screen log. `DrillDialog` is a view. |
| tamil-writing | Covered | Check call, progress store, screens log; `tamilWriting.flow.test.tsx` asserts the check lines. Canvases are pure drawing. |
| settings | Covered | All screens log. |
| leetcode | One gap closed, rest covered | Screens, voice hooks, plan hooks and store, bridge, stores and transports log. |

## Gap found and closed

- `leetcode/data/ExpoAudioRecorderAdapter.tsx` logged nothing: the mic permission verdict, the recorder start and the stop
  (with or without a file) were invisible, so a silent recording on Android could not be told apart from a denied
  permission. Now logs `audio.recorder.permission` (granted, canAskAgain), `audio.recorder.start` (platform),
  `audio.recorder.stop` (durationMs) and `audio.recorder.stop_without_uri` (warn).
  Test: `tests/flow/audioRecorderLogging.flow.test.tsx` (permission line asserted through `loggedLines`).

## Left as is, with reason

- `leetcode/ui/PlanTab.tsx`, `VoiceRecordingToolbar.tsx`, `VoiceReviewSheet.tsx`, `ProblemWorkspace.tsx` and the dialogs:
  views whose handlers call hooks or stores that log (`planStore`, `usePlanRealize`, `useVoiceEdit`, `useVoiceRecording`).
- `leetcode/routes/_layout.tsx` route load: no line of its own; screens log their own load.
- Recorder start errors thrown inside the adapter (`prepareToRecordAsync`) are logged by the caller
  (`voice_edit.record.start_failed`), so the adapter does not catch them.

## Logger assertions in flow tests

`tests/flow/loggedLines.ts` (`forgetLoggedLines`, `loggedLines`) already reads the in-memory tail through the app logger
wiring, and `coding-agent`'s `captureLogs` records lines before scrubbing. The gap
`docs/gaps/flow-tests-cannot-assert-logger.md` is therefore closed.
